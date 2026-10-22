import { invoke, isTauri } from "@tauri-apps/api/core";

/** Keeps the phone's screen on while `on` is true, and lets it sleep again
 * when called with false. It holds only while the app is on screen. Outside
 * Tauri, or on a desktop, it does nothing; this promise never rejects. */
export async function keepAwake(on: boolean): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke("plugin:keep-awake|set", { on });
  } catch {
    /* the phone keeps its own timeout */
  }
}
