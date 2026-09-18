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

/** The browser's own language choice, when one is available. Node and other
 * non-browser hosts may have no `navigator`, or one without a `language`. */
function browserLanguage(): string | undefined {
  return typeof navigator === "undefined" ? undefined : navigator.language;
}

/**
 * Asks the Rust `check` command whether the installed copy is too old to
 * run, in `lang` — the app's own chosen language, since the message must
 * read in that language and phones have no usable locale env vars. Defaults
 * to `navigator.language` when the caller passes none; if neither is
 * available, no `lang` is sent at all, and the command falls back through
 * the OS environment to `"en"`.
 *
 * Outside Tauri, and whenever the command cannot be reached, this resolves
 * to the open gate — the gate fails open, never closed by an error.
 */
export async function checkUpdate(lang?: string): Promise<Gate> {
  if (!isTauri()) return OPEN; // the web always serves the newest code
  const resolvedLang = lang ?? browserLanguage();
  const args = resolvedLang === undefined ? {} : { lang: resolvedLang };
  try {
    return await invoke<Gate>("plugin:update-gate|check", args);
  } catch {
    return OPEN; // a gate that cannot be read is open
  }
}
