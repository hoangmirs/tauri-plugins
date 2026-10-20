const COMMANDS: &[&str] = &["config", "prefetched", "read", "get", "set", "install_id"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
