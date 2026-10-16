const COMMANDS: &[&str] = &[
    "mode",
    "set_defaults",
    "fetch",
    "activate",
    "get_all",
    "log_event",
    "set_collection",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .ios_path("ios")
        .build();
}
