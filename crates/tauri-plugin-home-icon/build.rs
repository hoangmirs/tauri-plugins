const COMMANDS: &[&str] = &["supported", "current", "set"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).ios_path("ios").build();
}
