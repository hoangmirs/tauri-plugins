//! `tauri-plugin-launch`: keeps the app's own launch frame over the web view
//! until the page says it has painted.
//!
//! iOS takes its launch screen down as soon as the app has a window, and
//! Android as soon as the activity draws. The web view then needs about a
//! second to start and load the page, and the player sees an empty window.
//! This plugin puts the same frame back over the web view when it loads, and
//! takes it away when the page calls `ready`, or after `timeoutMs`. A page that
//! paints the same frame in the same place then takes over with no visible
//! change.
//!
//! Settings, under `plugins.launch` in `tauri.conf.json`:
//!
//! - `image`: the frame's image, by name. An asset catalog image on iOS, a
//!   drawable on Android. Drawn at its own size, in the middle. Default
//!   `LaunchFrame`.
//! - `background`: the colour around the image, `#rrggbb`. Default white.
//! - `timeoutMs`: the longest the frame stays when the page never calls
//!   `ready`. Default 3000, at most 10000.
//!
//! One command, `ready`, takes the frame away. It does nothing on a desktop.

use serde::Deserialize;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

/// The longest the frame may stay, whatever the settings say. A page that
/// fails must never leave the app covered for long.
pub const LIMIT_MAX_MS: u64 = 10_000;

/// The plugin's settings. The native side reads the same JSON.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub image: String,
    pub background: String,
    pub timeout_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            image: "LaunchFrame".into(),
            background: "#ffffff".into(),
            timeout_ms: 3000,
        }
    }
}

impl Config {
    /// `timeout_ms`, held to `LIMIT_MAX_MS`.
    #[must_use]
    pub fn limit_ms(&self) -> u64 {
        self.timeout_ms.min(LIMIT_MAX_MS)
    }
}

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_launch);

#[cfg(mobile)]
struct Native<R: Runtime>(tauri::plugin::PluginHandle<R>);

/// Takes the frame away. Never fails: a frame that is already gone is fine.
#[cfg(mobile)]
#[tauri::command]
async fn ready<R: Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    use tauri::Manager;
    let handle = app.state::<Native<R>>().0.clone();
    let _ = handle
        .run_mobile_plugin_async::<serde_json::Value>("ready", ())
        .await;
    Ok(())
}

#[cfg(not(mobile))]
#[tauri::command]
fn ready() {}

/// Registers the plugin. On iOS and Android the frame goes up when the web
/// view loads.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R, Option<Config>> {
    Builder::<R, Option<Config>>::new("launch")
        .invoke_handler(tauri::generate_handler![ready])
        .setup(|_app, _api| {
            #[cfg(mobile)]
            {
                use tauri::Manager;
                #[cfg(target_os = "android")]
                let handle =
                    _api.register_android_plugin("dev.hoangmirs.launch", "LaunchPlugin")?;
                #[cfg(target_os = "ios")]
                let handle = _api.register_ios_plugin(init_plugin_launch)?;
                _app.manage(Native(handle));
            }
            Ok(())
        })
        .build()
}
