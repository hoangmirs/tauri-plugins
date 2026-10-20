# remote-settings-hub

A Cloudflare Worker library. It copies one GrowthBook project to R2 as one
encrypted rules file for each app, so that apps with
`tauri-plugin-remote-settings` can read their rules through Cloudflare's cache.

- The hub is the only caller of GrowthBook's CDN. A device never calls it.
- A device never reads through the hub. It reads static files from R2 through
  the cache, so a read runs no Worker and no R2 operation.
- The hub writes only when an app's rules change.

## What it does

GrowthBook's free Starter plan allows one project and one SDK webhook. So
every app shares the one project, and each feature key starts with its app's
prefix, for example `appa.dark_mode`.

On each trigger, the hub:

1. Reads each connection's payload from `cdn.growthbook.io/api/features/<client key>`.
2. Splits the payload by prefix into one payload for each app, and removes the
   prefix. `appa.dark_mode` becomes `dark_mode`.
3. Keeps only the saved groups that the app's features use.
4. Compares each app's payload with the hash saved beside its file, and stops
   for that app when they match.
5. Encrypts the app's payload with the app's key for that stage, in the format
   the GrowthBook SDK decrypts.
6. Writes `<path>/<stage>/v<n>.json` with `Cache-Control: public, max-age=60`.

A feature whose key starts with no known prefix goes to no app. The hub logs
its key, so a typo shows in `wrangler tail`.

The triggers:

| Trigger | When |
|---|---|
| `POST /hook` | GrowthBook's event webhook, for every event. The hub checks `X-GrowthBook-Signature`, answers 200, reads at once and reads again 15 s later. |
| Cron | Once a day, for a change that sends no event. |
| `POST /refresh` | By hand, with the header `x-refresh-secret`. It answers with what it wrote. |

One app's failure never stops another app. A failed read keeps the last good
file.

## The deploy

The hub runs from a private repository, because its config names the apps and
its secrets hold their keys. A `src/index.ts`:

```ts
import { createHub } from "remote-settings-hub";

const hub = createHub({
  connections: [
    { stage: "production", clientKey: "sdk-prodKey" },
    { stage: "staging", clientKey: "sdk-stagingKey" },
  ],
  apps: [
    {
      prefix: "appa.",
      paths: ["appa"],
      keys: {
        production: [{ secret: "APPA_PRODUCTION_KEY", version: 1 }],
        staging: [{ secret: "APPA_STAGING_KEY", version: 1 }],
      },
    },
  ],
});

export default { fetch: hub.fetch, scheduled: hub.scheduled };
```

A client key is not a secret: it is in GrowthBook's public CDN address. The
config can keep it.

`wrangler.jsonc`:

```jsonc
{
  "name": "settings-hub",
  "main": "src/index.ts",
  "compatibility_date": "2026-10-01",
  "r2_buckets": [{ "binding": "SETTINGS", "bucket_name": "settings" }],
  "triggers": { "crons": ["0 3 * * *"] }
}
```

Secrets, each set with `npx wrangler secret put <NAME>`:

| Secret | Value |
|---|---|
| `GROWTHBOOK_EVENT_SECRET` | The event webhook's signing secret, `ewhk_…` |
| `REFRESH_SECRET` | A long random text, for `POST /refresh` |
| One for each app and stage | A 128-bit AES key in base64: `openssl rand -base64 16` |

The app gets the same key for the same stage. The key ships in the app, so it
only stops casual reading. No setting may be a secret.

The hub refuses a config where one prefix starts with another, a prefix that
does not end with `.`, or an app with no path.

## GrowthBook, once

1. Make one organization on Starter, with two-factor sign-in.
2. Keep the one project for every app.
3. Make the environments `production` and `staging`, and one SDK connection
   for each. Leave GrowthBook's own encryption off: the hub encrypts.
4. In **Settings → Webhooks**, add one event webhook to
   `https://<hub host>/hook`, for every event, every project and every
   environment. Put its secret in `GROWTHBOOK_EVENT_SECRET`.
5. Name each feature `<prefix><key>`, and tag it with its app, so the list can
   show one app at a time.

Never target install ids in production. The rules file is public, and only a
paid plan hashes attributes. Use the staging environment for testers.

## Cloudflare, for each settings domain

The hub, the bucket and every settings domain sit in one Cloudflare account.
A bucket's custom domain must be a zone in that account. One bucket can have
several custom domains.

1. Connect the domain to the bucket as a custom domain.
2. Add a Cache Rule on that host: eligible for cache, and the cache key leaves
   out the query string. JSON is not cached without it, and a random query
   string would miss the cache.
3. Add a response header rule on that host:
   - `x-country` set to `ip.src.country`
   - `access-control-allow-origin` set to `*`
   - `access-control-expose-headers` set to `x-country`
4. Turn Tiered Cache on.

## Add an app

1. Choose a prefix, for example `appb.`, and a path, for example `appb`.
2. Make a key for each stage, and set each as a secret.
3. Add the app to `apps`, deploy, and call `POST /refresh`.
4. Give the app its base URL (`https://<domain>/appb`), its stage and its key.

## Change a key

1. Add the new key with `version: 2` beside the old one, deploy, and call
   `POST /refresh`. The hub writes `v1.json` and `v2.json`.
2. Release the app with key version 2.
3. When no copy in use reads `v1.json`, remove the old key.

## The global file

Each app pushes its own `<path>/<stage>/global.json` from its own repository.
`buildGlobal(file, keys)` checks the file the way the device reads it. One bad
value stops the push:

```ts
import { buildGlobal } from "remote-settings-hub";

const built = buildGlobal(file, KEYS);
if (!built.ok) throw new Error(`these values do not read: ${built.bad.join(", ")}`);
// upload built.body, for example with `wrangler r2 object put`
```

Upload it with `--cache-control "public, max-age=60"` and
`--content-type application/json`.
