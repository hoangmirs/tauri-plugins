//! `tauri-plugin-firebase`: reads Firebase Remote Config and logs Firebase
//! Analytics events and Crashlytics reports on iOS and Android.
//!
//! The Rust side keeps the defaults. On a desktop, and on a phone without its
//! Firebase file, every command answers from them, so the app never needs a
//! second path. No command ever returns an error: a Firebase failure reads as
//! `"failed"`, `false` or the defaults.

mod crash;
mod values;

use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Map, Value};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime, State};

pub use crash::{Crash, CrashExt, Frame};
pub use values::Entry;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_firebase);

/// One symbol from each Firebase object, written by `build.rs`. See `SKIP`
/// there for why.
#[cfg(target_os = "ios")]
mod anchors {
    include!(concat!(env!("OUT_DIR"), "/anchors.rs"));
}

/// How the plugin runs. Only `minimum_fetch_interval` can be set.
#[derive(Clone, Copy)]
pub struct Config {
    minimum_fetch_interval: Duration,
}

impl Default for Config {
    /// A fetch at most every 15 minutes.
    fn default() -> Self {
        Self {
            minimum_fetch_interval: Duration::from_secs(15 * 60),
        }
    }
}

impl Config {
    /// The shortest time between two fetches. A `fetch` inside it answers
    /// from Firebase's cache.
    #[must_use]
    pub fn minimum_fetch_interval(mut self, d: Duration) -> Self {
        self.minimum_fetch_interval = d;
        self
    }
}

struct Defaults(Mutex<Map<String, Value>>);

impl Defaults {
    fn get(&self) -> Map<String, Value> {
        self.0.lock().map(|d| d.clone()).unwrap_or_default()
    }
}

#[cfg(mobile)]
struct Native<R: Runtime> {
    handle: tauri::plugin::PluginHandle<R>,
    /// `"native"`, or `"absent"` when the app has no Firebase file.
    mode: String,
}

#[cfg(mobile)]
impl<R: Runtime> Native<R> {
    /// The native answer, or `None` in `"absent"` mode or on any error.
    async fn call(&self, method: &str, args: Value) -> Option<Value> {
        if self.mode != "native" {
            return None;
        }
        self.handle
            .run_mobile_plugin_async::<Value>(method, args)
            .await
            .ok()
    }
}

#[cfg(mobile)]
fn native<R: Runtime>(app: &tauri::AppHandle<R>) -> State<'_, Native<R>> {
    app.state::<Native<R>>()
}

#[tauri::command]
#[allow(clippy::unused_async)]
async fn mode<R: Runtime>(app: tauri::AppHandle<R>) -> String {
    #[cfg(mobile)]
    return native(&app).mode.clone();
    #[cfg(not(mobile))]
    {
        let _ = app;
        "desktop".to_string()
    }
}

#[tauri::command]
#[allow(clippy::unused_async)]
async fn set_defaults<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Defaults>,
    defaults: Map<String, Value>,
) -> Result<(), ()> {
    let texts = values::as_texts(&defaults);
    if let Ok(mut d) = state.0.lock() {
        *d = defaults;
    }
    #[cfg(mobile)]
    native(&app)
        .call("setDefaults", serde_json::json!({ "defaults": texts }))
        .await;
    #[cfg(not(mobile))]
    let _ = (app, texts);
    Ok(())
}

#[tauri::command]
#[allow(clippy::unused_async)]
async fn fetch<R: Runtime>(app: tauri::AppHandle<R>) -> String {
    #[cfg(mobile)]
    if let Some(v) = native(&app).call("fetch", Value::Null).await {
        if let Some(r @ ("fetched" | "throttled")) = v.get("result").and_then(Value::as_str) {
            return r.to_string();
        }
    }
    #[cfg(not(mobile))]
    let _ = app;
    "failed".to_string()
}

#[tauri::command]
#[allow(clippy::unused_async)]
async fn activate<R: Runtime>(app: tauri::AppHandle<R>) -> bool {
    #[cfg(mobile)]
    if let Some(v) = native(&app).call("activate", Value::Null).await {
        return v.get("changed").and_then(Value::as_bool).unwrap_or(false);
    }
    #[cfg(not(mobile))]
    let _ = app;
    false
}

/// Starts the native listener for Remote Config publishes. Each update the
/// SDK fetched reaches the webview as the `configUpdate` event. In `"absent"`
/// mode, and on a desktop, it does nothing.
#[tauri::command]
#[allow(clippy::unused_async)]
async fn listen<R: Runtime>(app: tauri::AppHandle<R>) {
    #[cfg(mobile)]
    native(&app).call("listen", Value::Null).await;
    #[cfg(not(mobile))]
    let _ = app;
}

#[tauri::command]
#[allow(clippy::unused_async)]
async fn unlisten<R: Runtime>(app: tauri::AppHandle<R>) {
    #[cfg(mobile)]
    native(&app).call("unlisten", Value::Null).await;
    #[cfg(not(mobile))]
    let _ = app;
}

#[tauri::command]
#[allow(clippy::unused_async)]
async fn get_all<R: Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, Defaults>,
) -> Result<Map<String, Value>, ()> {
    #[cfg(mobile)]
    if let Some(v) = native(&app).call("getAll", Value::Null).await {
        if let Some(Value::Object(all)) = v.get("values") {
            return Ok(all.clone());
        }
    }
    #[cfg(not(mobile))]
    let _ = app;
    Ok(values::defaults_view(&state.get()))
}

/// Sets the custom signals that Remote Config conditions read. A number sets
/// a signal and `null` unsets it. On a desktop, and in `"absent"` mode, it
/// does nothing.
#[tauri::command]
#[allow(clippy::unused_async)]
async fn set_custom_signals<R: Runtime>(app: tauri::AppHandle<R>, signals: Map<String, Value>) {
    #[cfg(mobile)]
    native(&app)
        .call(
            "setCustomSignals",
            serde_json::json!({ "signals": signals }),
        )
        .await;
    #[cfg(not(mobile))]
    let _ = (app, signals);
}

#[tauri::command]
#[allow(clippy::unused_async)]
async fn log_event<R: Runtime>(
    app: tauri::AppHandle<R>,
    name: String,
    params: Option<Map<String, Value>>,
) {
    #[cfg(mobile)]
    native(&app)
        .call(
            "logEvent",
            serde_json::json!({ "name": name, "params": params.unwrap_or_default() }),
        )
        .await;
    #[cfg(not(mobile))]
    let _ = (app, name, params);
}

#[tauri::command]
#[allow(clippy::unused_async)]
async fn set_collection<R: Runtime>(app: tauri::AppHandle<R>, enabled: bool) {
    #[cfg(mobile)]
    native(&app)
        .call("setCollection", serde_json::json!({ "enabled": enabled }))
        .await;
    #[cfg(not(mobile))]
    let _ = (app, enabled);
}

/// Registers the plugin: the Swift side on iOS, the Kotlin side on Android,
/// and the defaults everywhere.
#[must_use]
pub fn init<R: Runtime>(config: Config) -> TauriPlugin<R> {
    Builder::new("firebase")
        .invoke_handler(tauri::generate_handler![
            mode,
            set_defaults,
            fetch,
            activate,
            get_all,
            listen,
            unlisten,
            set_custom_signals,
            log_event,
            set_collection,
            crash::crash_set_collection,
            crash::crash_set_keys,
            crash::crash_log,
            crash::crash_record
        ])
        .setup(move |app, api| {
            app.manage(Defaults(Mutex::new(Map::new())));
            #[cfg(mobile)]
            {
                #[cfg(target_os = "android")]
                let handle =
                    api.register_android_plugin("dev.hoangmirs.firebase", "FirebasePlugin")?;
                #[cfg(target_os = "ios")]
                let handle = {
                    std::hint::black_box(anchors::ANCHORS.as_ptr());
                    api.register_ios_plugin(init_plugin_firebase)?
                };
                let mode = handle
                    .run_mobile_plugin::<Value>(
                        "configure",
                        serde_json::json!({
                            "minimumFetchIntervalSeconds": config.minimum_fetch_interval.as_secs()
                        }),
                    )
                    .ok()
                    .and_then(|v| v.get("mode").and_then(Value::as_str).map(str::to_string))
                    .filter(|m| m == "native")
                    .unwrap_or_else(|| "absent".to_string());
                app.manage(Native { handle, mode });
            }
            #[cfg(not(mobile))]
            let _ = (api, config.minimum_fetch_interval);
            Ok(())
        })
        .build()
}
