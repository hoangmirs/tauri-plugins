//! `tauri-plugin-keep`: keeps a few small text values on an iPhone or iPad
//! after the app is deleted, so a reinstall on the same device reads them back.
//!
//! On iOS each value is a Keychain item, saved with
//! `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` and never synchronized:
//! it stays on this device, out of iCloud and out of a backup moved to another
//! phone. iOS keeps an app's Keychain items when the app is deleted. Apple does
//! not document that promise, so treat a missing value as a fresh install.
//! Erasing the device removes them.
//!
//! Three commands:
//!
//! - `get { key }` → the value, or `null` when nothing is kept under `key`
//! - `set { key, value }` → keeps `value` under `key`, in place of any older one
//! - `remove { key }` → forgets `key`; forgetting a key that is not there is fine
//!
//! Everywhere but iOS, `get` answers `null` and the others do nothing. Android
//! has its own way back after a reinstall: Auto Backup restores the app's data.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

/// The longest key, in bytes.
pub const KEY_MAX: usize = 64;
/// The largest value, in bytes. The Keychain is for small items; a larger
/// value belongs in a file or a cloud store.
pub const VALUE_MAX: usize = 64 * 1024;

/// A key is 1 to `KEY_MAX` letters, digits, `.`, `-` or `_`.
///
/// # Errors
/// Says why a key is refused.
pub fn check_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.len() > KEY_MAX {
        return Err(format!("a key is 1 to {KEY_MAX} characters"));
    }
    if !key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err("a key is letters, digits, '.', '-' or '_'".into());
    }
    Ok(())
}

/// A value is at most `VALUE_MAX` bytes of text.
///
/// # Errors
/// Says why a value is refused.
pub fn check_value(value: &str) -> Result<(), String> {
    if value.len() > VALUE_MAX {
        return Err(format!("a value is at most {VALUE_MAX} bytes"));
    }
    Ok(())
}

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_keep);

#[cfg(target_os = "ios")]
struct Native<R: Runtime>(tauri::plugin::PluginHandle<R>);

#[cfg(target_os = "ios")]
async fn ask<R: Runtime>(
    app: &tauri::AppHandle<R>,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    let handle = app.state::<Native<R>>().0.clone();
    handle
        .run_mobile_plugin_async::<serde_json::Value>(method, args)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "ios")]
#[tauri::command]
async fn get<R: Runtime>(app: tauri::AppHandle<R>, key: String) -> Result<Option<String>, String> {
    check_key(&key)?;
    let answer = ask(&app, "get", serde_json::json!({ "key": key })).await?;
    Ok(answer
        .get("value")
        .and_then(|v| v.as_str())
        .map(str::to_owned))
}

#[cfg(target_os = "ios")]
#[tauri::command]
async fn set<R: Runtime>(
    app: tauri::AppHandle<R>,
    key: String,
    value: String,
) -> Result<(), String> {
    check_key(&key)?;
    check_value(&value)?;
    ask(
        &app,
        "set",
        serde_json::json!({ "key": key, "value": value }),
    )
    .await
    .map(|_| ())
}

#[cfg(target_os = "ios")]
#[tauri::command]
async fn remove<R: Runtime>(app: tauri::AppHandle<R>, key: String) -> Result<(), String> {
    check_key(&key)?;
    ask(&app, "remove", serde_json::json!({ "key": key }))
        .await
        .map(|_| ())
}

#[cfg(not(target_os = "ios"))]
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands command arguments over by value.
fn get(key: String) -> Result<Option<String>, String> {
    check_key(&key)?;
    Ok(None)
}

#[cfg(not(target_os = "ios"))]
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands command arguments over by value.
fn set(key: String, value: String) -> Result<(), String> {
    check_key(&key)?;
    check_value(&value)
}

#[cfg(not(target_os = "ios"))]
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands command arguments over by value.
fn remove(key: String) -> Result<(), String> {
    check_key(&key)
}

/// Registers the plugin: the Swift side on iOS, and commands that keep
/// nothing everywhere else.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("keep")
        .invoke_handler(tauri::generate_handler![get, set, remove])
        .setup(|_app, _api| {
            #[cfg(target_os = "ios")]
            {
                use tauri::Manager;
                let handle = _api.register_ios_plugin(init_plugin_keep)?;
                _app.manage(Native(handle));
            }
            Ok(())
        })
        .build()
}
