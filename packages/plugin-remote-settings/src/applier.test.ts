import assert from "node:assert/strict";
import { test } from "node:test";
import { createApplier, type Snapshot } from "./applier.ts";
import type { Assignment, Mode } from "./types.ts";

const snap = (lab: string, assignments: Assignment[] = []): Snapshot => ({
  saved: { global: {}, source: { lab_x: lab }, from: "growthbook", country: null },
  used: "growthbook",
  assignments,
});

function make(mode: Mode, start: Snapshot | null = snap("off")) {
  const applies: { s: Snapshot; changed: string[] }[] = [];
  const reported: string[] = [];
  const a = createApplier({
    start,
    mode: () => mode,
    onApply: (s, changed) => applies.push({ s, changed }),
    onExperiment: (e, v) => reported.push(`${e}:${v}`),
  });
  const lab = () => a.applied()?.saved.source.lab_x;
  return { a, applies, reported, lab };
}

type Case = { name: string; setup: (a: ReturnType<typeof make>["a"]) => Promise<void>; act: (a: ReturnType<typeof make>["a"]) => Promise<void> };
const cases: Case[] = [
  {
    name: "landed while holding",
    setup: async (a) => a.holding(),
    act: async (a) => a.landed(snap("on")),
  },
  {
    name: "landed while out of sight",
    setup: async (a) => a.setVisible(false),
    act: async (a) => a.landed(snap("on")),
  },
  {
    name: "landed in sight",
    setup: async () => {},
    act: async (a) => a.landed(snap("on")),
  },
  {
    name: "landed in sight at rest",
    setup: async (a) => a.setAtRest(true),
    act: async (a) => a.landed(snap("on")),
  },
  {
    name: "hidden with pending",
    setup: async (a) => a.landed(snap("on")),
    act: async (a) => a.setVisible(false),
  },
  {
    name: "rest with pending",
    setup: async (a) => {
      await a.setAtRest(false);
      await a.landed(snap("on"));
    },
    act: async (a) => a.setAtRest(true),
  },
];
const want: Record<string, Record<Mode, boolean>> = {
  "landed while holding": { hidden: true, rest: true, now: true },
  "landed while out of sight": { hidden: true, rest: true, now: true },
  "landed in sight": { hidden: false, rest: false, now: true },
  "landed in sight at rest": { hidden: false, rest: true, now: true },
  "hidden with pending": { hidden: true, rest: true, now: true },
  "rest with pending": { hidden: false, rest: true, now: true },
};

for (const c of cases) {
  for (const mode of ["hidden", "rest", "now"] as const) {
    test(`${c.name}, mode ${mode}: ${want[c.name]?.[mode] ? "applies" : "waits"}`, async () => {
      const { a, lab } = make(mode);
      await c.setup(a);
      await c.act(a);
      assert.equal(lab(), want[c.name]?.[mode] ? "on" : "off");
    });
  }
}

test("rest acts as hidden until the app first calls setAtRest", async () => {
  const { a, lab } = make("rest");
  await a.landed(snap("on"));
  assert.equal(lab(), "off");
  await a.setVisible(false);
  assert.equal(lab(), "on");
});

test("a value that waits is pending", async () => {
  const { a } = make("hidden");
  await a.landed(snap("on"));
  assert.equal(a.pending()?.saved.source.lab_x, "on");
  await a.setVisible(false);
  assert.equal(a.pending(), null);
});

test("applyNow with keys applies only those keys", async () => {
  const { a } = make("hidden", { ...snap("off"), saved: { ...snap("off").saved, global: { gate: "1" } } });
  await a.landed({ ...snap("on"), saved: { ...snap("on").saved, global: { gate: "2" } } });
  await a.applyNow(["gate"]);
  assert.equal(a.applied()?.saved.global.gate, "2");
  assert.equal(a.applied()?.saved.source.lab_x, "off");
  assert.equal(a.pending()?.saved.source.lab_x, "on");
});

test("applyNow with a key the new values drop removes it", async () => {
  const { a } = make("hidden", { ...snap("off"), saved: { ...snap("off").saved, global: { gate: "1" } } });
  await a.landed(snap("off"));
  await a.applyNow(["gate"]);
  assert.equal(a.applied()?.saved.global.gate, undefined);
});

test("applyNow with no keys applies everything pending", async () => {
  const { a, lab } = make("hidden");
  await a.landed(snap("on"));
  await a.applyNow();
  assert.equal(lab(), "on");
});

test("changed lists only the keys whose text changed", async () => {
  const start: Snapshot = { ...snap("off"), saved: { ...snap("off").saved, global: { gate: "1", same: "s" } } };
  const { a, applies } = make("now", start);
  await a.landed({ ...snap("on"), saved: { ...snap("on").saved, global: { same: "s", fresh: "f" } } });
  assert.deepEqual([...(applies[0]?.changed ?? [])].sort(), ["fresh", "gate", "lab_x"]);
});

const exp: Assignment = { key: "lab_x", experiment: "exp1", variant: "1" };

test("a test group is reported when its value applies, not when it lands", async () => {
  const { a, reported } = make("hidden");
  await a.landed(snap("on", [exp]));
  assert.deepEqual(reported, []);
  await a.setVisible(false);
  assert.deepEqual(reported, ["exp1:1"]);
});

test("a test group is reported once, and again when its variant changes", async () => {
  const { a, reported } = make("now");
  await a.landed(snap("on", [exp]));
  await a.landed(snap("on", [exp]));
  await a.landed(snap("off", [{ ...exp, variant: "0" }]));
  assert.deepEqual(reported, ["exp1:1", "exp1:0"]);
});

test("applyNow reports only the groups of the keys it applies", async () => {
  const { a, reported } = make("hidden");
  await a.landed(snap("on", [exp, { key: "other", experiment: "exp2", variant: "1" }]));
  await a.applyNow(["lab_x"]);
  assert.deepEqual(reported, ["exp1:1"]);
});

test("a pending value that never applies reports nothing", async () => {
  const { a, reported } = make("hidden");
  await a.landed(snap("on", [exp]));
  await a.landed(snap("off"));
  await a.setVisible(false);
  assert.deepEqual(reported, []);
});

test("the start snapshot's groups are reported at creation", () => {
  const { reported } = make("hidden", snap("on", [exp]));
  assert.deepEqual(reported, ["exp1:1"]);
});

test("holdEnded applies what waits, in every mode", async () => {
  for (const mode of ["hidden", "rest", "now"] as const) {
    const { a, lab } = make(mode);
    await a.landed(snap("on"));
    a.holding();
    await a.holdEnded();
    assert.equal(lab(), "on");
  }
});

test("after holdEnded a read in sight waits again", async () => {
  const { a, lab } = make("hidden");
  a.holding();
  await a.holdEnded();
  await a.landed(snap("on"));
  assert.equal(lab(), "off");
});

test("beforeApply can replace the values that apply", async () => {
  const a = createApplier({
    start: null,
    mode: () => "now",
    beforeApply: async (s) => ({ ...s, used: "firebase" }),
    onApply: () => {},
    onExperiment: () => {},
  });
  await a.landed(snap("on"));
  assert.equal(a.applied()?.used, "firebase");
});

test("with nothing saved, the first read applies onto nothing", async () => {
  const { a, applies } = make("now", null);
  assert.equal(a.applied(), null);
  await a.landed(snap("on"));
  assert.deepEqual(applies[0]?.changed, ["lab_x"]);
});
