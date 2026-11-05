//! On a desktop, and on Android, there is no system paste button: `supported`
//! says so, and `place` and `hide` do nothing.

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

/// A mock app has no capability files, so grant every command to every
/// local window, as `paste-button:default` does in a real app.
fn allow_all() -> Resolved {
    let mut resolved = Resolved::default();
    for cmd in ["supported", "place", "hide"] {
        resolved.allowed_commands.insert(
            format!("plugin:paste-button|{cmd}"),
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
        .plugin(tauri_plugin_paste_button::init())
        .build(context)
        .unwrap();
    WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
        .build()
        .unwrap()
}

fn call(w: &WebviewWindow<MockRuntime>, cmd: &str, body: Value) -> Result<Value, Value> {
    get_ipc_response(
        w,
        InvokeRequest {
            cmd: format!("plugin:paste-button|{cmd}"),
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
    .map(|r| r.deserialize::<Value>().unwrap())
}

#[test]
fn the_plugin_registers_on_a_mock_app() {
    drop(window());
}

#[test]
fn a_desktop_is_not_supported() {
    assert_eq!(
        call(&window(), "supported", json!({})),
        Ok(json!({ "supported": false }))
    );
}

#[test]
fn a_desktop_takes_a_place_and_shows_nothing() {
    let place = json!({
        "x": 16, "y": 420, "w": 160, "h": 48, "vx": 16, "vy": 300,
        "scrollHeight": 1200, "clipTop": 0, "clipBottom": 700,
        "tint": "#1f6feb", "ink": "#ffffffcc", "radius": 14, "label": true
    });
    assert_eq!(call(&window(), "place", place), Ok(Value::Null));
}

#[test]
fn a_desktop_takes_a_hide() {
    assert_eq!(call(&window(), "hide", json!({})), Ok(Value::Null));
}
