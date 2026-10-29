const COMMANDS: &[&str] = &["available", "google", "apple", "web"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .ios_path("ios")
        .build();
}
