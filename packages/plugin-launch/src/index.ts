import { invoke, isTauri } from "@tauri-apps/api/core";

/** Tells the app that the page has painted its own launch frame, so the
 * native one over the web view can go. Call it once the page's frame is on
 * screen. Outside Tauri, or when the plugin is not there, it does nothing;
 * this promise never rejects. */
export async function ready(): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke("plugin:launch|ready");
  } catch {
    /* the native frame takes itself away after its limit */
  }
}
