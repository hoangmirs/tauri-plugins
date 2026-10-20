import assert from "node:assert/strict";
import { test } from "node:test";
import { buildGlobal } from "./global.ts";

const readJson = (t: string) => {
  try {
    return JSON.parse(t) as unknown;
  } catch {
    return undefined;
  }
};
const keys = {
  notice: { read: readJson, global: true },
  gate: { read: readJson, global: true },
  lab_x: { read: (t: string) => (t === "on" || t === "off" ? t : undefined) },
};

test("a good file builds", () => {
  assert.deepEqual(buildGlobal({ source: "growthbook", notice: { a: 1 } }, keys), {
    ok: true,
    body: '{"source":"growthbook","notice":{"a":1}}\n',
  });
});

test("the plugin keys take their own words", () => {
  assert.equal(buildGlobal({ source: "firebase", apply: "rest" }, keys).ok, true);
  assert.equal(buildGlobal({ apply: "now" }, keys).ok, true);
});

test("a key that is not global, an unknown key and a bad value are each refused", () => {
  assert.deepEqual(buildGlobal({ lab_x: "on", nope: 1, notice: "{not json", apply: "later" }, keys), {
    ok: false,
    bad: ["lab_x", "nope", "notice", "apply"],
  });
});

test("a source the plugin does not know is refused", () => {
  assert.deepEqual(buildGlobal({ source: "other" }, keys), { ok: false, bad: ["source"] });
});

test("an app key named like a plugin key is still checked as the plugin key", () => {
  const clash = { ...keys, apply: { read: (t: string) => t, global: true } };
  assert.deepEqual(buildGlobal({ apply: "anything" }, clash), { ok: false, bad: ["apply"] });
});
