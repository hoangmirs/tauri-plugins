//! `tauri-plugin-sign-in`: signs a player in with Google or Apple through the
//! phone's own sheet, and answers the provider's id token for a server to
//! check.
//!
//! Three commands:
//!
//! - `available` → `{ google, apple }`, which sheets this platform has. iOS
//!   has both, Android has Google, everything else has none
//! - `google { clientId, iosClientId?, nonce?, silent? }` → `{ idToken,
//!   nonce? }`. `clientId` is the web (server) client id, and the token's
//!   `aud`. iOS also needs `iosClientId`. `nonce` in the answer is the raw
//!   nonce the token carries
//! - `apple { nonce }` → `{ idToken, authorizationCode }`, iOS only. The
//!   plugin sends Apple the SHA-256 of `nonce`, so the token carries the hash
//!
//! Each rejects with `{ code, message }`, where `code` is `cancelled`,
//! `unavailable`, `no-account` or `failed`.

pub mod core;

use serde_json::Value;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{AppHandle, Runtime};

use crate::core::Platform;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_sign_in);

#[cfg(mobile)]
struct Native<R: Runtime>(tauri::plugin::PluginHandle<R>);

/// Async, so a sign-in sheet that stays open for minutes never holds the main
/// thread.
#[cfg(mobile)]
async fn native<R: Runtime>(
    app: &AppHandle<R>,
    method: &str,
    args: Value,
) -> Result<Value, core::Error> {
    use tauri::plugin::mobile::PluginInvokeError;
    use tauri::Manager;
    let handle = app.state::<Native<R>>().0.clone();
    handle
        .run_mobile_plugin_async::<Value>(method, args)
        .await
        .map_err(|e| match e {
            PluginInvokeError::InvokeRejected(r) => {
                core::rejected(r.code.as_deref(), r.message.as_deref())
            }
            other => core::Error::failed(other.to_string()),
        })
}

#[cfg(not(mobile))]
#[allow(clippy::unused_async)] // The same shape as the phones' version.
async fn native<R: Runtime>(
    _app: &AppHandle<R>,
    _method: &str,
    _args: Value,
) -> Result<Value, core::Error> {
    Err(core::Error::new(
        core::Code::Unavailable,
        "sign-in needs iOS or Android",
    ))
}

#[tauri::command]
fn available() -> core::Available {
    core::available(Platform::here())
}

#[tauri::command]
async fn google<R: Runtime>(
    app: AppHandle<R>,
    client_id: String,
    ios_client_id: Option<String>,
    nonce: Option<String>,
    silent: Option<bool>,
) -> Result<Value, core::Error> {
    let args = core::google_args(
        Platform::here(),
        &client_id,
        ios_client_id.as_deref(),
        nonce.as_deref(),
        silent.unwrap_or(false),
    )?;
    let answer = native(&app, "google", args).await?;
    core::google_answer(&answer, nonce.as_deref())
}

#[tauri::command]
async fn apple<R: Runtime>(app: AppHandle<R>, nonce: String) -> Result<Value, core::Error> {
    let args = core::apple_args(Platform::here(), &nonce)?;
    let answer = native(&app, "apple", args).await?;
    core::apple_answer(&answer)
}

/// Registers the plugin: the Swift side on iOS, the Kotlin side on Android,
/// and commands that reject with `unavailable` everywhere else.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("sign-in")
        .invoke_handler(tauri::generate_handler![available, google, apple])
        .setup(|_app, _api| {
            #[cfg(mobile)]
            {
                use tauri::Manager;
                #[cfg(target_os = "android")]
                let handle =
                    _api.register_android_plugin("dev.hoangmirs.signin", "SignInPlugin")?;
                #[cfg(target_os = "ios")]
                let handle = _api.register_ios_plugin(init_plugin_sign_in)?;
                _app.manage(Native(handle));
            }
            Ok(())
        })
        .build()
}
