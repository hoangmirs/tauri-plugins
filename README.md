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

## `tauri-plugin-posthog-anon`

Queues anonymous usage events on disk (or, on the plain web, in
`localStorage`) and sends them to PostHog in batches. No product names, no
secrets: the project key is public by design (PostHog's ingestion keys are
meant to ship in client code) and is passed in by the app, same as every
other plugin here.

Not to be confused with
[`tauri-plugin-posthog`](https://crates.io/crates/tauri-plugin-posthog), a
separate plugin that sends each event as it happens and supports `identify`.
This one keeps events on disk until they can be sent, refuses identity, has
an opt-out, and runs on the plain web too.

### Anonymous only

Every event carries `"$process_person_profile": false`, which a caller's
own properties can never override — PostHog never creates a person profile
for it, and bills it at its cheaper anonymous-event rate. There is no
`identify` call, no way to pass a `distinct_id`, and no person properties:
PostHog's identity events (`$identify`, `$create_alias`,
`$merge_dangerously`, `$groupidentify`) are refused as event names, and
`$set`, `$set_once`, `$unset`, `$groups` and `$anon_distinct_id` are
stripped from any event's properties. `distinct_id` is a random v4 UUID
made once per install (once per browser on the web) and kept on disk — an
app data file, or `localStorage` on the web — so the same install is
recognizable across launches without being identifiable. Under Tauri it is
only made when the first event is queued, so an install that opted out
before sending anything never gets one.

An event's `properties` are the caller's own, merged onto what the plugin
adds — `distinct_id`, `$process_person_profile: false`, `$lib`
(`tauri-plugin-posthog-anon`, or `tauri-plugin-posthog-anon-web` on the plain web),
`$lib_version`, `$app_version` (under Tauri, the `PackageInfo` version of
the app that captured the event, even if a later version sends it; on the
web, whatever `appVersion` was passed to `init`, and left out entirely when
none was), `$os` (`iOS`,
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
use tauri_plugin_posthog_anon::Config;

tauri::Builder::default()
    .plugin(tauri_plugin_posthog_anon::init(Config::new("phc_your_project_key").eu()))
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
{ "permissions": ["posthog-anon:default"] }
```

### Use (TypeScript)

```ts
import { init, track, flush, setOptOut, isOptedOut } from "tauri-plugin-posthog-anon-api";

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
  from the web — without it, web events carry no `$app_version` at all.
  If an earlier page left events queued, `init` starts sending them.
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
once on launch (on the web, at `init`), best-effort on app exit (waited
for at most **2 seconds**; whatever hasn't sent by then stays queued for
next launch), and — web only — on `pagehide`. Each request carries at most
**100 events**; a larger queue is sent in several requests, one after the
other, until it is empty or one fails. The `pagehide` flush is the
exception: it sends one request with `keepalive`, cut to at most **60 KiB**
because browsers refuse a larger keepalive body, and leaves the rest for
the next page. Every request is bounded by a
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
still queued at the moment of opting out is ever sent — not even if the
install opts straight back in before the worker gets round to clearing,
and not even an event that was already on its way to the worker when the
opt-out landed. `setOptOut(false)` opts back in; nothing already deleted
is replayed.

On the web, every tab of the same origin shares the one queue and opt-out
in `localStorage`, without coordinating: two tabs writing the queue at
once can lose an event, and an opt-out in one tab reaches another at that
tab's next `track`.

### Event names

An event name is trimmed and must come out non-empty, at most 200
characters, and not one of PostHog's identity events; an invalid name — or
an event too large for the queue — is dropped. Under Tauri the drop is
logged at `warn`, naming the event (cut to 64 characters) but never its
properties, so nothing a caller passed as a property value ends up in a
log line; on the web it is dropped silently.

A panic in the worker or the network stack is caught and answered as a
dropped send, the same as any other failure — but only when the app is
built with `panic = "unwind"`, Rust's default. An app built with
`panic = "abort"` crashes on such a panic instead of failing quietly.

## `tauri-plugin-silent-mode`

Tells an app whether the phone is set to silent or vibrate, and when that
changes, so its sounds can keep to the choice the player made for the whole
phone. The plugin plays and mutes nothing itself: what to do while the phone
is quiet is the app's call.

### Where it reads anything

- **Android** reads the ringer mode (`AudioManager.getRingerMode`) and sends
  `change` whenever it moves. Android's silent and vibrate modes only touch
  the ringer and notifications, so media, a game's sound included, plays on
  through them unless the app asks.
- **iOS** answers `normal`. The silent switch already mutes an app's sound
  when the page or app asks for ambient audio (`navigator.audioSession.type =
  "ambient"` in a webview), and there is no public way to read the switch.
- **Desktop and the plain web** answer `normal`: there is no such mode.

### Setup (Rust)

```rust
tauri::Builder::default().plugin(tauri_plugin_silent_mode::init())
```

The app must also enable the plugin's default permission in its capability
file, which allows `state` and listening for `change`:

```json
{ "permissions": ["silent-mode:default"] }
```

### Use (TypeScript)

```ts
import { isQuiet, onSilentModeChange, silentMode } from "tauri-plugin-silent-mode-api";

let quiet = isQuiet(await silentMode());
const stop = await onSilentModeChange((mode) => (quiet = isQuiet(mode)));
```

`mode` is `"normal"`, `"vibrate"` or `"silent"`. Read it once and keep it:
`change` says when it moves, so nothing needs asking before each sound.
Neither call ever rejects; anything unreadable is `"normal"`, so an app that
cannot tell stays audible.

## `tauri-plugin-store-review`

Asks the phone to show its own rating prompt: the App Store's on iOS 16.4 and
later, Google Play's on Android. On a desktop the command does nothing.

The phone decides whether the prompt shows. iOS shows it 3 times in 365 days
at most. Play has a limit it does not publish. Neither store tells the app
whether the prompt showed or what the player chose, so `request_review`
returns nothing. Ask after a moment the player enjoyed, and not from a
button: when the limit is used, the tap does nothing.

The prompt shows in an iOS development build every time, and sends nothing.
It never shows in TestFlight. On Android it shows only in an app installed
from Google Play, such as a test track.

### Setup (Rust)

```rust
#[cfg(mobile)]
app.handle().plugin(tauri_plugin_store_review::init())?;
```

Add `store-review:default` to a capability for the phones.

### Use (TypeScript)

```ts
import { requestReview } from "tauri-plugin-store-review-api";

await requestReview();
```

## `tauri-plugin-keep-awake`

Keeps a phone's screen on while the app asks it to, and lets it sleep again.
iOS sets `isIdleTimerDisabled`. Android sets `FLAG_KEEP_SCREEN_ON` on the
app's window. On a desktop the command does nothing.

A web view's screen wake lock is not enough here. Android's system web view
has none, and inside an iOS app it is not reliable. The platform's own switch
needs no permission and no prompt.

The switch holds only while the app is on screen. Once the app goes to the
background the phone sleeps as usual, and the switch is back in force when
the app returns. Turn it off when the reason ends, or the screen stays on
for as long as the app is open.

### Setup (Rust)

```rust
#[cfg(mobile)]
app.handle().plugin(tauri_plugin_keep_awake::init())?;
```

Add `keep-awake:default` to a capability for the phones.

### Use (TypeScript)

The package is `tauri-plugin-stay-awake-api`: npm refuses
`tauri-plugin-keep-awake-api` as too close to another package's name.

```ts
import { keepAwake } from "tauri-plugin-stay-awake-api";

await keepAwake(true);
// later
await keepAwake(false);
```

## `tauri-plugin-keep`

Keeps a few small text values on an iPhone or iPad after the app is deleted,
so a reinstall on the same device reads them back. Each value is a Keychain
item, saved as "after first unlock, this device only" and never
synchronized: it stays on this device, out of iCloud and out of a backup
moved to another phone. Erasing the device removes it.

iOS keeps an app's Keychain items when the app is deleted, and apps have
relied on that for years. Apple does not document it, so read a missing value
as a fresh install, never as an error.

Everywhere but iOS, `get` answers `null` and `set` and `remove` do nothing.
On Android, Auto Backup already restores an app's data after a reinstall.

A key is 1 to 64 letters, digits, `.`, `-` or `_`. A value is at most 64 KiB
of text. The Keychain is for small items.

### Setup (Rust)

```rust
app.handle().plugin(tauri_plugin_keep::init())?;
```

Add `keep:default` to a capability.

### Use (TypeScript)

```ts
import { invoke } from "@tauri-apps/api/core";

await invoke("plugin:keep|set", { key: "kept", value: JSON.stringify(state) });
const kept = await invoke<string | null>("plugin:keep|get", { key: "kept" });
await invoke("plugin:keep|remove", { key: "kept" });
```

## `tauri-plugin-purchase`

Buys and restores one-time purchases through the phone's own store:
StoreKit 2 on iOS 15 and later, and the Play Billing Library 9 on Android.
On a desktop every command answers an error, and the TypeScript side reads
that as a store it cannot ask.

The plugin never decides that a player owns something. It hands the app a
**proof**, and a server checks that proof with the store:

| Store | Proof | The server asks |
|---|---|---|
| App Store | The transaction Apple signed (its JWS) | App Store Server API, Get Transaction Info, for the transaction id in the JWS |
| Google Play | The purchase token | Play Developer API, `purchases.products.get`, for the token |

A proof can be copied, so a server that only reads it proves nothing. Ask the
store each time.

### What the plugin does on its own

- **iOS** finishes each verified transaction. It also listens for
  transactions that complete outside the purchase sheet, such as one a parent
  approves later, and finishes those.
- **Android** acknowledges each purchase it sees, from the purchase sheet,
  from `owned` and from `restore`. Play refunds a purchase with no
  acknowledgement after three days.
- Neither side treats a refunded or revoked purchase as owned.
- Neither side answers an error as "owns nothing". `owned` and `restore`
  answer null when the store cannot be asked.

Register the plugin at launch, so both happen even when the app never opens
its store screen.

### Setup (Rust)

```rust
#[cfg(mobile)]
app.handle().plugin(tauri_plugin_purchase::init())?;
```

Add `purchase:default` to a capability for the phones. Make each product a
non-consumable (App Store) or a one-time product (Google Play) with the same
id in both stores.

### Use (TypeScript)

```ts
import { buy, owned, products, restore } from "tauri-plugin-purchase-api";

const [pro] = await products(["app.pro"]);  // { id, price: "49.000 ₫" }
const out = await buy("app.pro");           // { state: "bought", proof } | { state: "cancelled" | "pending" | "failed" }
const held = await owned(["app.pro"]);      // [{ id, proof }], or null when the store cannot be asked
const back = await restore(["app.pro"]);    // the same, after the store syncs
```

None of the four ever rejects. When `owned` or `restore` answers null, keep
the last answer: reading null as "owns nothing" locks what a player paid for
each time the store has a bad moment. Call `restore` only from a button: on
iOS it can ask the player to sign in.

Ask `owned` again each time the app comes back to the screen. A payment a
parent approves and a refund both land while the app is away.

### Testing

- **iOS:** a StoreKit configuration file in Xcode runs purchases in the
  Simulator with no App Store Connect product. TestFlight builds buy in the
  sandbox, so the server must ask the sandbox when production answers 404.
- **Android:** purchases work only in an app installed from Google Play, for
  example from an internal test track, with a license tester account.

## `tauri-plugin-ads`

Shows Google Mobile Ads on iOS and Android: a native ad drawn as a card over
an empty box the page draws, and rewarded ads. On a desktop every command
answers its failure value, and nothing shows.

- **Native ad.** The page draws the box, in its own colours, and sends its
  place, size and colours with `show_native`. The plugin draws the ad's
  parts on a clear view over the box. On iOS the card rides inside the page's
  own scroll view, so it scrolls with the page. Google requires a media view
  of at least 120 x 120 points, so give the box at least that height plus
  padding.
  - **Clip.** Send `clipTop` and `clipBottom`: the part of the scroll box
    that no bar covers. A bar the page pins inside the box, such as a top
    bar, lies outside it. Both phones cut the card to it.
  - **Taps.** A tap on the card opens the ad. On iOS WebKit also sends the
    tap to the page under the card, so the box must react to no tap.
  - **New ads.** A failed load tries again after 30 s, 2 min, then every
    10 min, while the app still shows the card. A show after a hide gets a
    new ad when the current one recorded an impression 60 s ago or more, or
    loaded over an hour ago. A new `unitId` loads from the new unit.
  - **One card.** The plugin has one card. Show one slot at a time.
- **Rewarded ad.** `load_rewarded` gets one ad ready. `show_rewarded` answers
  `"earned"`, `"closed"` or `"failed"`. An ad loaded over an hour ago is
  dropped and loaded again.
- **Consent.** `consent` runs Google's User Messaging Platform, which shows
  its form only where the law asks for it. Call it before the first ad. It
  starts the SDK once ads may load. When the request fails, for example
  offline, a consent from an earlier session still counts.
- **Privacy options.** When `privacy_options` answers `"required"`, the app
  must offer a way to `show_privacy_options`, for example a settings row.
- **Revenue.** `onPaid` reports what Google says each ad earned. Counting it
  is part of what the app collects, so say so in the privacy policy.
- **Sound.** `set_muted` mutes every ad, for an app whose own sound is off.
- **Children.** `init` takes child-directed treatment and the highest content
  rating. The Android manifest removes the advertising id permission, as
  Google Play asks of an app for children.
- **No `-ObjC`.** The crate links Google's prebuilt frameworks itself and
  loads their objects through one symbol each, as `tauri-plugin-firebase`
  does. An app must not set `-ObjC`.

No command ever rejects.

### Setup (Rust)

```rust
.plugin(tauri_plugin_ads::init())
```

Add `ads:default` to a capability. The app adds its AdMob app id:
`GADApplicationIdentifier` in its iOS `Info.plist`, and the
`com.google.android.gms.ads.APPLICATION_ID` meta-data in its Android manifest.

### Use (TypeScript)

```ts
import {
  consent, init, loadRewarded, onNative, onPaid, privacyOptions, setMuted, showNative, showRewarded,
} from "tauri-plugin-ads-api";

await init({ childDirected: true, maxRating: "G" });
await consent();
const offerPrivacy = (await privacyOptions()) === "required"; // then a row calls showPrivacyOptions()
await setMuted(!soundOn);
await onPaid((p) => console.log(p)); // { format, micros, currency, precision }
const stop = await onNative((e) => console.log(e)); // native_loaded, native_failed, native_shown, native_tapped
await showNative(slot);
if ((await loadRewarded(unitId)) === "ready") await showRewarded();
```

## `tauri-plugin-firebase`

Reads Firebase Remote Config, logs Firebase Analytics events and sends
Crashlytics crash reports, so an app can change values, turn features on and
off, run A/B tests without a release, and see why it crashed.
iOS and Android use Firebase's native SDKs. The web and a Tauri desktop app use
the Firebase web SDK through the TypeScript package.

### Defaults first

The app passes a default for each key. The Rust side keeps them, so a phone
without its Firebase file, a desktop, and a failed fetch all answer with the
defaults. No command returns an error: a failure reads as `"failed"`, `false`
or the defaults.

Firebase gives every value as text. A default `true` reads `"true"`, `15`
reads `"15"`, and an object reads as its JSON text. The app parses each key
by its own type.

### Fetch, then activate

`fetch` gets new values and keeps them aside. The values change only when the
app calls `activate`. The app picks the moment a change shows, for example
never in the middle of a game. A `fetch` inside the minimum interval (15
minutes unless the app sets another) answers from Firebase's cache.

### Live updates

`listen(on)` hears each publish while the app is open, and calls `on` with
the changed keys. Firebase has already fetched the new values, so they wait
for `activate` like the values of a `fetch`. It resolves to the function that
stops listening.

```ts
import { activate, listen } from "tauri-plugin-firebase-api";

const stop = await listen((keys) => {
  // The values are fetched. Activate them when the screen can change.
  void activate();
});
// later: stop();
```

- iOS and Android use the SDK's config update listener, and the web and the
  Mac use the web SDK's `onConfigUpdate`.
- A phone holds one native listener for every `listen` call. The last stop
  ends it.
- A publish costs one fetch for each listening copy of the app. A copy that
  starts listening before its next scheduled fetch also fetches once, because
  it sees the newer template when it connects. Firebase counts each fetch.
  Holding the connection is free.
- The project needs the Firebase Remote Config Realtime API on, and no API key
  restriction that blocks it. Without it, `on` is never called. `listen` never
  rejects, so check a publish on a real phone.
- An app with no Firebase file hears nothing, and its stop does nothing.

### Analytics starts off

The plugin never turns Analytics on by itself. Turn collection off in each
app's native config, so nothing is sent before the app reads the player's
choice:

- iOS `Info.plist`: `FIREBASE_ANALYTICS_COLLECTION_ENABLED` and
  `GOOGLE_ANALYTICS_DEFAULT_ALLOW_AD_PERSONALIZATION_SIGNALS`, both `false`.
- Android: the library's manifest already sets
  `firebase_analytics_collection_enabled`,
  `google_analytics_adid_collection_enabled` and
  `google_analytics_default_allow_ad_personalization_signals` to `false`. An
  app overrides one with `tools:replace="android:value"`.

Then call `setCollection(true)` when the player allows it. On the web,
Analytics starts with automatic page views off, because a page view carries
the page's address and the full referring address. Turn off enhanced
measurement in the web stream's settings too. Both native SDKs
keep that choice across launches, so call `setCollection` on every launch with
the player's current answer.

### No advertising id

iOS links `FirebaseAnalyticsCore`, which has no advertising id support. The
Android library removes `AD_ID`, `ACCESS_ADSERVICES_ATTRIBUTION` and
`ACCESS_ADSERVICES_AD_ID` from the merged manifest. Google Play forbids the
advertising id in an app whose audience includes children.

### Crash reports

Crashlytics runs on iOS and Android only. On the web, on a desktop, and on a
phone without its Firebase file, each crash call does nothing and returns no
error.

The web view has four commands. Each needs `firebase:default` in the window's
capability.

- `setCrashCollection(enabled)` turns report collection on or off.
- `setCrashKeys(keys)` sets text keys that go with every later report.
- `crashLog(line)` adds a line to the log that goes with the next report.
- `recordError(name, message, stack)` reports an error that did not stop the
  app. The package reads a JavaScript stack of V8 or WebKit style into frames.

Rust code uses `CrashExt`. This is the way to report a Rust panic, because a
panic hook has no web view.

```rust
use tauri_plugin_firebase::{CrashExt, Frame};

let crash = app.handle().crash();
crash.log("panic in the render loop");
crash.record("PanicError", "index out of bounds", &[Frame {
    symbol: "render".into(),
    file: "board.rs".into(),
    line: 42,
}]);
```

Both Rust methods block until the native side answers. Keep these points in
mind:

- Collection is on when the app starts. To keep development builds out of the
  console, call `setCrashCollection(false)` in them.
- Crashlytics writes logs, records and keys a moment later, on its own queue.
  A call made just before the process stops can be lost, on iOS and on
  Android. The crash and its stack still arrive.
- On Android, Tauri runs every native call on the UI thread. A blocking
  `Crash::log` or `Crash::record` called from that thread never returns. Never
  call the Rust API from the Android UI thread. From a panic hook, call it
  from a new thread.

#### What the app adds for readable frames

Without these steps, a crash still arrives but its frames show addresses and
obfuscated names.

- iOS: upload the app's dSYM files with Crashlytics' `upload-symbols` tool
  after each build.
- Android: the library links `firebase-crashlytics-ndk`, so a crash in the
  app's native library reaches Crashlytics. For readable native frames, the
  app applies the Crashlytics Gradle plugin and uploads its native symbols.
  For readable Kotlin frames in a release build, the app also uploads the R8
  mapping file.
- Android: the library ships `crashlytics.xml`, which sets
  `com.google.firebase.crashlytics.RequireBuildId` to `false`. Without it, an
  app that does not apply the Crashlytics Gradle plugin has no build ID, and
  Crashlytics stops the app as it starts. An app that applies the plugin still
  gets its build ID.

### Build notes

- iOS: Firebase Analytics ships as prebuilt static frameworks. `build.rs`
  links them into the crate, because the Rust link does not see them. It finds
  them in the downloaded `.xcframework` slices, so debug and release builds
  work alike.
- iOS: no `-ObjC` flag is needed, and adding one to the app fails with
  duplicate symbols, because every Tauri plugin carries its own copy of
  Tauri's Swift API. Firebase's Objective-C categories need their objects
  loaded, so `build.rs` writes a reference to one symbol in each Firebase
  object instead.
- iOS: a `GoogleService-Info.plist` with a malformed value, such as an API key
  of the wrong length, stops the app at start. Firebase throws an Objective-C
  exception that Swift cannot catch. Use the file the Firebase console gives.
- Firebase starts when the plugin is created, not when the webview loads, so
  the plugin works whether the app registers it on the builder or in
  `setup`.
- Android: the library uses Firebase BoM 34.19.0. BoM 35 needs Kotlin 2.1 or
  later in the app, and Tauri's Android template uses Kotlin 1.9.
- Firebase's iOS resource bundles, with their privacy manifests, do not reach
  the app. Declare what they declare in the app's own `PrivacyInfo.xcprivacy`.

### Setup (Rust)

```rust
app.handle().plugin(tauri_plugin_firebase::init(Default::default()))?;
```

Add `firebase:default` to a capability for every window that reads values,
desktop included. Put `GoogleService-Info.plist` in the iOS app's resources
and `google-services.json` in the Android app, with Google's Gradle plugin.

### Use (TypeScript)

```ts
import {
  activate,
  crashLog,
  fetch,
  getAll,
  init,
  logEvent,
  recordError,
  setCollection,
  setCrashCollection,
  setCrashKeys,
  setDefaults,
} from "tauri-plugin-firebase-api";

// The web and the desktop only. A phone reads its own Firebase file.
await init({ options: firebaseWebConfig, minimumFetchIntervalMillis: 15 * 60_000 });
await setDefaults({ newShop: false, maxSeats: 6 });
const values = await getAll(); // { newShop: { value: "false", source: "default" }, ... }

if ((await fetch()) === "fetched") {
  // later, at a moment the app picks
  await activate();
}

await setCollection(playerAllowsCounts);
await logEvent("round_ended", { game: "chess", seats: 2 });

await setCrashCollection(!isDevelopmentBuild);
await setCrashKeys({ game: "chess" });
await crashLog("round started");
try {
  await loadBoard();
} catch (error) {
  if (error instanceof Error) await recordError(error.name, error.message, error.stack ?? "");
}
```

On the web the package loads `firebase` (12.8.0 or later) with a dynamic
`import()`. Add it to the app's dependencies only if the app runs on the web
or a desktop.

## `tauri-plugin-remote-settings`

Remote settings and feature flags for a Tauri app and its web build. The app
reads a global file and its GrowthBook rules from its own domain, through
Cloudflare's cache, so a read runs no Worker and never calls GrowthBook. The
JS package evaluates the rules on the device, with the same code on the web
and in the app. New values apply by the app's mode: when the app goes out of
sight, when the user is at rest, or at once. Firebase Remote Config can sit
behind the global file's `source` switch.

The Rust crate reads the two files at start, before the webview runs, and
keeps a small store and an install id. It needs no Swift or Kotlin.

Setup, the keys, the modes and test groups are in
[`packages/plugin-remote-settings/README.md`](packages/plugin-remote-settings/README.md).

## `remote-settings-hub`

A Cloudflare Worker library, not a Tauri plugin. It copies one GrowthBook
project to R2 as one encrypted rules file for each app, for
`tauri-plugin-remote-settings` to read through the cache. A device never
calls it. Setup, the deploy and GrowthBook's settings are in
[`packages/remote-settings-hub/README.md`](packages/remote-settings-hub/README.md).

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
