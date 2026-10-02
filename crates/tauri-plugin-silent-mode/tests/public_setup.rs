//! The plugin builds on a desktop host and answers the shape a webview
//! reads: `{ "mode": "normal" }`.

use tauri_plugin_silent_mode::{Mode, State};

#[test]
fn state_serialises_as_the_webview_reads_it() {
    let json = serde_json::to_value(State { mode: Mode::Normal }).unwrap();
    assert_eq!(json, serde_json::json!({ "mode": "normal" }));
    assert_eq!(serde_json::to_value(Mode::Vibrate).unwrap(), "vibrate");
    assert_eq!(serde_json::to_value(Mode::Silent).unwrap(), "silent");
}

#[test]
fn the_plugin_registers_on_a_mock_app() {
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_silent_mode::init())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("the plugin registers");
    drop(app);
}
