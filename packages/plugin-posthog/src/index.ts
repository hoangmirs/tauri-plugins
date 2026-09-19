//! Framework-neutral facade for `tauri-plugin-posthog`: the same five
//! functions whether the app is running under Tauri (which calls the
//! plugin's Rust commands) or on the plain web (which queues in
//! `localStorage` and sends with `fetch`, in `./web.ts`) — same event
//! shape either way, so `PostHog` cannot tell which side sent an event.

import { invoke, isTauri } from "@tauri-apps/api/core";
import * as web from "./web.ts";

export type { WebConfig } from "./web.ts";
import type { WebConfig } from "./web.ts";

/**
 * Configures the web path: the project API key, optionally a non-US (or
 * self-hosted) host, and the app's own version — then starts sending
 * whatever an earlier page left queued. A no-op under Tauri, where Rust
 * already holds its own config from `posthog::Config` at `init`.
 */
export function init(config: WebConfig): void {
  if (isTauri()) return;
  web.init(config);
}

/**
 * Queues `event` with `properties` for `PostHog`, adding `$locale` from
 * `navigator.language` unless the caller already set one. Under Tauri this
 * calls the plugin's `capture` command; on the web it queues in
 * `localStorage`, flushing once 20 events are waiting. Never rejects: any
 * failure — a dead command, a throwing `localStorage` — is dropped
 * silently, the same way the Rust side drops what it cannot send.
 */
export async function track(event: string, properties?: Record<string, unknown>): Promise<void> {
  if (isTauri()) {
    try {
      await invoke("plugin:posthog|capture", { event, properties: web.addLocale(properties) });
    } catch {
      // analytics never breaks the app
    }
    return;
  }
  await web.track(event, properties);
}

/**
 * Asks for a flush now. Under Tauri this returns as soon as the command
 * has been asked, without waiting for the network; on the web it awaits
 * the request itself. Never rejects.
 */
export async function flush(): Promise<void> {
  if (isTauri()) {
    try {
      await invoke("plugin:posthog|flush", {});
    } catch {
      // analytics never breaks the app
    }
    return;
  }
  await web.flush();
}

/**
 * Opts the install out (clearing anything already queued) or back in, and
 * persists the choice across launches. Never rejects.
 */
export async function setOptOut(out: boolean): Promise<void> {
  if (isTauri()) {
    try {
      await invoke("plugin:posthog|set_opt_out", { out });
    } catch {
      // analytics never breaks the app
    }
    return;
  }
  await web.setOptOut(out);
}

/**
 * Whether the install has opted out. Resolves `false`, never rejects, when
 * that cannot be determined — a dead command, or a throwing `localStorage`.
 */
export async function isOptedOut(): Promise<boolean> {
  if (isTauri()) {
    try {
      return await invoke<boolean>("plugin:posthog|is_opted_out", {});
    } catch {
      return false;
    }
  }
  return web.isOptedOut();
}
