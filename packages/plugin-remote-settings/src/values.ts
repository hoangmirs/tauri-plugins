import type { From, Keys, Mode, Part, Saved, SourceName, Values } from "./types.ts";

const SOURCES: readonly string[] = ["growthbook", "firebase"];
const MODES: readonly string[] = ["hidden", "rest", "now"];
const PLUGIN: Record<string, readonly string[]> = { source: SOURCES, apply: MODES };

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

export const asText = (v: unknown): string => (typeof v === "string" ? v : JSON.stringify(v));

/** The global file's own keys and the plugin's keys, as text. Null when the file is not an object. */
export function globalPart(raw: unknown, keys: Keys): Part | null {
  if (!isRecord(raw)) return null;
  const out: Part = {};
  for (const [k, v] of Object.entries(raw)) if (k in PLUGIN || keys[k]?.global) out[k] = asText(v);
  return out;
}

export function pluginKeys(global: Part): { source: SourceName | null; apply: Mode | null } {
  const source = global.source !== undefined && SOURCES.includes(global.source) ? (global.source as SourceName) : null;
  const apply = global.apply !== undefined && MODES.includes(global.apply) ? (global.apply as Mode) : null;
  return { source, apply };
}

const reads = (k: string, text: string, keys: Keys): boolean => {
  const words = PLUGIN[k];
  if (words) return words.includes(text);
  return keys[k]?.read(text) !== undefined;
};

/** The saved settings after a read. A part that failed keeps what was saved.
 *  In the global file, a key whose value does not read keeps its saved value,
 *  and the rest apply. */
export function nextSaved(
  prev: Saved | null,
  read: { global: Part | null; source: Part | null; from: SourceName; country: string | null },
  keys: Keys,
): Saved {
  let global = prev?.global ?? {};
  if (read.global) {
    global = {};
    for (const [k, text] of Object.entries(read.global)) {
      const kept = prev?.global[k];
      if (reads(k, text, keys)) global[k] = text;
      else if (kept !== undefined) global[k] = kept;
    }
  }
  return {
    global,
    source: read.source ?? prev?.source ?? {},
    from: read.source !== null ? read.from : (prev?.from ?? read.from),
    country: read.country ?? prev?.country ?? null,
  };
}

/** Each key's value, typed by its reader, where it came from, and which keys a
 *  source sent. `fresh` false marks every value as saved from an earlier launch. */
export function valuesOf<K extends Keys>(
  saved: Saved | null,
  keys: K,
  fresh: boolean,
): { values: Values<K>; from: Record<keyof K, From>; sent: (keyof K)[] } {
  const values: Record<string, unknown> = {};
  const from: Record<string, From> = {};
  const sent: (keyof K)[] = [];
  for (const [name, key] of Object.entries(keys)) {
    const text = key.global ? saved?.global[name] : saved?.source[name];
    const value = text === undefined ? undefined : key.read(text);
    if (value === undefined || !saved) {
      values[name] = key.fallback;
      from[name] = "default";
      continue;
    }
    values[name] = value;
    from[name] = fresh ? (key.global ? "global" : saved.from) : "saved";
    sent.push(name);
  }
  return { values: values as Values<K>, from: from as Record<keyof K, From>, sent };
}
