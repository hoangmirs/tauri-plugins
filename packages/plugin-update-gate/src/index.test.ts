import { test } from "node:test";
import assert from "node:assert/strict";

// `@tauri-apps/api/core`'s `isTauri()` reads `globalThis.isTauri` (a flag the
// Tauri runtime itself sets), and its `invoke()` reads
// `window.__TAURI_INTERNALS__.invoke(cmd, args, options)`. Under Node there
// is no `window`, so every "inside Tauri" test below stands one up, then
// tears it down — tests must not depend on execution order leaking globals
// between them.
type Globals = Record<string, unknown>;

function teardownTauri(): void {
  delete (globalThis as Globals).isTauri;
  delete (globalThis as Globals).window;
}

test("a browser with no Tauri runtime never gates", async () => {
  teardownTauri();
  const { checkUpdate } = await import("./index.ts");
  assert.deepEqual(await checkUpdate(), {
    state: "ok",
    latestVersion: null,
    message: null,
    url: null,
  });
});

test("under Tauri it returns what the command answered", async () => {
  (globalThis as Globals).isTauri = true;
  (globalThis as Globals).window = globalThis;
  (globalThis as Globals).__TAURI_INTERNALS__ = {
    invoke: async () => ({
      state: "forced",
      latestVersion: "1.4.0",
      message: "Cập nhật nha",
      url: "https://x",
    }),
  };
  try {
    const { checkUpdate } = await import("./index.ts?2");
    assert.equal((await checkUpdate()).state, "forced");
  } finally {
    teardownTauri();
  }
});

test("a thrown invoke fails open", async () => {
  (globalThis as Globals).isTauri = true;
  (globalThis as Globals).window = globalThis;
  (globalThis as Globals).__TAURI_INTERNALS__ = {
    invoke: async () => {
      throw new Error("no route to host");
    },
  };
  try {
    const { checkUpdate } = await import("./index.ts?3");
    assert.deepEqual(await checkUpdate(), {
      state: "ok",
      latestVersion: null,
      message: null,
      url: null,
    });
  } finally {
    teardownTauri();
  }
});

test("the caller's language is passed through to invoke", async () => {
  (globalThis as Globals).isTauri = true;
  (globalThis as Globals).window = globalThis;
  let received: unknown;
  (globalThis as Globals).__TAURI_INTERNALS__ = {
    invoke: async (_cmd: string, args: unknown) => {
      received = args;
      return { state: "ok", latestVersion: null, message: null, url: null };
    },
  };
  try {
    const { checkUpdate } = await import("./index.ts?4");
    await checkUpdate("vi");
    assert.equal((received as { lang?: string }).lang, "vi");
  } finally {
    teardownTauri();
  }
});

test("with no language given, the default comes from navigator.language", async () => {
  (globalThis as Globals).isTauri = true;
  (globalThis as Globals).window = globalThis;
  let received: unknown;
  (globalThis as Globals).__TAURI_INTERNALS__ = {
    invoke: async (_cmd: string, args: unknown) => {
      received = args;
      return { state: "ok", latestVersion: null, message: null, url: null };
    },
  };
  const originalNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  Object.defineProperty(globalThis, "navigator", {
    value: { language: "fr-FR" },
    configurable: true,
  });
  try {
    const { checkUpdate } = await import("./index.ts?5");
    await checkUpdate();
    assert.equal((received as { lang?: string }).lang, "fr-FR");
  } finally {
    teardownTauri();
    if (originalNavigator) {
      Object.defineProperty(globalThis, "navigator", originalNavigator);
    } else {
      delete (globalThis as Globals).navigator;
    }
  }
});
