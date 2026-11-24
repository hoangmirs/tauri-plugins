// `register_listener` and `remove_listener` are the core's own commands for a
// mobile plugin's events; listing them is what lets a webview hear `reading`.
const COMMANDS: &[&str] = &[
    "available",
    "start",
    "stop",
    "lock_rotation",
    "unlock_rotation",
    "register_listener",
    "remove_listener",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .ios_path("ios")
        .build();
}
