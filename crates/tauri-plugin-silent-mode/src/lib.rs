//! `tauri-plugin-silent-mode`: tells an app whether the phone is set to be
//! quiet, so its sounds can keep to the choice the player made for the whole
//! phone.
//!
//! Only Android has a ringer mode an app can read. There the Kotlin side
//! answers `state` and sends `change` whenever the mode moves. On iOS the
//! silent switch already mutes an app's sound when it asks for ambient audio,
//! and a desktop has no such mode, so everywhere else `state` answers
//! [`Mode::Normal`] and `change` never fires.

use serde::Serialize;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

/// How the phone's ringer is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Sounds play.
    Normal,
    /// The phone buzzes instead of ringing.
    Vibrate,
    /// The phone neither rings nor buzzes.
    Silent,
}

/// What `state` answers, in the shape the Kotlin side answers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct State {
    pub mode: Mode,
}

/// Where no ringer mode can be read: always [`Mode::Normal`]. On Android the
/// Kotlin plugin answers this command instead, as it has no Rust handler there.
#[cfg(not(target_os = "android"))]
#[tauri::command]
fn state() -> State {
    State { mode: Mode::Normal }
}

/// Registers the plugin: the Kotlin side on Android, the `state` stub
/// everywhere else.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    let builder = Builder::new("silent-mode");
    #[cfg(not(target_os = "android"))]
    let builder = builder.invoke_handler(tauri::generate_handler![state]);
    builder
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            _api.register_android_plugin("dev.hoangmirs.silentmode", "SilentModePlugin")?;
            Ok(())
        })
        .build()
}
