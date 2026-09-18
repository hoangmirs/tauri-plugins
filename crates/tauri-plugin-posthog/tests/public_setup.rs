//! Uses the crate only the way an app does, through its public paths, and
//! drives every command through Tauri's IPC with no real network.

use std::collections::BTreeMap;

use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::utils::acl::resolved::{Resolved, ResolvedCommand};
use tauri::utils::acl::ExecutionContext;
use tauri::webview::InvokeRequest;
use tauri::{WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_posthog::{init, Config};

const COMMANDS: [&str; 4] = ["capture", "flush", "set_opt_out", "is_opted_out"];

#[test]
fn the_readme_setup_compiles_against_the_public_api() {
    let _us = init::<tauri::test::MockRuntime>(Config::new("phc_test"));
    let _eu = init::<tauri::test::MockRuntime>(Config::new("phc_test").eu());
    let _back = init::<tauri::test::MockRuntime>(Config::new("phc_test").eu().us());
    let _own = init::<tauri::test::MockRuntime>(
        Config::new("phc_test").host("https://posthog.example.com"),
    );
}

/// A mock app has no capability files, so grant every command to every
/// local window the way `posthog:default` does in a real app.
fn allow_all() -> Resolved {
    let mut resolved = Resolved::default();
    for command in COMMANDS {
        resolved.allowed_commands.insert(
            format!("plugin:posthog|{command}"),
            vec![ResolvedCommand {
                context: ExecutionContext::Local,
                windows: vec!["*".parse().unwrap()],
                ..ResolvedCommand::default()
            }],
        );
    }
    resolved
}

fn invoke(
    webview: &WebviewWindow<tauri::test::MockRuntime>,
    command: &str,
    body: serde_json::Value,
) -> serde_json::Value {
    get_ipc_response(
        webview,
        InvokeRequest {
            cmd: format!("plugin:posthog|{command}"),
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
    .unwrap_or_else(|e| panic!("{command} was rejected: {e:?}"))
    .deserialize::<serde_json::Value>()
    .unwrap()
}

#[test]
fn every_command_answers_ok_though_the_host_refuses() {
    let data = tempfile::tempdir().unwrap();
    let mut context = mock_context(noop_assets());
    *context.runtime_authority_mut() =
        tauri::ipc::RuntimeAuthority::new(BTreeMap::new(), allow_all());
    // The app data dir is `<data_dir>/<identifier>`; an absolute identifier
    // replaces the whole path, so the queue and identity files land in the
    // temp dir instead of the real user profile.
    context.config_mut().identifier = data.path().to_string_lossy().into_owned();

    // Port 9 (discard) on loopback: refused at once, never touches the network.
    let app = mock_builder()
        .plugin(init(Config::new("phc_test").host("http://127.0.0.1:9")))
        .build(context)
        .unwrap();
    let webview = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
        .build()
        .unwrap();

    let none = serde_json::Value::Null;
    assert_eq!(
        invoke(
            &webview,
            "capture",
            serde_json::json!({ "event": "app_opened", "properties": { "screen": "home" } })
        ),
        none
    );
    assert_eq!(invoke(&webview, "flush", serde_json::json!({})), none);
    assert_eq!(
        invoke(&webview, "is_opted_out", serde_json::json!({})),
        serde_json::json!(false)
    );
    assert_eq!(
        invoke(&webview, "set_opt_out", serde_json::json!({ "out": true })),
        none
    );
    assert_eq!(
        invoke(&webview, "is_opted_out", serde_json::json!({})),
        serde_json::json!(true)
    );
    assert!(data.path().join("posthog-opt-out").exists());
}
