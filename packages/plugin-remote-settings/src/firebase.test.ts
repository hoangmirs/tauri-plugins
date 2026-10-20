import assert from "node:assert/strict";
import { test } from "node:test";
import { firebaseAdapter } from "./firebase.ts";

const api = (over: Record<string, unknown> = {}) =>
  ({
    getAll: async () => ({ a: { value: "1", source: "remote" }, b: { value: "2", source: "default" } }),
    fetch: async () => "fetched",
    activate: async () => true,
    listen: async (on: (keys: string[]) => void) => {
      on(["a"]);
      return () => {};
    },
    ...over,
  }) as unknown as Parameters<typeof firebaseAdapter>[0];

const boom = async () => {
  throw new Error("no firebase");
};

test("only the values Remote Config sent are given", async () => {
  assert.deepEqual(await firebaseAdapter(api()).values(), { a: "1" });
});

test("fetch is true only when new values came", async () => {
  assert.equal(await firebaseAdapter(api()).fetch(), true);
  assert.equal(await firebaseAdapter(api({ fetch: async () => "throttled" })).fetch(), false);
  assert.equal(await firebaseAdapter(api({ fetch: async () => "failed" })).fetch(), false);
});

test("listen passes each heard update on", async () => {
  let heard = 0;
  const stop = await firebaseAdapter(api()).listen(() => heard++);
  assert.equal(heard, 1);
  stop();
});

test("a failing Firebase call never throws", async () => {
  const a = firebaseAdapter(api({ getAll: boom, fetch: boom, activate: boom, listen: boom }));
  assert.equal(await a.values(), null);
  assert.equal(await a.fetch(), false);
  assert.equal(await a.activate(), false);
  const stop = await a.listen(() => {});
  stop();
});
