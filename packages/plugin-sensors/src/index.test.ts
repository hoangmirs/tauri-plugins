import assert from "node:assert/strict";
import { test } from "node:test";
import { available, lockRotation, onReading, readingOf, start, stop, unlockRotation } from "./index.ts";

test("outside Tauri the device has no sensors", async () => {
  assert.deepEqual(await available(), []);
});

test("outside Tauri listening gives a function that stops nothing", async () => {
  const stopListening = await onReading(() => assert.fail("called outside Tauri"));
  assert.equal(typeof stopListening, "function");
  stopListening();
});

test("outside Tauri the other calls resolve and do nothing", async () => {
  await start("motion", 30);
  await stop("motion");
  await lockRotation();
  await unlockRotation();
});

test("a motion and a heading reading are read as themselves", () => {
  const motion = { kind: "motion", g: [0, 0, -1], r: [0.1, 0.2, 0.3], a: [0, 0, 0], at: 12.5 };
  assert.deepEqual(readingOf(motion), motion);
  assert.deepEqual(readingOf({ kind: "heading", deg: 90, at: 3 }), { kind: "heading", deg: 90, at: 3 });
});

test("anything the bridge sends that is not a reading is dropped", () => {
  for (const raw of [
    undefined,
    null,
    "motion",
    {},
    { kind: "light", v: 3, at: 1 },
    { kind: "heading", deg: "90", at: 1 },
    { kind: "heading", deg: 90 },
    { kind: "motion", g: [0, 0], r: [0, 0, 0], a: [0, 0, 0], at: 1 },
    { kind: "motion", g: [0, 0, "1"], r: [0, 0, 0], a: [0, 0, 0], at: 1 },
  ])
    assert.equal(readingOf(raw), null, JSON.stringify(raw));
});
