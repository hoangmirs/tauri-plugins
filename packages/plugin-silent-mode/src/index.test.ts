import assert from "node:assert/strict";
import { test } from "node:test";
import { isQuiet, modeOf, onSilentModeChange, silentMode } from "./index.ts";

test("each mode the plugin sends is read as itself", () => {
  assert.equal(modeOf({ mode: "silent" }), "silent");
  assert.equal(modeOf({ mode: "vibrate" }), "vibrate");
  assert.equal(modeOf({ mode: "normal" }), "normal");
});

test("anything the bridge sends that is not a mode stays audible", () => {
  for (const raw of [undefined, null, "silent", {}, { mode: 1 }, { mode: "loud" }])
    assert.equal(modeOf(raw), "normal", JSON.stringify(raw));
});

test("silent and vibrate keep sounds off, and normal plays them", () => {
  assert.equal(isQuiet("silent"), true);
  assert.equal(isQuiet("vibrate"), true);
  assert.equal(isQuiet("normal"), false);
});

test("outside Tauri the phone reads as normal and never changes", async () => {
  assert.equal(await silentMode(), "normal");
  const stop = await onSilentModeChange(() => assert.fail("called outside Tauri"));
  stop();
});
