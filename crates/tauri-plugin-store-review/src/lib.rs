//! `tauri-plugin-store-review`: asks the phone to show its own rating prompt,
//! the App Store's on iOS and Google Play's on Android.
//!
//! The phone decides whether the prompt shows, and neither store tells the
//! app what the player chose. So `request_review` returns nothing. On a
//! desktop it does nothing.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_store_review);

#[cfg(mobile)]
struct Native<R: Runtime>(tauri::plugin::PluginHandle<R>);

/// Async, so the wait for the native side never holds the main thread, where
/// iOS has to show the prompt. The answer is read as any JSON value: the
/// native sides resolve with no data, which may arrive as `null` or `{}`.
#[cfg(mobile)]
#[tauri::command]
async fn request_review<R: Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    use tauri::Manager;
    let handle = app.state::<Native<R>>().0.clone();
    handle
        .run_mobile_plugin_async::<serde_json::Value>("requestReview", ())
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(not(mobile))]
#[tauri::command]
fn request_review() {}

/// Registers the plugin: the Swift side on iOS, the Kotlin side on Android,
/// and the command that does nothing everywhere else.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("store-review")
        .invoke_handler(tauri::generate_handler![request_review])
        .setup(|_app, _api| {
            #[cfg(mobile)]
            {
                use tauri::Manager;
                #[cfg(target_os = "android")]
                let handle =
                    _api.register_android_plugin("dev.hoangmirs.storereview", "StoreReviewPlugin")?;
                #[cfg(target_os = "ios")]
                let handle = _api.register_ios_plugin(init_plugin_store_review)?;
                _app.manage(Native(handle));
            }
            Ok(())
        })
        .build()
}
