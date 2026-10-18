import assert from "node:assert/strict";
import { test } from "node:test";
import { buy, owned, products, restore } from "./index.ts";

test("outside Tauri nothing is owned, nothing has a price, and a purchase fails without rejecting", async () => {
  assert.deepEqual(await products(["app.pro"]), []);
  assert.deepEqual(await owned(["app.pro"]), []);
  assert.deepEqual(await restore(["app.pro"]), []);
  assert.deepEqual(await buy("app.pro"), { state: "failed" });
});
