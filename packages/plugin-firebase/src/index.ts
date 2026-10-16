//! Framework-neutral facade for `tauri-plugin-firebase`: the same functions on
//! a phone, where they call the plugin's commands, and on the web or a Tauri
//! desktop app, where they use the Firebase web SDK in `./web.ts`. No function
//! ever rejects. A failure reads as `"failed"`, `false` or the defaults.

import { invoke, isTauri } from "@tauri-apps/api/core";
import * as web from "./web.ts";

export type { Defaults, Entry, Fetched, Loader, Params, Source, WebConfig } from "./web.ts";
import type { Defaults, Entry, Fetched, Loader, Params, WebConfig } from "./web.ts";

/** Firebase keeps at most 25 parameters on an event, and text up to 100. */
const MAX_PARAMS = 25;
const MAX_TEXT = 100;

type Route = "commands" | "web";

let mode: Promise<string> | undefined;

/** The plugin's mode, asked once: `"native"`, `"absent"` or `"desktop"`, or
 *  `"web"` outside Tauri. */
function modeOf(): Promise<string> {
  mode ??= isTauri() ? invoke<string>("plugin:firebase|mode").catch(() => "absent") : Promise.resolve("web");
  return mode;
}

/** Phones use the commands. A desktop uses the web SDK once `init` got a
 *  config, and the commands' defaults until then. The plain web always uses
 *  the web SDK. */
async function routeOf(): Promise<Route> {
  const m = await modeOf();
  return m === "web" || (m === "desktop" && web.isConfigured()) ? "web" : "commands";
}

async function command<T>(cmd: string, args: Record<string, unknown> | undefined, fallback: T): Promise<T> {
  try {
    return await invoke<T>(`plugin:firebase|${cmd}`, args);
  } catch {
    return fallback;
  }
}

let defaults: Defaults = {};

/**
 * Starts the web path with the project's web config. On a phone this does
 * nothing: the native SDK reads the app's own Firebase file. `loader`
 * replaces the Firebase SDK, for tests.
 */
export async function init(config?: WebConfig, loader?: Loader): Promise<void> {
  if (config === undefined) return;
  const m = await modeOf();
  if (m === "web" || m === "desktop") await web.init(config, loader);
}

export async function setDefaults(next: Defaults): Promise<void> {
  defaults = next;
  if ((await routeOf()) === "web") return web.setDefaults(next);
  await command("set_defaults", { defaults: next }, undefined);
}

export async function fetch(): Promise<Fetched> {
  if ((await routeOf()) === "web") return web.fetch();
  return command<Fetched>("fetch", undefined, "failed");
}

export async function activate(): Promise<boolean> {
  if ((await routeOf()) === "web") return web.activate();
  return command("activate", undefined, false);
}

export async function getAll(): Promise<Record<string, Entry>> {
  if ((await routeOf()) === "web") return web.getAll();
  return command("get_all", undefined, web.defaultsView(defaults));
}

/** Keeps the first 25 parameters and cuts text to 100 characters. */
export function trim(params: Params = {}): Params {
  return Object.fromEntries(
    Object.entries(params)
      .slice(0, MAX_PARAMS)
      .map(([k, v]) => [k, typeof v === "string" ? v.slice(0, MAX_TEXT) : v]),
  );
}

export async function logEvent(name: string, params?: Params): Promise<void> {
  const kept = trim(params);
  if ((await routeOf()) === "web") return web.logEvent(name, kept);
  await command("log_event", { name, params: kept }, undefined);
}

export async function setCollection(enabled: boolean): Promise<void> {
  if ((await routeOf()) === "web") return web.setCollection(enabled);
  await command("set_collection", { enabled }, undefined);
}

export async function resetForTests(): Promise<void> {
  mode = undefined;
  defaults = {};
  await web.resetForTests();
}
