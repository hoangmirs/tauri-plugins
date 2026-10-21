# Remote Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `tauri-plugin-remote-settings` (Rust crate and JS package) and the `remote-settings-hub` Worker library, so any app gets cached settings, GrowthBook rules evaluated on the device, and Remote Config behind a switch.

**Architecture:** The hub reads the two GrowthBook connections, splits each payload by feature prefix into one encrypted rules file per app, and writes it to R2. On a device, Rust reads the app's two files at start and keeps a small file store. JS evaluates the rules with the GrowthBook JS SDK, joins them with the global file, and applies new values by the app's mode. On the web, JS reads the files and uses `localStorage` as the store.

**Tech Stack:** Rust (Tauri 2, `ureq` with `rustls`, `uuid`, `serde_json`), TypeScript (ESM, `node --test --experimental-strip-types`), `@growthbook/growthbook` 1.8.0, Cloudflare Workers (R2 binding, cron trigger, WebCrypto).

**Spec:** `docs/superpowers/specs/2026-10-20-remote-settings-design.md`. Read it before any task. This plan argues from it.

## Global Constraints

- No product name, domain, key, client key or secret in this repository. Tests use `https://settings.example.com`, prefixes `appa.` and `appb.`, and keys made in the test.
- Workspace version `0.1.0-alpha.1` for the new crate and both new packages.
- Rust: the workspace lints (`unsafe_code = "forbid"`, clippy `all` and `pedantic` as warnings), `ureq` with `rustls` from the workspace, no Swift or Kotlin.
- JS packages: `"type": "module"`, `sideEffects: false`, scripts `build`, `prepack`, `typecheck`, `test` as in `packages/plugin-update-gate/package.json`. Peer `@tauri-apps/api` `^2.11.1`. TypeScript `7.0.2` as a dev dependency.
- A device never calls `cdn.growthbook.io`. The plugin never gives the GrowthBook SDK an `apiHost` or a `clientKey`.
- File names: `<base>/<stage>/global.json` and `<base>/<stage>/v<n>.json`. Every file the hub writes has `Cache-Control: public, max-age=60` and `Content-Type: application/json`.
- Plugin keys in the global file: `source` (`growthbook`, `firebase`) and `apply` (`hidden`, `rest`, `now`). Default mode `hidden`.
- No exported JS function rejects. A failure keeps the saved values.
- Rust commands fail open: a read error gives `null` parts, never `Err` to JS.
- Before each commit: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `npm test --workspaces --if-present`, `npm run typecheck --workspaces --if-present`, `npm run build --workspaces --if-present`. All must pass.
- Comments follow the owner's rule: none by default, one only for a constraint the code cannot carry. Docs and comments follow `~/.claude/ste100.md`, Strict tier.
- Commit messages: one plain sentence, as in `git log`. No `Co-Authored-By` line.

## Review Focus

1. GrowthBook sends the event before its CDN serves the new payload. The hub reads again 15 seconds after each event, so the change still arrives. Test in Task 3.
2. A rules file with the right shape that does not decrypt with the app's key (key change, wrong stage). The saved rules stay, and the bad file is never stored. Tests in Task 6 and Task 10.
3. Two app prefixes where one starts with the other (`appa.` and `appa.pro.`). `createHub` refuses the config, so no feature can reach two apps. Test in Task 2.
4. A read that lands while the app is out of sight, in each mode. It applies at once, with no wait for the next return. Test in Task 7.
5. `localStorage` that throws on every call (a private window). The web path gives no install id, keeps the code's defaults, and never throws. Test in Task 8.

---

## File Structure

```
packages/remote-settings-hub/
  package.json, tsconfig.json, README.md, LICENSE
  src/payload.ts        readPayload, encryptPayload, sha256Hex
  src/signature.ts      verifyEventSignature
  src/split.ts          checkApps, splitPayload
  src/hub.ts            createHub: fetch and scheduled handlers, refresh
  src/global.ts         buildGlobal for an app's push script
  src/index.ts          exports
  src/*.test.ts
packages/plugin-remote-settings/
  package.json, tsconfig.json, README.md, LICENSE
  src/types.ts          shared types
  src/values.ts         readText, globalPart, nextSaved, valuesOf
  src/rules.ts          evaluate (GrowthBook SDK)
  src/applier.ts        createApplier: modes, pending, test groups
  src/web.ts            web files, web store, web install id
  src/native.ts         commands of the Rust crate
  src/index.ts          init and the public API
  src/firebase.ts       the Firebase adapter (subpath ./firebase)
  src/*.test.ts
crates/tauri-plugin-remote-settings/
  Cargo.toml, build.rs, permissions/default.toml
  src/lib.rs            init, config, commands
  src/read.rs           the two-file read, ureq, country header
  src/store.rs          the store folder, files, install id
Cargo.toml              add the crate to `members`
README.md               a section for the plugin and one for the hub
```

---

### Task 1: Hub payload, encryption and event signature

**Files:**
- Create: `packages/remote-settings-hub/package.json`, `tsconfig.json`, `LICENSE` (copy from `packages/plugin-update-gate/`)
- Create: `packages/remote-settings-hub/src/payload.ts`, `src/signature.ts`, `src/index.ts`
- Test: `packages/remote-settings-hub/src/payload.test.ts`, `src/signature.test.ts`

**Interfaces:**
- Produces:
  - `type Payload = { features: Record<string, unknown>; experiments?: unknown[]; savedGroups?: Record<string, unknown>; [k: string]: unknown }`
  - `type EncryptedPayload = { encryptedFeatures: string; encryptedExperiments?: string; encryptedSavedGroups?: string; [k: string]: unknown }`
  - `readPayload(raw: unknown): Payload | null`
  - `encryptPayload(p: Payload, keyB64: string): Promise<EncryptedPayload>`
  - `sha256Hex(text: string): Promise<string>`
  - `verifyEventSignature(body: string, header: string | null, secret: string): Promise<boolean>`

The package name is `remote-settings-hub`, with `@growthbook/growthbook` `1.8.0` as a dev dependency for tests only. Port `readPayload` and `encryptPayload` from Ra Chơi's `site/config.ts` (spec "Facts": AES-CBC 128, `iv.ciphertext` in base64, a random IV for each field). Trim `keyB64` before import: `wrangler secret put` keeps a pasted newline.

- [ ] **Step 1: Write the failing tests**

```ts
test("a payload needs a features object", () => {
  assert.equal(readPayload({}), null);
  assert.equal(readPayload({ features: [] }), null);
  assert.equal(readPayload({ features: {}, experiments: {} }), null);
  assert.deepEqual(readPayload({ features: {} }), { features: {} });
});
test("the GrowthBook SDK decrypts what encryptPayload writes", async () => {
  const key = btoa(String.fromCharCode(...crypto.getRandomValues(new Uint8Array(16))));
  const enc = await encryptPayload({ features: { a: { defaultValue: "on" } }, savedGroups: {} }, `${key}\n`);
  const gb = new GrowthBook({ decryptionKey: key });
  await gb.setPayload(enc);
  assert.equal(gb.getFeatureValue("a", "off"), "on");
});
test("two encryptions of one payload differ", async () => { /* same input twice, encryptedFeatures not equal */ });
test("an event signature is the hex HMAC-SHA256 of the body", async () => {
  // expected = hex of HMAC-SHA256(key = UTF-8 of "ewhk_test", data = body), computed in the test with node:crypto
  assert.equal(await verifyEventSignature(body, expected, "ewhk_test"), true);
  assert.equal(await verifyEventSignature(body, expected.toUpperCase(), "ewhk_test"), true);
  assert.equal(await verifyEventSignature(`${body} `, expected, "ewhk_test"), false);
  assert.equal(await verifyEventSignature(body, null, "ewhk_test"), false);
  assert.equal(await verifyEventSignature(body, expected, "ewhk_test\n"), true);
});
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `npm test -w remote-settings-hub`
Expected: FAIL, the modules do not exist.

- [ ] **Step 3: Implement `payload.ts` and `signature.ts`**

`verifyEventSignature` trims the secret, compares in constant time, and lowercases the header. Use WebCrypto only (`crypto.subtle`), so the code runs in a Worker and in Node.

- [ ] **Step 4: Run the tests to see them pass**

Run: `npm test -w remote-settings-hub`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/remote-settings-hub package-lock.json
git commit -m "Add the hub's payload encryption and event signature check"
```

---

### Task 2: Split one payload into one payload per app

**Files:**
- Create: `packages/remote-settings-hub/src/split.ts`
- Test: `packages/remote-settings-hub/src/split.test.ts`

**Interfaces:**
- Consumes: `Payload` (Task 1)
- Produces:
  - `type AppEntry = { prefix: string; paths: string[]; keys: Record<string, { secret: string; version: number }[]> }`
    (`keys` maps a stage to the keys the hub writes for it. Two entries while a key changes.)
  - `checkApps(apps: AppEntry[]): string[]` gives the problems. Empty means good.
  - `splitPayload(p: Payload, prefixes: string[]): { byPrefix: Map<string, Payload>; unknown: string[] }`

- [ ] **Step 1: Write the failing tests**

```ts
test("each feature goes to its app, without its prefix", () => {
  const p = { features: { "appa.x": { defaultValue: 1 }, "appb.x": { defaultValue: 2 }, y: {} }, dateUpdated: "d" };
  const { byPrefix, unknown } = splitPayload(p, ["appa.", "appb."]);
  assert.deepEqual(byPrefix.get("appa.")?.features, { x: { defaultValue: 1 } });
  assert.deepEqual(byPrefix.get("appb.")?.features, { x: { defaultValue: 2 } });
  assert.equal(byPrefix.get("appa.")?.dateUpdated, "d");
  assert.deepEqual(unknown, ["y"]);
});
test("an app gets only the saved groups its features use", () => {
  // appa.x has a rule condition { id: { $inGroup: "grp_a" } }, appb.x uses { $notInGroup: "grp_b" }
  // savedGroups: { grp_a: [...], grp_b: [...], grp_c: [...] }
  assert.deepEqual(Object.keys(byPrefix.get("appa.")!.savedGroups!), ["grp_a"]);
  assert.deepEqual(Object.keys(byPrefix.get("appb.")!.savedGroups!), ["grp_b"]);
});
test("a saved group named deep inside $and and $or is found", () => { /* nested condition, group kept */ });
test("an app's experiments list is empty", () => { /* input experiments: [{...}], output experiments: [] */ });
test("an app with no feature still gets a payload, with no features", () => {
  assert.deepEqual(splitPayload({ features: {} }, ["appa."]).byPrefix.get("appa.")?.features, {});
});
test("checkApps refuses a prefix that starts another, an empty prefix, and one with no dot at the end", () => {
  assert.notDeepEqual(checkApps([app("appa."), app("appa.pro.")]), []);
  assert.notDeepEqual(checkApps([app("")]), []);
  assert.notDeepEqual(checkApps([app("appa")]), []);
  assert.notDeepEqual(checkApps([app("appa."), app("appa.")]), []);
  assert.deepEqual(checkApps([app("appa."), app("appb.")]), []);
});
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `npm test -w remote-settings-hub`
Expected: FAIL, `split.ts` does not exist.

- [ ] **Step 3: Implement `split.ts`**

A saved group is "used" when its id is the value of an `$inGroup` or `$notInGroup` key anywhere inside a feature's `rules`. Walk the rule objects. Do not match on text. A prefix must end with `.` and match the pattern `^[a-z0-9_-]+\.$`.

- [ ] **Step 4: Run the tests to see them pass**

Run: `npm test -w remote-settings-hub`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/remote-settings-hub/src/split.ts packages/remote-settings-hub/src/split.test.ts
git commit -m "Split a GrowthBook payload into one payload for each app by feature prefix"
```

---

### Task 3: The hub Worker

**Files:**
- Create: `packages/remote-settings-hub/src/hub.ts`
- Modify: `packages/remote-settings-hub/src/index.ts`
- Test: `packages/remote-settings-hub/src/hub.test.ts`

**Interfaces:**
- Consumes: Tasks 1 and 2.
- Produces:
  - `type HubConfig = { connections: { stage: string; clientKey: string }[]; apps: AppEntry[]; cdn?: string; recheckMs?: number }`
    (`cdn` defaults to `https://cdn.growthbook.io`, `recheckMs` to `15000`)
  - `type HubEnv = { SETTINGS: Bucket; GROWTHBOOK_EVENT_SECRET?: string; REFRESH_SECRET?: string; [secret: string]: unknown }`
  - `type Bucket = { get(key: string): Promise<{ text(): Promise<string> } | null>; put(key: string, body: string, o: { httpMetadata: { contentType: string; cacheControl: string } }): Promise<unknown> }`
  - `createHub(config: HubConfig, deps?: { fetch?: typeof fetch; sleep?: (ms: number) => Promise<void> }): { fetch(req: Request, env: HubEnv, ctx: { waitUntil(p: Promise<unknown>): void }): Promise<Response>; scheduled(ev: unknown, env: HubEnv, ctx: { waitUntil(p: Promise<unknown>): void }): Promise<void>; refresh(env: HubEnv): Promise<RefreshReport> }`
  - `type RefreshReport = { written: string[]; same: string[]; failed: string[] }` (each item is `<path>/<stage>/v<n>.json`)

`createHub` throws when `checkApps` gives a problem, so a bad deploy fails at its first request.

Routes:

| Request | Answer |
|---|---|
| `POST /hook` with a good `X-GrowthBook-Signature` | Run `refresh` in `waitUntil`, then again after `recheckMs`. Answer 200 at once. |
| `POST /hook` with a bad or missing signature | 401. No refresh. Log `remote-settings hub: refused an event, the signature does not match`. |
| `POST /refresh` with header `x-refresh-secret` equal to `REFRESH_SECRET` | Run `refresh`, answer 200 with the report as JSON. |
| `POST /refresh` with another secret | 401. |
| Anything else | 404. |
| `scheduled` | Run `refresh`. |

`refresh` reads each connection from `<cdn>/api/features/<clientKey>`, splits it, and for each app, path and key of that stage: hashes the app's plain payload JSON, compares it with `<path>/<stage>/v<n>.sha256`, and when it differs, encrypts with the key from `env[secret]`, writes `<path>/<stage>/v<n>.json`, then writes the hash. It logs each unknown feature key once per refresh, never a body.

- [ ] **Step 1: Write the failing tests**

Use a fake bucket (a `Map`), a fake `fetch` that serves payloads by client key, and `sleep` that resolves at once.

```ts
test("a refresh writes each app's file for each stage, encrypted with its own key", async () => {
  // two connections (production, staging), two apps; then decrypt each written file with the right key in the test
  assert.deepEqual(report.written.sort(), ["a/production/v1.json", "a/staging/v1.json", "b/production/v1.json", "b/staging/v1.json"]);
});
test("a second refresh with the same payload writes nothing", async () => { /* report.written = [], report.same has 4 */ });
test("a change in one app's features rewrites only that app's files", async () => {});
test("an app with two paths gets the same rules at both", async () => { /* paths ["recess-like", ""] → "x/production/v1.json" and "production/v1.json" */ });
test("an app with two keys for a stage gets v1 and v2", async () => {});
test("a connection that fails leaves the other stage written", async () => { /* staging fetch 500 → production files written, staging in failed */ });
test("a body that is not a payload writes nothing for that stage", async () => {});
test("a missing key secret fails only that app", async () => {});
test("a signed event answers 200 and refreshes twice", async () => {
  // ctx.waitUntil collects promises; await them; fetch called 2 × connections.length times
});
test("a signed event answers 200 even when every CDN read fails", async () => {});
test("an unsigned event answers 401 and refreshes nothing", async () => {});
test("refresh needs the refresh secret", async () => { /* wrong → 401, right → 200 with JSON report */ });
test("createHub refuses overlapping prefixes", () => { assert.throws(() => createHub({ connections: [], apps: [app("appa."), app("appa.pro.")] })); });
test("every written file has the cache headers", async () => {
  // put options: { httpMetadata: { contentType: "application/json", cacheControl: "public, max-age=60" } }
});
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `npm test -w remote-settings-hub`
Expected: FAIL, `hub.ts` does not exist.

- [ ] **Step 3: Implement `hub.ts`**

Run apps and stages with `Promise.allSettled`, so one failure never stops another. Join path and stage with `/`, and leave out an empty path, so `""` writes `production/v1.json`.

- [ ] **Step 4: Run the tests to see them pass**

Run: `npm test -w remote-settings-hub`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/remote-settings-hub/src
git commit -m "Add the hub Worker: an event, the cron or a manual refresh rewrites each app's rules"
```

---

### Task 4: The global file check for an app's push script, and the hub README

**Files:**
- Create: `packages/remote-settings-hub/src/global.ts`, `packages/remote-settings-hub/README.md`
- Modify: `packages/remote-settings-hub/src/index.ts`, `README.md` (a short section that points at the package README)
- Test: `packages/remote-settings-hub/src/global.test.ts`

**Interfaces:**
- Produces: `buildGlobal(file: Record<string, unknown>, keys: Record<string, { read: (text: string) => unknown; global?: boolean }>): { ok: true; body: string } | { ok: false; bad: string[] }`

A value is checked as the text the app reads: a string as it is, anything else as `JSON.stringify`. The plugin keys `source` and `apply` are checked against their own values (`growthbook`, `firebase` and `hidden`, `rest`, `now`). `body` ends with a newline.

- [ ] **Step 1: Write the failing tests**

```ts
test("a good file builds", () => {
  assert.deepEqual(buildGlobal({ source: "growthbook", notice: { a: 1 } }, keys), { ok: true, body: '{"source":"growthbook","notice":{"a":1}}\n' });
});
test("a key that is not global, an unknown key and a bad value are each refused", () => {
  assert.deepEqual(buildGlobal({ lab_x: "on", nope: 1, notice: "not json", apply: "later" }, keys), { ok: false, bad: ["lab_x", "nope", "notice", "apply"] });
});
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `npm test -w remote-settings-hub`
Expected: FAIL.

- [ ] **Step 3: Implement `global.ts`. Write the README.**

The README covers, in this order:

1. What the hub does, and what it never does: a device never reads through it.
2. The deploy repository: a `wrangler.jsonc` with the R2 binding `SETTINGS`, a cron trigger `0 3 * * *`, and the secrets `GROWTHBOOK_EVENT_SECRET`, `REFRESH_SECRET` and one per app and stage. Give a 15-line `src/index.ts` that calls `createHub`.
3. The GrowthBook setup from the spec: one project, two connections, one event webhook for every event and project to `https://<hub>/hook`, prefixes and tags.
4. The Cloudflare setup for each settings domain: the custom domain on the bucket, the Cache Rule (eligible for cache, query string left out of the key), the response header rule (`x-country` from `ip.src.country`, `access-control-allow-origin: *`, `access-control-expose-headers: x-country`), and Tiered Cache.
5. How to add an app, and how to change a key.

- [ ] **Step 4: Run the tests to see them pass**

Run: `npm test -w remote-settings-hub && npm run typecheck -w remote-settings-hub && npm run build -w remote-settings-hub`
Expected: PASS, and `dist/` holds `index.js` and `index.d.ts`.

- [ ] **Step 5: Commit**

```bash
git add packages/remote-settings-hub README.md
git commit -m "Add the global file check and the hub's setup guide"
```

---

### Task 5: Plugin types and value reading

**Files:**
- Create: `packages/plugin-remote-settings/package.json`, `tsconfig.json`, `LICENSE`
- Create: `packages/plugin-remote-settings/src/types.ts`, `src/values.ts`
- Test: `packages/plugin-remote-settings/src/values.test.ts`

**Interfaces:**
- Produces (`types.ts`):
  - `type Mode = "hidden" | "rest" | "now"`
  - `type SourceName = "growthbook" | "firebase"`
  - `type From = "global" | "growthbook" | "firebase" | "saved" | "default"`
  - `type Used = "growthbook" | "firebase" | "saved" | "default"`
  - `type Key<T> = { fallback: T; read: (text: string) => T | undefined; global?: boolean }`
  - `type Keys = Record<string, Key<unknown>>`
  - `type Values<K extends Keys> = { [N in keyof K]: K[N] extends Key<infer T> ? T : never }`
  - `type Part = Record<string, string>` (key to text)
  - `type Saved = { global: Part; source: Part; from: SourceName; country: string | null }`
  - `type Files = { global: string | null; rules: string | null; country: string | null }`
  - `type Assignment = { key: string; experiment: string; variant: string }`
  - `type Store = { get(name: string): Promise<string | null>; set(name: string, text: string): Promise<void> }`
- Produces (`values.ts`):
  - `asText(v: unknown): string` (a string as it is, anything else as JSON)
  - `globalPart(raw: unknown, keys: Keys): Part | null` (the global keys and the plugin keys, as text. Null when `raw` is not an object.)
  - `pluginKeys(global: Part): { source: SourceName | null; apply: Mode | null }`
  - `nextSaved(prev: Saved | null, read: { global: Part | null; source: Part | null; from: SourceName; country: string | null }, keys: Keys): Saved`
  - `valuesOf<K extends Keys>(saved: Saved | null, keys: K, loaded: boolean): { values: Values<K>; from: Record<keyof K, From>; sent: (keyof K)[] }`

The package name is `tauri-plugin-remote-settings-api`. Dependencies: `@growthbook/growthbook` `^1.8.0`. Peers: `@tauri-apps/api` `^2.11.1`, and `tauri-plugin-firebase-api` `>=0.1.0-alpha.4` marked optional. Dev dependency: `remote-settings-hub` (the workspace package, for tests that encrypt). Exports: `"."` and `"./firebase"`.

`valuesOf` reads each key from `saved.global` when the key is global, else from `saved.source`. A text that reads gives its value and `from` = `global` or the saved source name. A text that does not read gives the fallback and `default`. `loaded` false turns every `global`, `growthbook` and `firebase` into `saved`.

- [ ] **Step 1: Write the failing tests**

```ts
const keys = {
  notice: { fallback: null, read: readJson, global: true },
  lab_x: { fallback: "off", read: (t: string) => (["off", "lab", "on"].includes(t) ? t : undefined) },
};
test("a global value that does not read keeps its saved value, and the others apply", () => {
  const prev = { global: { notice: '{"a":1}' }, source: {}, from: "growthbook", country: null };
  const next = nextSaved(prev, { global: { notice: "{bad" , source: "growthbook" }, source: null, from: "growthbook", country: "VN" }, keys);
  assert.equal(next.global.notice, '{"a":1}');
  assert.equal(next.global.source, "growthbook");
  assert.equal(next.country, "VN");
});
test("a part that failed keeps what was saved", () => { /* read.source null → prev.source kept, prev.from kept */ });
test("a read with no country keeps the saved country", () => {});
test("a source key in the global file is ignored", () => { /* globalPart({ lab_x: "on" }, keys) has no lab_x */ });
test("a global key from a source is ignored", () => { /* valuesOf with source { notice: ... } → default */ });
test("each value says where it came from", () => {
  const v = valuesOf({ global: { notice: '{"a":1}' }, source: { lab_x: "on" }, from: "growthbook", country: null }, keys, true);
  assert.deepEqual(v.values, { notice: { a: 1 }, lab_x: "on" });
  assert.deepEqual(v.from, { notice: "global", lab_x: "growthbook" });
  assert.deepEqual(v.sent.sort(), ["lab_x", "notice"]);
});
test("a value that does not read gives the fallback and is not sent", () => {});
test("pluginKeys reads source and apply, and refuses other words", () => {
  assert.deepEqual(pluginKeys({ source: "firebase", apply: "rest" }), { source: "firebase", apply: "rest" });
  assert.deepEqual(pluginKeys({ source: "x", apply: "later" }), { source: null, apply: null });
});
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `npm test -w tauri-plugin-remote-settings-api`
Expected: FAIL.

- [ ] **Step 3: Implement `types.ts` and `values.ts`**

- [ ] **Step 4: Run the tests to see them pass**

Run: `npm test -w tauri-plugin-remote-settings-api`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/plugin-remote-settings package-lock.json
git commit -m "Add the remote settings package's types and value reading"
```

---

### Task 6: Evaluate the rules on the device

**Files:**
- Create: `packages/plugin-remote-settings/src/rules.ts`
- Test: `packages/plugin-remote-settings/src/rules.test.ts`

**Interfaces:**
- Consumes: `Keys`, `Part`, `Assignment` (Task 5). `encryptPayload` from `remote-settings-hub` in tests.
- Produces:
  - `type Attributes = { id: string | null; platform: "ios" | "android" | "macos" | "windows" | "linux" | "web"; web: boolean; version: string; country: string | null; language: string; now: string; [extra: string]: unknown }`
  - `evaluate(rulesText: string | null, attrs: Attributes, keyB64: string, keys: Keys): Promise<{ part: Part; assignments: Assignment[] } | null>`

Port `rulesValues` from Ra Chơi's `src/app/config/rules.ts`. Changes from it: evaluate only the non-global keys in `keys`, collect each assignment in a list with `result.featureId` as its key instead of calling a callback, and drop `null` attributes rather than passing them. Never pass `apiHost` or `clientKey`.

- [ ] **Step 1: Write the failing tests**

```ts
test("a rule on country gives its value, and the default elsewhere", async () => {});
test("only the app's own keys are evaluated", async () => {
  // payload has lab_x and lab_new (an experiment rule on lab_new); keys has only lab_x
  assert.deepEqual(Object.keys(r!.part), ["lab_x"]);
  assert.deepEqual(r!.assignments, []);
});
test("an experiment rule gives its variant and an assignment for its key", async () => {
  // experiment key "exp1", coverage 1, weights [0,1] → variant "1"
  assert.deepEqual(r!.assignments, [{ key: "lab_x", experiment: "exp1", variant: "1" }]);
});
test("a file for another key gives null", async () => { /* encrypted with key A, evaluated with key B */ });
test("text that is not JSON, or JSON with no features, gives null", async () => {});
test("an attribute that is null is left out, so a rule on it does not match", async () => {});
test("a JSON value comes back as JSON text", async () => { /* defaultValue { a: 1 } → '{"a":1}' */ });
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `npm test -w tauri-plugin-remote-settings-api`
Expected: FAIL.

- [ ] **Step 3: Implement `rules.ts`**

- [ ] **Step 4: Run the tests to see them pass**

Run: `npm test -w tauri-plugin-remote-settings-api`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/plugin-remote-settings/src/rules.ts packages/plugin-remote-settings/src/rules.test.ts
git commit -m "Evaluate an app's GrowthBook rules on the device, only for its own keys"
```

---

### Task 7: When values apply, and when test groups are reported

**Files:**
- Create: `packages/plugin-remote-settings/src/applier.ts`
- Test: `packages/plugin-remote-settings/src/applier.test.ts`

**Interfaces:**
- Consumes: `Saved`, `Mode`, `Used`, `Assignment` (Task 5).
- Produces:
  - `type Snapshot = { saved: Saved; used: Used; assignments: Assignment[] }`
  - `createApplier(o: { start: Snapshot | null; mode: () => Mode; beforeApply?: (s: Snapshot) => Promise<Snapshot>; onApply: (s: Snapshot, changed: string[]) => void; onExperiment: (experiment: string, variant: string) => void }): Applier`
  - `type Applier = { applied(): Snapshot | null; pending(): Snapshot | null; landed(s: Snapshot): Promise<void>; setVisible(visible: boolean): Promise<void>; setAtRest(atRest: boolean): Promise<void>; applyNow(keys?: string[]): Promise<void>; holdEnded(): Promise<void>; holding(): void }`

Rules, from the spec's "When values apply" and "Test groups":

| Event | `hidden` | `rest` | `now` |
|---|---|---|---|
| `landed` while `holding` (before `ready` resolved) | apply | apply | apply |
| `landed` while out of sight | apply | apply | apply |
| `landed` in sight | wait | apply if at rest, else wait | apply |
| `setVisible(false)` with pending | apply | apply | apply |
| `setAtRest(true)` with pending | wait | apply | apply |

- `rest` with no `setAtRest` call yet acts as `hidden`.
- `applyNow(keys)` moves only those keys from pending into applied. Other keys stay pending.
- `changed` lists the keys whose text differs between the old and the new applied `saved`, global and source together.
- `onExperiment` fires for an assignment when the snapshot that holds it applies, and when `applyNow` names its key. It fires once per experiment and variant in the life of the applier. The `start` snapshot's assignments fire at creation.
- `beforeApply` runs before each apply. The Firebase path uses it to activate fetched values.
- `holding()` starts the window before the first screen. `ready` calls it. `holdEnded()` applies everything pending, in every mode, and ends the window.

- [ ] **Step 1: Write the failing tests**

One test per row of the table, for each mode (15 cases, a table-driven test), plus:

```ts
test("rest acts as hidden until the app first calls setAtRest", async () => {});
test("applyNow with keys applies only those keys", async () => {});
test("changed lists only the keys whose text changed", async () => {});
test("a test group is reported when its value applies, not when it lands", async () => {
  // mode hidden, in sight: landed(snapshot with assignment) → onExperiment not called; setVisible(false) → called once
});
test("a test group is reported once, and again when its variant changes", async () => {});
test("a pending value that never applies reports nothing", async () => {});
test("the start snapshot's groups are reported at creation", () => {});
test("a read that lands while the app is out of sight applies at once in every mode", async () => {});
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `npm test -w tauri-plugin-remote-settings-api`
Expected: FAIL.

- [ ] **Step 3: Implement `applier.ts`**

The applier holds no timer and reads no global state. The facade (Task 10) feeds it visibility and rest.

- [ ] **Step 4: Run the tests to see them pass**

Run: `npm test -w tauri-plugin-remote-settings-api`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/plugin-remote-settings/src/applier.ts packages/plugin-remote-settings/src/applier.test.ts
git commit -m "Apply new settings by the app's mode, and report a test group when its value shows"
```

---

### Task 8: The web path: files, store and install id

**Files:**
- Create: `packages/plugin-remote-settings/src/web.ts`
- Test: `packages/plugin-remote-settings/src/web.test.ts`

**Interfaces:**
- Consumes: `Files`, `Store` (Task 5).
- Produces:
  - `readFilesWeb(o: { baseUrl: string; stage: string; keyVersion: number; countryHeader: string; fetcher?: typeof fetch; signal?: AbortSignal }): Promise<Files>`
  - `webStore(namespace: string, storage?: () => Pick<Storage, "getItem" | "setItem"> | undefined): Store`
  - `webInstallId(store: Store, seed: string | null): Promise<string | null>`
  - `countryOf(header: string | null): string | null`

`readFilesWeb` reads `global.json` and `v<n>.json` at once. A file that fails gives `null` and leaves the other. The country comes from either answer, the global file's first. `countryOf` gives two capital letters, and `null` for `XX`, `T1`, an empty value or anything else. `webStore` prefixes each name with `remote-settings:<namespace>:` and treats any throw as "no value" or "not kept". `webInstallId` keeps the first good v4 UUID: the stored one, else `seed`, else a new one. It gives `null` when the store does not keep it.

- [ ] **Step 1: Write the failing tests**

```ts
test("both files are read from the base URL, stage and key version", async () => {
  // urls requested: https://settings.example.com/appa/production/global.json and .../v2.json
});
test("one failed file leaves the other", async () => {});
test("the country comes from the header, and XX or T1 is no country", async () => {});
test("a store whose storage throws gives null and keeps nothing, and never throws", async () => {});
test("the install id is kept, and the same id comes back", async () => {});
test("a seed install id is kept when there is none", async () => {});
test("a bad seed is ignored", async () => {});
test("with no storage there is no install id", async () => { assert.equal(await webInstallId(webStore("x", () => undefined), null), null); });
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `npm test -w tauri-plugin-remote-settings-api`
Expected: FAIL.

- [ ] **Step 3: Implement `web.ts`**

`webInstallId` writes the id, then reads it back. It gives the id only when the read gives the same id.

- [ ] **Step 4: Run the tests to see them pass**

Run: `npm test -w tauri-plugin-remote-settings-api`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/plugin-remote-settings/src/web.ts packages/plugin-remote-settings/src/web.test.ts
git commit -m "Read the settings files and keep the store and install id on the web"
```

---

### Task 9: The Rust crate

**Files:**
- Create: `crates/tauri-plugin-remote-settings/Cargo.toml`, `build.rs`, `permissions/default.toml`
- Create: `crates/tauri-plugin-remote-settings/src/lib.rs`, `src/read.rs`, `src/store.rs`
- Modify: `Cargo.toml` (`members`)
- Test: unit tests in each Rust file, and `crates/tauri-plugin-remote-settings/tests/commands.rs` with `tauri::test`

**Interfaces:**
- Produces (Rust):
  - `pub struct Config { base_url: String, stage: String, key_version: u32, country_header: String, timeout: Duration }`, built with `Config::new(base_url, stage, key_version)`, and `.country_header(..)` (default `"x-country"`) and `.timeout(..)` (default 5 s).
  - `pub fn init<R: Runtime>(config: Config) -> TauriPlugin<R>`
- Produces (commands, plugin name `remote-settings`, all camelCase in JSON):

| Command | Arguments | Answer |
|---|---|---|
| `config` | none | `{ baseUrl, stage, keyVersion, countryHeader, platform, version }` |
| `prefetched` | none | The read started at plugin start, as `{ global, rules, country }`. Waits for it. Gives it once. Later calls give `null`. |
| `read` | none | A new read, as `{ global, rules, country }` |
| `get` | `name: String` | The stored text, or `null` |
| `set` | `name: String, text: String` | nothing |
| `install_id` | `seed: Option<String>` | The id, or `null` |

`platform` is the `PLATFORM` constant of `tauri-plugin-update-gate`'s `lib.rs`. `version` comes from `app.package_info().version`.

The store folder:

| Target | Folder |
|---|---|
| Android | `app.path().app_data_dir()` joined with `no_backup/remote-settings`. Tauri gives Android's `dataDir`, and Android leaves `no_backup` out of its automatic backup. |
| Others | `app.path().app_local_data_dir()` joined with `remote-settings` |

`get` and `set` accept only names that match `^[a-z0-9-]{1,40}$`, so JS cannot write outside the folder. A refused name reads as `null` and writes nothing. `set` writes through a temp file and a rename, as `identity.rs` in `tauri-plugin-posthog-anon` does. `install_id` keeps the first good v4 UUID: the file `install-id`, else `seed`, else a new one. When the file cannot be written, it gives `null`.

The read caps each body at 1 MiB and reads both files at once on `spawn_blocking`. A status other than 2xx, a timeout or a body over the cap gives `null` for that file. The country is the header's two capital letters, `null` for `XX`, `T1` or anything else.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn a_name_with_a_slash_or_dots_is_refused() {}
#[test] fn set_then_get_gives_the_text_back() {}
#[test] fn the_install_id_is_kept_across_calls() {}
#[test] fn a_seed_id_is_kept_when_there_is_none() {}
#[test] fn a_bad_seed_is_ignored() {}
#[test] fn an_unwritable_folder_gives_no_install_id() {}
#[test] fn the_urls_are_built_from_base_stage_and_key_version() {
    // "https://settings.example.com/appa/" + "production" + 3 → ".../appa/production/global.json" and ".../appa/production/v3.json"
}
#[test] fn the_country_header_is_read_and_xx_is_none() {}
#[test] fn one_failed_file_leaves_the_other() {} // through a fake `Fetch` trait, as `source.rs` in update-gate does
#[test] fn prefetched_is_given_once() {}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test -p tauri-plugin-remote-settings`
Expected: FAIL, the crate does not exist.

- [ ] **Step 3: Implement the crate**

`init` spawns the first read in `setup` on `tauri::async_runtime::spawn`, and keeps its join handle in managed state behind a `Mutex<Option<..>>`, so `prefetched` takes it once. `permissions/default.toml` allows every command in the table.

- [ ] **Step 4: Run the checks**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p tauri-plugin-remote-settings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/tauri-plugin-remote-settings
git commit -m "Add tauri-plugin-remote-settings: read an app's settings files at start and keep a small store"
```

---

### Task 10: `init` and the public API

**Files:**
- Create: `packages/plugin-remote-settings/src/native.ts`, `src/index.ts`
- Test: `packages/plugin-remote-settings/src/index.test.ts`

**Interfaces:**
- Consumes: Tasks 5 to 9, and `FirebaseAdapter` (defined here, built in Task 11).
- Produces (`index.ts`):

```ts
export type FirebaseAdapter = {
  values(): Promise<Part | null>;
  fetch(): Promise<boolean>;
  activate(): Promise<boolean>;
  listen(on: () => void): Promise<() => void>;
};
export type Options<K extends Keys> = {
  keys: K;
  key: { version: number; value: string };
  web?: { baseUrl: string; stage: string; version: string; countryHeader?: string };
  defaultSource?: SourceName; // default "growthbook"
  apply?: Mode;               // default "hidden"
  attributes?: () => Record<string, unknown>;
  firebase?: { adapter: FirebaseAdapter; listen?: () => boolean };
  onExperiment?: (experiment: string, variant: string) => void;
  legacy?: { installId?: () => string | null; saved?: () => Saved | null };
};
export type Settings<K extends Keys> = {
  values(): { values: Values<K>; from: Record<keyof K, From>; sent: (keyof K)[] };
  used(): Used;
  country(): string | null;
  pending(): Values<K> | null;
  applyNow(keys?: (keyof K)[]): Promise<void>;
  subscribe(listener: (changed: (keyof K)[]) => void): () => void;
  setAtRest(atRest: boolean): void;
  ready(deadlineMs: number): Promise<void>;
  refresh(): Promise<void>;
  installId(): string | null;
};
export function init<K extends Keys>(o: Options<K>): Settings<K>;
```

`init` returns at once, with the saved values loaded as soon as the store answers. In a Tauri app it uses the commands (`native.ts`). It takes `baseUrl`, `stage`, `keyVersion` and `countryHeader` from the `config` command, and ignores `o.web`. Outside Tauri it uses `web.ts` and `o.web`. The store names it uses: `global`, `rules`, `state`.

One read:

1. Get the files: `prefetched` for the first read in an app, `read` after it, `readFilesWeb` on the web.
2. `globalPart` of the global file. The source is `pluginKeys(global).source`, else the saved one, else `defaultSource`.
3. With `growthbook`: `evaluate` the new rules text. When it gives a result, store the rules text. When it gives null, evaluate the stored rules text instead. With `firebase` and an adapter: `values()`, then `fetch()`. With `firebase` and no adapter: no source part.
4. Store the global text when `globalPart` is not null.
5. `nextSaved`, then store it as `state`, then `applier.landed`.

`refresh` runs at `visibilitychange` to visible, as in Ra Chơi, and aborts a read still running. The applier's mode is `pluginKeys(applied global).apply ?? o.apply ?? "hidden"`. While the source is `firebase` and `o.firebase.listen?.()` is true and the page is visible, the adapter listens. A heard update marks the fetched values, and the next apply calls `activate()` and `values()` first. No call to the adapter happens while the source is `growthbook`.

`legacy` runs once: when the store has no `state`, `legacy.saved()` seeds it. `legacy.installId()` is the `seed` for the install id.

- [ ] **Step 1: Write the failing tests**

Run the facade on the web path with a fake `fetch`, a `Map` store and a fake `document` visibility. Then run it again with `@tauri-apps/api/mocks` `mockIPC` for the native path.

```ts
test("the first values are the saved ones, before any read", async () => {});
test("a read in sight waits in hidden mode and applies when the page hides", async () => {});
test("a rules file that does not decrypt keeps the stored rules, and is not stored", async () => {});
test("with source firebase and no adapter, no source values come and nothing throws", async () => {});
test("with source growthbook the Firebase adapter is never called", async () => {});
test("with source firebase, fetched values activate at the next apply", async () => {});
test("the global file's apply word changes the mode", async () => {});
test("ready applies what landed before it resolved, then marks the first screen", async () => {});
test("ready resolves at the deadline when the read is slow", async () => {});
test("legacy saved values seed the first launch, and only the first", async () => {});
test("in a Tauri app the files come from the prefetched read, then from read", async () => {});
test("the attributes carry the install id, the platform, the version and the country", async () => {});
test("an app attribute adds to the standard ones and cannot replace one", async () => {
  // attributes: () => ({ plan: "paid", platform: "x" }) → a rule on plan matches, a rule on platform = "web" still matches on the web
});
test("no exported function rejects when every command fails", async () => {});
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `npm test -w tauri-plugin-remote-settings-api`
Expected: FAIL.

- [ ] **Step 3: Implement `native.ts` and `index.ts`**

- [ ] **Step 4: Run all package checks**

Run: `npm test -w tauri-plugin-remote-settings-api && npm run typecheck -w tauri-plugin-remote-settings-api && npm run build -w tauri-plugin-remote-settings-api`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/plugin-remote-settings/src
git commit -m "Add init and the remote settings API, on the web and in a Tauri app"
```

---

### Task 11: The Firebase adapter, and the plugin README

**Files:**
- Create: `packages/plugin-remote-settings/src/firebase.ts`, `packages/plugin-remote-settings/README.md`
- Modify: `README.md` (a section for `tauri-plugin-remote-settings`)
- Test: `packages/plugin-remote-settings/src/firebase.test.ts`

**Interfaces:**
- Consumes: `FirebaseAdapter` (Task 10). `fetch`, `activate`, `getAll`, `listen` from `tauri-plugin-firebase-api`.
- Produces: `firebaseAdapter(api?: Pick<typeof import("tauri-plugin-firebase-api"), "fetch" | "activate" | "getAll" | "listen">): FirebaseAdapter`

`values()` keeps only entries with `source === "remote"`. `fetch()` is true only for `"fetched"`. A throw in any call gives `null`, `false`, or a stop function that does nothing.

- [ ] **Step 1: Write the failing tests**

```ts
test("only the values Remote Config sent are given", async () => {});
test("fetch is true only when new values came", async () => {});
test("a failing Firebase call never throws", async () => {});
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `npm test -w tauri-plugin-remote-settings-api`
Expected: FAIL.

- [ ] **Step 3: Implement `firebase.ts`. Write the README.**

The README covers, in this order:

1. What the plugin does, and the cost model: a read hits the cache and runs no Worker.
2. Setup in Rust (`Config::new`), the capability `remote-settings:default`, and `init` in JS, with a short example.
3. The keys and their readers. The rule that each default shows the least.
4. The three modes, `setAtRest`, `applyNow`, `subscribe`, and `ready`.
5. Test groups: when `onExperiment` fires.
6. Firebase as an option: `firebaseAdapter()`, and the `source` key.
7. What to tell users: the plugin sends nothing about the user, and the facts an app's privacy policy needs.
8. The Android backup rule: the store sits in `no_backup`.
9. Moving an app: `legacy`.

- [ ] **Step 4: Run every check in the repository**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && npm test --workspaces --if-present && npm run typecheck --workspaces --if-present && npm run build --workspaces --if-present`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add packages/plugin-remote-settings README.md
git commit -m "Add the Firebase adapter and the remote settings guide"
```

---

### Task 12: The open checks from the spec

This task needs the owner's GrowthBook organization and Cloudflare account. It writes no code here. It records what it finds in `packages/remote-settings-hub/README.md`.

- [ ] **Step 1:** (2026-10-22: done for features, saved groups and constants, through the deployed hub, not a request bin. Not done: an experiment, which needs a data source, and an SDK connection's projects. See "What GrowthBook sends" in the README.) In the owner's GrowthBook organization, make one event webhook for every event to a request bin. Change a feature, an experiment, a saved group and a constant. Edit an SDK connection's projects. Write down which change sent an event.
- [x] **Step 2:** (2026-10-22, read on the staging connection, because production had no features: each feature sits under its full key. A saved group comes in as `$in`, not `$inGroup`, while saved group references are off.) Read the production connection's CDN payload. Check that each feature sits under its full key, prefix included, and that a rule names a saved group with `$inGroup`.
- [x] **Step 3:** (2026-10-22: real events from GrowthBook pass `verifyEventSignature` and get 200.) Use the webhook's Test button against a deployed hub. Check that `X-GrowthBook-Signature` passes `verifyEventSignature`. If it fails, compare the signature with the HMAC of the body as sent, and fix `signature.ts` with a test from the real request.
- [ ] **Step 4:** Build a test app with the plugin on an Android emulator. Read the store folder's path in the log, and check that it ends in `no_backup/remote-settings`. Measure the time from the app's start to the first `prefetched` answer, against a read from the webview.
- [ ] **Step 5:** Commit the README changes.

```bash
git add packages/remote-settings-hub/README.md
git commit -m "Record what the GrowthBook and device checks found"
```
