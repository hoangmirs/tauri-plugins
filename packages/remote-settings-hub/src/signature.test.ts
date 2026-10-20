import assert from "node:assert/strict";
import { createHmac } from "node:crypto";
import { test } from "node:test";
import { verifyEventSignature } from "./signature.ts";

const body = '{"event":"feature.updated"}';
const expected = createHmac("sha256", "ewhk_test").update(body).digest("hex");

test("an event signature is the hex HMAC-SHA256 of the body", async () => {
  assert.equal(await verifyEventSignature(body, expected, "ewhk_test"), true);
});

test("a signature in capitals still matches", async () => {
  assert.equal(await verifyEventSignature(body, expected.toUpperCase(), "ewhk_test"), true);
});

test("a changed body does not match", async () => {
  assert.equal(await verifyEventSignature(`${body} `, expected, "ewhk_test"), false);
});

test("a missing or empty signature does not match", async () => {
  assert.equal(await verifyEventSignature(body, null, "ewhk_test"), false);
  assert.equal(await verifyEventSignature(body, "", "ewhk_test"), false);
});

test("a secret set with a trailing newline still matches", async () => {
  assert.equal(await verifyEventSignature(body, expected, "ewhk_test\n"), true);
});

test("an empty secret never matches", async () => {
  const signed = createHmac("sha256", "").update(body).digest("hex");
  assert.equal(await verifyEventSignature(body, signed, ""), false);
});
