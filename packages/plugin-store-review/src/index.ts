import { invoke, isTauri } from "@tauri-apps/api/core";

/** Asks the phone to show its own rating prompt. The phone decides whether it
 * shows, and neither store tells the app what the player chose, so this
 * returns nothing. Outside Tauri, or when the phone shows nothing, it does
 * nothing; this promise never rejects. */
export async function requestReview(): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke("plugin:store-review|request_review");
  } catch {
    /* the phone showed nothing */
  }
}
