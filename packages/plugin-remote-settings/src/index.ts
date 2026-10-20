//! Remote settings for a Tauri app and its web build: a cached global file,
//! GrowthBook rules evaluated on the device, and Firebase Remote Config behind
//! a switch. The same code evaluates the rules on every platform. No function
//! here rejects: a failure keeps the saved values, or the code's defaults.

import { isTauri } from "@tauri-apps/api/core";
import { type Applier, createApplier, type Snapshot } from "./applier.ts";
import { nativeConfig, nativeInstallId, nativePrefetched, nativeRead, nativeStore } from "./native.ts";
import { type Attributes, evaluate } from "./rules.ts";
import type { Assignment, Files, From, Keys, Mode, Part, Saved, SourceName, Store, Used, Values } from "./types.ts";
import { globalPart, nextSaved, pluginKeys, valuesOf } from "./values.ts";
import { readFilesWeb, webInstallId, webStore } from "./web.ts";

export type { Attributes } from "./rules.ts";
export type { From, Key, Keys, Mode, Part, Saved, SourceName, Used, Values } from "./types.ts";

export type FirebaseAdapter = {
  values(): Promise<Part | null>;
  fetch(): Promise<boolean>;
  activate(): Promise<boolean>;
  listen(on: () => void): Promise<() => void>;
};

export type Options<K extends Keys> = {
  keys: K;
  key: { version: number; value: string };
  /** Where the files are on the web. In a Tauri app the Rust config says it. */
  web?: { baseUrl: string; stage: string; version: string; countryHeader?: string };
  defaultSource?: SourceName;
  apply?: Mode;
  attributes?: () => Record<string, unknown>;
  firebase?: { adapter: FirebaseAdapter; listen?: () => boolean };
  onExperiment?: (experiment: string, variant: string) => void;
  /** An app's own earlier storage, read once, when the plugin has nothing kept. */
  legacy?: { installId?: () => string | null; saved?: () => Saved | null };
};

export type Settings<K extends Keys> = {
  values(): { values: Values<K>; from: Record<keyof K, From>; sent: (keyof K)[] };
  used(): Used;
  country(): string | null;
  pending(): Values<K> | null;
  applyNow(keys?: (keyof K)[]): Promise<void>;
  subscribe(listener: (changed: (keyof K)[]) => void): () => void;
  setAtRest(atRest: boolean): void;
  ready(deadlineMs: number): Promise<void>;
  refresh(): Promise<void>;
  installId(): string | null;
};

type Env = {
  native: boolean;
  store: Store;
  platform: Attributes["platform"];
  version: string;
  files: (first: boolean, signal: AbortSignal) => Promise<Files>;
};

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

const parse = (text: string | null): unknown => {
  if (text === null) return null;
  try {
    return JSON.parse(text) as unknown;
  } catch {
    return null;
  }
};

function readSaved(text: string | null): Saved | null {
  const raw = parse(text);
  if (!isRecord(raw) || !isRecord(raw.global) || !isRecord(raw.source)) return null;
  const allText = (p: Record<string, unknown>) => Object.values(p).every((v) => typeof v === "string");
  if (!allText(raw.global) || !allText(raw.source)) return null;
  if (raw.from !== "growthbook" && raw.from !== "firebase") return null;
  const country = typeof raw.country === "string" ? raw.country : null;
  return { global: raw.global as Part, source: raw.source as Part, from: raw.from, country };
}

const safe = <T>(fn: () => T, fallback: T): T => {
  try {
    return fn();
  } catch {
    return fallback;
  }
};

const visibleNow = () => (globalThis.document?.visibilityState ?? "visible") === "visible";

async function envOf<K extends Keys>(o: Options<K>): Promise<Env | null> {
  if (isTauri()) {
    const c = await nativeConfig();
    if (c) {
      // Rust reads v<n>.json for its own version. Rules for another version never decrypt with this key.
      const sameKey = c.keyVersion === o.key.version;
      if (!sameKey) {
        console.warn(
          `remote-settings: the Rust config reads key version ${c.keyVersion}, but init has key version ${o.key.version}. New rules are not evaluated until the two match.`,
        );
      }
      const keyed = (f: Files): Files => (sameKey ? f : { ...f, rules: null });
      return {
        native: true,
        store: nativeStore,
        platform: c.platform,
        version: c.version,
        files: async (first) => keyed(first ? ((await nativePrefetched()) ?? (await nativeRead())) : await nativeRead()),
      };
    }
  }
  const w = o.web;
  if (!w) return null;
  return {
    native: false,
    store: webStore(`${w.baseUrl}|${w.stage}`),
    platform: "web",
    version: w.version,
    files: (_first, signal) =>
      readFilesWeb({
        baseUrl: w.baseUrl,
        stage: w.stage,
        keyVersion: o.key.version,
        countryHeader: w.countryHeader ?? "x-country",
        signal,
      }),
  };
}

export function init<K extends Keys>(o: Options<K>): Settings<K> {
  const listeners = new Set<(changed: (keyof K)[]) => void>();
  let applier: Applier | null = null;
  let id: string | null = null;
  let holdAsked = false;
  let atRestAsked: boolean | null = null;
  let firebaseFetched = false;
  let stopListen: (() => void) | null = null;
  let listening = false;
  let reading: AbortController | null = null;
  let readyOnce: Promise<void> | null = null;
  let chain: Promise<void> = Promise.resolve();

  const latest = () => applier?.pending() ?? applier?.applied() ?? null;
  const sourceOf = (global: Part | null): SourceName =>
    (global ? pluginKeys(global).source : null) ??
    pluginKeys(latest()?.saved.global ?? {}).source ??
    o.defaultSource ??
    "growthbook";
  const mode = (): Mode => pluginKeys(latest()?.saved.global ?? {}).apply ?? o.apply ?? "hidden";

  const attributes = (env: Env, country: string | null): Attributes => ({
    ...safe(() => o.attributes?.() ?? {}, {}),
    id,
    platform: env.platform,
    web: !env.native,
    version: env.version,
    country,
    language: (globalThis.navigator?.language ?? "").slice(0, 2).toLowerCase(),
    now: new Date().toISOString(),
  });

  const fb = () => (sourceOf(null) === "firebase" ? o.firebase?.adapter : undefined);

  const manageListen = () => {
    const want = !!fb() && safe(() => o.firebase?.listen?.() ?? false, false) && visibleNow();
    if (want && !listening) {
      listening = true;
      void fb()
        ?.listen(() => {
          firebaseFetched = true;
          void refresh();
        })
        .then(
          (stop) => {
            if (listening) stopListen = stop;
            else safe(stop, undefined);
          },
          () => {
            listening = false;
          },
        );
    } else if (!want && listening) {
      listening = false;
      if (stopListen) safe(stopListen, undefined);
      stopListen = null;
    }
  };

  const notify = (changed: string[]) => {
    for (const fn of listeners) safe(() => fn(changed as (keyof K)[]), undefined);
    manageListen();
  };

  const evaluateStored = async (env: Env, saved: Saved | null) => {
    if (sourceOf(saved?.global ?? null) !== "growthbook") return null;
    return evaluate(await env.store.get("rules"), attributes(env, saved?.country ?? null), o.key.value, o.keys);
  };

  const env: Promise<Env | null> = (async () => {
    const e = await envOf(o).catch(() => null);
    if (!e) return null;
    let saved = readSaved(await e.store.get("state"));
    if (!saved) {
      saved = safe(() => o.legacy?.saved?.() ?? null, null);
      if (saved) await e.store.set("state", JSON.stringify(saved));
    }
    const seed = safe(() => o.legacy?.installId?.() ?? null, null);
    id = await (e.native ? nativeInstallId(seed) : webInstallId(e.store, seed)).catch(() => null);
    let start: Snapshot | null = null;
    if (saved) {
      const again = await evaluateStored(e, saved);
      start = {
        saved: again ? { ...saved, source: again.part, from: "growthbook" } : saved,
        used: "saved",
        assignments: again?.assignments ?? [],
      };
    }
    applier = createApplier({
      start,
      mode,
      beforeApply,
      onApply: (_s, changed) => notify(changed),
      onExperiment: (exp, variant) => safe(() => o.onExperiment?.(exp, variant), undefined),
    });
    if (holdAsked) applier.holding();
    if (atRestAsked !== null) await applier.setAtRest(atRestAsked);
    await applier.setVisible(visibleNow());
    if (start) notify(Object.keys({ ...start.saved.global, ...start.saved.source }));
    globalThis.document?.addEventListener?.("visibilitychange", () => {
      const v = visibleNow();
      void applier?.setVisible(v);
      manageListen();
      if (v) void refresh();
    });
    return e;
  })().catch(() => null);

  async function beforeApply(s: Snapshot): Promise<Snapshot> {
    // The snapshot that applies names its source. The one on screen may name the source before a switch.
    const a = s.saved.from === "firebase" ? o.firebase?.adapter : undefined;
    if (!firebaseFetched || !a) return s;
    firebaseFetched = false;
    try {
      await a.activate();
      const values = await a.values();
      if (!values) return s;
      const next: Snapshot = { ...s, saved: { ...s.saved, source: values, from: "firebase" }, used: "firebase" };
      const e = await env;
      await e?.store.set("state", JSON.stringify(next.saved));
      return next;
    } catch {
      return s;
    }
  }

  async function readOnce(first: boolean): Promise<void> {
    const e = await env;
    if (!e || !applier) return;
    reading?.abort();
    const ctl = new AbortController();
    reading = ctl;
    const files = await e.files(first, ctl.signal);
    if (ctl.signal.aborted) return;
    const prev = latest()?.saved ?? null;
    const global = globalPart(parse(files.global), o.keys);
    const source = sourceOf(global);
    let part: Part | null = null;
    let assignments: Assignment[] = [];
    let used: Used = prev ? "saved" : "default";
    if (source === "growthbook") {
      const attrs = attributes(e, files.country ?? prev?.country ?? null);
      let r = await evaluate(files.rules, attrs, o.key.value, o.keys);
      if (r && files.rules !== null) {
        await e.store.set("rules", files.rules);
        used = "growthbook";
      } else {
        r = await evaluate(await e.store.get("rules"), attrs, o.key.value, o.keys);
      }
      part = r?.part ?? null;
      assignments = r?.assignments ?? [];
    } else {
      const a = o.firebase?.adapter;
      if (a) {
        try {
          part = await a.values();
          if (part) used = "firebase";
          if (await a.fetch()) firebaseFetched = true;
        } catch {
          part = null;
        }
      }
    }
    const next = nextSaved(prev, { global, source: part, from: source, country: files.country }, o.keys);
    await e.store.set("state", JSON.stringify(next));
    await applier.landed({ saved: next, used, assignments });
  }

  function refresh(): Promise<void> {
    chain = chain.then(
      () => readOnce(false),
      () => readOnce(false),
    );
    return chain.catch(() => {});
  }

  const firstRead: Promise<void> = env
    .then(() => {
      chain = readOnce(true).catch(() => {});
      return chain;
    })
    .catch(() => {});

  const applied = () => applier?.applied() ?? null;

  return {
    values: () => {
      const a = applied();
      return valuesOf(a?.saved ?? null, o.keys, a?.used === "growthbook" || a?.used === "firebase");
    },
    used: () => applied()?.used ?? "default",
    country: () => applied()?.saved.country ?? null,
    pending: () => {
      const p = applier?.pending();
      return p ? valuesOf(p.saved, o.keys, true).values : null;
    },
    applyNow: async (keys) => {
      await env;
      await applier?.applyNow(keys as string[] | undefined).catch(() => {});
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => void listeners.delete(listener);
    },
    setAtRest: (atRest) => {
      atRestAsked = atRest;
      void applier?.setAtRest(atRest).catch(() => {});
    },
    // Only the first call holds: a later one, from a screen that renders again, must not apply mid-task.
    ready: (deadlineMs) => {
      readyOnce ??= (async () => {
        holdAsked = true;
        applier?.holding();
        await Promise.race([firstRead, new Promise((done) => setTimeout(done, deadlineMs))]);
        await env;
        await applier?.holdEnded().catch(() => {});
      })();
      return readyOnce;
    },
    refresh: async () => {
      await firstRead;
      await refresh();
    },
    installId: () => id,
  };
}
