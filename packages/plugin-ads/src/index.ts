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
  /** The visible part of the scroll box on screen, for example below a top
   * bar the page pins inside the box and above a fixed tab bar. Both phones
   * clip the card to it. */
  clipTop?: number;
  clipBottom?: number;
  /** The words on the ad label, in the player's language. The plugin shows
   * "Ad" without it, because Google requires a visible ad label. */
  label?: string;
  ink: string;
  muted: string;
  button: string;
  edge: string;
};

export type NativeEvent = "native_loaded" | "native_failed" | "native_shown" | "native_tapped";

/** What Google says one ad earned. `micros` is the value in millionths of
 * `currency`. */
export type Paid = {
  format: "native" | "rewarded";
  micros: number;
  currency: string;
  precision: "unknown" | "estimated" | "publisher" | "precise";
};

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

/** Whether the app must offer Google's privacy options form, for a user who
 * wants to change a past consent answer. Ask after `consent`. */
export const privacyOptions = (): Promise<"required" | "not_required"> =>
  ask<"required" | "not_required">("privacy_options", undefined, "not_required");

export const showPrivacyOptions = (): Promise<"shown" | "failed"> =>
  ask<"shown" | "failed">("show_privacy_options", undefined, "failed");

/** Mutes the sound of every ad, for an app whose own sound is off. */
export const setMuted = (muted: boolean): Promise<void> => ask("set_muted", { muted }, undefined);

/** Calls `fn` each time an ad earns money, as Google reports it. Resolves to
 * a function that stops listening. Outside Tauri it never calls `fn`. */
export async function onPaid(fn: (p: Paid) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  try {
    const listener = await addPluginListener<Paid>("ads", "paid", fn);
    return () => void listener.unregister();
  } catch {
    return () => {};
  }
}

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
