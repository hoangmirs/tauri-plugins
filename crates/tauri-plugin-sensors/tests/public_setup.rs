//! On a desktop the device has no sensors: `available` answers no kinds, and
//! every other command does nothing and answers `null`.

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
use tauri_plugin_sensors::{clamp_hz, Available, Kind};

/// A mock app has no capability files, so grant every command to every
/// local window, as `sensors:default` does in a real app.
fn allow_all() -> Resolved {
    let mut resolved = Resolved::default();
    for cmd in [
        "available",
        "start",
        "stop",
        "lock_rotation",
        "unlock_rotation",
    ] {
        resolved.allowed_commands.insert(
            format!("plugin:sensors|{cmd}"),
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
        .plugin(tauri_plugin_sensors::init())
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
            cmd: format!("plugin:sensors|{cmd}"),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: if cfg!(windows) {
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
fn a_desktop_has_no_sensors() {
    assert_eq!(
        serde_json::to_value(tauri_plugin_sensors::available()).unwrap(),
        json!({ "kinds": [] })
    );
    assert_eq!(
        call(&window(), "available", json!({})),
        Ok(json!({ "kinds": [] }))
    );
}

#[test]
fn a_desktop_does_nothing_for_the_other_commands() {
    let w = window();
    assert_eq!(
        call(&w, "start", json!({ "kind": "motion", "hz": 30 })),
        Ok(Value::Null)
    );
    assert_eq!(
        call(&w, "stop", json!({ "kind": "heading" })),
        Ok(Value::Null)
    );
    assert_eq!(call(&w, "lock_rotation", json!({})), Ok(Value::Null));
    assert_eq!(call(&w, "unlock_rotation", json!({})), Ok(Value::Null));
}

#[test]
fn each_kind_has_the_name_a_webview_sends() {
    let both = Available {
        kinds: vec![Kind::Motion, Kind::Heading],
    };
    assert_eq!(
        serde_json::to_value(both).unwrap(),
        json!({ "kinds": ["motion", "heading"] })
    );
    assert_eq!(
        serde_json::from_value::<Kind>(json!("heading")).unwrap(),
        Kind::Heading
    );
}

#[test]
fn hz_stays_within_1_and_60() {
    assert_eq!(clamp_hz(0.0), 1);
    assert_eq!(clamp_hz(-5.0), 1);
    assert_eq!(clamp_hz(30.0), 30);
    assert_eq!(clamp_hz(29.6), 30);
    assert_eq!(clamp_hz(60.0), 60);
    assert_eq!(clamp_hz(500.0), 60);
    assert_eq!(clamp_hz(f64::NAN), 1);
}
