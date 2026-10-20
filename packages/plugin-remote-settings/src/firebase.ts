//! Firebase Remote Config as a source, for an app that keeps it behind the
//! global file's `source` switch. Only an app that imports this path bundles
//! Firebase.

import type { FirebaseAdapter } from "./index.ts";
import type { Part } from "./types.ts";

type Api = {
  getAll(): Promise<Record<string, { value: string; source: string }>>;
  fetch(): Promise<string>;
  activate(): Promise<boolean>;
  listen(on: (keys: string[]) => void): Promise<() => void>;
};

// Loaded on first use, so this module loads even before the Firebase package is built.
const load = (): Promise<Api> => import("tauri-plugin-firebase-api") as Promise<Api>;

export function firebaseAdapter(api?: Api): FirebaseAdapter {
  const get = () => (api ? Promise.resolve(api) : load());
  return {
    async values() {
      try {
        const all = await (await get()).getAll();
        const out: Part = {};
        for (const [k, e] of Object.entries(all)) if (e.source === "remote") out[k] = e.value;
        return out;
      } catch {
        return null;
      }
    },
    async fetch() {
      try {
        return (await (await get()).fetch()) === "fetched";
      } catch {
        return false;
      }
    },
    async activate() {
      try {
        return await (await get()).activate();
      } catch {
        return false;
      }
    },
    async listen(on) {
      try {
        return await (await get()).listen(() => on());
      } catch {
        return () => {};
      }
    },
  };
}
