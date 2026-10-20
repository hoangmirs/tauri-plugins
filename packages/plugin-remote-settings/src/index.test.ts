import assert from "node:assert/strict";
import { test } from "node:test";
// The hub's source, not its package: CI runs the tests before any package is built.
import { encryptPayload } from "../../remote-settings-hub/src/payload.ts";
import { type FirebaseAdapter, init, type Options } from "./index.ts";
import type { Part } from "./types.ts";

type Globals = Record<string, unknown>;
const g = globalThis as Globals;

const KEY = Buffer.alloc(16, 3).toString("base64");
const OTHER = Buffer.alloc(16, 9).toString("base64");
const BASE = "https://settings.example.com/appa";
const keep = (t: string) => t;
const keys = {
  notice: { fallback: "none", read: keep, global: true },
  lab_x: { fallback: "off", read: (t: string) => (["off", "lab", "on"].includes(t) ? t : undefined) },
};

/** A page: storage, visibility, and the files the server answers. */
function page() {
  const storage = new Map<string, string>();
  const listeners: (() => void)[] = [];
  const doc = {
    visibilityState: "visible",
    addEventListener: (_: string, fn: () => void) => listeners.push(fn),
    removeEventListener: () => {},
  };
  const files: { global: unknown; rules: string | null; country: string | null } = { global: {}, rules: null, country: "VN" };
  const seen: string[] = [];
  g.localStorage = { getItem: (k: string) => storage.get(k) ?? null, setItem: (k: string, v: string) => void storage.set(k, v) };
  g.document = doc;
  delete g.isTauri;
  delete g.window;
  delete g.__TAURI_INTERNALS__;
  const fetcher = (async (u: RequestInfo | URL) => {
    const url = String(u);
    seen.push(url);
    const body = url.endsWith("global.json") ? JSON.stringify(files.global) : files.rules;
    if (body === null) return new Response("", { status: 404 });
    return new Response(body, { status: 200, headers: files.country ? { "x-country": files.country } : {} });
  }) as typeof fetch;
  g.fetch = fetcher;
  const setVisible = async (v: boolean) => {
    doc.visibilityState = v ? "visible" : "hidden";
    for (const fn of listeners) fn();
    await new Promise((r) => setTimeout(r, 10));
  };
  return { storage, files, seen, setVisible };
}

const rules = async (lab: string, key = KEY, extra: Record<string, unknown> = {}) =>
  JSON.stringify(await encryptPayload({ features: { lab_x: { defaultValue: lab }, ...extra }, savedGroups: {} }, key));

const options = (over: Partial<Options<typeof keys>> = {}): Options<typeof keys> => ({
  keys,
  key: { version: 1, value: KEY },
  web: { baseUrl: BASE, stage: "production", version: "1.0.0" },
  ...over,
});

const settle = () => new Promise((r) => setTimeout(r, 30));

test("the first read of a first launch applies, and reads both files", async () => {
  const p = page();
  p.files.global = { notice: "hello" };
  p.files.rules = await rules("on");
  const s = init(options());
  await s.ready(1000);
  assert.deepEqual(s.values().values, { notice: "hello", lab_x: "on" });
  assert.deepEqual(p.seen.sort(), [`${BASE}/production/global.json`, `${BASE}/production/v1.json`]);
  assert.equal(s.used(), "growthbook");
  assert.equal(s.country(), "VN");
});

test("the first values are the saved ones, before any read", async () => {
  const p = page();
  p.files.rules = await rules("on");
  const first = init(options());
  await first.ready(1000);
  p.files.rules = await rules("lab");
  const second = init(options());
  await settle();
  assert.equal(second.values().values.lab_x, "on");
  assert.equal(second.values().from.lab_x, "saved");
});

test("a read in sight waits in hidden mode and applies when the page hides", async () => {
  const p = page();
  p.files.rules = await rules("on");
  const s = init(options());
  await s.ready(1000);
  p.files.rules = await rules("lab");
  await s.refresh();
  assert.equal(s.values().values.lab_x, "on");
  assert.equal(s.pending()?.lab_x, "lab");
  await p.setVisible(false);
  assert.equal(s.values().values.lab_x, "lab");
});

test("a return to the page reads again", async () => {
  const p = page();
  const s = init(options());
  await s.ready(1000);
  const before = p.seen.length;
  await p.setVisible(false);
  await p.setVisible(true);
  await settle();
  assert.equal(p.seen.length, before + 2);
});

test("a rules file that does not decrypt keeps the stored rules, and is not stored", async () => {
  const p = page();
  p.files.rules = await rules("on");
  const first = init(options());
  await first.ready(1000);
  const stored = [...p.storage.entries()].find(([k]) => k.endsWith(":rules"))?.[1];
  p.files.rules = await rules("lab", OTHER);
  const s = init(options({ apply: "now" }));
  await s.ready(1000);
  assert.equal(s.values().values.lab_x, "on");
  assert.equal([...p.storage.entries()].find(([k]) => k.endsWith(":rules"))?.[1], stored);
});

const adapter = (over: Partial<FirebaseAdapter> = {}) => {
  const calls: string[] = [];
  const a: FirebaseAdapter = {
    values: async () => {
      calls.push("values");
      return { lab_x: "lab" } as Part;
    },
    fetch: async () => {
      calls.push("fetch");
      return false;
    },
    activate: async () => {
      calls.push("activate");
      return true;
    },
    listen: async () => {
      calls.push("listen");
      return () => calls.push("stop");
    },
    ...over,
  };
  return { a, calls };
};

test("with source growthbook the Firebase adapter is never called", async () => {
  const p = page();
  p.files.global = { source: "growthbook" };
  p.files.rules = await rules("on");
  const { a, calls } = adapter();
  const s = init(options({ firebase: { adapter: a, listen: () => true } }));
  await s.ready(1000);
  assert.deepEqual(calls, []);
});

test("with source firebase the values come from the adapter", async () => {
  const p = page();
  p.files.global = { source: "firebase" };
  const { a } = adapter();
  const s = init(options({ firebase: { adapter: a } }));
  await s.ready(1000);
  assert.equal(s.values().values.lab_x, "lab");
  assert.equal(s.used(), "firebase");
});

test("with source firebase and no adapter, no source values come and nothing throws", async () => {
  const p = page();
  p.files.global = { source: "firebase" };
  const s = init(options());
  await s.ready(1000);
  assert.equal(s.values().values.lab_x, "off");
});

test("with source firebase, fetched values activate at the next apply", async () => {
  const p = page();
  p.files.global = { source: "firebase" };
  let active: Part = { lab_x: "lab" };
  const { a, calls } = adapter({
    values: async () => active,
    fetch: async () => true,
    activate: async () => {
      active = { lab_x: "on" };
      calls.push("activate");
      return true;
    },
  });
  const s = init(options({ firebase: { adapter: a } }));
  await s.ready(1000);
  await s.refresh();
  await p.setVisible(false);
  assert.ok(calls.includes("activate"));
  assert.equal(s.values().values.lab_x, "on");
});

test("the adapter listens only while firebase is the source and the app asks", async () => {
  const p = page();
  p.files.global = { source: "firebase" };
  const { a, calls } = adapter();
  const s = init(options({ firebase: { adapter: a, listen: () => true } }));
  await s.ready(1000);
  await settle();
  assert.ok(calls.includes("listen"));
});

test("the global file's apply word changes the mode", async () => {
  const p = page();
  p.files.rules = await rules("on");
  const s = init(options());
  await s.ready(1000);
  p.files.global = { apply: "now" };
  p.files.rules = await rules("lab");
  await s.refresh();
  assert.equal(s.values().values.lab_x, "lab");
});

test("rest mode applies a read at once while the app is at rest", async () => {
  const p = page();
  p.files.rules = await rules("on");
  const s = init(options({ apply: "rest" }));
  await s.ready(1000);
  s.setAtRest(true);
  p.files.rules = await rules("lab");
  await s.refresh();
  assert.equal(s.values().values.lab_x, "lab");
});

test("subscribe hears the keys that changed", async () => {
  const p = page();
  p.files.rules = await rules("on");
  const s = init(options({ apply: "now" }));
  const heard: string[][] = [];
  s.subscribe((changed) => heard.push(changed.map(String)));
  await s.ready(1000);
  assert.ok(heard.some((c) => c.includes("lab_x")));
});

test("ready resolves at the deadline when the read is slow", async () => {
  page();
  g.fetch = (() => new Promise(() => {})) as unknown as typeof fetch;
  const s = init(options());
  const start = Date.now();
  await s.ready(50);
  assert.ok(Date.now() - start < 500);
});

test("legacy saved values seed the first launch, and only the first", async () => {
  const p = page();
  p.files.rules = null;
  const legacy = { saved: () => ({ global: { notice: "old" }, source: { lab_x: "lab" }, from: "growthbook" as const, country: null }) };
  const s = init(options({ legacy }));
  await settle();
  assert.equal(s.values().values.notice, "old");
  assert.equal(s.values().values.lab_x, "lab");
  let asked = 0;
  init(options({ legacy: { saved: () => (asked++, null) } }));
  await settle();
  assert.equal(asked, 0);
});

test("a legacy install id is kept", async () => {
  page();
  const seed = "0f8fad5b-d9cb-469f-a165-70867728950e";
  const s = init(options({ legacy: { installId: () => seed } }));
  await s.ready(1000);
  assert.equal(s.installId(), seed);
});

test("an experiment is reported when its value applies", async () => {
  const p = page();
  const exp = {
    lab_x: {
      defaultValue: "off",
      rules: [{ id: "r", key: "exp1", hashAttribute: "id", coverage: 1, weights: [0, 1], variations: ["off", "on"], meta: [{ key: "0" }, { key: "1" }] }],
    },
  };
  p.files.rules = JSON.stringify(await encryptPayload({ features: exp }, KEY));
  const heard: string[] = [];
  const s = init(options({ onExperiment: (e, v) => heard.push(`${e}:${v}`) }));
  await s.ready(1000);
  assert.deepEqual(heard, ["exp1:1"]);
});

test("an app attribute adds to the standard ones and cannot replace one", async () => {
  const p = page();
  const features = {
    lab_x: { defaultValue: "off", rules: [{ condition: { plan: "paid", platform: "web" }, force: "on" }] },
  };
  p.files.rules = JSON.stringify(await encryptPayload({ features }, KEY));
  const s = init(options({ attributes: () => ({ plan: "paid", platform: "ios" }) }));
  await s.ready(1000);
  assert.equal(s.values().values.lab_x, "on");
});

test("the attributes carry the install id, the version and the country", async () => {
  const p = page();
  const features = {
    lab_x: { defaultValue: "off", rules: [{ condition: { country: "VN", version: "1.0.0", web: true, id: { $exists: true } }, force: "on" }] },
  };
  p.files.rules = JSON.stringify(await encryptPayload({ features }, KEY));
  const s = init(options());
  await s.ready(1000);
  assert.equal(s.values().values.lab_x, "on");
});

function tauri(answers: Record<string, unknown>) {
  const calls: string[] = [];
  g.isTauri = true;
  g.window = globalThis;
  g.__TAURI_INTERNALS__ = {
    invoke: async (cmd: string) => {
      calls.push(cmd);
      if (!(cmd in answers)) throw new Error("no such command");
      const a = answers[cmd];
      return typeof a === "function" ? (a as () => unknown)() : a;
    },
    transformCallback: () => 0,
  };
  return calls;
}

test("in a Tauri app the files come from the prefetched read, then from read", async () => {
  page();
  const store = new Map<string, string>();
  let prefetchedGiven = false;
  const calls = tauri({
    "plugin:remote-settings|config": { baseUrl: BASE, stage: "production", keyVersion: 1, countryHeader: "x-country", platform: "ios", version: "2.0.0" },
    "plugin:remote-settings|prefetched": () => {
      if (prefetchedGiven) return null;
      prefetchedGiven = true;
      return { global: '{"notice":"native"}', rules: null, country: "VN" };
    },
    "plugin:remote-settings|read": { global: '{"notice":"again"}', rules: null, country: "VN" },
    "plugin:remote-settings|get": () => null,
    "plugin:remote-settings|set": () => null,
    "plugin:remote-settings|install_id": "0f8fad5b-d9cb-469f-a165-70867728950e",
  });
  void store;
  const s = init(options({ apply: "now" }));
  await s.ready(1000);
  assert.equal(s.values().values.notice, "native");
  await s.refresh();
  assert.equal(s.values().values.notice, "again");
  assert.ok(calls.includes("plugin:remote-settings|prefetched"));
  assert.ok(calls.includes("plugin:remote-settings|read"));
});

test("a Tauri app without the crate falls back to the web path", async () => {
  const p = page();
  p.files.global = { notice: "web" };
  tauri({});
  const s = init(options());
  await s.ready(1000);
  assert.equal(s.values().values.notice, "web");
  delete g.isTauri;
  delete g.window;
  delete g.__TAURI_INTERNALS__;
});

test("no exported function rejects when every command fails", async () => {
  page();
  tauri({});
  // No web place either, so nothing can be read or kept.
  const s = init({ keys, key: { version: 1, value: KEY } });
  await s.ready(200);
  await s.refresh();
  await s.applyNow();
  s.setAtRest(true);
  assert.deepEqual(s.values().values, { notice: "none", lab_x: "off" });
  assert.equal(s.installId(), null);
  delete g.isTauri;
  delete g.window;
  delete g.__TAURI_INTERNALS__;
});
