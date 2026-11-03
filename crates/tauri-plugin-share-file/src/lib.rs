//! `tauri-plugin-share-file`: opens Android's share sheet for one file, such
//! as a picture the app drew.
//!
//! Android's system web view has no `navigator.share`, so a page there cannot
//! hand a file to another app. The Kotlin side writes the file to the app's
//! cache and opens the system share sheet for it. It answers whether the
//! player picked an app or closed the sheet.
//!
//! One command:
//!
//! - `share { data, name, mime, title? }` → `{ outcome }`, where `outcome` is
//!   `"shared"`, `"cancelled"` or `"unsupported"`
//!
//! Everywhere but Android, `share` answers `"unsupported"`. iOS and desktop
//! web views have `navigator.share`.

use serde::{Deserialize, Serialize};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

/// The largest file, in bytes, after base64 decoding.
pub const DATA_MAX: usize = 20 * 1024 * 1024;
/// The longest file name, in bytes. Most Android file systems refuse longer.
pub const NAME_MAX: usize = 255;

/// What happened to the share sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    /// The player picked an app to share to.
    Shared,
    /// The player closed the sheet without a pick.
    Cancelled,
    /// This platform has no share sheet through this plugin.
    Unsupported,
}

/// What `share` answers, in the shape the Kotlin side answers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    pub outcome: Outcome,
}

/// A file name is 1 to `NAME_MAX` bytes, with no path separator and no `..`.
///
/// # Errors
/// Says why a name is refused.
pub fn check_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > NAME_MAX {
        return Err(format!("a file name is 1 to {NAME_MAX} bytes"));
    }
    if name.contains(['/', '\\', '\0']) || name.contains("..") {
        return Err("a file name has no path separator and no '..'".into());
    }
    Ok(())
}

/// The data is base64 with no `data:` prefix, and at most `DATA_MAX` bytes
/// once decoded.
///
/// # Errors
/// Says why the data is refused.
pub fn check_data(data: &str) -> Result<(), String> {
    if data.is_empty() {
        return Err("the file is empty".into());
    }
    if data.starts_with("data:") {
        return Err("the data is plain base64, with no 'data:' prefix".into());
    }
    if decoded_len(data) > DATA_MAX {
        return Err(format!("a file is at most {DATA_MAX} bytes"));
    }
    Ok(())
}

/// A MIME type has a type and a subtype, as in `image/png`.
///
/// # Errors
/// Says why the MIME type is refused.
pub fn check_mime(mime: &str) -> Result<(), String> {
    match mime.split_once('/') {
        Some((kind, sub)) if !kind.is_empty() && !sub.is_empty() => Ok(()),
        _ => Err("a MIME type is 'type/subtype', as in 'image/png'".into()),
    }
}

fn check(data: &str, name: &str, mime: &str) -> Result<(), String> {
    check_name(name)?;
    check_mime(mime)?;
    check_data(data)
}

fn decoded_len(data: &str) -> usize {
    let padding = data.bytes().rev().take_while(|&b| b == b'=').count();
    let len = data.len();
    (len / 4 * 3 + len % 4 * 3 / 4).saturating_sub(padding)
}

#[cfg(target_os = "android")]
struct Native<R: Runtime>(tauri::plugin::PluginHandle<R>);

#[cfg(target_os = "android")]
#[derive(Serialize)]
struct ShareArgs {
    data: String,
    name: String,
    mime: String,
    title: Option<String>,
}

/// Async, so the wait for the player never holds the main thread, where the
/// share sheet's answer arrives.
#[cfg(target_os = "android")]
#[tauri::command]
async fn share<R: Runtime>(
    app: tauri::AppHandle<R>,
    data: String,
    name: String,
    mime: String,
    title: Option<String>,
) -> Result<Answer, String> {
    use tauri::Manager;
    check(&data, &name, &mime)?;
    let handle = app.state::<Native<R>>().0.clone();
    handle
        .run_mobile_plugin_async::<Answer>(
            "share",
            ShareArgs {
                data,
                name,
                mime,
                title,
            },
        )
        .await
        .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "android"))]
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands command arguments over by value.
fn share(
    data: String,
    name: String,
    mime: String,
    title: Option<String>,
) -> Result<Answer, String> {
    let _ = title;
    check(&data, &name, &mime)?;
    Ok(Answer {
        outcome: Outcome::Unsupported,
    })
}

/// Registers the plugin: the Kotlin side on Android, and a command that
/// answers `"unsupported"` everywhere else.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("share-file")
        .invoke_handler(tauri::generate_handler![share])
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            {
                use tauri::Manager;
                let handle =
                    _api.register_android_plugin("dev.hoangmirs.sharefile", "ShareFilePlugin")?;
                _app.manage(Native(handle));
            }
            Ok(())
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_name_passes() {
        assert!(check_name("recess-story.png").is_ok());
        assert!(check_name("a.b.png").is_ok());
    }

    #[test]
    fn a_name_with_a_path_is_refused() {
        for name in [
            "", "a/b.png", "a\\b.png", "..", "../x.png", "x..png", "a\0.png",
        ] {
            assert!(check_name(name).is_err(), "{name:?} should be refused");
        }
        assert!(check_name(&"a".repeat(NAME_MAX + 1)).is_err());
        assert!(check_name(&"a".repeat(NAME_MAX)).is_ok());
    }

    #[test]
    fn empty_or_prefixed_data_is_refused() {
        assert!(check_data("").is_err());
        assert!(check_data("data:image/png;base64,AAAA").is_err());
        assert!(check_data("iVBORw0KGgo=").is_ok());
    }

    #[test]
    fn the_size_counts_decoded_bytes() {
        assert_eq!(decoded_len("AAAA"), 3);
        assert_eq!(decoded_len("AAA="), 2);
        assert_eq!(decoded_len("AA=="), 1);
        assert_eq!(decoded_len("AAA"), 2);
        assert_eq!(decoded_len("AA"), 1);
    }

    #[test]
    fn data_over_the_limit_is_refused() {
        let at_limit = "A".repeat(DATA_MAX / 3 * 4);
        assert_eq!(decoded_len(&at_limit), DATA_MAX - DATA_MAX % 3);
        assert!(check_data(&at_limit).is_ok());
        let over = "A".repeat(DATA_MAX / 3 * 4 + 8);
        assert!(check_data(&over).is_err());
    }

    #[test]
    fn a_mime_type_needs_both_halves() {
        assert!(check_mime("image/png").is_ok());
        for mime in ["", "image", "image/", "/png"] {
            assert!(check_mime(mime).is_err(), "{mime:?} should be refused");
        }
    }

    #[test]
    fn the_stub_answers_unsupported_or_refuses() {
        let answer = share("AAAA".into(), "a.png".into(), "image/png".into(), None);
        assert_eq!(
            answer,
            Ok(Answer {
                outcome: Outcome::Unsupported
            })
        );
        assert!(share("AAAA".into(), "../a.png".into(), "image/png".into(), None).is_err());
    }
}
