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

/// Asks the managed `Source` whether the running copy may still play. The
/// version comes from Tauri's own `PackageInfo`, never from the caller, so a
/// webview can't lie about which build it is.
///
/// Returns `Result` only because Tauri requires it of an async command that
/// borrows `State`; the gate itself fails open, so this never returns `Err`.
#[tauri::command]
async fn check<R: Runtime>(app: AppHandle<R>, source: Managed<'_, Source>) -> Result<Gate, String> {
    let running = app.package_info().version.to_string();
    Ok(source.gate(&running, &language(), PLATFORM).await)
}

/// Reads the two-letter language code the OS reports, lowercased. Tauri
/// 2.11 has no direct locale API on desktop, so this falls back to the
/// POSIX locale environment variables; on mobile those are typically unset,
/// so `language_from`'s `"en"` fallback is what actually runs there.
fn language() -> String {
    let raw = std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .ok();
    language_from(raw.as_deref())
}

/// Pulls a lowercase two-letter language code out of a POSIX locale string
/// such as `"vi_VN.UTF-8"` or `"en-US"`. Anything that doesn't yield exactly
/// two letters — including `None`, the empty string, and the `"C"`/`"POSIX"`
/// locale, which names no language at all — falls back to `"en"`, matching
/// `Document::message`'s own English fallback.
fn language_from(raw: Option<&str>) -> String {
    raw.and_then(|s| s.split(['_', '.', '-']).next())
        .map(str::to_lowercase)
        .filter(|code| code.len() == 2 && code.bytes().all(|b| b.is_ascii_lowercase()))
        .unwrap_or_else(|| "en".to_string())
}

#[cfg(test)]
mod tests {
    use super::language_from;

    #[test]
    fn reads_the_language_before_region_or_encoding() {
        assert_eq!(language_from(Some("vi_VN.UTF-8")), "vi");
        assert_eq!(language_from(Some("en-US")), "en");
        assert_eq!(language_from(Some("FR.UTF-8")), "fr");
    }

    #[test]
    fn falls_back_to_english_when_nothing_usable() {
        assert_eq!(language_from(None), "en");
        assert_eq!(language_from(Some("")), "en");
        assert_eq!(language_from(Some("C")), "en");
        assert_eq!(language_from(Some("POSIX")), "en");
    }
}
