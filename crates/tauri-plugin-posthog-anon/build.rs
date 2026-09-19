const COMMANDS: &[&str] = &["capture", "flush", "set_opt_out", "is_opted_out"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
