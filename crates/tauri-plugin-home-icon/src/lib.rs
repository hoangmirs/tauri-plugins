//! `tauri-plugin-home-icon`: reads and changes the icon a phone shows for the
//! app on its home screen.
//!
//! The plugin speaks the system's own names. On iOS a name is an alternate
//! icon listed in the app's `CFBundleAlternateIcons`, and `None` is the
//! primary icon. The app keeps its own ids and maps them onto these names.
//!
//! Only iOS changes the icon for now. On Android a change means a new
//! `<activity-alias>` in the manifest, which reaches every player in the
//! update and can move the home-screen icon with no tap from the player. So
//! Android, like a desktop, answers "not supported" until it has its own
//! release.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_home_icon);

#[cfg(target_os = "ios")]
mod ios {
    use serde::{Deserialize, Serialize};
    use tauri::{AppHandle, Manager, Runtime};

    pub struct Native<R: Runtime>(pub tauri::plugin::PluginHandle<R>);

    #[derive(Deserialize)]
    struct Supported {
        supported: bool,
    }

    #[derive(Deserialize)]
    struct Current {
        #[serde(default)]
        name: Option<String>,
    }

    #[derive(Serialize)]
    struct SetArgs {
        name: Option<String>,
    }

    fn handle<R: Runtime>(app: &AppHandle<R>) -> tauri::plugin::PluginHandle<R> {
        app.state::<Native<R>>().0.clone()
    }

    /// Async, so the wait for the native side never holds the main thread,
    /// where UIKit reads and changes the icon.
    #[tauri::command]
    pub async fn supported<R: Runtime>(app: AppHandle<R>) -> Result<bool, String> {
        handle(&app)
            .run_mobile_plugin_async::<Supported>("supported", ())
            .await
            .map(|r| r.supported)
            .map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub async fn current<R: Runtime>(app: AppHandle<R>) -> Result<Option<String>, String> {
        handle(&app)
            .run_mobile_plugin_async::<Current>("current", ())
            .await
            .map(|r| r.name)
            .map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub async fn set<R: Runtime>(app: AppHandle<R>, name: Option<String>) -> Result<(), String> {
        handle(&app)
            .run_mobile_plugin_async::<serde_json::Value>("set", SetArgs { name })
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

#[cfg(not(target_os = "ios"))]
mod elsewhere {
    #[tauri::command]
    pub fn supported() -> bool {
        false
    }

    #[tauri::command]
    pub fn current() -> Option<String> {
        None
    }

    #[tauri::command]
    #[allow(clippy::needless_pass_by_value)] // a command takes its arguments by value
    pub fn set(name: Option<String>) -> Result<(), String> {
        let _ = name;
        Err("the home-screen icon is not supported here".into())
    }
}

#[cfg(not(target_os = "ios"))]
use elsewhere::{current, set, supported};
#[cfg(target_os = "ios")]
use ios::{current, set, supported};

/// Registers the plugin: the Swift side on iOS, and the answers "not
/// supported" everywhere else.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("home-icon")
        .invoke_handler(tauri::generate_handler![supported, current, set])
        .setup(|_app, _api| {
            #[cfg(target_os = "ios")]
            {
                use tauri::Manager;
                let handle = _api.register_ios_plugin(init_plugin_home_icon)?;
                _app.manage(ios::Native(handle));
            }
            Ok(())
        })
        .build()
}
