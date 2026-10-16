//! `tauri-plugin-firebase`: reads Firebase Remote Config and logs Firebase
//! Analytics events on iOS and Android.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_firebase);

#[cfg(mobile)]
struct Native<R: Runtime>(tauri::plugin::PluginHandle<R>);

#[cfg(mobile)]
#[tauri::command]
async fn mode<R: Runtime>(app: tauri::AppHandle<R>) -> String {
    use tauri::Manager;
    let handle = app.state::<Native<R>>().0.clone();
    handle
        .run_mobile_plugin_async::<serde_json::Value>("mode", ())
        .await
        .ok()
        .and_then(|v| v.get("mode").and_then(|m| m.as_str()).map(str::to_string))
        .unwrap_or_else(|| "absent".to_string())
}

#[cfg(not(mobile))]
#[tauri::command]
fn mode() -> String {
    "desktop".to_string()
}

#[derive(Default)]
pub struct Config;

#[must_use]
pub fn init<R: Runtime>(_config: Config) -> TauriPlugin<R> {
    Builder::new("firebase")
        .invoke_handler(tauri::generate_handler![mode])
        .setup(|_app, _api| {
            #[cfg(mobile)]
            {
                use tauri::Manager;
                #[cfg(target_os = "android")]
                let handle =
                    _api.register_android_plugin("dev.hoangmirs.firebase", "FirebasePlugin")?;
                #[cfg(target_os = "ios")]
                let handle = _api.register_ios_plugin(init_plugin_firebase)?;
                _app.manage(Native(handle));
            }
            Ok(())
        })
        .build()
}
