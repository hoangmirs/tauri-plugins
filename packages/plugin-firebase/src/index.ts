//! Framework-neutral facade for `tauri-plugin-firebase`: the same functions on
//! a phone, where they call the plugin's commands, and on the web or a Tauri
//! desktop app, where they use the Firebase web SDK in `./web.ts`. No function
//! ever rejects. A failure reads as `"failed"`, `false` or the defaults.

import { addPluginListener, invoke, isTauri, type PluginListener } from "@tauri-apps/api/core";
import * as web from "./web.ts";

export type { Defaults, Entry, Fetched, Loader, Params, Signals, Source, WebConfig } from "./web.ts";
import type { Defaults, Entry, Fetched, Loader, Params, Signals, WebConfig } from "./web.ts";

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
  // The web module keeps them even on the command route: a desktop moves to
  // the web route at `init`, and must not lose defaults set before it.
  await web.setDefaults(next);
  if ((await routeOf()) === "web") return;
  await command("set_defaults", { defaults: next }, undefined);
}

export async function fetch(): Promise<Fetched> {
  if ((await routeOf()) === "web") return web.fetch();
  return command<Fetched>("fetch", undefined, "failed");
}

/** Calls `on` with the changed keys after each publish Firebase fetched. The
 *  values are fetched, not active: activate them as after `fetch`. Resolves to
 *  the function that stops listening. Never rejects. */
export async function listen(on: (keys: string[]) => void): Promise<() => void> {
  if ((await routeOf()) === "web") return web.listen(on);
  hearers.add(on);
  native ??= startNative();
  await native;
  let stopped = false;
  return () => {
    if (stopped) return;
    stopped = true;
    hearers.delete(on);
    if (hearers.size === 0) void stopNative(native);
  };
}

/** Every `on` the app gave. The phone holds one native listener for all of
 *  them: each extra one would hear every publish again. */
const hearers = new Set<(keys: string[]) => void>();
let native: Promise<PluginListener | null> | undefined;
/** The last stop's `unlisten`. Tauri runs each async command on its own task,
 *  so a `listen` sent before it ends could reach the phone first. */
let stopping: Promise<void> = Promise.resolve();

const keysOf = (raw: unknown): string[] => {
  const keys = (raw as { keys?: unknown } | null)?.keys;
  return Array.isArray(keys) ? keys.filter((k): k is string => typeof k === "string") : [];
};

async function startNative(): Promise<PluginListener | null> {
  try {
    await stopping;
    const listener = await addPluginListener("firebase", "configUpdate", (raw: unknown) => {
      const keys = keysOf(raw);
      for (const on of hearers) {
        try {
          on(keys);
        } catch {
          /* one app handler failing does not stop the others */
        }
      }
    });
    await command("listen", undefined, undefined);
    return listener;
  } catch {
    return null;
  }
}

async function stopNative(started: Promise<PluginListener | null> | undefined): Promise<void> {
  const listener = await started;
  // Another stop already took this start down, and a later start may be on.
  if (native !== started) {
    await listener?.unregister().catch(() => {});
    return;
  }
  // A listen that came back while this stop waited keeps the native side on.
  if (hearers.size > 0) return;
  native = undefined;
  stopping = (async () => {
    await command("unlisten", undefined, undefined);
    await listener?.unregister().catch(() => {});
  })();
  await stopping;
}

export async function activate(): Promise<boolean> {
  if ((await routeOf()) === "web") return web.activate();
  return command("activate", undefined, false);
}

/** Sets the custom signals that Remote Config conditions read, for the
 *  next fetch. A number sets a signal and `null` unsets it. Never rejects. */
export async function setCustomSignals(signals: Signals): Promise<void> {
  if ((await routeOf()) === "web") return web.setCustomSignals(signals);
  await command("set_custom_signals", { signals }, undefined);
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

/** One line of a stack trace. */
export type Frame = { symbol: string; file: string; line: number };

const ANONYMOUS = "(anonymous)";
const V8_FRAME = /^\s*at\s+(?:(.*?)\s+\()?(.+?):(\d+)(?::\d+)?\)?\s*$/;
const WEBKIT_FRAME = /^([^@]*)@(.+?):(\d+)(?::\d+)?\s*$/;

/** Reads the frames of a V8 or a WebKit stack. A line it cannot read, such as
 *  the error's own first line, gives no frame. */
export function parseStack(stack: string): Frame[] {
  const frames: Frame[] = [];
  for (const text of stack.split("\n")) {
    const m = V8_FRAME.exec(text) ?? WEBKIT_FRAME.exec(text);
    if (m === null) continue;
    frames.push({ symbol: m[1]?.trim() || ANONYMOUS, file: m[2] ?? "", line: Number(m[3]) });
  }
  return frames;
}

/** Crash reports exist on a phone with its Firebase file only. Everywhere
 *  else each crash call does nothing. */
async function crashCommand(cmd: string, args: Record<string, unknown>): Promise<void> {
  if ((await modeOf()) !== "native") return;
  await command(cmd, args, undefined);
}

export async function setCrashCollection(enabled: boolean): Promise<void> {
  await crashCommand("crash_set_collection", { enabled });
}

/** Sets the keys that go with every later crash report. */
export async function setCrashKeys(keys: Record<string, string>): Promise<void> {
  await crashCommand("crash_set_keys", { keys });
}

/** Adds a line to the log that goes with the next crash report. */
export async function crashLog(line: string): Promise<void> {
  await crashCommand("crash_log", { line });
}

/** Reports an error that did not stop the app. */
export async function recordError(name: string, message: string, stack: string): Promise<void> {
  await crashCommand("crash_record", { name, message, frames: parseStack(stack) });
}

export async function resetForTests(): Promise<void> {
  mode = undefined;
  defaults = {};
  hearers.clear();
  native = undefined;
  stopping = Promise.resolve();
  await web.resetForTests();
}
