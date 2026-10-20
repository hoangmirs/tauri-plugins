import type { Payload } from "./payload.ts";

/** One app in the hub's config. `keys` maps a stage to the keys the hub writes
 *  for it: two while a key changes, so copies on the old key keep reading. */
export type AppEntry = {
  prefix: string;
  paths: string[];
  keys: Record<string, { secret: string; version: number }[]>;
};

const PREFIX = /^[a-z0-9_-]+\.$/;

/** The problems in the apps' config. Empty means good. */
export function checkApps(apps: AppEntry[]): string[] {
  const problems: string[] = [];
  for (const a of apps) {
    if (!PREFIX.test(a.prefix)) problems.push(`prefix "${a.prefix}" must be lower-case letters, digits, _ or -, then "."`);
    if (a.paths.length === 0) problems.push(`app "${a.prefix}" has no path`);
  }
  const paths = apps.flatMap((a) => a.paths);
  for (const p of new Set(paths)) {
    if (paths.filter((x) => x === p).length > 1) problems.push(`path "${p}" is used twice, so one app's file would reach another`);
  }
  for (const a of apps) {
    for (const b of apps) {
      if (a !== b && b.prefix.startsWith(a.prefix)) problems.push(`prefix "${b.prefix}" starts with "${a.prefix}"`);
    }
  }
  return problems;
}

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

function groupsNamed(node: unknown, into: Set<string>): void {
  if (Array.isArray(node)) {
    for (const n of node) groupsNamed(n, into);
    return;
  }
  if (!isRecord(node)) return;
  for (const [k, v] of Object.entries(node)) {
    if ((k === "$inGroup" || k === "$notInGroup") && typeof v === "string") into.add(v);
    else groupsNamed(v, into);
  }
}

/** One payload for each prefix, with the prefix taken off each key. `unknown`
 *  lists the keys that belong to no app. */
export function splitPayload(p: Payload, prefixes: string[]): { byPrefix: Map<string, Payload>; unknown: string[] } {
  const features = new Map<string, Record<string, unknown>>(prefixes.map((x) => [x, {}]));
  const unknown: string[] = [];
  for (const [key, def] of Object.entries(p.features)) {
    const prefix = prefixes.find((x) => key.startsWith(x) && key.length > x.length);
    if (prefix === undefined) unknown.push(key);
    else (features.get(prefix) as Record<string, unknown>)[key.slice(prefix.length)] = def;
  }
  const { features: _all, experiments: _visual, savedGroups, ...rest } = p;
  const byPrefix = new Map<string, Payload>();
  for (const [prefix, own] of features) {
    const named = new Set<string>();
    for (const def of Object.values(own)) if (isRecord(def)) groupsNamed(def.rules, named);
    const groups: Record<string, unknown> = {};
    for (const id of named) if (savedGroups && id in savedGroups) groups[id] = savedGroups[id];
    byPrefix.set(prefix, { ...rest, features: own, experiments: [], savedGroups: groups });
  }
  return { byPrefix, unknown };
}
