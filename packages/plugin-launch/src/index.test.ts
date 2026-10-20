import assert from "node:assert/strict";
import { test } from "node:test";
import { ready } from "./index.ts";

type Globals = Record<string, unknown>;

test("outside Tauri ready does nothing and does not reject", async () => {
  assert.equal(await ready(), undefined);
});

test("under Tauri ready asks the plugin's ready command once", async () => {
  const calls: string[] = [];
  (globalThis as Globals).isTauri = true;
  (globalThis as Globals).window = globalThis;
  (globalThis as Globals).__TAURI_INTERNALS__ = { invoke: async (cmd: string) => void calls.push(cmd) };
  try {
    await ready();
    assert.deepEqual(calls, ["plugin:launch|ready"]);
  } finally {
    delete (globalThis as Globals).isTauri;
    delete (globalThis as Globals).window;
    delete (globalThis as Globals).__TAURI_INTERNALS__;
  }
});

test("a command that fails does not reject", async () => {
  (globalThis as Globals).isTauri = true;
  (globalThis as Globals).window = globalThis;
  (globalThis as Globals).__TAURI_INTERNALS__ = {
    invoke: async () => {
      throw new Error("plugin launch not found");
    },
  };
  try {
    assert.equal(await ready(), undefined);
  } finally {
    delete (globalThis as Globals).isTauri;
    delete (globalThis as Globals).window;
    delete (globalThis as Globals).__TAURI_INTERNALS__;
  }
});
