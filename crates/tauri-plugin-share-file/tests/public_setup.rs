//! The plugin builds on a desktop host and answers the shape a webview
//! reads: `{ "outcome": "unsupported" }`.

use tauri_plugin_share_file::{Answer, Outcome};

#[test]
fn the_answer_serialises_as_the_webview_reads_it() {
    let json = serde_json::to_value(Answer {
        outcome: Outcome::Unsupported,
    })
    .unwrap();
    assert_eq!(json, serde_json::json!({ "outcome": "unsupported" }));
    assert_eq!(serde_json::to_value(Outcome::Shared).unwrap(), "shared");
    assert_eq!(
        serde_json::to_value(Outcome::Cancelled).unwrap(),
        "cancelled"
    );
}

#[test]
fn the_answer_reads_what_the_kotlin_side_sends() {
    let answer: Answer = serde_json::from_str(r#"{ "outcome": "cancelled" }"#).unwrap();
    assert_eq!(answer.outcome, Outcome::Cancelled);
}

#[test]
fn the_plugin_registers_on_a_mock_app() {
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_share_file::init())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("the plugin registers");
    drop(app);
}
