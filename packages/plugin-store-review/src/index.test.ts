import assert from "node:assert/strict";
import { test } from "node:test";
import { requestReview } from "./index.ts";

test("outside Tauri an ask does nothing and does not reject", async () => {
  assert.equal(await requestReview(), undefined);
});
