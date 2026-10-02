// `register_listener` and `remove_listener` are the core's own commands for a
// mobile plugin's events; listing them is what lets a webview hear `change`.
const COMMANDS: &[&str] = &["state", "register_listener", "remove_listener"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
