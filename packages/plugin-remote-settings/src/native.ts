/** The Rust crate's commands. Each call that fails answers its fallback, so
 *  an app without the crate registered still runs on its defaults. */

import { invoke } from "@tauri-apps/api/core";
import type { Files, Store } from "./types.ts";

export type NativeConfig = {
  baseUrl: string;
  stage: string;
  keyVersion: number;
  countryHeader: string;
  platform: "ios" | "android" | "macos" | "windows" | "linux";
  version: string;
};

const NO_FILES: Files = { global: null, rules: null, country: null };

async function call<T>(cmd: string, args: Record<string, unknown> | undefined, fallback: T): Promise<T> {
  try {
    return (await invoke<T>(`plugin:remote-settings|${cmd}`, args)) ?? fallback;
  } catch {
    return fallback;
  }
}

export const nativeConfig = () => call<NativeConfig | null>("config", undefined, null);
export const nativePrefetched = () => call<Files | null>("prefetched", undefined, null);
export const nativeRead = () => call<Files>("read", undefined, NO_FILES);
export const nativeInstallId = (seed: string | null) => call<string | null>("install_id", { seed }, null);
export const nativeStore: Store = {
  get: (name) => call<string | null>("get", { name }, null),
  set: (name, text) => call<void>("set", { name, text }, undefined),
};
