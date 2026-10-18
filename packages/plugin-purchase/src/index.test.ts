import assert from "node:assert/strict";
import { test } from "node:test";
import { mockIPC } from "@tauri-apps/api/mocks";
import { buy, owned, products, restore } from "./index.ts";

test("outside Tauri the store cannot be asked, nothing has a price, and a purchase fails without rejecting", async () => {
  assert.deepEqual(await products(["app.pro"]), []);
  assert.equal(await owned(["app.pro"]), null);
  assert.equal(await restore(["app.pro"]), null);
  assert.deepEqual(await buy("app.pro"), { state: "failed" });
});

test("a store that answers gives what it holds among the ids, and a store that errs gives null", async () => {
  const g = globalThis as Record<string, unknown>;
  g.window = globalThis;
  g.isTauri = true;
  let down = false;
  mockIPC((cmd) => {
    if (down) throw new Error("Play answered 2");
    if (cmd === "plugin:purchase|owned" || cmd === "plugin:purchase|restore")
      return { owned: [{ id: "app.pro", proof: "tok" }, { id: "app.other", proof: "x" }, { id: "app.pro" }] };
    return null;
  });
  try {
    assert.deepEqual(await owned(["app.pro"]), [{ id: "app.pro", proof: "tok" }]);
    assert.deepEqual(await restore(["app.none"]), []);
    down = true;
    assert.equal(await owned(["app.pro"]), null, "an error is no answer, not nothing owned");
  } finally {
    delete g.isTauri;
    delete g.window;
  }
});
