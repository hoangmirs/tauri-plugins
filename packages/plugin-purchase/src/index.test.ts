import assert from "node:assert/strict";
import { test } from "node:test";
import { mockIPC } from "@tauri-apps/api/mocks";
import { buy, finish, onArrived, owned, products, restore, unfinished } from "./index.ts";

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

test("a consumable keeps its tx, is finished by tx, and unfinished lists what is open among the ids", async () => {
  const g = globalThis as Record<string, unknown>;
  g.window = globalThis;
  g.isTauri = true;
  const seen: Record<string, unknown>[] = [];
  let down = false;
  mockIPC((cmd, args) => {
    seen.push({ cmd, ...(args as object) });
    if (down) throw new Error("Play answered 2");
    if (cmd === "plugin:purchase|buy") {
      return (args as { id: string }).id === "app.pack"
        ? { state: "bought", proof: "jws", tx: "42" }
        : { state: "bought", proof: "jws" };
    }
    if (cmd === "plugin:purchase|finish") return {};
    if (cmd === "plugin:purchase|unfinished")
      return { open: [{ id: "app.pack", tx: "42" }, { id: "app.other", tx: "7" }, { id: "app.pack" }] };
    return null;
  });
  try {
    assert.deepEqual(await buy("app.pack", { consumable: true }), { state: "bought", proof: "jws", tx: "42" });
    assert.deepEqual(seen[0], { cmd: "plugin:purchase|buy", id: "app.pack", consumable: true });
    assert.deepEqual(await buy("app.pro"), { state: "bought", proof: "jws" });
    assert.deepEqual(seen[1], { cmd: "plugin:purchase|buy", id: "app.pro" });
    assert.deepEqual(await buy("app.nope", { consumable: true }), { state: "failed" }, "no tx cannot be finished");
    assert.equal(await finish("42"), true);
    assert.deepEqual(await unfinished(["app.pack"]), [{ id: "app.pack", tx: "42" }]);
    down = true;
    assert.equal(await finish("42"), false);
    assert.equal(await unfinished(["app.pack"]), null);
  } finally {
    delete g.isTauri;
    delete g.window;
  }
});

test("outside Tauri nothing is open, nothing finishes and nothing arrives", async () => {
  assert.equal(await finish("42"), false);
  assert.equal(await unfinished(["app.pack"]), null);
  const stop = await onArrived(() => assert.fail("no event outside Tauri"));
  stop();
});
