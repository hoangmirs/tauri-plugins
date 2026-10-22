import assert from "node:assert/strict";
import { test } from "node:test";
import { keepAwake } from "./index.ts";

test("outside Tauri keeping the screen on does nothing and does not reject", async () => {
  assert.equal(await keepAwake(true), undefined);
  assert.equal(await keepAwake(false), undefined);
});
