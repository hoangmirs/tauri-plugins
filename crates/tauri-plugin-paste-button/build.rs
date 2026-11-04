/// `register_listener` and `remove_listener` are the core's own commands for a
/// mobile plugin's events. Listing them lets a webview hear `paste`.
const COMMANDS: &[&str] = &[
    "register_listener",
    "remove_listener",
    "supported",
    "place",
    "hide",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).ios_path("ios").build();
}
