//! Firebase Remote Config as a source, for an app that keeps it behind the
//! global file's `source` switch. The app passes `tauri-plugin-firebase-api`
//! in, so this package depends on no Firebase package.

import type { FirebaseAdapter } from "./index.ts";
import type { Part } from "./types.ts";

/** The part of `tauri-plugin-firebase-api` the adapter uses. */
export type FirebaseApi = {
  getAll(): Promise<Record<string, { value: string; source: string }>>;
  fetch(): Promise<string>;
  activate(): Promise<boolean>;
  listen(on: (keys: string[]) => void): Promise<() => void>;
};

export function firebaseAdapter(api: FirebaseApi): FirebaseAdapter {
  const get = async () => api;
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
