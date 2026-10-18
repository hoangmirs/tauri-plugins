//! The plugin builds on a desktop host and registers on an app.

#[test]
fn the_plugin_registers_on_a_mock_app() {
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_purchase::init())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("the plugin registers");
    drop(app);
}
