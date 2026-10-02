import { addPluginListener, invoke, isTauri } from "@tauri-apps/api/core";

/** How the phone's ringer is set. Only Android reports anything but
 * `"normal"`: iOS mutes an app's ambient sound itself, and a desktop has no
 * such mode. */
export type Mode = "normal" | "vibrate" | "silent";

/** Whether sounds should stay off: silent or vibrate. */
export const isQuiet = (mode: Mode): boolean => mode !== "normal";

/** A mode from the other side of the bridge, checked. Anything that is not
 * one of the three is `"normal"`: an app that cannot tell stays audible
 * rather than going quiet for no reason the player can see. */
export function modeOf(raw: unknown): Mode {
  const mode = raw && typeof raw === "object" ? (raw as { mode?: unknown }).mode : undefined;
  return mode === "silent" || mode === "vibrate" ? mode : "normal";
}

/** How the ringer is set now. Outside Tauri, or when the command cannot be
 * reached, `"normal"`; this promise never rejects. */
export async function silentMode(): Promise<Mode> {
  if (!isTauri()) return "normal";
  try {
    return modeOf(await invoke("plugin:silent-mode|state"));
  } catch {
    return "normal";
  }
}

/** Calls `onChange` whenever the ringer mode moves, until the returned
 * function is called. Outside Android it never calls, and the function does
 * nothing; this promise never rejects. */
export async function onSilentModeChange(onChange: (mode: Mode) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  try {
    const listener = await addPluginListener("silent-mode", "change", (raw: unknown) => onChange(modeOf(raw)));
    return () => {
      listener.unregister().catch(() => {});
    };
  } catch {
    return () => {};
  }
}
