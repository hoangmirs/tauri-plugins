# tauri-plugins

MIT-licensed Tauri v2 plugins, built so an app needs only a line or two to
get each one. No product names, no secrets: everything product-specific is
passed in by the app that uses them.

## `tauri-plugin-update-gate`

Reads one JSON document over HTTP and tells an app whether the installed
copy is too old to run. The plugin renders nothing — the update screen, in
the app's own language and brand, is the app's job.

### The gate fails open

If the document can't be fetched, the response is over 64 KiB or isn't
valid JSON, or a version field doesn't parse as semver, the gate answers
`ok` rather than raising an error. **A gate that cannot be read must never lock anyone out.**
Correspondingly: never raise `minVersion` to a version the app stores
haven't actually released yet — the moment you do, every install still on
an older build is locked out with no way to update.

### The JSON document

```json
{
  "minVersion": "1.2.0",
  "latestVersion": "1.4.0",
  "message": { "en": "A new version is available.", "vi": "Đã có bản cập nhật." },
  "url": { "ios": null, "android": null, "macos": "https://example.com/download" },
  "platforms": { "ios": { "minVersion": "1.1.0" } }
}
```

- `minVersion` / `latestVersion` — semver strings (pre-releases like
  `1.2.0-alpha.1` compare as older than their release). Either may be
  omitted. Leave build metadata (`+…`) out of them: semver compares it.
- `message` — a map of language code to the text shown to the player. The
  command picks the entry for the resolved language, falling back to `en`.
  Keys are two-letter language codes only (`en`, `vi`, `pt`): the resolved
  language is the part before the first `-`, `_` or `.`, and must be two
  letters, so `pt-BR` and `zh-Hant` look up `pt` and `zh`, and a
  three-letter code such as `fil` is never used.
- `url` — a map of platform (`ios`, `android`, `macos`, `windows`, `linux`)
  to a download link, or `null` when there isn't one for that platform.
- `platforms` — optional. A platform's own `minVersion` and/or
  `latestVersion`, for when the stores release on different days: raise the
  top level once Android has 1.3.0, and hold iOS back until its review
  passes. A field a platform leaves out comes from the top level, and
  `latestVersion` in the answer is that platform's own. In the example
  above, iOS forces below 1.1.0 while everything else forces below 1.2.0.
- Any other field in the document is ignored, so an app can keep its own
  keys in the same file without the plugin tripping over them.

### Setup (Rust)

```rust
use tauri_plugin_update_gate::Config;

tauri::Builder::default()
    .plugin(tauri_plugin_update_gate::init(
        Config::url("https://example.com/app-config.json"),
    ))
```

`Config::url` also takes `.ttl(Duration)` (default 6 hours — how long a
fetched document is trusted before it's fetched again) and
`.timeout(Duration)` (default 5 seconds — how long the fetch waits for a
response). TLS is verified against the Mozilla root bundle compiled into
the plugin, not the OS store, so serve the document from a host with a
publicly trusted certificate. The running version always comes from
Tauri's own
`PackageInfo`; no app passes its version in, so none can pass the wrong
one.

The app must also enable the plugin's default permission in its capability
file:

```json
{ "permissions": ["update-gate:default"] }
```

### Use (TypeScript)

```ts
import { checkUpdate } from "tauri-plugin-update-gate-api";

const gate = await checkUpdate(myAppsChosenLanguage);
// gate.state: "ok" | "optional" | "forced"
```

`checkUpdate(lang?: string, options?: { timeoutMs?: number })` calls the
`check` command and resolves to:

```ts
type Gate = {
  state: "ok" | "optional" | "forced";
  latestVersion: string | null;
  message: string | null; // picked for `lang`, already localized
  url: string | null;     // picked for this platform
};
```

- `forced` — the running version is below `minVersion`.
- `optional` — it's below `latestVersion` but not below `minVersion`.
- `ok` — otherwise, and on every failure (see "The gate fails open" above).

**Pass `lang` explicitly.** If you don't, the package sends
`navigator.language`, which every Tauri webview defines — so the message
comes back in the OS's UI language, not the one the app is showing. Pass
the language the app itself is showing, from its own Settings, so the
message matches it. (Only when there is no `navigator.language` at all does
the Rust side fall back to the OS locale environment — `LC_ALL`, then
`LC_MESSAGES`, then `LANG` — and then `"en"`.)

`checkUpdate` never rejects. If the command doesn't answer within
`timeoutMs` (default 10 000), it resolves to the open gate. Keep
`timeoutMs` above the Rust `.timeout(...)` (default 5 seconds), so a slow
network is answered by the plugin, and this only fires when the command
itself never answers. A `timeoutMs` that isn't a positive, finite number of
milliseconds a timer can hold (0, a negative, `NaN`, above 2³¹−1) means the
default, since a timer that fires at once would switch the gate off.

**Confirm the gate is live.** A missing `update-gate:default` capability,
or a plugin that was never registered, makes `checkUpdate()` quietly
resolve to the open gate, exactly as an unreachable document does. Once, in
a dev build, point the app at a document whose `minVersion` is above the
running version and check that it answers `forced`.

On the plain web (no Tauri runtime) `checkUpdate` resolves to the open gate
without making a request, since a browser always loads the newest code.

The command signature is `Result<Gate, String>` because Tauri requires
`Result` of an async command that borrows managed state — it never
actually returns `Err`; every failure is folded into the open gate before
`check` returns.

A panic in the network stack is caught and answered as the open gate too —
but only when the app is built with `panic = "unwind"`, Rust's default. An
app built with `panic = "abort"` crashes on such a panic instead of failing
open.

## `tauri-plugin-posthog`

Queues anonymous usage events on disk (or, on the plain web, in
`localStorage`) and sends them to PostHog in batches. No product names, no
secrets: the project key is public by design (PostHog's ingestion keys are
meant to ship in client code) and is passed in by the app, same as every
other plugin here.

### Anonymous only

Every event carries `"$process_person_profile": false`, which a caller's
own properties can never override — PostHog never creates a person profile
for it, and bills it at its cheaper anonymous-event rate. There is no
`identify` call, no way to pass a `distinct_id`, and no person properties.
`distinct_id` is a random v4 UUID made once per install (once per browser
on the web) and kept on disk — an app data file, or `localStorage` on the
web — so the same install is recognizable across launches without being
identifiable.

An event's `properties` are the caller's own, merged onto what the plugin
adds — `distinct_id`, `$process_person_profile: false`, `$lib`
(`tauri-plugin-posthog`, or `tauri-plugin-posthog-web` on the plain web),
`$lib_version`, `$app_version` (the running `PackageInfo` version under
Tauri; whatever `appVersion` was passed to `init` on the web), `$os` (`iOS`,
`Android`, `macOS`, `Windows`, `Linux`, or `Web`), and `platform` (the same,
lowercased, `windows`/`linux`/`web`/…). The JS side also adds `$locale`
from `navigator.language`, when there is one, since only it can see the
browser's locale. A caller's property of the same name overrides the
plugin's own — except `distinct_id` and `$process_person_profile`, which
are re-applied last and can never be shadowed. Each event also carries its
own `uuid` (v4) and its capture-time UTC `timestamp`, both assigned once
and kept unchanged through however many retries it takes to send — so a
resend lands as the same event, and PostHog drops the duplicate by `uuid`.

### Setup (Rust)

```rust
use tauri_plugin_posthog::Config;

tauri::Builder::default()
    .plugin(tauri_plugin_posthog::init(Config::new("phc_your_project_key").eu()))
```

`Config::new(key)` defaults to PostHog's US cloud; `.eu()` switches to the
EU cloud, `.us()` back to US, and `.host(url)` points at a self-hosted
instance or a reverse proxy. TLS is verified against the Mozilla root
bundle compiled into the plugin, not the OS store — same as update-gate,
and for the same reason: the platform verifier needs JNI set up on
Android, which a plugin with no native code cannot do, and panics without
it.

The app must also enable the plugin's default permission in its capability
file:

```json
{ "permissions": ["posthog:default"] }
```

### Use (TypeScript)

```ts
import { init, track, flush, setOptOut, isOptedOut } from "tauri-plugin-posthog-api";

init({ apiKey: "phc_your_project_key", host: "https://eu.i.posthog.com", appVersion: "1.2.3" });
await track("game_finished", { moves: 12 });
await flush();
await setOptOut(true);
await isOptedOut(); // false once opted back in
```

- `init({ apiKey, host?, appVersion? })` — **web only.** Under Tauri it's a
  no-op, since Rust already holds its own config from `Config` at plugin
  `init`; `host` defaults to PostHog's US cloud, and `appVersion` (the
  app's own version, not the package's) feeds `$app_version` on events sent
  from the web.
- `track(event, properties?)` — queues an event, the same shape whichever
  side sends it, so PostHog can't tell.
- `flush()` — asks for a flush now. Under Tauri it returns as soon as the
  worker has been asked, without waiting for the network; on the web it
  awaits the request itself.
- `setOptOut(out)` — opts the install out or back in.
- `isOptedOut()` — the install's current opt-out state.

None of the five ever rejects: a dead command, a throwing `localStorage`, a
network failure — every failure is swallowed and logged (Rust side, at
`warn`), never surfaced to the caller. Analytics must never be the reason
an app crashes or shows an error.

### Queue, flushing, and retries

Events wait in a queue — a JSON-lines file in the app's data directory
under Tauri, a JSON array in `localStorage` on the web — until they're
sent. Past **1,000 events or 1 MB**, whichever comes first, the oldest
events are dropped to make room; a single event whose own serialized size
is already over the 1 MB limit is refused outright, since eviction can
only make room by dropping *other* events.

A flush is triggered when **20 events** are waiting, every **30 seconds**,
once on launch, best-effort on app exit (waited for at most **2 seconds**;
whatever hasn't sent by then stays queued for next launch), and — web
only — on `pagehide`. Each request carries at most **100 events**; a
larger queue is sent in several requests. Every request is bounded by a
**10-second** timeout, treated the same as a network failure. A failed
send keeps its events queued and backs off before retrying: **30 seconds**
the first time, doubling on each failure after that, capped at **10
minutes**; any successful send resets the backoff to zero. Because each
event keeps the same `uuid` across retries, a batch that PostHog actually
received but whose response was lost is simply de-duplicated on resend.

### Opting out

`setOptOut(true)` takes effect before the call returns: capture stops at
once, the choice is saved (and persists across launches), and a flush
already in flight stops after the batch currently on the wire — at most
one batch sent before the opt-out can still land. The queued events
themselves are deleted at once on the web; under Tauri, the worker clears
them right after — once any request already in flight has finished — or,
if the app quits before that, at the next launch. Either way, nothing
still queued at the moment of opting out is ever sent. `setOptOut(false)`
opts back in; nothing already deleted is replayed.

### Event names

An event name is trimmed and must come out non-empty and at most 200
characters; an invalid or oversized name — or an event too large for the
queue — is dropped, with a warning that names the event but never its
properties, so nothing a caller passed as a property value ends up in a
log line.

A panic in the worker or the network stack is caught and answered as a
dropped send, the same as any other failure — but only when the app is
built with `panic = "unwind"`, Rust's default. An app built with
`panic = "abort"` crashes on such a panic instead of failing quietly.

## Development

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm ci
npm test --workspaces --if-present
npm run typecheck --workspaces --if-present
npm run build --workspaces --if-present
```
