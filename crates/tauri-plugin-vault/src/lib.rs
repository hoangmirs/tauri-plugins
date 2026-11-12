//! `tauri-plugin-vault`: keeps a text value that an edit made outside the app
//! cannot change without failing a check. A key on the device signs each value,
//! and the key never leaves the device.
//!
//! On iOS and macOS the value is a Keychain item and the key is a Secure
//! Enclave key. Without an Enclave, such as in the Simulator or on an Intel Mac
//! without a T2 chip, the key is a software key kept as a Keychain item. The
//! Mac uses the data protection keychain, the iPhone's kind, so only a Mac app
//! signed with an App ID entitlement can use the vault: a Mac App Store build
//! has one. On Android the value is a file in the app's data and the
//! key lives in the Keystore, in `StrongBox` or the TEE when the phone has one.
//! A key is made at the first `write`, or at a `read` that finds a value
//! whose key is gone. A device that cannot make one stores the value without a
//! signature.
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
//! On Windows and Linux both commands answer an error. On a Mac without the
//! entitlement, such as a dev build, the Keychain refuses (-34018) and both
//! commands answer an error.

pub mod core;

use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_vault);

#[cfg(target_os = "macos")]
mod macos;

/// The platform's side as JSON calls, one method and its arguments each: the
/// Swift or Kotlin plugin on a phone, the Swift bridge on the Mac.
#[cfg(any(mobile, target_os = "macos"))]
mod bridge {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use serde_json::{json, Value};

    use crate::core::{Level, Native, Stored};

    #[async_trait::async_trait]
    pub trait Ask: Send + Sync {
        async fn ask(&self, method: &str, args: Value) -> Result<Value, String>;
    }

    pub struct Bridge<A>(pub A);

    impl<A: Ask> Bridge<A> {
        async fn level(&self, method: &str) -> Result<Option<Level>, String> {
            let answer = self.0.ask(method, json!({})).await?;
            match answer.get("level") {
                None | Some(Value::Null) => Ok(None),
                Some(level) => serde_json::from_value(level.clone())
                    .map(Some)
                    .map_err(|e| e.to_string()),
            }
        }
    }

    #[async_trait::async_trait]
    impl<A: Ask> Native for Bridge<A> {
        async fn load(&self, key: &str) -> Result<Stored, String> {
            let answer = self.0.ask("load", json!({ "key": key })).await?;
            let answer = answer
                .as_object()
                .ok_or("the answer to load is not an object")?;
            match answer.get("unreadable") {
                None | Some(Value::Null | Value::Bool(false)) => {}
                Some(Value::Bool(true)) => return Ok(Stored::Unreadable),
                Some(_) => return Err("the answer to load has an odd unreadable".to_owned()),
            }
            match answer.get("text") {
                None | Some(Value::Null) => Ok(Stored::Missing),
                Some(Value::String(text)) => Ok(Stored::Text(text.clone())),
                Some(_) => Err("the answer to load has a text that is not a string".to_owned()),
            }
        }

        async fn save(&self, key: &str, text: &str) -> Result<(), String> {
            self.0
                .ask("save", json!({ "key": key, "text": text }))
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
                .0
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
                .0
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

#[cfg(mobile)]
mod mobile {
    use serde_json::Value;
    use tauri::plugin::PluginHandle;
    use tauri::Runtime;

    pub struct Mobile<R: Runtime>(pub PluginHandle<R>);

    /// Async, so a Keychain or Keystore call never holds the main thread.
    #[async_trait::async_trait]
    impl<R: Runtime> crate::bridge::Ask for Mobile<R> {
        async fn ask(&self, method: &str, args: Value) -> Result<Value, String> {
            self.0
                .run_mobile_plugin_async::<Value>(method, args)
                .await
                .map_err(|e| e.to_string())
        }
    }
}

#[derive(serde::Serialize)]
struct Written {
    state: core::State,
}

#[cfg(mobile)]
type Platform<R> = bridge::Bridge<mobile::Mobile<R>>;
#[cfg(target_os = "macos")]
type Platform<R> = bridge::Bridge<macos::Swift<R>>;

#[cfg(any(mobile, target_os = "macos"))]
#[tauri::command]
async fn read<R: Runtime>(app: tauri::AppHandle<R>, key: String) -> Result<core::Read, String> {
    use tauri::Manager;
    core::check_key(&key)?;
    core::read(app.state::<Platform<R>>().inner(), &key).await
}

#[cfg(any(mobile, target_os = "macos"))]
#[tauri::command]
async fn write<R: Runtime>(
    app: tauri::AppHandle<R>,
    key: String,
    value: String,
) -> Result<Written, String> {
    use tauri::Manager;
    core::check_key(&key)?;
    core::check_value(&value)?;
    let state = core::write(app.state::<Platform<R>>().inner(), &key, &value).await?;
    Ok(Written { state })
}

#[cfg(not(any(mobile, target_os = "macos")))]
const NO_VAULT: &str = "no vault on this platform";

#[cfg(not(any(mobile, target_os = "macos")))]
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands command arguments over by value.
fn read(key: String) -> Result<core::Read, String> {
    core::check_key(&key)?;
    Err(NO_VAULT.into())
}

#[cfg(not(any(mobile, target_os = "macos")))]
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri hands command arguments over by value.
fn write(key: String, value: String) -> Result<Written, String> {
    core::check_key(&key)?;
    core::check_value(&value)?;
    Err(NO_VAULT.into())
}

/// Registers the plugin: the Swift or Kotlin side on a phone, the Swift bridge
/// on the Mac, and commands that answer an error everywhere else.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("vault")
        .invoke_handler(tauri::generate_handler![read, write])
        .setup(|app, api| {
            #[cfg(mobile)]
            {
                use tauri::Manager;
                #[cfg(target_os = "android")]
                let handle = api.register_android_plugin("dev.hoangmirs.vault", "VaultPlugin")?;
                #[cfg(target_os = "ios")]
                let handle = api.register_ios_plugin(init_plugin_vault)?;
                app.manage(bridge::Bridge(mobile::Mobile(handle)));
            }
            #[cfg(target_os = "macos")]
            {
                use tauri::Manager;
                app.manage(bridge::Bridge(macos::Swift::<R>::new()));
            }
            #[cfg(not(any(mobile, target_os = "macos")))]
            let _ = (app, api);
            #[cfg(target_os = "macos")]
            let _ = api;
            Ok(())
        })
        .build()
}
