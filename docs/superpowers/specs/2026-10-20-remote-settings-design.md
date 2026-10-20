# Remote settings: cached files, GrowthBook rules on the device, Remote Config behind a switch

Date: 2026-10-20. Status: draft for the owner's approval.

## Why

Ra Chơi (`techtips-vn/rachoi-app`, PR #557, merged 2026-10-20) reads its
settings in a new way:

1. A **global file** holds the values that are the same for every user, and a
   switch, `source`, that chooses where the other values come from.
2. A **rules file** holds GrowthBook's rules, encrypted. The app evaluates the
   rules on the device with the GrowthBook JavaScript SDK.
3. **Firebase Remote Config** stays behind the switch until the move is proven.

The app reads both files from R2 through Cloudflare's cache. A cache hit runs no
Worker and no R2 operation, so the read cost does not grow with users. A device
never calls GrowthBook.

Ra Chơi builds this by hand, in `src/app/config/` and `site/config.ts`. The
owner has more apps to come, TraDa first. The owner wants one plugin that every
app can use for a long time, with the same cost model and the same behaviour.

## Terms

| Term | Meaning |
|---|---|
| App | One product, for example Ra Chơi or TraDa. |
| Stage | `production` or `staging`. GrowthBook has one environment for each stage. |
| Connection | A GrowthBook SDK connection. There is one for each stage, shared by every app. |
| Prefix | The start of a feature key that names its app, for example `trada.` in `trada.lab_market`. |
| Global file | `global.json`. Plain JSON. Values that are the same for every user. |
| Rules file | `v<n>.json`. GrowthBook's payload, encrypted with key version `n`. |
| Source | Where the conditional values come from: `growthbook` or `firebase`. |
| Hub | The one Worker that reads each connection's payload from GrowthBook, splits it by app, and writes each app's rules file to R2. |
| Apply | The moment new values replace the values the app shows. |
| Out of sight | `document.visibilityState` is `hidden`. |
| At rest | The app says the user is not in the middle of something. Ra Chơi: Home, nothing open over it, no match running. |
| Reader | The app's function that turns a key's text into its type, or gives nothing when the text does not read. |

## Goals

- An app gets remote settings and feature flags by passing its address, its
  stage, its key and its keys' readers. It writes no fetch, storage or
  evaluation code.
- A settings read costs nothing at 1,000,000 daily users. A read never runs a
  Worker, and a device never calls GrowthBook.
- Many apps share one GrowthBook organization on the free Starter plan. Each
  app reads only its own files.
- iOS, Android, macOS and the web give the same values for the same rules.
- No value changes while the user is in the middle of something. The app and
  the owner choose how strict this is (see "When values apply").
- A file that does not read never replaces a good one. The app works offline
  with the last good values.
- The plugin can change inside for years without a change in the apps.

## Non-goals

- Moving Ra Chơi to the plugin. A later pull request in `rachoi-app` does it.
  This design only makes sure the move is possible with no data loss.
- A launch screen. Each app draws its own. The plugin gives the wait.
- Analytics. The plugin tells the app when a user falls into a test group. The
  app counts it where it counts everything else.
- A self-hosted GrowthBook, or GrowthBook's own results page.
- Replacing `tauri-plugin-update-gate`. An app can keep its gate in the global
  file, as Ra Chơi does, but the gate's rules stay the app's.
- A React hook. The API is plain functions and a subscription. An app wraps it.

## Anti-goals

These make the work a failure even if it runs.

- A device calls `cdn.growthbook.io`. The free plan's CDN allows 1,000,000
  requests a month, and GrowthBook may "throttle or block" an account past it.
- A read runs a Worker. Each Worker request is billed past 10,000,000 a month.
- One app's file reaches another app, or one app's failure stops another app's
  updates.
- Web and app give different values for the same rules.
- A product name, a key, a domain or a secret in the plugin repository. The
  repository's README says: "everything product-specific is passed in by the
  app that uses them."
- The hub depends on a GrowthBook route that has no documentation.

## Facts checked on 2026-10-20

Sources are in the session note `settings-plugin-findings.md`, with file paths
in the `growthbook/growthbook` repository.

| Question | Answer |
|---|---|
| SDK webhooks on Starter | One for the whole organization. The create route refuses a second without `multiple-sdk-webhooks`, a Pro feature. |
| One SDK webhook for several connections | The server supports it (`webhook.sdks`), and each POST carries the header `webhook-sdk-key`. The web UI cannot add a second connection. Only the internal route `PUT /sdk-webhooks/:id` takes `sdks`, behind a browser session. It has no documentation. |
| `webhook-sdk-key` | Not covered by the signature. The signature covers `<id>.<timestamp>.<body>`. |
| SDK webhook failures | Posts go to each connection in turn. One failure stops the rest, and the job retries after 30 s and 5 min, then stops. After 10 failures in a row, GrowthBook turns the webhook off. |
| Event webhooks | A separate object in Settings → Webhooks. No plan limit in the code. Filters by project, environment and event type. The body is signed: `X-GrowthBook-Signature` is the HMAC-SHA256 of the body, in hex. Three tries. |
| Event coverage | Events exist for features, experiments, holdouts, saved groups and constants. A scheduled rule changes the payload with no event (`jobs/updateScheduledFeatures.ts`). Scheduled rules, ramp schedules and safe rollouts are Pro features (`schedule-feature-flag`, `ramp-schedules`, `safe-rollout`), so Starter has no change that comes from time alone. Not checked: an edit to an SDK connection itself. |
| Proxy webhook | Each connection can post its signed payload to a proxy host on each change. On Cloud this needs `cloud-proxy`, a Pro feature. |
| Projects on Starter | One. `FREE_ORG_LIMITS.maxProjects` is 1 in `packages/shared/src/enterprise/entitlements.ts`, and `ProjectModel` refuses a second. Pro allows 3. |
| Feature keys | Letters, numbers and `_ - . : \|` (`FEATURE_KEY_PATTERN`). |
| Payload `experiments` | Holds visual and URL-redirect experiments only. Both are Pro features (`visual-editor`, `redirects`). A feature's own experiment rules sit inside the feature. |
| GrowthBook Rust SDK | `growthbook/growthbook-rust` is official. It decrypts, tracks and has sticky bucketing. It cannot run in a browser. |
| GrowthBook JS SDK | Ra Chơi uses `@growthbook/growthbook` 1.8.0. It decrypts `encryptedFeatures`, `encryptedExperiments` and `encryptedSavedGroups` with AES-CBC 128, `iv.ciphertext` in base64. |
| Cache hit | Runs no Worker and no R2 operation. JSON needs a Cache Rule to be cached. |
| Ra Chơi paths | `https://config.joinrecess.app/<stage>/global.json` and `/<stage>/v1.json`, on `develop` and in version 1.4.0. |
| Ra Chơi device data | `localStorage` keys `rachoi.config` (saved values) and `rachoi.install` (install id), as of PR #557. Check them again when Ra Chơi moves. |
| Repository shape | Each plugin is a crate in `crates/` and a JS package in `packages/` with a web path. Rust does HTTP with `ureq` and `rustls`. No plugin here needs Swift or Kotlin for HTTP. |

## Decisions the owner made

1. One reusable plugin for every app, made to last. Long-term use comes before
   a small first version.
2. GrowthBook is the source for feature flags. Firebase Remote Config is
   optional: an app may use it later, but an app that does not use it does not
   load it.
3. Apps can share one domain or use their own. The client reads its files the
   same way in both cases.
4. Every app sits in one GrowthBook organization and its one project. Each
   feature key starts with its app's prefix, for example `recess.game_list`.
   The hub splits the payload by prefix into one file for each app, and
   removes the prefix, so app code keeps reading `game_list`. Experiments
   travel with their features. An app gets only the saved groups its features
   use. Each app's file has its own key. A key with no known prefix goes to no
   app. A device downloads only the files of its own app.
5. New values wait until the app is out of sight by default. The global file
   can change this for an app, with no release. Ra Chơi applies a value at
   rest instead (owner's decision of 2026-10-20 in the PR #557 session).
6. The design was checked against GrowthBook's source code before it was
   chosen.
7. The hub, every bucket and every settings domain sit in one Cloudflare
   account.
8. The hub is the only writer. GrowthBook never writes to R2 itself: the one
   SDK webhook on Starter serves one connection, and only the hub encrypts.
9. A user counts in a test group from the moment its value applies, not from
   the moment GrowthBook evaluates it.

## Design

### Names

| Part | Name | Where |
|---|---|---|
| Tauri plugin | `tauri-plugin-remote-settings` | `crates/tauri-plugin-remote-settings` |
| JS package | `tauri-plugin-remote-settings-api` | `packages/plugin-remote-settings` |
| Firebase adapter | `tauri-plugin-remote-settings-api/firebase` | a subpath export of the JS package |
| Hub library | `remote-settings-hub` | `packages/remote-settings-hub` |
| Hub deploy | a private repository, for example `settings-hub` | outside this repository |

The names are the design author's call. The owner can change them before the
build starts.

### Files on the server

An app has a **base URL**, for example `https://config.joinrecess.app/trada`.
The app reads:

- `<base>/<stage>/global.json`
- `<base>/<stage>/v<n>.json`, where `n` is the key version the app holds

Ra Chơi's prefix is `recess.` and its base URL becomes
`https://config.joinrecess.app/recess`. Today it reads
`https://config.joinrecess.app/<stage>/v1.json` with names that have no
prefix, so the move needs a Ra Chơi change (see "Moving an app to the
plugin").

An app on its own domain uses that domain as its base URL. The hub, every
bucket and every settings domain sit in one Cloudflare account, the Workers
Paid account that Ra Chơi uses. A Worker binds only buckets in its own account,
and a bucket's custom domain must be a zone in that account. One bucket can
have several custom domains. Each new domain needs its own Cache Rule and
country header rule, as `config.joinrecess.app` has.

Each answer must have:

- `Cache-Control: public, max-age=60`
- a header with the user's country, from a Cloudflare response header rule
  (Ra Chơi uses `x-country`, set from `ip.src.country`)
- `Access-Control-Expose-Headers` that names the country header, for the web

The country header name is an option, so an app can use another one.

### The global file

Plain JSON. Two keys belong to the plugin. All other keys belong to the app.

| Key | Values | Meaning |
|---|---|---|
| `source` | `growthbook`, `firebase` | Where the conditional values come from. A missing or unknown value gives the app's `defaultSource`. |
| `apply` | `hidden`, `rest`, `now` | When new values apply (see "When values apply"). A missing or unknown value gives the app's `apply` option, then `hidden`. |

The app's keys in the global file never come from a source. A key is either a
global key or a source key, never both. The app says which keys are global.

### The rules file

The rules file is a GrowthBook SDK payload with `features`, `experiments` and
`savedGroups` replaced by their encrypted forms. The hub writes it (see "The
hub"). The key is AES-128, one per app and stage, in base64. The app holds the
key, and so does the hub, as a secret.

The key ships in the app and in the web page. It stops casual reading only. No
value in the settings may be a secret.

To change a key, an app release carries the new key with version `n + 1`. The
hub writes both `v<n>.json` and `v<n+1>.json` until no copy in use reads
`v<n>`.

### The plugin on a device

The Tauri plugin has a small Rust part. The JS package does the rest.

**Rust does:**

1. At plugin start, read both files with `ureq` on a background thread. The
   read never holds back the webview.
2. Keep the last good bytes of each file, and the country, in the app's data
   folder. "Good" means the global file is a JSON object, and the rules file is
   a JSON object with `encryptedFeatures` or `features`.
3. Keep the install id in a file in the same folder (see "The install id").
4. Answer the JS package's commands: give the saved files and country, and
   read the files again.

Rust does not evaluate rules and does not know any key. The plugin needs no
Swift or Kotlin.

**JS does:**

1. Ask Rust for the files. On the web, read them with `fetch` and keep them in
   `localStorage`.
2. Choose the source from the global file.
3. Evaluate the rules with `@growthbook/growthbook`, or ask the Firebase
   adapter.
4. Join the global keys and the source keys into one set of text values.
5. Hold new values until they may apply. Then apply them and tell the app.

The web path and the app path differ only in step 1. The same code evaluates
the rules on every platform.

### What the app gives

`init` takes one object. The exact shape is the builder's choice. It holds:

| Option | Example | Note |
|---|---|---|
| `baseUrl` | `https://config.joinrecess.app/trada` | |
| `stage` | `production` | |
| `key` | `{ version: 1, value: "<base64>" }` | |
| `keys` | `{ notice: { read, global: true }, lab_market: { read } }` | Every key the app reads, with its reader. `global` marks a key that only the global file gives. |
| `defaultSource` | `growthbook` | Used when the global file has no readable `source`. |
| `apply` | `rest` | Used when the global file has no readable `apply`. Default `hidden`. |
| `attributes` | a function that returns extra attributes | Added to the standard set. |
| `firebase` | the Firebase adapter, or nothing | |
| `onExperiment` | `(experiment, variant) => void` | Called when a test group's value applies (see "Test groups"). |
| `countryHeader` | `x-country` | Default `x-country`. |
| `legacy` | Ra Chơi's `localStorage` keys | See "Moving an app to the plugin". |

The Rust side reads `baseUrl`, `stage` and the key version from the plugin's
config in `tauri.conf.json` or from the plugin's builder, because it reads
before JS runs. The two places must not disagree: the builder chooses one place, and
JS reads it from Rust.

### What the app reads

The plugin reads each value with the key's reader, as `readValues` does in Ra
Chơi. A value that does not read gives the key's saved value, then the app's
default. The plugin evaluates only the keys in `keys`. A feature in the payload
that this app version does not read never assigns a test group.

The API gives:

- `values()`: the applied values, typed by the readers, and for each key
  where its value came from: `global`, `growthbook`, `firebase`, `saved` or
  `default`. The app can also see which keys a source sent, as `sent` does in
  Ra Chơi.
- `used()`: which source the applied values came from this session:
  `growthbook`, `firebase`, `saved` or `default`. Ra Chơi sends this in its
  `app_opened` count to know when to remove Remote Config.
- `country()`: the last country, or `null`.
- `pending()`: the values that wait to apply, or `null`.
- `applyNow(keys?)`: apply all pending values, or only the named keys.
- `subscribe(listener)`: called after each apply, with the keys that changed.
  The app uses the list to animate a part that comes or goes. Returns a
  function that stops it.
- `setAtRest(atRest)`: the app tells the plugin when the user is at rest and
  when the user is not. Only an app with `apply` set to `rest` calls it.
- `ready(deadline)`: resolves when the first read of this launch ends, or at
  the deadline, whichever comes first. A known failure, for example no
  network, resolves at once. An app may use it to hold a first screen. Ra
  Chơi no longer does: it removed its web launch screen on 2026-10-20.
- `refresh()`: read again now. The plugin calls it itself when the app comes
  back into sight.
- `installId()`: the install id, for an app that wants it.

### Attributes

The plugin gives these to every rule:

| Attribute | Value |
|---|---|
| `id` | The install id |
| `platform` | `ios`, `android`, `macos`, `windows`, `linux` or `web` |
| `web` | `true` in a browser |
| `version` | The app's version. Rust reads it from Tauri's `PackageInfo`. The web build passes it in. |
| `country` | Two letters, or absent |
| `language` | The device language, two letters |
| `now` | The current time, as an ISO date |

The app's `attributes` function adds more, for example a paid plan. An app
attribute cannot replace a standard one. A rule can test only an attribute
the app gives, so a new kind of condition needs a release.

### When values apply

The first values of a launch are the saved ones. A read that lands later
follows the `apply` mode:

| Mode | A read applies | Who uses it |
|---|---|---|
| `hidden` | When the app goes out of sight, or at once if it already is. | The default. |
| `rest` | At once if the app is at rest or out of sight. Otherwise when the app next reports rest or goes out of sight. | Ra Chơi, from 2026-10-20. |
| `now` | At once. | An app with no screen that a change can break. |

- In every mode, a read that lands before `ready` resolves applies at once.
  Nothing is on screen yet.
- An app in `rest` mode that never calls `setAtRest` behaves as `hidden`. A
  missing call can then only delay a value, never show it mid-task.
- The app can call `applyNow` for a key that must not wait, such as a version
  gate.
- The plugin never decides what "at rest" means, and never animates. The app
  gets the changed keys from `subscribe` and animates them. Ra Chơi pops a new
  part in with one glow of its border, and shrinks and fades a part that goes.

The owner decided on 2026-10-20 that the default is `hidden`, and that the
global file can change the mode for an app with no release. The same day, the
owner moved Ra Chơi from "out of sight" to "at rest", in the session that built
PR #557. `rest` is the plugin's form of that rule.

Each app's defaults in its own code must show the least: a new feature is off.
A value that comes late can then only add something at the next apply. The
plugin docs state this rule.

### Test groups

GrowthBook assigns a test group when it evaluates a feature with an experiment
rule. The plugin does not call `onExperiment` then. It keeps the assignment
with the pending value, and calls `onExperiment` when that value applies: at a
normal apply, or at `applyNow` for that key. A user is then counted in group B
only from the moment group B shows.

- The saved values that start a launch apply at once, so their test groups
  are reported at launch.
- The plugin reports each experiment once per launch, with its variant. A new
  variant for the same experiment in the same launch is reported again.
- A pending value that never applies reports nothing.

The owner chose this on 2026-10-20. Ra Chơi today reports at evaluation, so
its move to the plugin changes this. Counts after the move are not comparable
to counts before it for a test that runs across the move.

### The install id

- A random v4 UUID, made once and kept on the device. It never leaves the
  device through the plugin.
- On a device, Rust keeps it in a file in the app's data folder. On the web,
  JS keeps it in `localStorage`.
- When storage cannot keep it, there is no id. A rule that splits by `id` then
  gives the default. An id made new at each launch would move a user between
  groups.
- A reinstall makes a new id. Accept this.

### Saved values

- The plugin saves the last good global file, the last good rules file, the
  last good Firebase values and the country.
- It saves the raw files, not the evaluated values. A launch evaluates the
  saved rules again, with today's attributes. Then a rule on `now` or `version`
  is right even with no network.
- A part that fails to read keeps what was saved. In the global file, a key
  whose value does not read keeps its saved value, and the other keys apply,
  as `nextSaved` does in Ra Chơi.
- Rust keeps a rules file only after JS confirms that it decrypts. A file with
  the right shape for another key never replaces a good one.
- The files sit in a folder that Android's automatic backup leaves out, so a
  restore never brings back another install's id.

### The Firebase adapter

The adapter lets the switch choose Remote Config. It is a subpath export, so
an app that does not import it does not bundle Firebase. It wraps
`tauri-plugin-firebase-api` as a peer dependency.

The adapter gives the core four things:

1. The active values that Remote Config sent, without the app's defaults.
2. A fetch, which reports whether new values came.
3. An activate, which the core calls when pending values apply.
4. A listener for realtime updates, which the core starts only while `source`
   is `firebase` and the app asks for it.

While `source` is `growthbook`, the core makes no Remote Config call, so
Firebase bills nothing for that copy.

### The hub

The hub reads each connection's payload from GrowthBook, splits it by app,
and writes one rules file for each app. It is the only caller of GrowthBook's
CDN.

**The library** (`packages/remote-settings-hub`) exports a Worker's `fetch`
and `scheduled` handlers, made from a config with two lists:

- **Connections**, one for each stage: the stage and the client key.
- **Apps**: the app's feature prefix, its R2 path, and for each stage the name
  of the secret that holds the AES key and the key version.

Every app has a prefix. A feature with no known prefix goes to no app.

**The split.** For each app and stage, the hub builds the app's payload:

1. Keep only the features whose key starts with the app's prefix.
2. Remove the prefix from each key. `trada.lab_market` becomes `lab_market`,
   so the app reads its own key names.
3. Keep only the saved groups that the kept features name. One app's saved
   groups, which can hold tester ids, never reach another app.
4. Copy the other payload fields, for example `dateUpdated`. Leave
   `experiments` empty: on Starter it holds nothing.

A feature that matches no app goes in no file. The hub logs its key once, so
a typo in a prefix shows.

**The triggers.** The hub has three:

1. An event webhook from GrowthBook, for every event type and every project.
   This is the main trigger. The hub checks `X-GrowthBook-Signature` against
   the body. A body that fails the check changes nothing.
2. A cron once a day. It catches a change that sends no event, for example an
   edit to an SDK connection.
3. A manual refresh: `POST /refresh` with a secret in a header. The owner uses
   it after an edit that sends no event.

Each trigger does the same thing, whatever the event names:

1. Read each connection's payload from
   `cdn.growthbook.io/api/features/<client key>`.
2. Refuse a body that is not a payload with a `features` object.
3. Split the payload into one payload for each app.
4. For each app, compare the hash of its payload with the hash saved beside its
   file. Go to the next app when they match.
5. Encrypt the app's payload with the app's key for that stage, in the format
   the SDK decrypts. Each field gets its own random IV.
6. Write `<app path>/<stage>/v<n>.json` with
   `Cache-Control: public, max-age=60`. Then write the new hash.

One app's failure never stops the others. One stage's failure never stops the
other stage. The hub logs the reason for a refusal, never the body.

The hub answers 200 to a signed event, even when a read fails. The next event,
the daily cron or a manual refresh tries again. A refused signature gets 401.
A 200 also keeps GrowthBook from counting failures: GrowthBook turns a webhook
off after 10 failures in a row.

If the owner later moves to a plan with time-driven rules, the cron must run
more often than once a day. The cron interval is a setting of the deploy.

**The deploy** lives in a private repository. It holds the config, the
`wrangler.jsonc`, the R2 binding and the secrets. To add an app, the owner adds
one entry to the app list, adds its two keys as secrets and deploys.

**GrowthBook setup.**

- One project, shared by every app. Starter allows only one.
- Two connections, `production` and `staging`, shared by every app.
- One event webhook for the whole organization, which points at the hub.
- Each feature's key starts with its app's prefix, for example
  `trada.lab_market`. Tag each feature with its app too, so the feature list
  can show one app at a time. The hub does not read tags.
- An app's GrowthBook features and its keys in code have the same names, after
  the hub removes the prefix.

If the owner later moves to a plan with one project for each app, the hub
config can take one connection for each app. The app side does not change.

### The global file push

Each app pushes its own global file from its own repository, as
`npm run config:push` does in Ra Chơi. The hub library exports a function that
builds the file and checks it before an upload:

- each key is a global key or a plugin key
- each value reads with the app's reader
- one bad value stops the push, and nothing is written

The upload stays in the app's script. The app chooses `wrangler` or another
tool.

### Moving an app to the plugin

Ra Chơi must move with no change for its users:

- The `legacy` option names the old `localStorage` keys. At the first launch
  with the plugin, the plugin reads the old install id and keeps it, so no
  user changes test group. It reads the old saved values, so the first launch
  is not a launch with defaults.
- Ra Chơi's GrowthBook features get the prefix `recess.`. The hub removes
  it, so the key names in Ra Chơi's code stay the same.
- Copies already in use read `<stage>/v1.json` and `<stage>/global.json` at
  the root. An app entry in the hub config can name more than one R2 path.
  The hub writes Ra Chơi's rules to both `recess/<stage>/` and `<stage>/`
  until no copy in use reads the root. Ra Chơi's push writes its global file
  to both paths for the same time.
- Then the hub replaces the SDK webhook and the per-minute staging cron in Ra
  Chơi's Worker (`site/config.ts`). Remove that code after the hub runs.

## Cost

GrowthBook CDN calls come only from the hub:

- Each event reads the two connections once, however many apps there are.
  With 50 events a day, that is 3,000 reads a month, 0.3% of the 1,000,000.
- The daily cron adds 60 reads a month.
- The hub runs one Worker request for each event, plus 30 a month for the
  cron. Each check reads one hash from R2, a Class B operation, inside the
  10,000,000 free each month.

The event count is a guess. Many saves in one hour cost more CDN reads, but
each costs only two reads.

Device reads cost nothing while they hit the cache, at any number of users.

## Privacy, for each app

The plugin sends nothing about the user. Each read carries only what any
download carries: the network address. The install id, the attributes and the
test group stay on the device.

Each app still states, in its own privacy policy, that it downloads its
settings and chooses them on the device. The plugin README gives the facts an
app needs for that text.

## Left to the builder

- The exact shapes of `init` and the API, inside the lists above.
- Where Rust takes `baseUrl` and `stage` from, and how JS reads them from Rust.
- The file names in the app's data folder.
- How the hub stores each hash.
- The tests, as the repository does them: `cargo test`, clippy with
  `-D warnings`, and `node --test` in each package.
- The README sections for the plugin and the hub.

## Calls made by the design author, not by the owner

- The names in "Names".
- Rust reads the files at start and keeps them on disk. JS evaluates.
- The hub library sits in this repository. Its deploy sits in a private one.
- Event webhook for every event, a daily cron and a manual refresh, not the
  SDK webhook. Each trigger refreshes every app.
- The prefix separator is `.`, and the hub removes the prefix.
- The hub copies only the saved groups that an app's features name.
- A test group is reported once per launch and experiment, and again when its
  variant changes.
- The hub answers 200 to a signed event even when a read fails.
- `apply` and `source` are plugin keys in the global file.
- The `rest` mode, the `setAtRest` call, and `rest` acting as `hidden` until
  the app first calls it.
- `init` takes every key with its reader. The plugin evaluates only those keys.
- A global value that does not read keeps its saved value, as in Ra Chơi.
- Rust reads on a background thread. Rust keeps a rules file only after it
  decrypts. Saved files sit outside Android's automatic backup.
- The plugin saves raw files and evaluates again at each launch.
- The push helper builds and checks the global file, and the app uploads it.

## Open checks during the build

1. Make one event webhook on the real organization. Change a feature, an
   experiment, a saved group and a constant, and check that each change sends
   an event. Edit an SDK connection's projects, and see whether it sends one.
2. Check that the CDN payload holds each feature under its full key, prefix
   included, and that a feature's rules name saved groups by id in a way the
   hub can find.
3. Measure the time the Rust read saves at a cold start on Android, against a
   read from the webview.
