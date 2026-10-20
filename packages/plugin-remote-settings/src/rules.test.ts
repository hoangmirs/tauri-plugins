import assert from "node:assert/strict";
import { test } from "node:test";
// The hub's source, not its package: CI runs the tests before any package is built.
import { encryptPayload } from "../../remote-settings-hub/src/payload.ts";
import { type Attributes, evaluate } from "./rules.ts";

const KEY = Buffer.alloc(16, 3).toString("base64");
const OTHER = Buffer.alloc(16, 9).toString("base64");

const attrs = (over: Partial<Attributes> = {}): Attributes => ({
  id: "install-1",
  platform: "ios",
  web: false,
  version: "1.4.0",
  country: "VN",
  language: "vi",
  now: "2026-10-20T00:00:00.000Z",
  ...over,
});

const keep = (t: string) => t;
const keys = {
  list: { fallback: "", read: keep },
  lab_x: { fallback: "off", read: keep },
  notice: { fallback: "", read: keep, global: true },
};

const experiment = {
  id: "r1",
  key: "exp1",
  hashAttribute: "id",
  coverage: 1,
  weights: [0, 1],
  variations: ["off", "on"],
  meta: [{ key: "0" }, { key: "1" }],
};

const file = async (features: Record<string, unknown>, key = KEY) =>
  JSON.stringify(await encryptPayload({ features, savedGroups: {} }, key));

test("a rule on country gives its value, and the default elsewhere", async () => {
  const text = await file({ list: { defaultValue: "all", rules: [{ condition: { country: "VN" }, force: "vn" }] } });
  assert.equal((await evaluate(text, attrs({ country: "VN" }), KEY, keys))?.part.list, "vn");
  assert.equal((await evaluate(text, attrs({ country: "US" }), KEY, keys))?.part.list, "all");
});

test("only the app's own keys are evaluated", async () => {
  const text = await file({ lab_x: { defaultValue: "on" }, lab_new: { defaultValue: "off", rules: [experiment] } });
  const r = await evaluate(text, attrs(), KEY, keys);
  assert.deepEqual(Object.keys(r?.part ?? {}), ["lab_x"]);
  assert.deepEqual(r?.assignments, []);
});

test("a global key is never taken from the rules", async () => {
  const r = await evaluate(await file({ notice: { defaultValue: "x" } }), attrs(), KEY, keys);
  assert.deepEqual(r?.part, {});
});

test("an experiment rule gives its variant and an assignment for its key", async () => {
  const r = await evaluate(await file({ lab_x: { defaultValue: "off", rules: [experiment] } }), attrs(), KEY, keys);
  assert.equal(r?.part.lab_x, "on");
  assert.deepEqual(r?.assignments, [{ key: "lab_x", experiment: "exp1", variant: "1" }]);
});

test("a file for another key gives null", async () => {
  assert.equal(await evaluate(await file({ lab_x: { defaultValue: "on" } }), attrs(), OTHER, keys), null);
});

test("text that is not JSON, or JSON with no features, gives null", async () => {
  assert.equal(await evaluate("not json", attrs(), KEY, keys), null);
  assert.equal(await evaluate("[]", attrs(), KEY, keys), null);
  assert.equal(await evaluate('{"encryptedFeatures":"nope"}', attrs(), KEY, keys), null);
  assert.equal(await evaluate(null, attrs(), KEY, keys), null);
});

test("an attribute that is null is left out, so a rule on it does not match", async () => {
  const text = await file({ list: { defaultValue: "all", rules: [{ condition: { country: { $exists: false } }, force: "none" }] } });
  assert.equal((await evaluate(text, attrs({ country: null }), KEY, keys))?.part.list, "none");
});

test("an extra attribute can be tested", async () => {
  const text = await file({ list: { defaultValue: "free", rules: [{ condition: { plan: "paid" }, force: "paid" }] } });
  assert.equal((await evaluate(text, attrs({ plan: "paid" }), KEY, keys))?.part.list, "paid");
});

test("a JSON value comes back as JSON text", async () => {
  const r = await evaluate(await file({ list: { defaultValue: { a: 1 } } }), attrs(), KEY, keys);
  assert.equal(r?.part.list, '{"a":1}');
});

test("a feature with a null value is left out", async () => {
  const r = await evaluate(await file({ list: { defaultValue: null } }), attrs(), KEY, keys);
  assert.deepEqual(r?.part, {});
});
