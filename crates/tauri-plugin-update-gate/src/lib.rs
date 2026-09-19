//! `tauri-plugin-update-gate`: reads a JSON document over HTTP and tells an
//! app whether the installed copy is too old to run.

mod decide;
mod source;

use tauri::plugin::{Builder, TauriPlugin};
use tauri::{AppHandle, Manager, Runtime, State as Managed};

use source::Source;

pub use decide::{Gate, State};
pub use source::Config;

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
    let lang = resolve_language(
        lang.as_deref(),
        env_language(|key| std::env::var(key).ok()).as_deref(),
    );
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

/// Reads the OS's language hint, unparsed, from the POSIX locale variables
/// through `var`, in the order POSIX gives them for messages: `LC_ALL`
/// overrides everything, `LANG` is only the default. An empty variable
/// counts as unset, as POSIX says. Desktop only: mobile targets typically
/// leave these unset.
fn env_language(var: impl Fn(&str) -> Option<String>) -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|key| var(key).filter(|v| !v.is_empty()))
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
    use super::{env_language, resolve_language};

    /// An environment holding only `vars`, so the lookup order can be tested
    /// without touching the real one.
    fn env<'a>(vars: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            vars.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_string())
        }
    }

    #[test]
    fn the_environment_is_read_in_posix_order() {
        let all = [
            ("LANG", "en_US"),
            ("LC_MESSAGES", "fr_FR"),
            ("LC_ALL", "vi_VN"),
        ];
        assert_eq!(env_language(env(&all)).as_deref(), Some("vi_VN"));
        let no_all = [("LANG", "en_US"), ("LC_MESSAGES", "fr_FR")];
        assert_eq!(env_language(env(&no_all)).as_deref(), Some("fr_FR"));
        assert_eq!(
            env_language(env(&[("LANG", "en_US")])).as_deref(),
            Some("en_US")
        );
        assert_eq!(env_language(env(&[])), None);
    }

    #[test]
    fn an_empty_variable_counts_as_unset() {
        let vars = [("LANG", "en_US"), ("LC_ALL", "")];
        assert_eq!(env_language(env(&vars)).as_deref(), Some("en_US"));
    }

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
