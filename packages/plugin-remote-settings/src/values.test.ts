import assert from "node:assert/strict";
import { test } from "node:test";
import type { Saved } from "./types.ts";
import { asText, globalPart, nextSaved, pluginKeys, valuesOf } from "./values.ts";

const readJson = (t: string): unknown => {
  try {
    return JSON.parse(t) as unknown;
  } catch {
    return undefined;
  }
};
const keys = {
  notice: { fallback: null as unknown, read: readJson, global: true },
  lab_x: { fallback: "off", read: (t: string) => (["off", "lab", "on"].includes(t) ? t : undefined) },
};
const saved = (over: Partial<Saved> = {}): Saved => ({ global: {}, source: {}, from: "growthbook", country: null, ...over });

test("asText keeps a string and writes anything else as JSON", () => {
  assert.equal(asText("on"), "on");
  assert.equal(asText({ a: 1 }), '{"a":1}');
  assert.equal(asText(true), "true");
});

test("globalPart keeps the global keys and the plugin keys, as text", () => {
  assert.deepEqual(globalPart({ notice: { a: 1 }, source: "firebase", apply: "rest", lab_x: "on", other: 1 }, keys), {
    notice: '{"a":1}',
    source: "firebase",
    apply: "rest",
  });
});

test("globalPart of something that is not an object is null", () => {
  assert.equal(globalPart([], keys), null);
  assert.equal(globalPart(null, keys), null);
  assert.equal(globalPart("x", keys), null);
});

test("pluginKeys reads source and apply, and refuses other words", () => {
  assert.deepEqual(pluginKeys({ source: "firebase", apply: "rest" }), { source: "firebase", apply: "rest" });
  assert.deepEqual(pluginKeys({ source: "x", apply: "later" }), { source: null, apply: null });
  assert.deepEqual(pluginKeys({}), { source: null, apply: null });
});

test("a global value that does not read keeps its saved value, and the others apply", () => {
  const prev = saved({ global: { notice: '{"a":1}' } });
  const next = nextSaved(prev, { global: { notice: "{bad", source: "growthbook" }, source: null, from: "growthbook", country: "VN" }, keys);
  assert.equal(next.global.notice, '{"a":1}');
  assert.equal(next.global.source, "growthbook");
  assert.equal(next.country, "VN");
});

test("a plugin key with a bad word keeps its saved word", () => {
  const prev = saved({ global: { apply: "rest" } });
  const next = nextSaved(prev, { global: { apply: "later" }, source: null, from: "growthbook", country: null }, keys);
  assert.equal(next.global.apply, "rest");
});

test("a key the new global file leaves out is gone", () => {
  const prev = saved({ global: { notice: '{"a":1}' } });
  const next = nextSaved(prev, { global: {}, source: null, from: "growthbook", country: null }, keys);
  assert.deepEqual(next.global, {});
});

test("a part that failed keeps what was saved", () => {
  const prev = saved({ global: { notice: "1" }, source: { lab_x: "on" }, from: "firebase" });
  const next = nextSaved(prev, { global: null, source: null, from: "growthbook", country: null }, keys);
  assert.deepEqual(next.global, { notice: "1" });
  assert.deepEqual(next.source, { lab_x: "on" });
  assert.equal(next.from, "firebase");
});

test("a fresh source part replaces the saved one and names its source", () => {
  const prev = saved({ source: { lab_x: "on" }, from: "firebase" });
  const next = nextSaved(prev, { global: null, source: { lab_x: "lab" }, from: "growthbook", country: null }, keys);
  assert.deepEqual(next.source, { lab_x: "lab" });
  assert.equal(next.from, "growthbook");
});

test("a read with no country keeps the saved country", () => {
  const next = nextSaved(saved({ country: "VN" }), { global: null, source: null, from: "growthbook", country: null }, keys);
  assert.equal(next.country, "VN");
});

test("nothing saved and nothing read gives empty parts", () => {
  assert.deepEqual(nextSaved(null, { global: null, source: null, from: "growthbook", country: null }, keys), saved());
});

test("each value says where it came from", () => {
  const v = valuesOf(saved({ global: { notice: '{"a":1}' }, source: { lab_x: "on" } }), keys, true);
  assert.deepEqual(v.values, { notice: { a: 1 }, lab_x: "on" });
  assert.deepEqual(v.from, { notice: "global", lab_x: "growthbook" });
  assert.deepEqual([...v.sent].sort(), ["lab_x", "notice"]);
});

test("a source key in the global part is ignored", () => {
  const v = valuesOf(saved({ global: { lab_x: "on" } }), keys, true);
  assert.equal(v.values.lab_x, "off");
  assert.equal(v.from.lab_x, "default");
});

test("a global key from a source is ignored", () => {
  const v = valuesOf(saved({ source: { notice: '{"a":1}' } }), keys, true);
  assert.equal(v.values.notice, null);
  assert.equal(v.from.notice, "default");
});

test("a value that does not read gives the fallback and is not sent", () => {
  const v = valuesOf(saved({ source: { lab_x: "maybe" } }), keys, true);
  assert.equal(v.values.lab_x, "off");
  assert.equal(v.from.lab_x, "default");
  assert.deepEqual(v.sent, []);
});

test("values from an earlier launch say saved", () => {
  const v = valuesOf(saved({ global: { notice: "1" }, source: { lab_x: "on" } }), keys, false);
  assert.deepEqual(v.from, { notice: "saved", lab_x: "saved" });
});

test("no saved values give every fallback", () => {
  const v = valuesOf(null, keys, false);
  assert.deepEqual(v.values, { notice: null, lab_x: "off" });
  assert.deepEqual(v.from, { notice: "default", lab_x: "default" });
});

test("a reader that throws gives the fallback, and nothing throws", () => {
  const throwing = { lab_x: { fallback: "off", read: (_t: string): string | undefined => { throw new Error("bad reader"); } } };
  const v = valuesOf(saved({ source: { lab_x: "on" } }), throwing, true);
  assert.equal(v.values.lab_x, "off");
  const g = { gate: { fallback: null, global: true, read: (_t: string): unknown => { throw new Error("bad"); } } };
  const next = nextSaved(saved({ global: { gate: "1" } }), { global: { gate: "2" }, source: null, from: "growthbook", country: null }, g);
  assert.equal(next.global.gate, "1");
});
