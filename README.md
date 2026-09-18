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
  "url": { "ios": null, "android": null, "macos": "https://example.com/download" }
}
```

- `minVersion` / `latestVersion` — semver strings (pre-releases like
  `1.2.0-alpha.1` compare as older than their release). Either may be
  omitted. Leave build metadata (`+…`) out of them: semver compares it.
- `message` — a map of language code to the text shown to the player. The
  command picks the entry for the resolved language, falling back to `en`.
  Keys are two-letter language codes only (`en`, `vi`, `pt`): the resolved
  language is cut down to its first two letters, so `pt-BR` and `zh-Hant`
  look up `pt` and `zh`, and a three-letter code never matches.
- `url` — a map of platform (`ios`, `android`, `macos`, `windows`, `linux`)
  to a download link, or `null` when there isn't one for that platform.
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
itself never answers.

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

## `tauri-plugin-posthog` (planned)

A queued PostHog event sender, for apps that need basic usage counting
without pulling in a native SDK. Not built yet.

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
