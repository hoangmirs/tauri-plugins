//! Crash reports. Four commands for the web view, and a Rust API for the
//! app's panic hook. On a desktop, and on a phone without its Firebase file,
//! each call does nothing. No call returns an error or panics.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
#[cfg(mobile)]
use serde_json::{json, Value};
#[cfg(mobile)]
use tauri::Manager;
use tauri::{AppHandle, Runtime};

/// One line of a stack trace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub symbol: String,
    pub file: String,
    pub line: u32,
}

/// The crash API of one app. Every method is synchronous, so a panic hook can
/// call it on any thread.
pub struct Crash<R: Runtime> {
    #[cfg_attr(not(mobile), allow(dead_code))]
    app: AppHandle<R>,
}

pub trait CrashExt<R: Runtime> {
    fn crash(&self) -> Crash<R>;
}

impl<R: Runtime> CrashExt<R> for AppHandle<R> {
    fn crash(&self) -> Crash<R> {
        Crash { app: self.clone() }
    }
}

impl<R: Runtime> Crash<R> {
    /// Adds a line to the log that goes with the next crash report.
    pub fn log(&self, line: &str) {
        #[cfg(mobile)]
        self.call("crashLog", json!({ "line": line }));
        #[cfg(not(mobile))]
        let _ = line;
    }

    /// Reports an error that did not stop the app.
    pub fn record(&self, name: &str, message: &str, frames: &[Frame]) {
        #[cfg(mobile)]
        self.call(
            "crashRecord",
            json!({ "name": name, "message": message, "frames": frames }),
        );
        #[cfg(not(mobile))]
        let _ = (name, message, frames);
    }

    /// Blocks until the native side answers. It uses `try_state`, because
    /// `state` panics before the plugin has set up.
    #[cfg(mobile)]
    fn call(&self, method: &str, args: Value) {
        let Some(native) = self.app.try_state::<crate::Native<R>>() else {
            return;
        };
        if native.mode == "native" {
            let _ = native.handle.run_mobile_plugin::<Value>(method, args);
        }
    }
}

#[tauri::command]
#[allow(clippy::unused_async)]
pub(crate) async fn crash_set_collection<R: Runtime>(app: AppHandle<R>, enabled: bool) {
    #[cfg(mobile)]
    crate::native(&app)
        .call("crashSetCollection", json!({ "enabled": enabled }))
        .await;
    #[cfg(not(mobile))]
    let _ = (app, enabled);
}

#[tauri::command]
#[allow(clippy::unused_async)]
pub(crate) async fn crash_set_keys<R: Runtime>(app: AppHandle<R>, keys: BTreeMap<String, String>) {
    #[cfg(mobile)]
    crate::native(&app)
        .call("crashSetKeys", json!({ "keys": keys }))
        .await;
    #[cfg(not(mobile))]
    let _ = (app, keys);
}

#[tauri::command]
#[allow(clippy::unused_async)]
pub(crate) async fn crash_log<R: Runtime>(app: AppHandle<R>, line: String) {
    #[cfg(mobile)]
    crate::native(&app)
        .call("crashLog", json!({ "line": line }))
        .await;
    #[cfg(not(mobile))]
    let _ = (app, line);
}

#[tauri::command]
#[allow(clippy::unused_async)]
pub(crate) async fn crash_record<R: Runtime>(
    app: AppHandle<R>,
    name: String,
    message: String,
    frames: Vec<Frame>,
) {
    #[cfg(mobile)]
    crate::native(&app)
        .call(
            "crashRecord",
            json!({ "name": name, "message": message, "frames": frames }),
        )
        .await;
    #[cfg(not(mobile))]
    let _ = (app, name, message, frames);
}
