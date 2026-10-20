//! The plugin's settings, as an app writes them under `plugins.launch`.

use tauri_plugin_launch::Config;

#[test]
fn an_app_with_no_settings_gets_the_defaults() {
    let c: Config = serde_json::from_str("{}").unwrap();
    assert_eq!(c.image, "LaunchFrame");
    assert_eq!(c.background, "#ffffff");
    assert_eq!(c.timeout_ms, 3000);
}

#[test]
fn an_app_names_its_own_frame_colour_and_limit() {
    let c: Config = serde_json::from_str(
        r##"{"image":"launch_frame","background":"#fff3e2","timeoutMs":2500}"##,
    )
    .unwrap();
    assert_eq!(c.image, "launch_frame");
    assert_eq!(c.background, "#fff3e2");
    assert_eq!(c.timeout_ms, 2500);
}

#[test]
fn a_limit_past_ten_seconds_is_held_to_ten() {
    let c: Config = serde_json::from_str(r#"{"timeoutMs":60000}"#).unwrap();
    assert_eq!(c.limit_ms(), 10_000);
}

#[test]
fn the_plugin_registers_on_a_mock_app() {
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_launch::init())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("the plugin registers");
    drop(app);
}
