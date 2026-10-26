//! On a desktop, and on Android until its own release, the phone cannot
//! change its icon: each command answers so, and never reaches a system.

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
/// local window, as `home-icon:default` does in a real app.
fn allow_all() -> Resolved {
    let mut resolved = Resolved::default();
    for cmd in ["supported", "current", "set"] {
        resolved.allowed_commands.insert(
            format!("plugin:home-icon|{cmd}"),
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
        .plugin(tauri_plugin_home_icon::init())
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
            cmd: format!("plugin:home-icon|{cmd}"),
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
    assert_eq!(call(&window(), "supported", json!({})), Ok(json!(false)));
}

#[test]
fn a_desktop_has_the_primary_icon() {
    assert_eq!(call(&window(), "current", json!({})), Ok(Value::Null));
}

#[test]
fn a_desktop_refuses_a_change() {
    let err = call(&window(), "set", json!({ "name": "AppIcon-caro" })).unwrap_err();
    assert!(err.to_string().contains("not supported"), "{err}");
}
