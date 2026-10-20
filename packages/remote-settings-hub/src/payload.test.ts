import assert from "node:assert/strict";
import { test } from "node:test";
import { GrowthBook } from "@growthbook/growthbook";
import { encryptPayload, readPayload, sha256Hex } from "./payload.ts";

const newKey = () => btoa(String.fromCharCode(...crypto.getRandomValues(new Uint8Array(16))));

test("a payload needs a features object", () => {
  assert.equal(readPayload(null), null);
  assert.equal(readPayload({}), null);
  assert.equal(readPayload({ features: [] }), null);
  assert.equal(readPayload({ features: {}, experiments: {} }), null);
  assert.equal(readPayload({ features: {}, savedGroups: [] }), null);
  assert.deepEqual(readPayload({ features: {} }), { features: {} });
});

test("the GrowthBook SDK decrypts what encryptPayload writes", async () => {
  const key = newKey();
  const enc = await encryptPayload(
    { features: { a: { defaultValue: "on" } }, savedGroups: {}, dateUpdated: "d" },
    `${key}\n`,
  );
  assert.equal(enc.dateUpdated, "d");
  assert.equal("features" in enc, false);
  const gb = new GrowthBook({ decryptionKey: key });
  await gb.setPayload(enc as Parameters<GrowthBook["setPayload"]>[0]);
  assert.equal(gb.getFeatureValue("a", "off"), "on");
  gb.destroy();
});

test("two encryptions of one payload differ", async () => {
  const key = newKey();
  const p = { features: { a: { defaultValue: 1 } } };
  const one = await encryptPayload(p, key);
  const two = await encryptPayload(p, key);
  assert.notEqual(one.encryptedFeatures, two.encryptedFeatures);
});

test("sha256Hex gives the hex digest of the text", async () => {
  assert.equal(await sha256Hex("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
});

test("a payload larger than the call stack's argument limit still encrypts", async () => {
  const key = newKey();
  const features: Record<string, unknown> = {};
  for (let i = 0; i < 4000; i++) features[`f${i}`] = { defaultValue: "x".repeat(60) };
  const enc = await encryptPayload({ features }, key);
  const gb = new GrowthBook({ decryptionKey: key });
  await gb.setPayload(enc as Parameters<GrowthBook["setPayload"]>[0]);
  assert.equal(gb.getFeatureValue("f3999", "none"), "x".repeat(60));
  gb.destroy();
});
