//! `tauri-plugin-sensors`: reads a phone's motion and compass heading, and
//! holds the screen's rotation while the app reads them.
//!
//! Two kinds of reading exist. `motion` carries gravity, rotation rate and
//! user acceleration together, from `CoreMotion`'s device motion on iOS and
//! from the gravity, gyroscope and linear acceleration sensors on Android.
//! `heading` carries degrees from magnetic north. Each reading reaches the
//! webview as the plugin event `reading`, in the device's natural frame.
//!
//! None of these sensors asks the player for a permission, and no reading
//! leaves the device. A desktop has no sensors: `available` answers no kinds
//! and the other commands do nothing.

use serde::{Deserialize, Serialize};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_sensors);

/// A kind of reading the device can send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Gravity, rotation rate and user acceleration, sent together.
    Motion,
    /// Degrees from magnetic north, 0 to 360.
    Heading,
}

/// What `available` answers: the kinds this device can send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Available {
    pub kinds: Vec<Kind>,
}

/// The rate a sensor runs at, from what the webview asked: 1 to 60 readings a
/// second, well under the 200 Hz above which Android 12 asks for a permission.
#[must_use]
pub fn clamp_hz(hz: f64) -> u32 {
    if hz.is_nan() {
        return 1;
    }
    // Within 1..=60 after the clamp, so the cast keeps every value.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let hz = hz.round().clamp(1.0, 60.0) as u32;
    hz
}

/// What a desktop answers: no kinds.
#[cfg(not(mobile))]
#[must_use]
pub fn available() -> Available {
    Available { kinds: Vec::new() }
}

#[cfg(mobile)]
mod native {
    use serde::Serialize;
    use tauri::{AppHandle, Manager, Runtime};

    use super::{clamp_hz, Available, Kind};

    pub struct Native<R: Runtime>(pub tauri::plugin::PluginHandle<R>);

    #[derive(Serialize)]
    struct StartArgs {
        kind: Kind,
        hz: u32,
    }

    #[derive(Serialize)]
    struct StopArgs {
        kind: Kind,
    }

    /// Async, so the wait for the native side never holds the main thread,
    /// where both platforms start the sensors and turn the screen.
    async fn run<R: Runtime, T: serde::de::DeserializeOwned>(
        app: &AppHandle<R>,
        method: &str,
        args: impl Serialize,
    ) -> Result<T, String> {
        let handle = app.state::<Native<R>>().0.clone();
        handle
            .run_mobile_plugin_async::<T>(method, args)
            .await
            .map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub async fn available<R: Runtime>(app: AppHandle<R>) -> Result<Available, String> {
        run(&app, "available", ()).await
    }

    #[tauri::command]
    pub async fn start<R: Runtime>(app: AppHandle<R>, kind: Kind, hz: f64) -> Result<(), String> {
        let hz = clamp_hz(hz);
        run::<R, serde_json::Value>(&app, "start", StartArgs { kind, hz })
            .await
            .map(|_| ())
    }

    #[tauri::command]
    pub async fn stop<R: Runtime>(app: AppHandle<R>, kind: Kind) -> Result<(), String> {
        run::<R, serde_json::Value>(&app, "stop", StopArgs { kind })
            .await
            .map(|_| ())
    }

    #[tauri::command]
    pub async fn lock_rotation<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
        run::<R, serde_json::Value>(&app, "lockRotation", ())
            .await
            .map(|_| ())
    }

    #[tauri::command]
    pub async fn unlock_rotation<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
        run::<R, serde_json::Value>(&app, "unlockRotation", ())
            .await
            .map(|_| ())
    }
}

#[cfg(not(mobile))]
mod elsewhere {
    use super::{Available, Kind};

    #[tauri::command]
    pub fn available() -> Available {
        super::available()
    }

    #[tauri::command]
    pub fn start(kind: Kind, hz: f64) {
        let _ = (kind, hz);
    }

    #[tauri::command]
    pub fn stop(kind: Kind) {
        let _ = kind;
    }

    #[tauri::command]
    pub fn lock_rotation() {}

    #[tauri::command]
    pub fn unlock_rotation() {}
}

#[cfg(not(mobile))]
use elsewhere as commands;
#[cfg(mobile)]
use native as commands;

/// Registers the plugin: the Swift side on iOS, the Kotlin side on Android,
/// and commands that read nothing everywhere else.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("sensors")
        .invoke_handler(tauri::generate_handler![
            commands::available,
            commands::start,
            commands::stop,
            commands::lock_rotation,
            commands::unlock_rotation,
        ])
        .setup(|app, api| {
            #[cfg(mobile)]
            {
                use tauri::Manager;
                #[cfg(target_os = "android")]
                let handle =
                    api.register_android_plugin("dev.hoangmirs.sensors", "SensorsPlugin")?;
                #[cfg(target_os = "ios")]
                let handle = api.register_ios_plugin(init_plugin_sensors)?;
                app.manage(native::Native(handle));
            }
            #[cfg(not(mobile))]
            let _ = (app, api);
            Ok(())
        })
        .build()
}
