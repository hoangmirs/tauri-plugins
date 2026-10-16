import { afterEach, test } from "node:test";
import assert from "node:assert/strict";
import { activate, fetch, getAll, init, logEvent, resetForTests, setCollection, setDefaults } from "./index.ts";
import type { Sdk } from "./web.ts";

// `@tauri-apps/api/core` reads `globalThis.isTauri` and
// `window.__TAURI_INTERNALS__.invoke` fresh on every call, so each test sets
// the globals it needs and `afterEach` removes them.
type Globals = Record<string, unknown>;
type Call = { cmd: string; args: unknown };

function setupTauri(answer: (cmd: string, args: unknown) => unknown): Call[] {
  const calls: Call[] = [];
  (globalThis as Globals).isTauri = true;
  (globalThis as Globals).window = globalThis;
  (globalThis as Globals).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: unknown) => {
      calls.push({ cmd, args });
      return answer(cmd, args);
    },
  };
  return calls;
}

afterEach(async () => {
  delete (globalThis as Globals).isTauri;
  delete (globalThis as Globals).window;
  delete (globalThis as Globals).__TAURI_INTERNALS__;
  await resetForTests();
});

const WEB = { options: { apiKey: "k", appId: "a", projectId: "p" }, minimumFetchIntervalMillis: 900_000 };

/** A stand-in for the Firebase web SDK that records what it was asked. */
function fakeSdk(over: Partial<Sdk> = {}) {
  const log: string[] = [];
  let defaults: Record<string, string | number | boolean> = {};
  const sdk: Sdk = {
    initializeApp: () => {
      log.push("initializeApp");
      return {};
    },
    getRemoteConfig: () => {
      const rc = {
        settings: { minimumFetchIntervalMillis: 0, fetchTimeoutMillis: 60_000 },
        get defaultConfig() {
          return defaults;
        },
        set defaultConfig(d) {
          defaults = d;
        },
      };
      return rc;
    },
    fetchConfig: async () => {
      log.push("fetchConfig");
    },
    activate: async () => true,
    getAll: () =>
      Object.fromEntries(
        Object.entries(defaults).map(([k, v]) => [k, { asString: () => String(v), getSource: () => "default" as const }]),
      ),
    isSupported: async () => true,
    initializeAnalytics: (_app, options) => {
      log.push(`initializeAnalytics ${JSON.stringify(options)}`);
      return {};
    },
    logEvent: (_a, name, params) => {
      log.push(`logEvent ${name} ${JSON.stringify(params)}`);
    },
    setAnalyticsCollectionEnabled: (_a, on) => {
      log.push(`collection ${on}`);
    },
    ...over,
  };
  return { sdk, log };
}

test("on a phone, each function calls its command", async () => {
  const calls = setupTauri((cmd) =>
    cmd === "plugin:firebase|mode" ? "native" : cmd === "plugin:firebase|fetch" ? "fetched" : cmd.endsWith("activate") ? true : cmd.endsWith("get_all") ? {} : null,
  );
  await setDefaults({ a: true });
  assert.equal(await fetch(), "fetched");
  assert.equal(await activate(), true);
  assert.deepEqual(await getAll(), {});
  await logEvent("opened", { n: 1 });
  await setCollection(true);
  assert.deepEqual(
    calls.map((c) => c.cmd),
    [
      "plugin:firebase|mode",
      "plugin:firebase|set_defaults",
      "plugin:firebase|fetch",
      "plugin:firebase|activate",
      "plugin:firebase|get_all",
      "plugin:firebase|log_event",
      "plugin:firebase|set_collection",
    ],
  );
  assert.deepEqual(calls[1]?.args, { defaults: { a: true } });
  assert.deepEqual(calls[5]?.args, { name: "opened", params: { n: 1 } });
  assert.deepEqual(calls[6]?.args, { enabled: true });
});

test("on a phone, a rejected invoke gives the safe answer", async () => {
  setupTauri((cmd) => {
    if (cmd === "plugin:firebase|mode") return "native";
    throw new Error("gone");
  });
  await setDefaults({ a: 1 });
  assert.equal(await fetch(), "failed");
  assert.equal(await activate(), false);
  assert.deepEqual(await getAll(), { a: { value: "1", source: "default" } });
});

test("on the Mac, mode desktop sends calls to the web path", async () => {
  const calls = setupTauri((cmd) => (cmd === "plugin:firebase|mode" ? "desktop" : null));
  const { sdk, log } = fakeSdk();
  await init(WEB, async () => sdk);
  assert.equal(await fetch(), "fetched");
  assert.ok(log.includes("fetchConfig"));
  assert.deepEqual(
    calls.map((c) => c.cmd),
    ["plugin:firebase|mode"],
  );
});

test("on the Mac with no web config, getAll gives the defaults", async () => {
  setupTauri((cmd) =>
    cmd === "plugin:firebase|mode" ? "desktop" : cmd.endsWith("get_all") ? { a: { value: "true", source: "default" } } : null,
  );
  await init();
  await setDefaults({ a: true });
  assert.deepEqual(await getAll(), { a: { value: "true", source: "default" } });
});

test("on the web, init twice starts one Firebase app", async () => {
  const { sdk, log } = fakeSdk();
  await init(WEB, async () => sdk);
  await init(WEB, async () => sdk);
  assert.equal(log.filter((l) => l === "initializeApp").length, 1);
});

test("on the web, a throwing IndexedDB gives the defaults", async () => {
  const { sdk } = fakeSdk({
    getRemoteConfig: () => {
      throw new Error("IndexedDB unavailable");
    },
  });
  await init(WEB, async () => sdk);
  await setDefaults({ a: false, b: "x" });
  assert.equal(await fetch(), "failed");
  assert.deepEqual(await getAll(), {
    a: { value: "false", source: "default" },
    b: { value: "x", source: "default" },
  });
});

test("on the web, a throttled fetch says throttled", async () => {
  const { sdk } = fakeSdk({
    fetchConfig: async () => {
      throw Object.assign(new Error("throttled"), { code: "remoteconfig/fetch-throttle" });
    },
  });
  await init(WEB, async () => sdk);
  assert.equal(await fetch(), "throttled");
});

test("logEvent trims to 25 params and 100-character text", async () => {
  const calls = setupTauri((cmd) => (cmd === "plugin:firebase|mode" ? "native" : null));
  const params = Object.fromEntries(Array.from({ length: 30 }, (_, i) => [`p${i}`, "y".repeat(150)]));
  await logEvent("big", params);
  const sent = (calls[1]?.args as { params: Record<string, string> }).params;
  assert.equal(Object.keys(sent).length, 25);
  assert.equal(sent.p0?.length, 100);
});

test("logEvent before setCollection(true) does not start Analytics on the web", async () => {
  const { sdk, log } = fakeSdk();
  await init(WEB, async () => sdk);
  await logEvent("early", { a: 1 });
  assert.ok(!log.some((l) => l.startsWith("initializeAnalytics")));
  await setCollection(true);
  await logEvent("later", { a: 1 });
  assert.ok(log.includes('initializeAnalytics {"config":{"send_page_view":false}}'));
  assert.ok(log.includes('logEvent later {"a":1}'));
  assert.ok(!log.some((l) => l.startsWith("logEvent early")));
});

test("no function ever rejects", async () => {
  const boom = async () => {
    throw new Error("no sdk");
  };
  await init(WEB, boom);
  await setDefaults({ a: 1 });
  assert.equal(await fetch(), "failed");
  assert.equal(await activate(), false);
  assert.deepEqual(await getAll(), { a: { value: "1", source: "default" } });
  await logEvent("x");
  await setCollection(true);
  await setCollection(false);
});

test("on the Mac, defaults set before init still answer after it", async () => {
  setupTauri((cmd) => (cmd === "plugin:firebase|mode" ? "desktop" : null));
  await setDefaults({ a: true });
  await init(WEB, async () => {
    throw new Error("no sdk");
  });
  assert.deepEqual(await getAll(), { a: { value: "true", source: "default" } });
});
