import { addPluginListener, invoke, isTauri } from "@tauri-apps/api/core";

/** What every ad request carries. */
export type Settings = { childDirected: boolean; maxRating: "G" | "PG" | "T" | "MA" };

/** The page's empty box the native ad sits in, in CSS pixels. `x` and `y`
 * are its place in the scroll box's content, `vx` and `vy` its place on
 * screen. `scrollHeight` is the scroll box's content height. Colours are CSS
 * `rgb()` text. */
export type Slot = {
  unitId: string;
  x: number;
  y: number;
  vx: number;
  vy: number;
  w: number;
  h: number;
  scrollHeight: number;
  /** The visible part of the scroll box on screen, for example above a fixed
   * tab bar. Android clips the card to it. */
  clipTop?: number;
  clipBottom?: number;
  ink: string;
  muted: string;
  button: string;
  edge: string;
};

export type NativeEvent = "native_loaded" | "native_failed" | "native_shown" | "native_tapped";

/** The command's answer, or `fallback` outside Tauri or when the call fails.
 * No call ever rejects. */
async function ask<T>(cmd: string, args: Record<string, unknown> | undefined, fallback: T): Promise<T> {
  if (!isTauri()) return fallback;
  try {
    return await invoke<T>(`plugin:ads|${cmd}`, args);
  } catch {
    return fallback;
  }
}

export const init = (settings: Settings): Promise<void> => ask("init", { settings }, undefined);

export const consent = (): Promise<"obtained" | "not_required" | "failed"> =>
  ask<"obtained" | "not_required" | "failed">("consent", undefined, "failed");

export const showNative = (slot: Slot): Promise<void> => ask("show_native", { slot }, undefined);

export const hideNative = (): Promise<void> => ask("hide_native", undefined, undefined);

export const loadRewarded = (unitId: string): Promise<"ready" | "failed"> =>
  ask<"ready" | "failed">("load_rewarded", { unitId }, "failed");

export const showRewarded = (): Promise<"earned" | "closed" | "failed"> =>
  ask<"earned" | "closed" | "failed">("show_rewarded", undefined, "failed");

/** Calls `fn` for each native ad event. Resolves to a function that stops
 * listening. Outside Tauri it never calls `fn`. */
export async function onNative(fn: (e: NativeEvent) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  try {
    const listener = await addPluginListener<{ event: NativeEvent }>("ads", "native", (p) => fn(p.event));
    return () => void listener.unregister();
  } catch {
    return () => {};
  }
}
