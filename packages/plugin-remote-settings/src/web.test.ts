import assert from "node:assert/strict";
import { test } from "node:test";
import { countryOf, readFilesWeb, webInstallId, webStore } from "./web.ts";

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

function memory() {
  const m = new Map<string, string>();
  return {
    m,
    storage: () => ({ getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) }),
  };
}

const answer = (body: string, status = 200, country?: string) =>
  new Response(body, { status, headers: country ? { "x-country": country } : {} });

const opts = (fetcher: typeof fetch) => ({
  baseUrl: "https://settings.example.com/appa/",
  stage: "production",
  keyVersion: 2,
  countryHeader: "x-country",
  fetcher,
});

test("both files are read from the base URL, stage and key version", async () => {
  const urls: string[] = [];
  const fetcher = (async (u: RequestInfo | URL) => {
    urls.push(String(u));
    return answer(String(u).endsWith("global.json") ? "{}" : '{"encryptedFeatures":"x"}', 200, "VN");
  }) as typeof fetch;
  const files = await readFilesWeb(opts(fetcher));
  assert.deepEqual(urls.sort(), [
    "https://settings.example.com/appa/production/global.json",
    "https://settings.example.com/appa/production/v2.json",
  ]);
  assert.deepEqual(files, { global: "{}", rules: '{"encryptedFeatures":"x"}', country: "VN" });
});

test("one failed file leaves the other", async () => {
  const fetcher = (async (u: RequestInfo | URL) => {
    if (String(u).endsWith("global.json")) throw new Error("offline");
    return answer("{}", 200, "US");
  }) as typeof fetch;
  assert.deepEqual(await readFilesWeb(opts(fetcher)), { global: null, rules: "{}", country: "US" });
});

test("a file that answers an error status gives null", async () => {
  const fetcher = (async () => answer("nope", 404)) as typeof fetch;
  assert.deepEqual(await readFilesWeb(opts(fetcher)), { global: null, rules: null, country: null });
});

test("the country comes from the header, and XX or T1 is no country", () => {
  assert.equal(countryOf("VN"), "VN");
  assert.equal(countryOf("vn"), null);
  assert.equal(countryOf("XX"), null);
  assert.equal(countryOf("T1"), null);
  assert.equal(countryOf(""), null);
  assert.equal(countryOf(null), null);
});

test("a store keeps text under its namespace", async () => {
  const { m, storage } = memory();
  const s = webStore("appa", storage);
  await s.set("state", "x");
  assert.equal(await s.get("state"), "x");
  assert.deepEqual([...m.keys()], ["remote-settings:appa:state"]);
  assert.equal(await s.get("other"), null);
});

test("a store whose storage throws gives null and keeps nothing, and never throws", async () => {
  const s = webStore("appa", () => ({
    getItem: () => {
      throw new Error("denied");
    },
    setItem: () => {
      throw new Error("denied");
    },
  }));
  await s.set("state", "x");
  assert.equal(await s.get("state"), null);
  const gone = webStore("appa", () => {
    throw new Error("no storage");
  });
  assert.equal(await gone.get("state"), null);
});

test("the install id is kept, and the same id comes back", async () => {
  const s = webStore("appa", memory().storage);
  const id = await webInstallId(s, null);
  assert.match(id ?? "", UUID);
  assert.equal(await webInstallId(s, null), id);
});

test("a seed install id is kept when there is none", async () => {
  const s = webStore("appa", memory().storage);
  const seed = "0f8fad5b-d9cb-469f-a165-70867728950e";
  assert.equal(await webInstallId(s, seed), seed);
  assert.equal(await webInstallId(s, "1b4e28ba-2fa1-41d2-883f-0016d3cca427"), seed);
});

test("a bad seed is ignored", async () => {
  const s = webStore("appa", memory().storage);
  const id = await webInstallId(s, "not-a-uuid");
  assert.match(id ?? "", UUID);
  assert.notEqual(id, "not-a-uuid");
});

test("with no storage there is no install id", async () => {
  assert.equal(await webInstallId(webStore("x", () => undefined), null), null);
});

test("a store that throws gives no install id", async () => {
  const s = webStore("appa", () => ({
    getItem: () => null,
    setItem: () => {
      throw new Error("full");
    },
  }));
  assert.equal(await webInstallId(s, null), null);
});

test("a read that hangs gives up at the timeout", async () => {
  const fetcher = ((_u: RequestInfo | URL, init?: RequestInit) =>
    new Promise((_done, fail) => init?.signal?.addEventListener("abort", () => fail(new Error("aborted"))))) as typeof fetch;
  const start = Date.now();
  const files = await readFilesWeb({ ...opts(fetcher), timeoutMs: 50 });
  assert.deepEqual(files, { global: null, rules: null, country: null });
  assert.ok(Date.now() - start < 1000);
});

test("an install id is made even where crypto.randomUUID is missing", async () => {
  const c = globalThis.crypto as { randomUUID?: unknown };
  const real = c.randomUUID;
  Object.defineProperty(c, "randomUUID", { value: undefined, configurable: true, writable: true });
  try {
    const id = await webInstallId(webStore("appa", memory().storage), null);
    assert.match(id ?? "", UUID);
  } finally {
    Object.defineProperty(c, "randomUUID", { value: real, configurable: true, writable: true });
  }
});
