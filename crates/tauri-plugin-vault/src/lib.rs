//! `tauri-plugin-vault`: keeps a text value that an edit made outside the app
//! cannot change without failing a check. A key on the device signs each value,
//! and the key never leaves the device.
//!
//! On iOS the value is a Keychain item and the key is a Secure Enclave key.
//! Without an Enclave, such as in the Simulator, the key is a software key kept
//! as a Keychain item. On Android the value is a file in the app's data and the
//! key lives in the Keystore, in `StrongBox` or the TEE when the phone has one.
//! A key is made at the first `write`. A device that cannot make one stores the
//! value without a signature.
//!
//! Two commands:
//!
//! - `read { key }` → `{ value, state }`
//! - `write { key, value }` → `{ state }`
//!
//! `state` is one of:
//!
//! - `empty`: nothing is stored under `key`
//! - `verified`: the signature matches
//! - `restored`: a value is stored and its key is gone, as after a reinstall
//!   that brought the file back. The plugin signs the value again with a new
//!   key and returns it.
//! - `tampered`: a value is stored and the plugin cannot verify it. `value` is
//!   the stored value, or `null` when nothing can be read.
//! - `unsigned`: the device cannot make a key
//!
//! A stored value that cannot be verified is `tampered`, never an error. An
//! error means a Keychain or Keystore call failed.
//!
//! Everywhere but iOS and Android, both commands answer an error.

pub mod core;

use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_vault);

#[cfg(mobile)]
mod mobile {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use serde_json::{json, Value};
    use tauri::plugin::PluginHandle;
    use tauri::Runtime;

    use crate::core::{Level, Native, Stored};

    pub struct Mobile<R: Runtime>(pub PluginHandle<R>);

    impl<R: Runtime> Mobile<R> {
        /// Async, so a Keychain or Keystore call never holds the main thread.
        async fn ask(&self, method: &str, args: Value) -> Result<Value, String> {
            self.0
                .run_mobile_plugin_async::<Value>(method, args)
                .await
                .map_err(|e| e.to_string())
        }

        async fn level(&self, method: &str) -> Result<Option<Level>, String> {
            let answer = self.ask(method, json!({})).await?;
            match answer.get("level") {
                None | Some(Value::Null) => Ok(None),
                Some(level) => serde_json::from_value(level.clone())
                    .map(Some)
                    .map_err(|e| e.to_string()),
            }
        }
    }

    #[async_trait::async_trait]
    impl<R: Runtime> Native for Mobile<R> {
        async fn load(&self, key: &str) -> Result<Stored, String> {
            let answer = self.ask("load", json!({ "key": key })).await?;
            if answer.get("unreadable").and_then(Value::as_bool) == Some(true) {
                return Ok(Stored::Unreadable);
            }
            Ok(answer
                .get("text")
                .and_then(Value::as_str)
                .map_or(Stored::Missing, |t| Stored::Text(t.to_owned())))
        }

        async fn save(&self, key: &str, text: &str) -> Result<(), String> {
            self.ask("save", json!({ "key": key, "text": text }))
                .await
                .map(|_| ())
        }

        async fn key_level(&self) -> Result<Option<Level>, String> {
            self.level("keyLevel").await
        }

        async fn make_key(&self) -> Result<Option<Level>, String> {
            self.level("makeKey").await
        }

        async fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, String> {
            let answer = self
                .ask("sign", json!({ "msg": STANDARD.encode(msg) }))
                .await?;
            let sig = answer
                .get("sig")
                .and_then(Value::as_str)
                .ok_or("the signature is missing")?;
            STANDARD.decode(sig).map_err(|e| e.to_string())
        }

        async fn verify(&self, msg: &[u8], sig: &[u8]) -> Result<bool, String> {
            let answer = self
                .ask(
                    "verify",
                    json!({ "msg": STANDARD.encode(msg), "sig": STANDARD.encode(sig) }),
                )
                .await?;
            answer
                .get("ok")
                .and_then(Value::as_bool)
                .ok_or_else(|| "the answer to verify is missing".to_owned())
        }
    }
}

#[derive(serde::Serialize)]
struct Written {
    state: core::State,
}

#[cfg(mobile)]
#[tauri::command]
async fn read<R: Runtime>(app: tauri::AppHandle<R>, key: String) -> Result<core::Read, String> {
    use tauri::Manager;
    core::check_key(&key)?;
    core::read(app.state::<mobile::Mobile<R>>().inner(), &key).await
}

#[cfg(mobile)]
#[tauri::command]
async fn write<R: Runtime>(
    app: tauri::AppHandle<R>,
    key: String,
    value: String,
) -> Result<Written, String> {
    use tauri::Manager;
    core::check_key(&key)?;
    core::check_value(&value)?;
    let state = core::write(app.state::<mobile::Mobile<R>>().inner(), &key, &value).await?;
    Ok(Written { state })
}

#[cfg(not(mobile))]
const NO_VAULT: &str = "no vault on this platform";

#[cfg(not(mobile))]
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands command arguments over by value.
fn read(key: String) -> Result<core::Read, String> {
    core::check_key(&key)?;
    Err(NO_VAULT.into())
}

#[cfg(not(mobile))]
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands command arguments over by value.
fn write(key: String, value: String) -> Result<Written, String> {
    core::check_key(&key)?;
    core::check_value(&value)?;
    Err(NO_VAULT.into())
}

/// Registers the plugin: the Swift or Kotlin side on a phone, and commands that
/// answer an error everywhere else.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("vault")
        .invoke_handler(tauri::generate_handler![read, write])
        .setup(|_app, _api| {
            #[cfg(mobile)]
            {
                use tauri::Manager;
                #[cfg(target_os = "android")]
                let handle = _api.register_android_plugin("dev.hoangmirs.vault", "VaultPlugin")?;
                #[cfg(target_os = "ios")]
                let handle = _api.register_ios_plugin(init_plugin_vault)?;
                _app.manage(mobile::Mobile(handle));
            }
            Ok(())
        })
        .build()
}
