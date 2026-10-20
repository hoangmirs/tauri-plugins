import { afterEach, test } from "node:test";
import assert from "node:assert/strict";
import {
  activate,
  crashLog,
  fetch,
  getAll,
  init,
  listen,
  logEvent,
  parseStack,
  recordError,
  resetForTests,
  setCollection,
  setCrashCollection,
  setCrashKeys,
  setDefaults,
} from "./index.ts";
import type { Sdk } from "./web.ts";

// `@tauri-apps/api/core` reads `globalThis.isTauri` and
// `window.__TAURI_INTERNALS__.invoke` fresh on every call, so each test sets
// the globals it needs and `afterEach` removes them.
type Globals = Record<string, unknown>;
type Call = { cmd: string; args: unknown };

/** The channels `addPluginListener` made, by id, so a test can send an event. */
const channels = new Map<number, (raw: unknown) => void>();

function setupTauri(answer: (cmd: string, args: unknown) => unknown): Call[] {
  const calls: Call[] = [];
  channels.clear();
  let next = 1;
  (globalThis as Globals).isTauri = true;
  (globalThis as Globals).window = globalThis;
  (globalThis as Globals).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: unknown) => {
      calls.push({ cmd, args });
      if (cmd === "plugin:firebase|remove_listener") channels.delete((args as { channelId: number }).channelId);
      return answer(cmd, args);
    },
    transformCallback: (cb: (raw: unknown) => void) => {
      const id = next++;
      channels.set(id, cb);
      return id;
    },
  };
  return calls;
}

/** Sends one plugin event down every open channel, as the core does. */
const sendEvent = (message: unknown) => {
  for (const cb of channels.values()) cb({ index: 0, message });
};

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
    onConfigUpdate: () => () => {},
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

test("parseStack reads a WebKit stack", () => {
  assert.deepEqual(parseStack("f@https://a/assets/index.js:1:200\n@https://a/x.js:3:4"), [
    { symbol: "f", file: "https://a/assets/index.js", line: 1 },
    { symbol: "(anonymous)", file: "https://a/x.js", line: 3 },
  ]);
});

test("parseStack reads a V8 stack", () => {
  assert.deepEqual(parseStack("Error: x\n    at g (https://a/b.js:7:9)\n    at https://a/c.js:2:1"), [
    { symbol: "g", file: "https://a/b.js", line: 7 },
    { symbol: "(anonymous)", file: "https://a/c.js", line: 2 },
  ]);
});

test("parseStack keeps no frame from a line it cannot read", () => {
  assert.deepEqual(parseStack("garbage"), []);
});

test("recordError on a phone sends crash_record with frames", async () => {
  const calls = setupTauri((cmd) => (cmd === "plugin:firebase|mode" ? "native" : null));
  await recordError("TypeError", "x is undefined", "f@https://a/b.js:7:9");
  assert.deepEqual(
    calls.map((c) => c.cmd),
    ["plugin:firebase|mode", "plugin:firebase|crash_record"],
  );
  assert.deepEqual(calls[1]?.args, {
    name: "TypeError",
    message: "x is undefined",
    frames: [{ symbol: "f", file: "https://a/b.js", line: 7 }],
  });
});

test("on a phone, the other crash calls send their commands", async () => {
  const calls = setupTauri((cmd) => (cmd === "plugin:firebase|mode" ? "native" : null));
  await setCrashCollection(true);
  await setCrashKeys({ screen: "home" });
  await crashLog("opened");
  assert.deepEqual(
    calls.map((c) => c.cmd),
    ["plugin:firebase|mode", "plugin:firebase|crash_set_collection", "plugin:firebase|crash_set_keys", "plugin:firebase|crash_log"],
  );
  assert.deepEqual(calls[1]?.args, { enabled: true });
  assert.deepEqual(calls[2]?.args, { keys: { screen: "home" } });
  assert.deepEqual(calls[3]?.args, { line: "opened" });
});

test("a rejected crash invoke does not reject", async () => {
  setupTauri((cmd) => {
    if (cmd === "plugin:firebase|mode") return "native";
    throw new Error("gone");
  });
  await setCrashCollection(true);
  await setCrashKeys({ a: "b" });
  await crashLog("x");
  await recordError("Error", "m", "");
});

test("crash calls on the Mac send nothing", async () => {
  const calls = setupTauri((cmd) => (cmd === "plugin:firebase|mode" ? "desktop" : null));
  await setCrashCollection(true);
  await crashLog("x");
  await recordError("Error", "m", "f@a.js:1:1");
  assert.deepEqual(
    calls.map((c) => c.cmd),
    ["plugin:firebase|mode"],
  );
});

test("crash calls on the web send nothing", async () => {
  await setCrashCollection(true);
  await setCrashKeys({ a: "b" });
  await crashLog("x");
  await recordError("Error", "m", "f@a.js:1:1");
});

/** A fake `onConfigUpdate` that keeps the observer, so a test can fire an update. */
function liveSdk() {
  const watch: { next?: (u: { getUpdatedKeys(): Set<string> }) => void; error?: (e: unknown) => void; off: number } = { off: 0 };
  const { sdk, log } = fakeSdk({
    onConfigUpdate: (_rc, observer) => {
      watch.next = observer.next;
      watch.error = observer.error;
      return () => {
        watch.off++;
        watch.next = undefined;
      };
    },
  });
  return { sdk, log, watch };
}

test("listen on the web route calls on with the updated keys", async () => {
  const { sdk, watch } = liveSdk();
  await init(WEB, async () => sdk);
  const heard: string[][] = [];
  await listen((keys) => heard.push(keys));
  watch.next?.({ getUpdatedKeys: () => new Set(["a", "b"]) });
  assert.deepEqual(heard, [["a", "b"]]);
});

test("the stop function unsubscribes", async () => {
  const { sdk, watch } = liveSdk();
  await init(WEB, async () => sdk);
  const heard: string[][] = [];
  const stop = await listen((keys) => heard.push(keys));
  stop();
  watch.next?.({ getUpdatedKeys: () => new Set(["a"]) });
  assert.equal(watch.off, 1);
  assert.deepEqual(heard, []);
});

test("listen with no web config resolves to a stop that does nothing, and never calls on", async () => {
  const heard: string[][] = [];
  const stop = await listen((keys) => heard.push(keys));
  assert.doesNotThrow(() => stop());
  assert.deepEqual(heard, []);
});

test("a listener error calls nothing and throws nothing", async () => {
  const { sdk, watch } = liveSdk();
  await init(WEB, async () => sdk);
  const heard: string[][] = [];
  await listen((keys) => heard.push(keys));
  assert.doesNotThrow(() => watch.error?.(new Error("realtime refused")));
  assert.deepEqual(heard, []);
});

const phone = (cmd: string) => (cmd === "plugin:firebase|mode" ? "native" : null);

test("listen on the command route registers configUpdate and invokes listen", async () => {
  const calls = setupTauri(phone);
  const heard: string[][] = [];
  await listen((keys) => heard.push(keys));
  const names = calls.map((c) => c.cmd);
  assert.ok(names.includes("plugin:firebase|register_listener"));
  assert.equal((calls.find((c) => c.cmd === "plugin:firebase|register_listener")?.args as { event: string }).event, "configUpdate");
  assert.ok(names.includes("plugin:firebase|listen"));
  sendEvent({ keys: ["a"] });
  assert.deepEqual(heard, [["a"]]);
});

test("the stop function removes the listener and invokes unlisten", async () => {
  const calls = setupTauri(phone);
  const stop = await listen(() => {});
  stop();
  await new Promise((r) => setTimeout(r, 0));
  const names = calls.map((c) => c.cmd);
  assert.ok(names.includes("plugin:firebase|remove_listener"));
  assert.ok(names.includes("plugin:firebase|unlisten"));
});

test("calling listen twice holds one native listener", async () => {
  const calls = setupTauri(phone);
  const one = await listen(() => {});
  const two = await listen(() => {});
  const count = (cmd: string) => calls.filter((c) => c.cmd === cmd).length;
  assert.equal(count("plugin:firebase|listen"), 1);
  one();
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(count("plugin:firebase|unlisten"), 0);
  two();
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(count("plugin:firebase|unlisten"), 1);
});

/** Lets `n` rounds of queued promise callbacks run. */
const ticks = async (n: number) => {
  for (let i = 0; i < n; i++) await Promise.resolve();
};

test("a listen that arrives while the last stop runs leaves one channel open", async () => {
  for (let gap = 0; gap < 12; gap++) {
    await resetForTests();
    setupTauri(phone);
    const stop = await listen(() => {});
    const heard: string[][] = [];
    const next = listen((keys) => heard.push(keys));
    await ticks(gap);
    stop();
    await next;
    await new Promise((r) => setTimeout(r, 0));
    sendEvent({ keys: ["a"] });
    assert.deepEqual(heard, [["a"]], `gap ${gap}`);
    assert.equal(channels.size, 1, `gap ${gap}`);
  }
});

test("a listen right after the last stop sends listen after unlisten", async () => {
  let release = () => {};
  const calls = setupTauri((cmd) => {
    if (cmd === "plugin:firebase|unlisten") return new Promise<null>((r) => (release = () => r(null)));
    return phone(cmd);
  });
  const stop = await listen(() => {});
  stop();
  const next = listen(() => {});
  await new Promise((r) => setTimeout(r, 0));
  const names = () => calls.map((c) => c.cmd).filter((c) => c === "plugin:firebase|listen" || c === "plugin:firebase|unlisten");
  assert.deepEqual(names(), ["plugin:firebase|listen", "plugin:firebase|unlisten"]);
  release();
  await next;
  assert.deepEqual(names(), ["plugin:firebase|listen", "plugin:firebase|unlisten", "plugin:firebase|listen"]);
});

test("an event that is not a key list calls on with no keys", async () => {
  setupTauri(phone);
  const heard: string[][] = [];
  await listen((keys) => heard.push(keys));
  sendEvent({ keys: "a" });
  assert.deepEqual(heard, [[]]);
});

test("on a phone whose bridge refuses, listen resolves and never throws", async () => {
  setupTauri((cmd) => {
    if (cmd === "plugin:firebase|mode") return "native";
    throw new Error("gone");
  });
  const stop = await listen(() => assert.fail("heard nothing"));
  assert.doesNotThrow(() => stop());
});
