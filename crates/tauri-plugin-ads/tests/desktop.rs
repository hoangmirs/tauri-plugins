//! On a desktop there are no ads, so every command answers its failure value,
//! or nothing, and never an error.

use std::collections::BTreeMap;

use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY,
};
use tauri::utils::acl::resolved::{Resolved, ResolvedCommand};
use tauri::utils::acl::ExecutionContext;
use tauri::webview::InvokeRequest;
use tauri::{WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const COMMANDS: &[&str] = &[
    "init",
    "consent",
    "show_native",
    "hide_native",
    "load_rewarded",
    "show_rewarded",
    "privacy_options",
    "show_privacy_options",
    "set_muted",
];

/// A mock app has no capability files, so grant every command to every
/// local window, as `ads:default` does in a real app.
fn allow_all() -> Resolved {
    let mut resolved = Resolved::default();
    for cmd in COMMANDS {
        resolved.allowed_commands.insert(
            format!("plugin:ads|{cmd}"),
            vec![ResolvedCommand {
                context: ExecutionContext::Local,
                windows: vec!["*".parse().unwrap()],
                ..ResolvedCommand::default()
            }],
        );
    }
    resolved
}

fn window() -> WebviewWindow<MockRuntime> {
    let mut context = mock_context(noop_assets());
    *context.runtime_authority_mut() =
        tauri::ipc::RuntimeAuthority::new(BTreeMap::new(), allow_all());
    let app = mock_builder()
        .plugin(tauri_plugin_ads::init())
        .build(context)
        .unwrap();
    WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
        .build()
        .unwrap()
}

fn call(w: &WebviewWindow<MockRuntime>, cmd: &str, body: Value) -> Value {
    let response = get_ipc_response(
        w,
        InvokeRequest {
            cmd: format!("plugin:ads|{cmd}"),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: if cfg!(any(windows, target_os = "android")) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .unwrap(),
            body: InvokeBody::Json(body),
            headers: tauri::http::HeaderMap::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .unwrap_or_else(|e| panic!("{cmd} failed: {e}"));
    response.deserialize::<Value>().unwrap()
}

#[test]
fn every_command_answers_without_an_error_on_a_desktop() {
    let w = window();
    assert_eq!(
        call(
            &w,
            "init",
            json!({ "settings": { "childDirected": true, "maxRating": "G" } })
        ),
        Value::Null
    );
    assert_eq!(call(&w, "consent", json!({})), json!("not_required"));
    let slot = json!({ "slot": {
        "unitId": "u", "x": 0, "y": 900, "vx": 16, "vy": 400, "w": 343, "h": 140,
        "scrollHeight": 1200, "ink": "rgb(0, 0, 0)", "muted": "rgb(0, 0, 0)",
        "button": "rgb(0, 0, 0)", "edge": "rgb(0, 0, 0)"
    } });
    assert_eq!(call(&w, "show_native", slot), Value::Null);
    assert_eq!(call(&w, "hide_native", json!({})), Value::Null);
    assert_eq!(
        call(&w, "load_rewarded", json!({ "unitId": "u" })),
        json!("failed")
    );
    assert_eq!(call(&w, "show_rewarded", json!({})), json!("failed"));
    assert_eq!(
        call(&w, "privacy_options", json!({})),
        json!("not_required")
    );
    assert_eq!(call(&w, "show_privacy_options", json!({})), json!("failed"));
    assert_eq!(call(&w, "set_muted", json!({ "muted": true })), Value::Null);
}
