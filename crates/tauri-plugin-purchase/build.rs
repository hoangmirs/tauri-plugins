const COMMANDS: &[&str] = &["products", "buy", "owned", "restore"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .ios_path("ios")
        .build();
}
