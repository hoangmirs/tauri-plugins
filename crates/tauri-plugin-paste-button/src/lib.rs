//! `tauri-plugin-paste-button`: lays the system paste button over a box the
//! page draws, and sends the page what it pasted.
//!
//! On iOS 16 and later the button is a `UIPasteControl`. It pastes in one tap,
//! with no "Allow Paste?" alert and no second "Paste" bubble from the web view. The
//! page sends the box's place and the button's colours with `place`, and hears
//! the text in the `paste` event.
//!
//! Everywhere else `supported` answers `{ supported: false }`, and `place` and
//! `hide` do nothing.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_paste_button);

#[cfg(not(target_os = "ios"))]
mod elsewhere {
    use serde::Serialize;

    #[derive(Serialize)]
    pub struct Supported {
        supported: bool,
    }

    #[tauri::command]
    pub fn supported() -> Supported {
        Supported { supported: false }
    }

    /// Takes the page's box and ignores it: there is no button to place.
    #[tauri::command]
    pub fn place() {}

    #[tauri::command]
    pub fn hide() {}
}

/// Registers the plugin. On iOS the Rust side has no commands of its own: Tauri
/// hands each command, with the page's arguments as they are, to the Swift
/// side.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    let builder = Builder::new("paste-button");
    #[cfg(not(target_os = "ios"))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        elsewhere::supported,
        elsewhere::place,
        elsewhere::hide
    ]);
    builder
        .setup(|_app, _api| {
            #[cfg(target_os = "ios")]
            _api.register_ios_plugin(init_plugin_paste_button)?;
            Ok(())
        })
        .build()
}
