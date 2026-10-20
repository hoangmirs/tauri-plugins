import assert from "node:assert/strict";
import { createHmac } from "node:crypto";
import { test } from "node:test";
import { GrowthBook } from "@growthbook/growthbook";
import { type Bucket, createHub, type HubConfig, type HubEnv } from "./hub.ts";
import type { AppEntry } from "./split.ts";

const newKey = () => btoa(String.fromCharCode(...crypto.getRandomValues(new Uint8Array(16))));

type Put = { key: string; body: string; opts: unknown };

function fakeBucket(): Bucket & { files: Map<string, string>; puts: Put[] } {
  const files = new Map<string, string>();
  const puts: Put[] = [];
  return {
    files,
    puts,
    async get(key) {
      const v = files.get(key);
      return v === undefined ? null : { text: async () => v };
    },
    async put(key, body, opts) {
      puts.push({ key, body, opts });
      files.set(key, body);
    },
  };
}

const payloads = new Map<string, unknown>();
let calls: string[] = [];
const fakeFetch = (async (input: RequestInfo | URL) => {
  const url = String(input);
  calls.push(url);
  const key = url.split("/").pop() ?? "";
  const body = payloads.get(key);
  if (body === undefined) return new Response("nope", { status: 500 });
  return new Response(typeof body === "string" ? body : JSON.stringify(body), { status: 200 });
}) as typeof fetch;

const keys = { aProd: newKey(), aStage: newKey(), bProd: newKey(), bStage: newKey() };
const appA: AppEntry = {
  prefix: "appa.",
  paths: ["a"],
  keys: { production: [{ secret: "KEY_A_PROD", version: 1 }], staging: [{ secret: "KEY_A_STAGE", version: 1 }] },
};
const appB: AppEntry = {
  prefix: "appb.",
  paths: ["b"],
  keys: { production: [{ secret: "KEY_B_PROD", version: 1 }], staging: [{ secret: "KEY_B_STAGE", version: 1 }] },
};
const config: HubConfig = {
  connections: [
    { stage: "production", clientKey: "sdk-prod" },
    { stage: "staging", clientKey: "sdk-stage" },
  ],
  apps: [appA, appB],
  cdn: "https://cdn.example.com",
};

function setup(over: Partial<HubEnv> = {}) {
  calls = [];
  payloads.clear();
  payloads.set("sdk-prod", { features: { "appa.x": { defaultValue: "p" }, "appb.x": { defaultValue: "pb" } } });
  payloads.set("sdk-stage", { features: { "appa.x": { defaultValue: "s" }, "appb.x": { defaultValue: "sb" } } });
  const bucket = fakeBucket();
  const env: HubEnv = {
    SETTINGS: bucket,
    GROWTHBOOK_EVENT_SECRET: "ewhk_test",
    REFRESH_SECRET: "refresh-me",
    KEY_A_PROD: keys.aProd,
    KEY_A_STAGE: keys.aStage,
    KEY_B_PROD: keys.bProd,
    KEY_B_STAGE: keys.bStage,
    ...over,
  };
  const waits: Promise<unknown>[] = [];
  const ctx = { waitUntil: (p: Promise<unknown>) => void waits.push(p) };
  const hub = createHub(config, { fetch: fakeFetch, sleep: async () => {} });
  return { bucket, env, ctx, waits, hub };
}

async function valueIn(file: string | undefined, key: string, feature: string) {
  assert.ok(file, "the file was written");
  const gb = new GrowthBook({ decryptionKey: key });
  await gb.setPayload(JSON.parse(file));
  const v = gb.getFeatureValue(feature, "none");
  gb.destroy();
  return v;
}

test("a refresh writes each app's file for each stage, encrypted with its own key", async () => {
  const { bucket, env, hub } = setup();
  const report = await hub.refresh(env);
  assert.deepEqual(report.written.sort(), ["a/production/v1.json", "a/staging/v1.json", "b/production/v1.json", "b/staging/v1.json"]);
  assert.equal(await valueIn(bucket.files.get("a/production/v1.json"), keys.aProd, "x"), "p");
  assert.equal(await valueIn(bucket.files.get("a/staging/v1.json"), keys.aStage, "x"), "s");
  assert.equal(await valueIn(bucket.files.get("b/production/v1.json"), keys.bProd, "x"), "pb");
  assert.equal(await valueIn(bucket.files.get("a/production/v1.json"), keys.bProd, "x"), "none");
});

test("a second refresh with the same payload writes nothing", async () => {
  const { bucket, env, hub } = setup();
  await hub.refresh(env);
  const before = bucket.puts.length;
  const report = await hub.refresh(env);
  assert.deepEqual(report.written, []);
  assert.equal(report.same.length, 4);
  assert.equal(bucket.puts.length, before);
});

test("a change in one app's features rewrites only that app's files", async () => {
  const { env, hub } = setup();
  await hub.refresh(env);
  payloads.set("sdk-prod", { features: { "appa.x": { defaultValue: "p2" }, "appb.x": { defaultValue: "pb" } } });
  const report = await hub.refresh(env);
  assert.deepEqual(report.written, ["a/production/v1.json"]);
});

test("an app with two paths gets the same rules at both", async () => {
  const { bucket, env } = setup();
  const hub = createHub({ ...config, apps: [{ ...appA, paths: ["a", ""] }] }, { fetch: fakeFetch, sleep: async () => {} });
  const report = await hub.refresh(env);
  assert.ok(report.written.includes("a/production/v1.json"));
  assert.ok(report.written.includes("production/v1.json"));
  assert.equal(await valueIn(bucket.files.get("production/v1.json"), keys.aProd, "x"), "p");
});

test("an app with two keys for a stage gets v1 and v2", async () => {
  const two = newKey();
  const { bucket, env } = setup({ KEY_A_PROD_2: two });
  const entry = { ...appA, keys: { production: [{ secret: "KEY_A_PROD", version: 1 }, { secret: "KEY_A_PROD_2", version: 2 }] } };
  const hub = createHub({ ...config, apps: [entry] }, { fetch: fakeFetch, sleep: async () => {} });
  await hub.refresh(env);
  assert.equal(await valueIn(bucket.files.get("a/production/v1.json"), keys.aProd, "x"), "p");
  assert.equal(await valueIn(bucket.files.get("a/production/v2.json"), two, "x"), "p");
});

test("a connection that fails leaves the other stage written", async () => {
  const { env, hub } = setup();
  payloads.delete("sdk-stage");
  const report = await hub.refresh(env);
  assert.deepEqual(report.written.sort(), ["a/production/v1.json", "b/production/v1.json"]);
  assert.deepEqual(report.failed.sort(), ["a/staging/v1.json", "b/staging/v1.json"]);
});

test("a body that is not a payload writes nothing for that stage", async () => {
  const { env, hub } = setup();
  payloads.set("sdk-stage", "not json");
  payloads.set("sdk-prod", { nope: true });
  const report = await hub.refresh(env);
  assert.deepEqual(report.written, []);
  assert.equal(report.failed.length, 4);
});

test("a missing key secret fails only that app", async () => {
  const { env, hub } = setup({ KEY_B_PROD: undefined });
  const report = await hub.refresh(env);
  assert.deepEqual(report.failed, ["b/production/v1.json"]);
  assert.equal(report.written.length, 3);
});

const sign = (body: string) => createHmac("sha256", "ewhk_test").update(body).digest("hex");
const hookRequest = (body: string, signature: string | null) =>
  new Request("https://hub.example.com/hook", {
    method: "POST",
    body,
    headers: signature === null ? {} : { "X-GrowthBook-Signature": signature },
  });

test("a signed event answers 200 and refreshes twice", async () => {
  const { env, ctx, waits, hub } = setup();
  const body = '{"event":"feature.updated"}';
  const res = await hub.fetch(hookRequest(body, sign(body)), env, ctx);
  assert.equal(res.status, 200);
  await Promise.all(waits);
  assert.equal(calls.length, 4);
});

test("a signed event answers 200 even when every CDN read fails", async () => {
  const { env, ctx, waits, hub } = setup();
  payloads.clear();
  const body = "{}";
  const res = await hub.fetch(hookRequest(body, sign(body)), env, ctx);
  assert.equal(res.status, 200);
  await Promise.all(waits);
});

test("an unsigned event answers 401 and refreshes nothing", async () => {
  const { env, ctx, waits, hub } = setup();
  const res = await hub.fetch(hookRequest("{}", null), env, ctx);
  assert.equal(res.status, 401);
  const bad = await hub.fetch(hookRequest("{}", sign("{ }")), env, ctx);
  assert.equal(bad.status, 401);
  await Promise.all(waits);
  assert.equal(calls.length, 0);
});

test("an event with no secret set answers 401", async () => {
  const { env, ctx, hub } = setup({ GROWTHBOOK_EVENT_SECRET: undefined });
  const res = await hub.fetch(hookRequest("{}", sign("{}")), env, ctx);
  assert.equal(res.status, 401);
});

test("refresh needs the refresh secret", async () => {
  const { env, ctx, hub } = setup();
  const req = (secret: string) =>
    new Request("https://hub.example.com/refresh", { method: "POST", headers: { "x-refresh-secret": secret } });
  assert.equal((await hub.fetch(req("wrong"), env, ctx)).status, 401);
  const ok = await hub.fetch(req("refresh-me"), env, ctx);
  assert.equal(ok.status, 200);
  const report = (await ok.json()) as { written: string[] };
  assert.equal(report.written.length, 4);
});

test("an empty refresh secret refuses every refresh", async () => {
  const { env, ctx, hub } = setup({ REFRESH_SECRET: "" });
  const req = new Request("https://hub.example.com/refresh", { method: "POST", headers: { "x-refresh-secret": "" } });
  assert.equal((await hub.fetch(req, env, ctx)).status, 401);
});

test("any other request answers 404", async () => {
  const { env, ctx, hub } = setup();
  assert.equal((await hub.fetch(new Request("https://hub.example.com/hook"), env, ctx)).status, 404);
  assert.equal((await hub.fetch(new Request("https://hub.example.com/"), env, ctx)).status, 404);
});

test("the cron refreshes", async () => {
  const { bucket, env, ctx, hub } = setup();
  await hub.scheduled({}, env, ctx);
  assert.ok(bucket.files.has("a/production/v1.json"));
});

test("createHub refuses overlapping prefixes", () => {
  assert.throws(() => createHub({ connections: [], apps: [appA, { ...appA, prefix: "appa.pro." }] }));
});

test("every written file has the cache headers", async () => {
  const { bucket, env, hub } = setup();
  await hub.refresh(env);
  const rules = bucket.puts.filter((p) => p.key.endsWith(".json"));
  assert.equal(rules.length, 4);
  for (const p of rules) {
    assert.deepEqual(p.opts, { httpMetadata: { contentType: "application/json", cacheControl: "public, max-age=60" } });
  }
});

test("the hub reads each connection from the CDN by its client key", async () => {
  const { env, hub } = setup();
  await hub.refresh(env);
  assert.deepEqual(calls.sort(), ["https://cdn.example.com/api/features/sdk-prod", "https://cdn.example.com/api/features/sdk-stage"]);
});
