# tauri-plugins

MIT-licensed Tauri v2 plugins, built so an app needs only a line or two to
get each one. No product names, no secrets: everything product-specific is
passed in by the app that uses them.

## `tauri-plugin-update-gate`

Reads one JSON document over HTTP and tells an app whether the installed
copy is too old to run. The plugin renders nothing — the update screen, in
the app's own language and brand, is the app's job.

### The gate fails open

If the document can't be fetched, the response isn't valid JSON, or a
version field doesn't parse as semver, the gate answers `ok` rather than
raising an error. **A gate that cannot be read must never lock anyone out.**
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
  omitted.
- `message` — a map of language code to the text shown to the player. The
  command picks the entry for the resolved language, falling back to `en`.
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
response). The running version always comes from Tauri's own
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

`checkUpdate(lang?: string)` calls the `check` command and resolves to:

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

**Pass `lang` explicitly.** If you don't, the package falls back to
`navigator.language`, and if that's unavailable, the Rust side falls back
to the OS locale environment (`LANG` / `LC_ALL` / `LC_MESSAGES`) and then
`"en"`. On iOS and Android there is no such environment in practice, so an
app that skips `lang` there will silently always get English. Pass the
language the app itself is already showing — from its own Settings, not
just the OS — so the message picks it correctly everywhere.

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
```
