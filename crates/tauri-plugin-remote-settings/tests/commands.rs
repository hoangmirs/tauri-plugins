//! The commands through Tauri's IPC, as the JS package calls them.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY,
};
use tauri::utils::acl::resolved::{Resolved, ResolvedCommand};
use tauri::utils::acl::ExecutionContext;
use tauri::webview::InvokeRequest;
use tauri::{WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_remote_settings::Config;

const COMMANDS: &[&str] = &["config", "prefetched", "read", "get", "set", "install_id"];

fn allow_all() -> Resolved {
    let mut resolved = Resolved::default();
    for cmd in COMMANDS {
        resolved.allowed_commands.insert(
            format!("plugin:remote-settings|{cmd}"),
            vec![ResolvedCommand {
                context: ExecutionContext::Local,
                windows: vec!["*".parse().unwrap()],
                ..ResolvedCommand::default()
            }],
        );
    }
    resolved
}

/// Port 9 (discard) refuses at once, so every read fails fast and offline.
fn window(dir: &Path) -> WebviewWindow<MockRuntime> {
    let mut context = mock_context(noop_assets());
    *context.runtime_authority_mut() =
        tauri::ipc::RuntimeAuthority::new(BTreeMap::new(), allow_all());
    let app = mock_builder()
        .plugin(tauri_plugin_remote_settings::init(
            Config::new("http://127.0.0.1:9/appa", "production", 2)
                .timeout(Duration::from_secs(2))
                .store_dir(dir.to_path_buf()),
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
            cmd: format!("plugin:remote-settings|{cmd}"),
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
fn config_gives_the_files_place_the_platform_and_the_version() {
    let dir = tempfile::tempdir().unwrap();
    let answer = call(&window(dir.path()), "config", json!({}));
    assert_eq!(answer["baseUrl"], "http://127.0.0.1:9/appa");
    assert_eq!(answer["stage"], "production");
    assert_eq!(answer["keyVersion"], 2);
    assert_eq!(answer["countryHeader"], "x-country");
    assert!(answer["platform"].is_string());
    assert!(answer["version"].is_string());
}

#[test]
fn prefetched_gives_the_first_read_once() {
    let dir = tempfile::tempdir().unwrap();
    let w = window(dir.path());
    assert_eq!(
        call(&w, "prefetched", json!({})),
        json!({ "global": null, "rules": null, "country": null })
    );
    assert_eq!(call(&w, "prefetched", json!({})), Value::Null);
}

#[test]
fn a_failed_read_gives_nulls() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        call(&window(dir.path()), "read", json!({})),
        json!({ "global": null, "rules": null, "country": null })
    );
}

#[test]
fn set_then_get_through_ipc() {
    let dir = tempfile::tempdir().unwrap();
    let w = window(dir.path());
    assert_eq!(call(&w, "get", json!({ "name": "state" })), Value::Null);
    call(&w, "set", json!({ "name": "state", "text": "x" }));
    assert_eq!(call(&w, "get", json!({ "name": "state" })), json!("x"));
    call(&w, "set", json!({ "name": "../out", "text": "x" }));
    assert_eq!(call(&w, "get", json!({ "name": "../out" })), Value::Null);
}

#[test]
fn the_install_id_takes_a_seed_once() {
    let dir = tempfile::tempdir().unwrap();
    let w = window(dir.path());
    let seed = "0f8fad5b-d9cb-469f-a165-70867728950e";
    assert_eq!(call(&w, "install_id", json!({ "seed": seed })), json!(seed));
    assert_eq!(call(&w, "install_id", json!({})), json!(seed));
}
