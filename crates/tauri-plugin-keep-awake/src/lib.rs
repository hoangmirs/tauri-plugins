//! `tauri-plugin-keep-awake`: keeps a phone's screen on while the app asks it
//! to, and lets it go again.
//!
//! A web view's screen wake lock is missing on Android's system web view and
//! unreliable inside an iOS app, so the phones get the platform's own switch:
//! `isIdleTimerDisabled` on iOS, `FLAG_KEEP_SCREEN_ON` on Android. Both hold
//! only while the app is on screen, and the phone sleeps as usual once the app
//! goes to the background. On a desktop the command does nothing.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_keep_awake);

#[cfg(mobile)]
struct Native<R: Runtime>(tauri::plugin::PluginHandle<R>);

#[cfg(mobile)]
#[derive(serde::Serialize)]
struct SetArgs {
    on: bool,
}

/// Async, so the wait for the native side never holds the main thread, where
/// both platforms change the switch.
#[cfg(mobile)]
#[tauri::command]
async fn set<R: Runtime>(app: tauri::AppHandle<R>, on: bool) -> Result<(), String> {
    use tauri::Manager;
    let handle = app.state::<Native<R>>().0.clone();
    handle
        .run_mobile_plugin_async::<serde_json::Value>("set", SetArgs { on })
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(not(mobile))]
#[tauri::command]
fn set(on: bool) {
    let _ = on;
}

/// Registers the plugin: the Swift side on iOS, the Kotlin side on Android,
/// and the command that does nothing everywhere else.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("keep-awake")
        .invoke_handler(tauri::generate_handler![set])
        .setup(|_app, _api| {
            #[cfg(mobile)]
            {
                use tauri::Manager;
                #[cfg(target_os = "android")]
                let handle =
                    _api.register_android_plugin("dev.hoangmirs.keepawake", "KeepAwakePlugin")?;
                #[cfg(target_os = "ios")]
                let handle = _api.register_ios_plugin(init_plugin_keep_awake)?;
                _app.manage(Native(handle));
            }
            Ok(())
        })
        .build()
}
