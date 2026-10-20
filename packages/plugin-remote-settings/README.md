# tauri-plugin-remote-settings-api

Remote settings and feature flags for a Tauri app and its web build. The app
reads two small files from its own domain, through Cloudflare's cache:

| File | Holds |
|---|---|
| `<base>/<stage>/global.json` | The values that are the same for every user, and the plugin's two keys `source` and `apply` |
| `<base>/<stage>/v<n>.json` | The app's GrowthBook rules, encrypted with key version `n` |

`remote-settings-hub` writes the rules file. Each app pushes its own global
file. The device evaluates the rules with the GrowthBook JS SDK, so web and
app give the same values for the same rules.

## The cost model

- A read hits the cache. It runs no Worker and no R2 operation, so the cost
  does not grow with users.
- A device never calls GrowthBook. The plugin never gives the SDK a host or a
  client key.
- Firebase Remote Config is an option. While `source` is `growthbook`, the
  plugin makes no Remote Config call.

## Setup (Rust)

```rust
use tauri_plugin_remote_settings::Config;

tauri::Builder::default()
    .plugin(tauri_plugin_remote_settings::init(
        Config::new("https://config.example.com/appa", "production", 1),
    ))
```

`Config` also takes `.country_header(..)` (default `x-country`),
`.timeout(..)` (default 5 seconds for each file) and `.store_dir(..)`. The
plugin starts the read at start, before the webview runs.

Add the permission to the app's capability file:

```json
{ "permissions": ["remote-settings:default"] }
```

## Use (TypeScript)

```ts
import { init } from "tauri-plugin-remote-settings-api";

const onOff = (t: string) => (t === "on" || t === "off" ? t : undefined);
const readJson = (t: string) => {
  try {
    return JSON.parse(t);
  } catch {
    return undefined;
  }
};

export const settings = init({
  keys: {
    notice: { fallback: null, read: readJson, global: true },
    dark_mode: { fallback: "off", read: onOff },
  },
  key: { version: 1, value: "<the stage's key, base64>" },
  web: { baseUrl: "https://config.example.com/appa", stage: "production", version: APP_VERSION },
  apply: "rest",
  onExperiment: (experiment, variant) => count("experiment_viewed", { experiment, variant }),
});

settings.subscribe((changed) => render(settings.values().values, changed));
```

In a Tauri app the Rust config gives the base URL, the stage and the key
version, and `web` is used only when the crate is not registered. The key
version in `Config::new` and in `key` must be the same. When they differ, the
plugin warns and evaluates no new rules, so change both in one release.

### Keys

Each key has a `fallback` and a `read`. `read` gives `undefined` for text that
does not read. Then the key keeps its saved value, or its fallback. A key with
`global: true` comes only from the global file. Every other key comes only
from the source.

**Each fallback shows the least.** A new feature is off. A value that comes
late can then only add something at the next apply. It never takes away what
the user saw.

The plugin evaluates only the keys in `keys`. A feature for a newer release
never assigns a test group in this one.

### When values apply

The first values of a launch are the saved ones. A later read follows the
mode. The global file's `apply` key wins over the `apply` option, so the
owner can change the mode with no release.

| Mode | A read applies |
|---|---|
| `hidden` (default) | When the app goes out of sight, or at once if it already is |
| `rest` | At once if the app is at rest or out of sight, else at the next rest or the next time out of sight |
| `now` | At once |

- `setAtRest(atRest)` tells the plugin when the user is at rest. The app
  decides what "at rest" means. Until the first call, `rest` acts as
  `hidden`.
- `applyNow(keys?)` applies pending values now, all of them or only the named
  keys. Use it for a key that must not wait, such as a version gate.
- `subscribe(listener)` hears each apply, with the keys that changed. Use the
  list to animate a part that comes or goes.
- `ready(deadlineMs)` resolves when the first read ends, or at the deadline.
  A read that lands before it resolves applies at once. Use it to hold a
  launch screen.
- `pending()` gives the values that wait, or `null`.

The plugin reads again each time the page comes back into sight. `refresh()`
reads at once.

### Test groups

GrowthBook assigns a group when it evaluates a feature with an experiment
rule. The plugin calls `onExperiment` only when that value applies, so a user
counts in group B from the moment group B shows. It reports each experiment
and variant once per launch.

### Firebase as a source

```ts
import * as firebase from "tauri-plugin-firebase-api";
import { firebaseAdapter } from "tauri-plugin-remote-settings-api/firebase";

init({ /* … */, defaultSource: "firebase", firebase: { adapter: firebaseAdapter(firebase), listen: () => liveOn } });
```

`source` in the global file chooses `growthbook` or `firebase`. A global file
with no readable `source` gives `defaultSource`, else `growthbook`. Fetched
Remote Config values activate at the next apply. The app passes
`tauri-plugin-firebase-api` to the adapter, so an app with no Firebase never
installs it.

### Attributes

Every rule can test `id` (the install id), `platform`, `web`, `version`,
`country`, `language` and `now`. `attributes` adds more, for example a paid
plan. It cannot replace a standard one. A new kind of condition needs a
release.

## What the plugin sends

Nothing about the user. Each read carries only what any download carries: the
network address. The install id, the attributes and the test groups stay on
the device. The install id is a random UUID the plugin makes and keeps. A
reinstall makes a new one.

An app's privacy policy still says that the app downloads its settings and
chooses them, and its test group, on the device.

## Storage

- In an app, the plugin keeps its files in `remote-settings/` in the app's
  local data folder. On Android it uses `no_backup/remote-settings/` in the
  app's data folder, which Android's automatic backup leaves out. A restore
  never brings another install's id.
- On the web, the plugin uses `localStorage` keys that start with
  `remote-settings:`. A private window gets the code's defaults.

## Moving an app to the plugin

`legacy` reads the app's own earlier storage once, when the plugin has nothing
kept: `legacy.installId()` keeps the old install id, so no user changes test
group, and `legacy.saved()` gives the old saved values, so the first launch is
not a launch with defaults.
