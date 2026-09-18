//! `tauri-plugin-update-gate`: reads a JSON document over HTTP and tells an
//! app whether the installed copy is too old to run.

pub mod decide;
pub mod source;

use tauri::plugin::{Builder, TauriPlugin};
use tauri::{AppHandle, Manager, Runtime, State as Managed};

use decide::Gate;
use source::{Config, Source};

/// This build's OS, in the vocabulary `decide::Document::url` keys are
/// written in. A target this list doesn't know about still compiles; the
/// document simply won't carry a URL for it.
const PLATFORM: &str = if cfg!(target_os = "ios") {
    "ios"
} else if cfg!(target_os = "android") {
    "android"
} else if cfg!(target_os = "macos") {
    "macos"
} else if cfg!(target_os = "windows") {
    "windows"
} else if cfg!(target_os = "linux") {
    "linux"
} else {
    "unknown"
};

/// Registers the `check` command, backed by a `Source` built from `config`
/// and held as managed state so the cache in `Source::gate` survives across
/// calls.
#[must_use]
pub fn init<R: Runtime>(config: Config) -> TauriPlugin<R> {
    Builder::new("update-gate")
        .invoke_handler(tauri::generate_handler![check])
        .setup(|app, _api| {
            app.manage(Source::new(config));
            Ok(())
        })
        .build()
}

/// Asks the managed `Source` whether the running copy may still play, in
/// `lang` — the app's own language choice, e.g. one the player picked in
/// its own Settings, independent of the OS — falling back through
/// `resolve_language` to the OS environment and then `"en"` when `lang` is
/// absent or unusable. The version comes from Tauri's own `PackageInfo`,
/// never from the caller, so a webview can't lie about which build it is.
///
/// Returns `Result` only because Tauri requires it of an async command that
/// borrows `State`; the gate itself fails open, so this never returns `Err`.
#[tauri::command]
async fn check<R: Runtime>(
    app: AppHandle<R>,
    source: Managed<'_, Source>,
    lang: Option<String>,
) -> Result<Gate, String> {
    let running = app.package_info().version.to_string();
    let lang = resolve_language(lang.as_deref(), env_language().as_deref());
    Ok(source.gate(&running, &lang, PLATFORM).await)
}

/// Resolves the language to check the gate against: the caller's own
/// choice first, then the OS environment, then `"en"` if neither yields a
/// usable two-letter code. Kept pure and separate from where each side
/// comes from so the fallback order is unit-testable without touching the
/// environment.
fn resolve_language(caller: Option<&str>, env: Option<&str>) -> String {
    caller
        .and_then(parse_language)
        .or_else(|| env.and_then(parse_language))
        .unwrap_or_else(|| "en".to_string())
}

/// Reads the OS's language hint, unparsed, from the usual POSIX locale
/// environment variables. Desktop only: mobile targets typically leave
/// these unset, so `resolve_language`'s `"en"` fallback is what actually
/// runs there whenever the caller doesn't supply its own language.
fn env_language() -> Option<String> {
    std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .ok()
}

/// Pulls a lowercase two-letter language code out of a locale-ish string —
/// a bare code like `"vi"`, or a POSIX locale like `"vi_VN.UTF-8"` or
/// `"en-US"` — by taking the substring before the first `_`, `.`, or `-`.
/// Returns `None` for anything that doesn't reduce to exactly two ASCII
/// letters, including `""`, `"C"`, and `"POSIX"`, none of which name a
/// language.
fn parse_language(raw: &str) -> Option<String> {
    raw.split(['_', '.', '-'])
        .next()
        .map(str::to_lowercase)
        .filter(|code| code.len() == 2 && code.bytes().all(|b| b.is_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::resolve_language;

    #[test]
    fn the_callers_language_wins_over_the_environment() {
        assert_eq!(resolve_language(Some("vi"), Some("en_US.UTF-8")), "vi");
    }

    #[test]
    fn the_callers_language_is_parsed_the_same_way_as_the_environment() {
        assert_eq!(resolve_language(Some("vi-VN"), None), "vi");
    }

    #[test]
    fn an_absent_or_unusable_caller_falls_back_to_the_environment() {
        assert_eq!(resolve_language(None, Some("fr.UTF-8")), "fr");
        assert_eq!(resolve_language(Some(""), Some("fr.UTF-8")), "fr");
        assert_eq!(resolve_language(Some("123"), Some("fr.UTF-8")), "fr");
    }

    #[test]
    fn neither_side_usable_falls_back_to_english() {
        assert_eq!(resolve_language(None, None), "en");
        assert_eq!(resolve_language(Some("C"), Some("POSIX")), "en");
    }
}
