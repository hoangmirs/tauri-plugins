# tauri-plugin-update-gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Tauri plugin that reads a JSON document over HTTP and tells the app whether this installed copy is too old to keep playing.

**Architecture:** Pure Rust, no native code, so one crate serves macOS, Windows, Linux, iOS and Android. The decision (compare versions, pick message and URL) is a pure function with no I/O, tested on its own. HTTP sits behind a `Fetch` trait so tests never touch the network. The JS package exposes one `check()` that calls the command under Tauri and returns `{ state: "ok" }` on the plain web.

**Tech Stack:** Rust 2021, `tauri` 2.11.5, `semver`, `reqwest` (rustls), `serde`, TypeScript, `node --test`.

**Spec:** `docs/superpowers/specs/2026-09-18-update-gate-and-posthog-design.md`

## Global Constraints

- **Every dependency pinned exactly** (`=1.2.3`), latest version at the time of writing.
- **Crate version `0.1.0-alpha.1`**, licence MIT, repository `https://github.com/hoangmirs/tauri-plugins`.
- **The gate fails open.** Any error — network, HTTP status, malformed JSON, unparseable version — returns `state: "ok"`. There is no path that returns `forced` from a failure.
- **No secrets, no product detail** in this repo. The URL is passed in by the app.
- **The plugin renders no UI** and reads the running version from Tauri's `PackageInfo`, never from an argument.
- **Lints:** `unsafe_code = "forbid"`, clippy `all` and `pedantic` at warn, matching gattify.
- **Tests must pass with no network.**

---

### Task 1: Workspace skeleton

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `LICENSE`, `README.md`, `.gitignore`
- Create: `crates/tauri-plugin-update-gate/Cargo.toml`, `crates/tauri-plugin-update-gate/src/lib.rs`
- Create: `package.json`

**Interfaces:**
- Consumes: nothing.
- Produces: a cargo workspace whose member is `crates/tauri-plugin-update-gate`, and an npm workspace over `packages/*`.

- [ ] **Step 1: Write the workspace root**

`Cargo.toml` — copy gattify's shape (`~/Documents/Projects/Personal/gattify/Cargo.toml`): `[workspace] members = ["crates/tauri-plugin-update-gate"]`, `resolver = "2"`, shared `[workspace.package]` (version `0.1.0-alpha.1`, edition 2021, licence MIT, repository), `[workspace.dependencies]` with every crate pinned `=`, and the two lint tables.

`package.json`: `{ "name": "tauri-plugins-workspace", "private": true, "workspaces": ["packages/*"] }`.

- [ ] **Step 2: Write the crate manifest**

`crates/tauri-plugin-update-gate/Cargo.toml` with `tauri`, `serde`, `serde_json`, `semver`, `thiserror`, `reqwest` (`default-features = false`, `rustls-tls`, `json`), `log`, and `[build-dependencies] tauri-plugin` (the `build` feature). Add `links = "tauri-plugin-update-gate"`.

- [ ] **Step 3: Verify it builds**

Run: `cargo check --workspace`
Expected: success (`src/lib.rs` may be empty for now).

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "Start the workspace the two plugins live in"
```

---

### Task 2: The decision, as a pure function

**Files:**
- Create: `crates/tauri-plugin-update-gate/src/decide.rs`
- Modify: `crates/tauri-plugin-update-gate/src/lib.rs` (add `mod decide;`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  ```rust
  pub struct Document { pub min_version: Option<String>, pub latest_version: Option<String>,
                        pub message: HashMap<String, String>, pub url: HashMap<String, Option<String>> }
  pub enum State { Ok, Optional, Forced }          // serde: lowercase
  pub struct Gate { pub state: State, pub latest_version: Option<String>,
                    pub message: Option<String>, pub url: Option<String> }
  pub fn decide(doc: &Document, running: &str, lang: &str, platform: &str) -> Gate
  ```

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn below_min_is_forced() {
    let g = decide(&doc(r#"{"minVersion":"1.2.0","latestVersion":"1.4.0"}"#), "1.1.9", "vi", "ios");
    assert!(matches!(g.state, State::Forced));
}

#[test]
fn between_min_and_latest_is_optional() {
    let g = decide(&doc(r#"{"minVersion":"1.2.0","latestVersion":"1.4.0"}"#), "1.3.0", "vi", "ios");
    assert!(matches!(g.state, State::Optional));
}

#[test]
fn newest_is_ok() {
    let g = decide(&doc(r#"{"minVersion":"1.2.0","latestVersion":"1.4.0"}"#), "1.4.0", "vi", "ios");
    assert!(matches!(g.state, State::Ok));
}

#[test]
fn a_prerelease_is_older_than_its_release() {
    let g = decide(&doc(r#"{"minVersion":"1.2.0"}"#), "1.2.0-alpha.1", "vi", "ios");
    assert!(matches!(g.state, State::Forced));
}

#[test]
fn an_unreadable_version_opens_the_gate() {
    let g = decide(&doc(r#"{"minVersion":"not a version"}"#), "1.0.0", "vi", "ios");
    assert!(matches!(g.state, State::Ok));
    let g = decide(&doc(r#"{"minVersion":"9.0.0"}"#), "nonsense", "vi", "ios");
    assert!(matches!(g.state, State::Ok));
}

#[test]
fn an_empty_document_opens_the_gate() {
    let g = decide(&doc("{}"), "1.0.0", "vi", "ios");
    assert!(matches!(g.state, State::Ok));
    assert!(g.message.is_none());
}

#[test]
fn it_picks_the_language_and_falls_back_to_english() {
    let d = doc(r#"{"minVersion":"2.0.0","message":{"vi":"Cập nhật nha","en":"Update"}}"#);
    assert_eq!(decide(&d, "1.0.0", "vi", "ios").message.as_deref(), Some("Cập nhật nha"));
    assert_eq!(decide(&d, "1.0.0", "fr", "ios").message.as_deref(), Some("Update"));
}

#[test]
fn it_picks_the_url_for_this_platform_and_tolerates_null() {
    let d = doc(r#"{"minVersion":"2.0.0","url":{"ios":"https://apps.apple.com/x","android":null}}"#);
    assert_eq!(decide(&d, "1.0.0", "vi", "ios").url.as_deref(), Some("https://apps.apple.com/x"));
    assert_eq!(decide(&d, "1.0.0", "vi", "android").url, None);
    assert_eq!(decide(&d, "1.0.0", "vi", "macos").url, None);
}
```

`fn doc(s: &str) -> Document { serde_json::from_str(s).unwrap() }` — the test helper also proves unknown fields and absent fields parse.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p tauri-plugin-update-gate`
Expected: FAIL — `decide` not found.

- [ ] **Step 3: Implement `decide`**

Parse `running`, `min_version` and `latest_version` with `semver::Version::parse`. A parse error on any of them means `State::Ok` with no message. Otherwise: `running < min` → `Forced`; `running < latest` → `Optional`; else `Ok`. Message: `message[lang]`, else `message["en"]`, else `None`. URL: `url[platform]` flattened, so a JSON `null` and a missing key both give `None`. `Document` derives `Deserialize` with `#[serde(rename_all = "camelCase", default)]`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p tauri-plugin-update-gate`
Expected: PASS, 8 tests.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "Decide whether a version may still play"
```

---

### Task 3: Fetching, caching, and failing open

**Files:**
- Create: `crates/tauri-plugin-update-gate/src/source.rs`
- Modify: `crates/tauri-plugin-update-gate/src/lib.rs`

**Interfaces:**
- Consumes: `decide`, `Document`, `Gate` from Task 2.
- Produces:
  ```rust
  pub struct Config { url: String, ttl: Duration, timeout: Duration }
  impl Config { pub fn url(u: impl Into<String>) -> Self;      // ttl 6h, timeout 5s
                pub fn ttl(self, d: Duration) -> Self;
                pub fn timeout(self, d: Duration) -> Self; }
  #[async_trait] pub trait Fetch: Send + Sync { async fn get(&self, url: &str) -> Result<String, String>; }
  pub struct Source { /* config, fetcher, cache: Mutex<Option<(Instant, Document)>> */ }
  impl Source { pub async fn gate(&self, running: &str, lang: &str, platform: &str) -> Gate }
  ```

- [ ] **Step 1: Write the failing tests**

```rust
struct Canned(Result<String, String>, Arc<AtomicUsize>);   // body, call count

#[tokio::test]
async fn a_network_error_opens_the_gate() {
    let s = Source::with(Config::url("http://x"), Canned(Err("offline".into()), n()));
    assert!(matches!(s.gate("1.0.0", "vi", "ios").await.state, State::Ok));
}

#[tokio::test]
async fn malformed_json_opens_the_gate() {
    let s = Source::with(Config::url("http://x"), Canned(Ok("<html>404</html>".into()), n()));
    assert!(matches!(s.gate("1.0.0", "vi", "ios").await.state, State::Ok));
}

#[tokio::test]
async fn it_fetches_once_inside_the_ttl() {
    let calls = n();
    let s = Source::with(Config::url("http://x"), Canned(Ok(r#"{"minVersion":"2.0.0"}"#.into()), calls.clone()));
    assert!(matches!(s.gate("1.0.0", "vi", "ios").await.state, State::Forced));
    assert!(matches!(s.gate("1.0.0", "vi", "ios").await.state, State::Forced));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn it_fetches_again_once_the_ttl_has_passed() {
    let calls = n();
    let s = Source::with(Config::url("http://x").ttl(Duration::ZERO),
                         Canned(Ok(r#"{"minVersion":"2.0.0"}"#.into()), calls.clone()));
    s.gate("1.0.0", "vi", "ios").await;
    s.gate("1.0.0", "vi", "ios").await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p tauri-plugin-update-gate`
Expected: FAIL — `Source` not found.

- [ ] **Step 3: Implement `Source`**

`Source::with(config, fetcher)` for tests; `Source::new(config)` builds a `reqwest::Client` with `Config::timeout`. `gate()` returns the cached `Document` when `Instant::now() - at < ttl`, else fetches, parses, caches on success. Any `Err`, or a parse failure, logs at `warn` and returns `Gate { state: Ok, .. }` — do not cache a failure. The reqwest fetcher treats a non-2xx status as `Err`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p tauri-plugin-update-gate`
Expected: PASS, 12 tests.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "Read the document, keep it briefly, and never shut on a failure"
```

---

### Task 4: The plugin, its command and its permission

**Files:**
- Create: `crates/tauri-plugin-update-gate/build.rs`
- Create: `crates/tauri-plugin-update-gate/permissions/default.toml`
- Modify: `crates/tauri-plugin-update-gate/src/lib.rs`

**Interfaces:**
- Consumes: `Source`, `Config`, `Gate` from Task 3.
- Produces: `pub fn init<R: Runtime>(config: Config) -> TauriPlugin<R>`, command `check`, permission `update-gate:default`.

- [ ] **Step 1: Write `build.rs` and the permission**

```rust
const COMMANDS: &[&str] = &["check"];
fn main() { tauri_plugin::Builder::new(COMMANDS).build(); }
```

`permissions/default.toml`: description "Ask whether this copy of the app is still allowed to run", `permissions = ["allow-check"]`.

- [ ] **Step 2: Write the command**

```rust
#[tauri::command]
async fn check<R: Runtime>(app: AppHandle<R>, source: State<'_, Source>) -> Result<Gate, ()> {
    let running = app.package_info().version.to_string();
    Ok(source.gate(&running, &language(&app), PLATFORM).await)
}
```

`PLATFORM` is `"ios" | "android" | "macos" | "windows" | "linux"` from `cfg!(target_os)`. `language` reads the two-letter code the OS reports, lowercased. `init` builds the `Source`, manages it as state, and registers the handler.

- [ ] **Step 3: Verify it compiles and the permission is generated**

Run: `cargo build -p tauri-plugin-update-gate`
Expected: success, and `permissions/autogenerated/commands/check.toml` now exists.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "Ask the gate from the app, through one command"
```

---

### Task 5: The JS package, on Tauri and on the plain web

**Files:**
- Create: `packages/plugin-update-gate/package.json`, `src/index.ts`, `src/index.test.ts`, `tsconfig.json`

**Interfaces:**
- Consumes: the `check` command from Task 4.
- Produces: npm package `tauri-plugin-update-gate-api` exporting
  ```ts
  export type Gate = { state: "ok" | "optional" | "forced"; latestVersion: string | null;
                       message: string | null; url: string | null };
  export function checkUpdate(): Promise<Gate>;
  ```

- [ ] **Step 1: Write the failing test**

```ts
test("a browser with no Tauri runtime never gates", async () => {
  delete (globalThis as Record<string, unknown>).__TAURI_INTERNALS__;
  const { checkUpdate } = await import("./index.ts");
  assert.deepEqual(await checkUpdate(), { state: "ok", latestVersion: null, message: null, url: null });
});

test("under Tauri it returns what the command answered", async () => {
  (globalThis as Record<string, unknown>).__TAURI_INTERNALS__ = {
    invoke: async () => ({ state: "forced", latestVersion: "1.4.0", message: "Cập nhật nha", url: "https://x" }),
  };
  const { checkUpdate } = await import("./index.ts?2");
  assert.equal((await checkUpdate()).state, "forced");
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `node --test --experimental-strip-types "packages/plugin-update-gate/src/*.test.ts"`
Expected: FAIL — cannot find `./index.ts`.

- [ ] **Step 3: Implement it**

```ts
const OPEN: Gate = { state: "ok", latestVersion: null, message: null, url: null };
const onTauri = () => "__TAURI_INTERNALS__" in globalThis;

export async function checkUpdate(): Promise<Gate> {
  if (!onTauri()) return OPEN;                       // the web always serves the newest code
  try { return await invoke<Gate>("plugin:update-gate|check"); }
  catch { return OPEN; }                             // a gate that cannot be read is open
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `node --test --experimental-strip-types "packages/plugin-update-gate/src/*.test.ts"`
Expected: PASS, 2 tests.

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "One check, whether or not Tauri is underneath"
```

---

### Task 6: README and CI

**Files:**
- Create: `.github/workflows/ci.yml`
- Modify: `README.md`

**Interfaces:**
- Consumes: everything above.
- Produces: a green check on every push.

- [ ] **Step 1: Write the workflow**

Ubuntu runner, on push and pull request: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `npm ci`, `npm test --workspaces --if-present`. No Tauri system dependencies are needed for a crate that never builds a window.

- [ ] **Step 2: Write the README**

What each plugin is, the two setup snippets from the spec, the JSON document's shape, and the rule that the gate fails open.

- [ ] **Step 3: Run the checks locally**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && npm test --workspaces --if-present`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add -A && git commit -m "Check the workspace on every push"
```

---

## Self-review

- **Spec coverage.** Document shape → Task 2. `check()` and its three states → Tasks 2 and 4. Fail open → Tasks 2, 3 and 5. `ttl`, `timeout`, version from `PackageInfo` → Tasks 3 and 4. Semver with pre-releases → Task 2. No UI → nothing renders anywhere. Web no-op → Task 5. Permissions → Task 4. Pinned, MIT, lints → Task 1. Tests without a network → the `Fetch` seam in Task 3.
- **Not covered here, by design.** `tauri-plugin-posthog` gets its own plan, and wiring Ra chơi's update screen belongs to a plan in `rachoi-app`.
- **Names checked across tasks:** `decide`, `Document`, `Gate`, `State`, `Config::url/ttl/timeout`, `Source::new/with/gate`, `Fetch::get`, command `check`, JS `checkUpdate`. Consistent.
