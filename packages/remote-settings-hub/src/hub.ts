/** The one Worker that writes every app's rules. It reads each GrowthBook
 *  connection from the CDN, splits the payload by app, encrypts each app's
 *  part with that app's key, and writes it to R2. A device reads the files
 *  through the cache and never reaches this Worker. */

import { encryptPayload, type Payload, readPayload, sha256Hex } from "./payload.ts";
import { verifyEventSignature } from "./signature.ts";
import { type AppEntry, checkApps, splitPayload } from "./split.ts";

export type Bucket = {
  get(key: string): Promise<{ text(): Promise<string> } | null>;
  put(key: string, body: string, o: { httpMetadata: { contentType: string; cacheControl: string } }): Promise<unknown>;
};

export type HubEnv = {
  SETTINGS: Bucket;
  GROWTHBOOK_EVENT_SECRET?: string | undefined;
  REFRESH_SECRET?: string | undefined;
  [secret: string]: unknown;
};

export type HubConfig = {
  connections: { stage: string; clientKey: string }[];
  apps: AppEntry[];
  cdn?: string;
  recheckMs?: number;
};

export type RefreshReport = { written: string[]; same: string[]; failed: string[] };

type Ctx = { waitUntil(p: Promise<unknown>): void };

const HEADERS = { httpMetadata: { contentType: "application/json", cacheControl: "public, max-age=60" } };

const fileOf = (path: string, stage: string, version: number) =>
  [path, stage, `v${version}.json`].filter((s) => s !== "").join("/");

/** GrowthBook moves `dateUpdated` on any edit in the project, so a hash over it would rewrite every app's file. */
const hashedText = ({ dateUpdated: _moved, ...stable }: Payload) => JSON.stringify(stable);

function same(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

export function createHub(
  config: HubConfig,
  deps: { fetch?: typeof fetch; sleep?: (ms: number) => Promise<void> } = {},
) {
  const problems = checkApps(config.apps);
  if (problems.length > 0) throw new Error(`remote-settings hub: bad config: ${problems.join("; ")}`);
  const fetcher = deps.fetch ?? fetch;
  const sleep = deps.sleep ?? ((ms: number) => new Promise<void>((done) => setTimeout(done, ms)));
  const cdn = (config.cdn ?? "https://cdn.growthbook.io").replace(/\/$/, "");
  const recheckMs = config.recheckMs ?? 15000;
  const prefixes = config.apps.map((a) => a.prefix);

  async function readConnection(clientKey: string): Promise<Payload | null> {
    try {
      const res = await fetcher(`${cdn}/api/features/${clientKey.trim()}`);
      if (!res.ok) return null;
      return readPayload(JSON.parse(await res.text()));
    } catch {
      return null;
    }
  }

  async function writeOne(env: HubEnv, file: string, plain: string, part: Payload, secret: unknown) {
    if (typeof secret !== "string" || secret.trim() === "") throw new Error("no key");
    const hashFile = file.replace(/\.json$/, ".sha256");
    const hash = await sha256Hex(plain);
    const last = await env.SETTINGS.get(hashFile);
    if (last && (await last.text()) === hash) return "same" as const;
    await env.SETTINGS.put(file, JSON.stringify(await encryptPayload(part, secret)), HEADERS);
    await env.SETTINGS.put(hashFile, hash, HEADERS);
    return "written" as const;
  }

  async function refresh(env: HubEnv): Promise<RefreshReport> {
    const report: RefreshReport = { written: [], same: [], failed: [] };
    const unknown = new Set<string>();
    await Promise.allSettled(
      config.connections.map(async ({ stage, clientKey }) => {
        const payload = await readConnection(clientKey);
        const split = payload ? splitPayload(payload, prefixes) : null;
        for (const k of split?.unknown ?? []) unknown.add(k);
        const jobs: Promise<void>[] = [];
        for (const app of config.apps) {
          const part = split?.byPrefix.get(app.prefix);
          const plain = part ? hashedText(part) : "";
          for (const { secret, version } of app.keys[stage] ?? []) {
            for (const path of app.paths) {
              const file = fileOf(path, stage, version);
              if (!part) {
                report.failed.push(file);
                continue;
              }
              jobs.push(
                writeOne(env, file, plain, part, env[secret]).then(
                  (r) => void report[r].push(file),
                  (e: unknown) => {
                    console.warn(`remote-settings hub: ${file} not written: ${e instanceof Error ? e.message : "error"}`);
                    report.failed.push(file);
                  },
                ),
              );
            }
          }
        }
        await Promise.all(jobs);
      }),
    );
    if (unknown.size > 0) console.warn(`remote-settings hub: features with no known prefix: ${[...unknown].join(", ")}`);
    return report;
  }

  async function onHook(req: Request, env: HubEnv, ctx: Ctx): Promise<Response> {
    const body = await req.text();
    const ok = await verifyEventSignature(body, req.headers.get("x-growthbook-signature"), env.GROWTHBOOK_EVENT_SECRET ?? "");
    if (!ok) {
      console.warn("remote-settings hub: refused an event, the signature does not match");
      return new Response(null, { status: 401 });
    }
    // GrowthBook can send the event before its CDN serves the new payload, so read again a little later.
    ctx.waitUntil(
      (async () => {
        await refresh(env);
        await sleep(recheckMs);
        await refresh(env);
      })(),
    );
    return new Response(null, { status: 200 });
  }

  async function onRefresh(req: Request, env: HubEnv): Promise<Response> {
    const want = (env.REFRESH_SECRET ?? "").trim();
    const got = (req.headers.get("x-refresh-secret") ?? "").trim();
    if (want === "" || !same(got, want)) return new Response(null, { status: 401 });
    const report = await refresh(env);
    return new Response(JSON.stringify(report), { status: 200, headers: { "content-type": "application/json" } });
  }

  return {
    refresh,
    async fetch(req: Request, env: HubEnv, ctx: Ctx): Promise<Response> {
      const { pathname } = new URL(req.url);
      if (req.method === "POST" && pathname === "/hook") return onHook(req, env, ctx);
      if (req.method === "POST" && pathname === "/refresh") return onRefresh(req, env);
      return new Response(null, { status: 404 });
    },
    async scheduled(_event: unknown, env: HubEnv, _ctx: Ctx): Promise<void> {
      await refresh(env);
    },
  };
}
