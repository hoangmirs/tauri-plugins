import { test } from "node:test";
import assert from "node:assert/strict";
import { init, track, flush, setOptOut, isOptedOut } from "./index.ts";
import * as web from "./web.ts";

// `@tauri-apps/api/core`'s `isTauri()` reads `globalThis.isTauri`, and its
// `invoke()` reads `window.__TAURI_INTERNALS__.invoke(cmd, args, options)` —
// both fresh on every call, not cached at import time (checked against the
// installed package's source). So, unlike some other test suites in this
// repo, every test here can share the one top-level import of `index.ts`
// and `web.ts` and just flip the globals each test sets up and tears down.
type Globals = Record<string, unknown>;

const QUEUE_KEY = "posthog-plugin:queue";
const ID_KEY = "posthog-plugin:id";
const OPT_OUT_KEY = "posthog-plugin:opt-out";

function teardownTauri(): void {
  delete (globalThis as Globals).isTauri;
  delete (globalThis as Globals).window;
  delete (globalThis as Globals).__TAURI_INTERNALS__;
}

function setupTauri(invoke: (cmd: string, args: unknown) => Promise<unknown>): void {
  (globalThis as Globals).isTauri = true;
  (globalThis as Globals).window = globalThis;
  (globalThis as Globals).__TAURI_INTERNALS__ = { invoke };
}

// A `localStorage` double. Empty unless seeded, and every method can be
// told to throw, the way a browser in private mode does.
class FakeStorage {
  private data = new Map<string, string>();
  throwing = false;

  private guard(): void {
    if (this.throwing) throw new Error("storage disabled");
  }

  getItem(key: string): string | null {
    this.guard();
    return this.data.has(key) ? (this.data.get(key) as string) : null;
  }

  setItem(key: string, value: string): void {
    this.guard();
    this.data.set(key, value);
  }

  removeItem(key: string): void {
    this.guard();
    this.data.delete(key);
  }

  raw(key: string): string | undefined {
    return this.data.get(key);
  }
}

let storage: FakeStorage | undefined;

// Every web test starts from the same clean slate: no Tauri globals, a
// fresh fake `localStorage`, and the web module's own in-memory state
// (config, timer, pagehide flag) reset — since `web.ts` is one module
// instance shared by every test in this file and by `index.ts`'s own
// import of it.
async function setupWeb(): Promise<FakeStorage> {
  teardownTauri();
  await web.resetForTests();
  storage = new FakeStorage();
  // A real `EventTarget` so `window.addEventListener`/`dispatchEvent` work,
  // unlike bare `globalThis` in Node (which has neither).
  (globalThis as Globals).window = new EventTarget();
  (globalThis as Globals).localStorage = storage;
  return storage;
}

async function teardownWeb(): Promise<void> {
  await web.resetForTests();
  delete (globalThis as Globals).window;
  delete (globalThis as Globals).localStorage;
  storage = undefined;
}

type FetchCall = { url: string; init: RequestInit };

function setupFetch(handler: (call: FetchCall) => Promise<Response> | Response): FetchCall[] {
  const calls: FetchCall[] = [];
  (globalThis as Globals).fetch = async (url: string, init: RequestInit) => {
    const call = { url, init: init ?? {} };
    calls.push(call);
    return handler(call);
  };
  return calls;
}

function teardownFetch(): void {
  delete (globalThis as Globals).fetch;
}

/** A `fetch` double whose responses the test resolves by hand, one call at
 * a time — for provoking overlap between two flush attempts: the first
 * request is left hanging while a second is requested, so a test can
 * prove they never run concurrently. */
function setupDeferredFetch(): { calls: FetchCall[]; resolveNext: (status: number) => void } {
  const calls: FetchCall[] = [];
  const resolvers: Array<(response: Response) => void> = [];
  (globalThis as Globals).fetch = (url: string, init: RequestInit) => {
    calls.push({ url, init: init ?? {} });
    return new Promise<Response>((resolve) => resolvers.push(resolve));
  };
  return {
    calls,
    resolveNext(status: number) {
      const resolve = resolvers.shift();
      if (resolve === undefined) throw new Error("no pending fetch request to resolve");
      resolve(new Response(null, { status }));
    },
  };
}

function ok(): Response {
  return new Response(null, { status: 200 });
}

function serverError(): Response {
  return new Response(null, { status: 500 });
}

type StoredEvent = { uuid: string; event: string; timestamp: string; properties: Record<string, unknown> };

function queueOf(store: FakeStorage): StoredEvent[] {
  const raw = store.raw(QUEUE_KEY);
  return raw === undefined ? [] : (JSON.parse(raw) as StoredEvent[]);
}

// ---------------------------------------------------------------------------
// Tauri path
// ---------------------------------------------------------------------------

test("under Tauri, track invokes capture with the event, properties and $locale", async () => {
  let received: { cmd: string; args: unknown } | undefined;
  setupTauri(async (cmd, args) => {
    received = { cmd, args };
  });
  const originalNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  Object.defineProperty(globalThis, "navigator", { value: { language: "vi-VN" }, configurable: true });
  try {
    await track("game_finished", { game: "caro" });
    assert.equal(received?.cmd, "plugin:posthog|capture");
    const args = received?.args as { event: string; properties: Record<string, unknown> };
    assert.equal(args.event, "game_finished");
    assert.equal(args.properties.game, "caro");
    assert.equal(args.properties.$locale, "vi-VN");
  } finally {
    teardownTauri();
    if (originalNavigator) Object.defineProperty(globalThis, "navigator", originalNavigator);
    else delete (globalThis as Globals).navigator;
  }
});

test("under Tauri, a caller's own $locale wins over navigator.language", async () => {
  let received: unknown;
  setupTauri(async (_cmd, args) => {
    received = args;
  });
  const originalNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  Object.defineProperty(globalThis, "navigator", { value: { language: "vi-VN" }, configurable: true });
  try {
    await track("game_finished", { $locale: "fr-FR" });
    const args = received as { properties: Record<string, unknown> };
    assert.equal(args.properties.$locale, "fr-FR");
  } finally {
    teardownTauri();
    if (originalNavigator) Object.defineProperty(globalThis, "navigator", originalNavigator);
    else delete (globalThis as Globals).navigator;
  }
});

test("under Tauri, a throwing invoke makes track resolve, not reject", async () => {
  setupTauri(async () => {
    throw new Error("no route to host");
  });
  try {
    await assert.doesNotReject(track("x"));
  } finally {
    teardownTauri();
  }
});

test("under Tauri, a throwing invoke makes flush resolve, not reject", async () => {
  setupTauri(async () => {
    throw new Error("no route to host");
  });
  try {
    await assert.doesNotReject(flush());
  } finally {
    teardownTauri();
  }
});

test("under Tauri, a throwing invoke makes setOptOut resolve, not reject", async () => {
  setupTauri(async () => {
    throw new Error("no route to host");
  });
  try {
    await assert.doesNotReject(setOptOut(true));
  } finally {
    teardownTauri();
  }
});

test("under Tauri, isOptedOut resolves false when invoke throws", async () => {
  setupTauri(async () => {
    throw new Error("no route to host");
  });
  try {
    assert.equal(await isOptedOut(), false);
  } finally {
    teardownTauri();
  }
});

test("under Tauri, isOptedOut resolves whatever the command answers", async () => {
  setupTauri(async (cmd) => {
    assert.equal(cmd, "plugin:posthog|is_opted_out");
    return true;
  });
  try {
    assert.equal(await isOptedOut(), true);
  } finally {
    teardownTauri();
  }
});

test("under Tauri, flush invokes plugin:posthog|flush with no args", async () => {
  let received: { cmd: string; args: unknown } | undefined;
  setupTauri(async (cmd, args) => {
    received = { cmd, args };
  });
  try {
    await flush();
    assert.equal(received?.cmd, "plugin:posthog|flush");
    assert.deepEqual(received?.args, {});
  } finally {
    teardownTauri();
  }
});

test("under Tauri, setOptOut invokes plugin:posthog|set_opt_out with out", async () => {
  let received: { cmd: string; args: unknown } | undefined;
  setupTauri(async (cmd, args) => {
    received = { cmd, args };
  });
  try {
    await setOptOut(true);
    assert.equal(received?.cmd, "plugin:posthog|set_opt_out");
    assert.deepEqual(received?.args, { out: true });
  } finally {
    teardownTauri();
  }
});

test("under Tauri, init is a no-op", async () => {
  setupTauri(async () => undefined);
  try {
    assert.doesNotThrow(() => init({ apiKey: "phc_test" }));
  } finally {
    teardownTauri();
  }
});

// ---------------------------------------------------------------------------
// Web path
// ---------------------------------------------------------------------------

test("on the web with no init, track is a silent no-op", async () => {
  const store = await setupWeb();
  const calls = setupFetch(() => ok());
  try {
    await track("game_finished");
    assert.equal(store.raw(QUEUE_KEY), undefined);
    assert.equal(calls.length, 0);
  } finally {
    await teardownWeb();
    teardownFetch();
  }
});

test("on the web after init, track stores a fully-shaped event in localStorage", async () => {
  const store = await setupWeb();
  const originalNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  Object.defineProperty(globalThis, "navigator", { value: { language: "vi-VN" }, configurable: true });
  try {
    init({ apiKey: "phc_test", appVersion: "1.2.3" });
    await track("game_finished", { game: "caro" });

    const queue = queueOf(store);
    assert.equal(queue.length, 1);
    const stored = queue[0];
    assert.ok(stored);

    assert.match(stored.uuid, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i);
    assert.equal(stored.event, "game_finished");
    assert.ok(stored.timestamp.endsWith("Z"));
    assert.ok(!Number.isNaN(Date.parse(stored.timestamp)));

    const id = store.raw(ID_KEY);
    assert.ok(id);
    assert.equal(stored.properties.distinct_id, id);
    assert.equal(stored.properties.$process_person_profile, false);
    assert.equal(stored.properties.$lib, "tauri-plugin-posthog-web");
    assert.equal(stored.properties.$os, "Web");
    assert.equal(stored.properties.platform, "web");
    assert.equal(stored.properties.$app_version, "1.2.3");
    assert.equal(stored.properties.game, "caro");
    assert.equal(stored.properties.$locale, "vi-VN");
  } finally {
    await teardownWeb();
    if (originalNavigator) Object.defineProperty(globalThis, "navigator", originalNavigator);
    else delete (globalThis as Globals).navigator;
  }
});

test("on the web, a caller cannot override distinct_id or $process_person_profile", async () => {
  const store = await setupWeb();
  try {
    init({ apiKey: "phc_test" });
    await track("x", { distinct_id: "someone@example.com", $process_person_profile: true });

    const queue = queueOf(store);
    const id = store.raw(ID_KEY);
    assert.equal(queue[0]?.properties.distinct_id, id);
    assert.equal(queue[0]?.properties.$process_person_profile, false);
  } finally {
    await teardownWeb();
  }
});

test("on the web, an empty or overlong event name is dropped", async () => {
  const store = await setupWeb();
  try {
    init({ apiKey: "phc_test" });
    await track("   ");
    await track("x".repeat(201));
    assert.equal(store.raw(QUEUE_KEY), undefined);
    await track("x".repeat(200));
    assert.equal(queueOf(store).length, 1);
  } finally {
    await teardownWeb();
  }
});

test("on the web, flush POSTs {api_key, batch} to {host}/batch/ and removes events on 2xx", async () => {
  const store = await setupWeb();
  const calls = setupFetch(() => ok());
  try {
    init({ apiKey: "phc_test", host: "https://eu.i.posthog.com/" });
    await track("a");
    await track("b");
    await flush();

    assert.equal(calls.length, 1);
    assert.equal(calls[0]?.url, "https://eu.i.posthog.com/batch/");
    assert.equal(calls[0]?.init.method, "POST");
    const headers = calls[0]?.init.headers as Record<string, string>;
    assert.equal(headers["Content-Type"], "application/json");
    const body = JSON.parse(calls[0]?.init.body as string) as { api_key: string; batch: unknown[] };
    assert.equal(body.api_key, "phc_test");
    assert.equal(body.batch.length, 2);

    assert.equal(queueOf(store).length, 0);
  } finally {
    await teardownWeb();
    teardownFetch();
  }
});

test("on the web, events stay queued after a failed (non-2xx) flush", async () => {
  const store = await setupWeb();
  setupFetch(() => serverError());
  try {
    init({ apiKey: "phc_test" });
    await track("a");
    await flush();
    assert.equal(queueOf(store).length, 1);
  } finally {
    await teardownWeb();
    teardownFetch();
  }
});

test("on the web, events stay queued when fetch itself throws", async () => {
  const store = await setupWeb();
  (globalThis as Globals).fetch = async () => {
    throw new Error("offline");
  };
  try {
    init({ apiKey: "phc_test" });
    await track("a");
    await assert.doesNotReject(flush());
    assert.equal(queueOf(store).length, 1);
  } finally {
    await teardownWeb();
    teardownFetch();
  }
});

test("on the web, flush sends at most 100 events per request", async () => {
  const store = await setupWeb();
  const calls = setupFetch(() => ok());
  try {
    init({ apiKey: "phc_test" });
    // Seed 150 already-shaped events directly, bypassing `track`'s own
    // 20-event auto-flush so this test only exercises the 100-per-request
    // cap, not the threshold trigger (covered separately).
    const seeded = Array.from({ length: 150 }, (_, i) => ({
      uuid: `00000000-0000-4000-8000-${String(i).padStart(12, "0")}`,
      event: `e${i}`,
      timestamp: new Date().toISOString(),
      properties: { distinct_id: "install-1" },
    }));
    store.setItem(QUEUE_KEY, JSON.stringify(seeded));

    await flush();

    assert.equal(calls.length, 1);
    const body = JSON.parse(calls[0]?.init.body as string) as { batch: unknown[] };
    assert.equal(body.batch.length, 100);
    const remaining = queueOf(store);
    assert.equal(remaining.length, 50);
    assert.equal(remaining[0]?.event, "e100");
  } finally {
    await teardownWeb();
    teardownFetch();
  }
});

test("on the web, 20 queued events trigger an automatic flush", async () => {
  await setupWeb();
  const calls = setupFetch(() => ok());
  try {
    init({ apiKey: "phc_test" });
    for (let i = 0; i < 19; i += 1) {
      // eslint-disable-next-line no-await-in-loop
      await track(`e${i}`);
    }
    assert.equal(calls.length, 0);
    await track("e19");
    assert.equal(calls.length, 1);
  } finally {
    await teardownWeb();
    teardownFetch();
  }
});

test("on the web, the queue keeps at most 1,000 events, oldest dropped", async () => {
  const store = await setupWeb();
  // A flush that always fails never removes anything, so this exercises
  // only `track`'s own eviction, not a race with a successful flush.
  setupFetch(() => serverError());
  try {
    init({ apiKey: "phc_test" });
    for (let i = 0; i < 1_010; i += 1) {
      // eslint-disable-next-line no-await-in-loop
      await track(`e${i}`);
    }
    const queue = queueOf(store);
    assert.equal(queue.length, 1_000);
    assert.equal(queue[0]?.event, "e10");
    assert.equal(queue[queue.length - 1]?.event, "e1009");
  } finally {
    await teardownWeb();
    teardownFetch();
  }
});

test("on the web, setOptOut(true) clears the queue, stops track, and persists", async () => {
  const store = await setupWeb();
  setupFetch(() => ok());
  try {
    init({ apiKey: "phc_test" });
    await track("a");
    await setOptOut(true);
    assert.equal(queueOf(store).length, 0);

    await track("b");
    assert.equal(queueOf(store).length, 0);

    assert.equal(await isOptedOut(), true);
    assert.equal(store.raw(OPT_OUT_KEY), "true");
  } finally {
    await teardownWeb();
    teardownFetch();
  }
});

test("a throwing localStorage makes every web function a silent no-op", async () => {
  const store = await setupWeb();
  store.throwing = true;
  setupFetch(() => ok());
  try {
    assert.doesNotThrow(() => init({ apiKey: "phc_test" }));
    await assert.doesNotReject(track("a"));
    await assert.doesNotReject(flush());
    await assert.doesNotReject(setOptOut(true));
    assert.equal(await isOptedOut(), false);
  } finally {
    await teardownWeb();
    teardownFetch();
  }
});

// ---------------------------------------------------------------------------
// Web internals: timers and pagehide
// ---------------------------------------------------------------------------

test("the web's periodic flush starts lazily on the first track after init, and is unref'd", async () => {
  await setupWeb();
  const calls = setupFetch(() => ok());

  let unrefCalled = false;
  const originalSetInterval = globalThis.setInterval;
  (globalThis as Globals).setInterval = ((handler: TimerHandler, ms?: number) => {
    const id = originalSetInterval(handler as never, ms);
    const t = id as unknown as { unref?: () => void };
    if (typeof t.unref === "function") {
      const original = t.unref.bind(t);
      t.unref = () => {
        unrefCalled = true;
        return original();
      };
    }
    return id;
  }) as typeof setInterval;

  try {
    init({ apiKey: "phc_test" });
    assert.equal(calls.length, 0);
    await track("a");
    assert.ok(unrefCalled, "the periodic timer should be unref'd so it never blocks the process from exiting");
  } finally {
    (globalThis as Globals).setInterval = originalSetInterval;
    await teardownWeb();
    teardownFetch();
  }
});

test("the periodic timer flushes every 30 seconds", async (t) => {
  await setupWeb();
  const calls = setupFetch(() => ok());
  t.mock.timers.enable({ apis: ["setInterval"] });
  try {
    init({ apiKey: "phc_test" });
    await track("a");
    assert.equal(calls.length, 0, "one queued event is below the 20-event auto-flush threshold");

    t.mock.timers.tick(30_000);
    // let the fire-and-forget flush's fetch continuation run
    await Promise.resolve();
    await Promise.resolve();
    assert.equal(calls.length, 1, "the 30s timer should have triggered exactly one flush");
  } finally {
    await teardownWeb();
    teardownFetch();
  }
});

test("on the web, pagehide flushes with fetch keepalive: true", async () => {
  await setupWeb();
  const calls = setupFetch(() => ok());
  try {
    init({ apiKey: "phc_test" });
    await track("a");
    assert.equal(calls.length, 0);

    window.dispatchEvent(new Event("pagehide"));
    // the handler fires a chained, fire-and-forget flush — give it a turn
    await Promise.resolve();
    await Promise.resolve();

    assert.equal(calls.length, 1);
    assert.equal(calls[0]?.init.keepalive, true);
  } finally {
    await teardownWeb();
    teardownFetch();
  }
});

// ---------------------------------------------------------------------------
// Flush serialization and retry backoff
// ---------------------------------------------------------------------------

test("overlapping flush() calls are serialized: no event is lost or sent twice", async () => {
  const store = await setupWeb();
  const { calls, resolveNext } = setupDeferredFetch();
  try {
    init({ apiKey: "phc_test" });
    await track("a");
    await track("b");

    const first = flush();
    // let the chained attempt reach its `fetch` call
    await Promise.resolve();
    await Promise.resolve();
    assert.equal(calls.length, 1, "the first flush's request should be in flight");

    // more events arrive while the first request is still in flight
    await track("c");

    const second = flush();
    await Promise.resolve();
    await Promise.resolve();
    assert.equal(calls.length, 1, "the second flush must wait its turn, not overlap the first");

    resolveNext(200); // the first request succeeds, sending [a, b]
    await first;
    // the second attempt's own turn now runs
    await Promise.resolve();
    await Promise.resolve();
    assert.equal(calls.length, 2, "the second flush should now have made its own request");

    resolveNext(200); // the second request succeeds, sending [c]
    await second;

    assert.equal(queueOf(store).length, 0, "every event should have been sent");
    const sentEvents = calls.flatMap(
      (call) => (JSON.parse(call.init.body as string) as { batch: Array<{ event: string }> }).batch,
    );
    assert.deepEqual(
      sentEvents.map((e) => e.event).sort(),
      ["a", "b", "c"],
      "every event should have been sent exactly once, none lost or duplicated",
    );
  } finally {
    await teardownWeb();
    teardownFetch();
  }
});

test("a successful flush leaves exactly the events tracked during its request", async () => {
  const store = await setupWeb();
  const { calls, resolveNext } = setupDeferredFetch();
  try {
    init({ apiKey: "phc_test" });
    await track("a");

    const pending = flush();
    await Promise.resolve();
    await Promise.resolve();
    assert.equal(calls.length, 1);

    // tracked while the in-flight request is still unresolved
    await track("b");

    resolveNext(200);
    await pending;

    const remaining = queueOf(store);
    assert.equal(remaining.length, 1);
    assert.equal(remaining[0]?.event, "b");
  } finally {
    await teardownWeb();
    teardownFetch();
  }
});

test("a failed flush backs off 30s, doubling, capped at 600s; a success resets it", async () => {
  const store = await setupWeb();
  let clockNow = 1_000_000;
  web.setClockForTests(() => clockNow);
  let succeed = false;
  const calls = setupFetch(() => (succeed ? ok() : serverError()));
  try {
    init({ apiKey: "phc_test" });
    await track("a");

    await flush();
    assert.equal(calls.length, 1, "the first attempt fails, starting a 30s backoff");

    await flush();
    assert.equal(calls.length, 1, "within the 30s wait, no new request is made");

    clockNow += 29_999;
    await flush();
    assert.equal(calls.length, 1, "still one millisecond short of the 30s wait");

    clockNow += 1;
    await flush();
    assert.equal(calls.length, 2, "the 30s wait has elapsed, so a retry is made");

    clockNow += 59_999;
    await flush();
    assert.equal(calls.length, 2, "the second failure doubled the wait to 60s");
    clockNow += 1;
    await flush();
    assert.equal(calls.length, 3);

    clockNow += 120_000;
    await flush();
    assert.equal(calls.length, 4, "doubled again to 120s");

    clockNow += 240_000;
    await flush();
    assert.equal(calls.length, 5, "doubled again to 240s");

    clockNow += 480_000;
    await flush();
    assert.equal(calls.length, 6, "doubled again to 480s");

    clockNow += 599_999;
    await flush();
    assert.equal(calls.length, 6, "capped at 600s: one millisecond short still waits");
    clockNow += 1;
    await flush();
    assert.equal(calls.length, 7, "600s elapsed: a retry is made");

    clockNow += 600_000;
    await flush();
    assert.equal(calls.length, 8, "still capped at 600s, not still doubling past it");

    // now let a send succeed, and confirm success resets the backoff
    succeed = true;
    clockNow += 600_000; // wait out the capped 600s from attempt 8 before retrying
    await flush();
    assert.equal(calls.length, 9);
    assert.equal(queueOf(store).length, 0, "the event was finally sent");

    succeed = false;
    await track("b");
    await flush();
    assert.equal(calls.length, 10, "fails again -> a fresh 30s backoff, not a continued 600s one");
    await flush();
    assert.equal(calls.length, 10, "an immediate retry is still refused");
    clockNow += 29_999;
    await flush();
    assert.equal(calls.length, 10);
    clockNow += 1;
    await flush();
    assert.equal(calls.length, 11, "30s again confirms the backoff was reset by the earlier success");
  } finally {
    await teardownWeb();
    teardownFetch();
  }
});

// ---------------------------------------------------------------------------
// $lib_version
// ---------------------------------------------------------------------------

test("the web $lib_version constant matches package.json's version", async () => {
  const fs = await import("node:fs/promises");
  const url = await import("node:url");
  const pkgPath = url.fileURLToPath(new URL("../package.json", import.meta.url));
  const pkg = JSON.parse(await fs.readFile(pkgPath, "utf8")) as { version: string };

  const store = await setupWeb();
  try {
    init({ apiKey: "phc_test" });
    await track("a");
    const queue = queueOf(store);
    assert.equal(queue[0]?.properties.$lib_version, pkg.version);
  } finally {
    await teardownWeb();
  }
});
