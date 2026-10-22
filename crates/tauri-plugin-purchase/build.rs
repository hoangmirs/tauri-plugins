/// `register_listener` and `remove_listener` are the core's own commands for a
/// mobile plugin's events. Listing them lets a webview hear `arrived`.
const COMMANDS: &[&str] = &[
    "register_listener",
    "remove_listener",
    "products",
    "buy",
    "owned",
    "restore",
    "finish",
    "unfinished",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .ios_path("ios")
        .build();
}
