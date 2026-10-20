import assert from "node:assert/strict";
import { test } from "node:test";
import { type AppEntry, checkApps, splitPayload } from "./split.ts";

const app = (prefix: string): AppEntry => ({ prefix, paths: [prefix.slice(0, -1)], keys: {} });

test("each feature goes to its app, without its prefix", () => {
  const p = { features: { "appa.x": { defaultValue: 1 }, "appb.x": { defaultValue: 2 }, y: {} }, dateUpdated: "d" };
  const { byPrefix, unknown } = splitPayload(p, ["appa.", "appb."]);
  assert.deepEqual(byPrefix.get("appa.")?.features, { x: { defaultValue: 1 } });
  assert.deepEqual(byPrefix.get("appb.")?.features, { x: { defaultValue: 2 } });
  assert.equal(byPrefix.get("appa.")?.dateUpdated, "d");
  assert.deepEqual(unknown, ["y"]);
});

test("an app gets only the saved groups its features use", () => {
  const p = {
    features: {
      "appa.x": { defaultValue: 0, rules: [{ condition: { id: { $inGroup: "grp_a" } }, force: 1 }] },
      "appb.x": { defaultValue: 0, rules: [{ condition: { id: { $notInGroup: "grp_b" } }, force: 1 }] },
    },
    savedGroups: { grp_a: ["1"], grp_b: ["2"], grp_c: ["3"] },
  };
  const { byPrefix } = splitPayload(p, ["appa.", "appb."]);
  assert.deepEqual(byPrefix.get("appa.")?.savedGroups, { grp_a: ["1"] });
  assert.deepEqual(byPrefix.get("appb.")?.savedGroups, { grp_b: ["2"] });
});

test("a saved group named deep inside $and and $or is found", () => {
  const p = {
    features: {
      "appa.x": {
        rules: [{ condition: { $and: [{ country: "VN" }, { $or: [{ id: { $inGroup: "grp_deep" } }] }] } }],
      },
    },
    savedGroups: { grp_deep: ["1"], grp_other: ["2"] },
  };
  assert.deepEqual(splitPayload(p, ["appa."]).byPrefix.get("appa.")?.savedGroups, { grp_deep: ["1"] });
});

test("a saved group the payload does not hold is left out", () => {
  const p = { features: { "appa.x": { rules: [{ condition: { id: { $inGroup: "gone" } } }] } }, savedGroups: {} };
  assert.deepEqual(splitPayload(p, ["appa."]).byPrefix.get("appa.")?.savedGroups, {});
});

test("an app's experiments list is empty", () => {
  const p = { features: {}, experiments: [{ key: "visual" }] };
  assert.deepEqual(splitPayload(p, ["appa."]).byPrefix.get("appa.")?.experiments, []);
});

test("an app with no feature still gets a payload, with no features", () => {
  assert.deepEqual(splitPayload({ features: {} }, ["appa."]).byPrefix.get("appa.")?.features, {});
});

test("a feature named only by the prefix goes to no app", () => {
  assert.deepEqual(splitPayload({ features: { "appa.": {} } }, ["appa."]).unknown, ["appa."]);
});

test("checkApps refuses a prefix that starts another", () => {
  assert.notDeepEqual(checkApps([app("appa."), app("appa.pro.")]), []);
});

test("checkApps refuses an empty prefix, one with no dot at the end, and one used twice", () => {
  assert.notDeepEqual(checkApps([{ prefix: "", paths: ["x"], keys: {} }]), []);
  assert.notDeepEqual(checkApps([app("appa"), app("appb.")]), []);
  assert.notDeepEqual(checkApps([app("appa."), app("appa.")]), []);
});

test("checkApps refuses an app with no path", () => {
  assert.notDeepEqual(checkApps([{ prefix: "appa.", paths: [], keys: {} }]), []);
});

test("checkApps accepts apps with distinct prefixes", () => {
  assert.deepEqual(checkApps([app("appa."), app("appb.")]), []);
});

test("checkApps refuses two apps that share a path", () => {
  assert.notDeepEqual(checkApps([{ prefix: "appa.", paths: ["x"], keys: {} }, { prefix: "appb.", paths: ["x"], keys: {} }]), []);
  assert.notDeepEqual(checkApps([{ prefix: "appa.", paths: ["x", "x"], keys: {} }]), []);
});
