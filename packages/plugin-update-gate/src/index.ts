import { invoke, isTauri } from "@tauri-apps/api/core";

/** What `plugin:update-gate|check` answers, camelCase as Rust serialises it. */
export type Gate = {
  state: "ok" | "optional" | "forced";
  latestVersion: string | null;
  message: string | null;
  url: string | null;
};

/** The gate a caller sees when there is nothing to gate against: the plain
 * web (which always serves the newest code) or a command that could not be
 * reached. */
const OPEN: Gate = { state: "ok", latestVersion: null, message: null, url: null };

/** How long `checkUpdate` waits for the command before it gives up and
 * answers the open gate. Above the Rust side's default 5-second fetch
 * timeout, so a slow network is answered by Rust, and this only fires when
 * the command itself never answers. */
const DEFAULT_TIMEOUT_MS = 10_000;

/** The longest delay `setTimeout` keeps. Anything above it overflows and
 * fires at once, as zero, a negative or NaN does. */
const MAX_TIMEOUT_MS = 2 ** 31 - 1;

/** A timer that fired at once would answer the open gate before the command
 * could, turning the gate off without a word. So a duration that is not a
 * real one falls back to the default. */
function timeoutOf(ms: number | undefined): number {
  return ms !== undefined && Number.isFinite(ms) && ms > 0 && ms <= MAX_TIMEOUT_MS ? ms : DEFAULT_TIMEOUT_MS;
}

export type CheckOptions = {
  /** How long to wait for the command, in milliseconds, before answering
   * the open gate. Keep it above the plugin's Rust `timeout`. A value that is
   * not a positive, finite duration a timer can hold means the default. */
  timeoutMs?: number;
};

/** The browser's own language choice, when one is available. Node and other
 * non-browser hosts may have no `navigator`, or one without a `language`. */
function browserLanguage(): string | undefined {
  return typeof navigator === "undefined" ? undefined : navigator.language;
}

/**
 * Asks the Rust `check` command whether the installed copy is too old to
 * run, in `lang` — the app's own chosen language, since the message must
 * read in that language. Defaults to `navigator.language` (the OS language
 * in a Tauri webview) when the caller passes none; if that is unavailable
 * too, no `lang` is sent at all, and the command falls back through the OS
 * environment to `"en"`.
 *
 * Outside Tauri, whenever the command cannot be reached, and when it does
 * not answer within `options.timeoutMs` (default 10 s), this resolves to
 * the open gate — the gate fails open, and this promise never rejects.
 */
export async function checkUpdate(lang?: string, options: CheckOptions = {}): Promise<Gate> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    if (!isTauri()) return OPEN; // the web always serves the newest code
    const resolvedLang = lang ?? browserLanguage();
    const args = resolvedLang === undefined ? {} : { lang: resolvedLang };
    const timedOut = new Promise<Gate>((resolve) => {
      timer = setTimeout(() => resolve(OPEN), timeoutOf(options.timeoutMs));
    });
    return await Promise.race([invoke<Gate>("plugin:update-gate|check", args), timedOut]);
  } catch {
    return OPEN; // a gate that cannot be read is open
  } finally {
    clearTimeout(timer);
  }
}
