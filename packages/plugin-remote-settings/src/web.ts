/** The web path: the browser reads the two files itself and keeps the store
 *  in `localStorage`. A private window, whose storage throws, still gets the
 *  code's defaults. */

import type { Files, Store } from "./types.ts";

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

/** Two capital letters. Cloudflare sends `XX` for an unknown country and `T1` for Tor. */
export function countryOf(header: string | null): string | null {
  if (header === null || !/^[A-Z]{2}$/.test(header) || header === "XX" || header === "T1") return null;
  return header;
}

export const urlsOf = (baseUrl: string, stage: string, keyVersion: number) => {
  const root = `${baseUrl.replace(/\/+$/, "")}/${stage}`;
  return { global: `${root}/global.json`, rules: `${root}/v${keyVersion}.json` };
};

async function one(url: string, header: string, fetcher: typeof fetch, signal?: AbortSignal) {
  try {
    const res = await fetcher(url, signal ? { signal } : {});
    const country = countryOf(res.headers.get(header));
    return { body: res.ok ? await res.text() : null, country };
  } catch {
    return { body: null, country: null };
  }
}

export async function readFilesWeb(o: {
  baseUrl: string;
  stage: string;
  keyVersion: number;
  countryHeader: string;
  fetcher?: typeof fetch;
  signal?: AbortSignal;
}): Promise<Files> {
  const urls = urlsOf(o.baseUrl, o.stage, o.keyVersion);
  const fetcher = o.fetcher ?? fetch;
  const [global, rules] = await Promise.all([
    one(urls.global, o.countryHeader, fetcher, o.signal),
    one(urls.rules, o.countryHeader, fetcher, o.signal),
  ]);
  return { global: global.body, rules: rules.body, country: global.country ?? rules.country };
}

type Storage2 = Pick<Storage, "getItem" | "setItem">;

const local = (): Storage2 | undefined => globalThis.localStorage;

export function webStore(namespace: string, storage: () => Storage2 | undefined = local): Store {
  const name = (n: string) => `remote-settings:${namespace}:${n}`;
  return {
    async get(n) {
      try {
        return storage()?.getItem(name(n)) ?? null;
      } catch {
        return null;
      }
    },
    async set(n, text) {
      try {
        storage()?.setItem(name(n), text);
      } catch {
        // A full or private store: the next launch reads the network again.
      }
    },
  };
}

/** The first good id: the kept one, else `seed`, else a new one. Null when the
 *  store does not keep it: an id made new at each launch would move the user
 *  between test groups. */
export async function webInstallId(store: Store, seed: string | null): Promise<string | null> {
  const kept = await store.get("install-id");
  if (kept && UUID.test(kept)) return kept;
  const id = seed && UUID.test(seed) ? seed : crypto.randomUUID();
  await store.set("install-id", id);
  return (await store.get("install-id")) === id ? id : null;
}
