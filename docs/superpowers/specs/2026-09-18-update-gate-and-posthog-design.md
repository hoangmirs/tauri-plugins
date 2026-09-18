# Two plugins: a version gate, and a way to count what players do

## Why

Ra chơi ships to five places from one codebase, and the apps that follow it will
too. Two things every one of them needs, and neither belongs in the app:

- **A way to tell an old copy it is too old.** A phone cannot know a newer build
  exists, and a relay change can leave an old copy talking nonsense.
- **A way to count what players do.** Which rooms finish a game, how many games
  a table plays, whether a host comes back next week. Without it, every product
  decision is a guess.

Each app should get both in a line or two, so `hoangmirs/tauri-plugins` holds
them: a cargo workspace of two crates, an npm workspace of their two API
packages. Public and MIT, like gattify, so nothing about a product goes in.

## The constraints

1. **A Tauri plugin does not exist on the web.** Ra chơi also deploys as a plain
   web build at `rachoi.techtipsvn.com`, where `invoke` is not there to call.
   Every plugin here therefore ships a JS package with two paths behind one
   function, and the app never branches.
2. **Tables play with no internet.** Bluetooth rooms run offline for a whole
   session. Events that live only in memory die with the app, so the queue
   belongs on disk — which is the reason the counting side is Rust at all.
3. **Rust reaches every target; native code would not.** Both plugins speak
   HTTP and nothing else, so neither needs a line of Swift or Kotlin. That is a
   deliberate limit: anything needing a native SDK does not belong here.
4. **Firebase Remote Config has no client REST API.** The documented endpoints
   are the admin ones, behind a service account. Its clients register a Firebase
   Installation first, which is a lot of Rust for one number. So the gate reads
   plain JSON over `GET`, and Firebase, if it is ever wanted, sits behind a
   server that serves that same JSON.
5. **A gate is dangerous.** A wrong `minVersion` locks every player out of the
   app, and they cannot undo it. The gate fails open, always.

## Design

### 1. `tauri-plugin-update-gate`

Reads one JSON document and answers one question: may this copy keep playing?

**The document** (Ra chơi's lives at `rachoi.techtipsvn.com/app-config.json`):

```json
{ "minVersion": "0.0.0", "latestVersion": "0.1.0",
  "message": { "vi": "…", "en": "…" },
  "url": { "ios": null, "android": null, "macos": null } }
```

Unknown fields are ignored, so an app may keep its own keys in the same file.

**Per-platform versions** (added 2026-09-18). One app version ships to every
platform, but the stores do not release it on the same day: Android is live
in hours, iOS waits for review. So a document may carry
`"platforms": { "ios": { "minVersion": "…", "latestVersion": "…" } }`. A
platform's own field wins; a field it leaves out, a `null` entry, or no entry
falls back to the top level. An unreadable version in an override opens the
gate on that platform only. The answer's `latestVersion` is the platform's own.

**Setup**, in the app's `lib.rs`:

```rust
.plugin(tauri_plugin_update_gate::init(
    Config::url("https://rachoi.techtipsvn.com/app-config.json"),
))
```

`Config` also carries `ttl` (default 6 hours) and `timeout` (default 5 seconds).
Nothing else: the running version comes from Tauri's own `PackageInfo`, so no
app passes its version in and no app can pass the wrong one.

**One command.** `check()` answers:

```ts
type Gate = { state: "ok" | "optional" | "forced";
              latestVersion: string | null;
              message: string | null;  // picked for the app's language
              url: string | null };    // picked for this platform
```

- `forced` when the running version is below `minVersion`;
- `optional` when it is below `latestVersion`;
- `ok` otherwise — **and on every failure**: no network, a 500, malformed JSON,
  a `minVersion` that is not a version. A gate that cannot be read is open.

Comparison is semver, through the `semver` crate, so `1.2.0-alpha.1` sorts below
`1.2.0` as it should. The answer is cached for `ttl` in memory; the process
restarting re-fetches. The plugin renders nothing — the update screen is the
app's, in the app's language and brand.

**On the web** the JS package returns `{ state: "ok" }` without a request. A
browser always loads the newest code, so there is nothing to gate.

### 2. `tauri-plugin-posthog`

Sends events to PostHog's `/batch/`, from a queue that survives the app closing.

**Setup:**

```rust
.plugin(tauri_plugin_posthog::init(Config::new(POSTHOG_KEY).eu()))
```

**Use:**

```ts
await track("game_finished", { game: "caro", players: 3 });
```

**What the plugin adds to every event**, so no app repeats it: `$app_version`,
`$os`, `$lib`, `$lib_version`, the device's locale, and
`$process_person_profile: false` — which is both the privacy stance and the
cheaper rate PostHog bills anonymous events at.

**Identity.** A random UUID, made once and kept in the app's data directory.
There is no `identify`, no way to pass one in, and no user properties. These
apps are played by schoolchildren; the plugin cannot be the thing that collects
a name.

**The queue.** JSON lines in the app data directory. Appended on capture,
flushed when 20 events are waiting or 30 seconds pass, whichever comes first,
and again on launch and on exit. A send that fails keeps the events and retries
with backoff. Above 1,000 events or 1 MB the oldest go first — a table that
plays offline all evening must not fill a phone.

**Opting out.** `setOptOut(true)` stops capture and drops the queue; the choice
persists. Ra chơi will put the switch in Settings.

**On the web** the same `track` posts to the same endpoint with the same
payload, queued in `localStorage`. It does not use `posthog-js`: the events here
are deliberately coarse, autocapture and replay are unwanted, and one hand-built
payload keeps web and native events identical — two shapes would quietly split
every funnel. The key comes from the app's build env there, since Rust's config
is not in the browser.

### 3. Both plugins

- **Permissions.** `update-gate:default` allows `check`. `posthog:default`
  allows `capture`, `flush` and `set-opt-out`. An app opts in through its
  capability file, the way Ra chơi already does for gattify.
- **No secrets, no product.** Keys, URLs and copy are passed in by the app.
- **Versions pinned exactly**, `0.1.0-alpha.1` to start, MIT, one
  `rust-toolchain.toml`, clippy pedantic — gattify's setup, copied.
- **Tests run without a network.** The parts worth testing are pure: comparing
  versions and choosing a state, picking message and URL, building a payload,
  evicting the queue. HTTP is one seam behind a trait, faked in tests. The JS
  packages test under `node --test`, as Ra chơi does.

## What this does not do

- No native SDKs, no push, no crash reporting, no feature flags. Flags would
  spend PostHog's 1M free requests a month on a thing the gate already does.
- No UI.
- No Expo support. bloom-app and the other Expo apps cannot load a Tauri plugin;
  what they share with these is the shape of the JSON, not the code.
