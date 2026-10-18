//! On a desktop there is no Firebase, so every command answers from the
//! defaults the app set, and nothing ever fails.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY,
};
use tauri::utils::acl::resolved::{Resolved, ResolvedCommand};
use tauri::utils::acl::ExecutionContext;
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_firebase::{Config, CrashExt, Frame};

const COMMANDS: &[&str] = &[
    "mode",
    "set_defaults",
    "fetch",
    "activate",
    "get_all",
    "log_event",
    "set_collection",
    "crash_set_collection",
    "crash_set_keys",
    "crash_log",
    "crash_record",
];

/// A mock app has no capability files, so grant every command to every
/// local window, as `firebase:default` does in a real app.
fn allow_all() -> Resolved {
    let mut resolved = Resolved::default();
    for cmd in COMMANDS {
        resolved.allowed_commands.insert(
            format!("plugin:firebase|{cmd}"),
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
        .plugin(tauri_plugin_firebase::init(
            Config::default().minimum_fetch_interval(Duration::from_secs(60)),
        ))
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
            cmd: format!("plugin:firebase|{cmd}"),
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
fn mode_is_desktop() {
    assert_eq!(call(&window(), "mode", json!({})), json!("desktop"));
}

#[test]
fn get_all_is_empty_before_defaults() {
    assert_eq!(call(&window(), "get_all", json!({})), json!({}));
}

#[test]
fn get_all_returns_the_defaults_as_text() {
    let w = window();
    call(
        &w,
        "set_defaults",
        json!({ "defaults": { "a": true, "b": 15, "c": "x", "d": { "k": 1 } } }),
    );
    assert_eq!(
        call(&w, "get_all", json!({})),
        json!({
            "a": { "value": "true", "source": "default" },
            "b": { "value": "15", "source": "default" },
            "c": { "value": "x", "source": "default" },
            "d": { "value": "{\"k\":1}", "source": "default" },
        })
    );
}

#[test]
fn a_later_set_defaults_replaces_the_earlier() {
    let w = window();
    call(
        &w,
        "set_defaults",
        json!({ "defaults": { "a": true, "b": 1 } }),
    );
    call(&w, "set_defaults", json!({ "defaults": { "a": false } }));
    assert_eq!(
        call(&w, "get_all", json!({})),
        json!({ "a": { "value": "false", "source": "default" } })
    );
}

#[test]
fn fetch_fails_on_desktop() {
    assert_eq!(call(&window(), "fetch", json!({})), json!("failed"));
}

#[test]
fn activate_is_false_on_desktop() {
    assert_eq!(call(&window(), "activate", json!({})), json!(false));
}

#[test]
fn log_event_and_set_collection_do_nothing_on_desktop() {
    let w = window();
    assert_eq!(
        call(
            &w,
            "log_event",
            json!({ "name": "app_opened", "params": { "n": 1 } })
        ),
        Value::Null
    );
    assert_eq!(
        call(&w, "set_collection", json!({ "enabled": true })),
        Value::Null
    );
}

#[test]
fn crash_commands_answer_on_a_desktop() {
    let w = window();
    assert_eq!(
        call(&w, "crash_set_collection", json!({ "enabled": true })),
        Value::Null
    );
    assert_eq!(
        call(
            &w,
            "crash_set_keys",
            json!({ "keys": { "screen": "home" } })
        ),
        Value::Null
    );
    assert_eq!(
        call(&w, "crash_log", json!({ "line": "opened" })),
        Value::Null
    );
    assert_eq!(
        call(
            &w,
            "crash_record",
            json!({
                "name": "Error",
                "message": "boom",
                "frames": [{ "symbol": "f", "file": "a.js", "line": 3 }]
            })
        ),
        Value::Null
    );
}

#[test]
fn crash_api_does_nothing_on_a_desktop() {
    let w = window();
    let handle = w.app_handle();
    handle.crash().log("x");
    handle.crash().record("n", "m", &[]);
    handle.crash().record(
        "n",
        "m",
        &[Frame {
            symbol: "f".into(),
            file: "a.rs".into(),
            line: 1,
        }],
    );
}
