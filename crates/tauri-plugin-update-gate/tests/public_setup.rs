//! Uses the crate only the way an app does, through its public paths, so
//! the README's setup can't drift into something that doesn't compile.

use std::collections::BTreeMap;
use std::time::Duration;

use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::utils::acl::resolved::{Resolved, ResolvedCommand};
use tauri::utils::acl::ExecutionContext;
use tauri::webview::InvokeRequest;
use tauri::{WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_update_gate::{Config, Gate, State};

const CHECK: &str = "plugin:update-gate|check";

/// The README's setup, word for word, plus the two knobs it mentions.
fn config(url: &str) -> Config {
    Config::url(url)
        .ttl(Duration::from_secs(60))
        .timeout(Duration::from_millis(500))
}

#[test]
fn the_readme_setup_compiles_against_the_public_api() {
    let _plugin = tauri_plugin_update_gate::init::<tauri::test::MockRuntime>(config(
        "https://example.com/app-config.json",
    ));
    // `Gate` and `State` are what an app matches on in Rust.
    let _: fn(&Gate) -> State = |g| g.state;
}

/// A mock app has no capability files, so grant `check` to every local
/// window the way `update-gate:default` does in a real app.
fn allow_check() -> Resolved {
    let mut resolved = Resolved::default();
    resolved.allowed_commands.insert(
        CHECK.to_string(),
        vec![ResolvedCommand {
            context: ExecutionContext::Local,
            windows: vec!["*".parse().unwrap()],
            ..ResolvedCommand::default()
        }],
    );
    resolved
}

#[test]
fn an_unreachable_document_answers_the_open_gate() {
    let mut context = mock_context(noop_assets());
    *context.runtime_authority_mut() =
        tauri::ipc::RuntimeAuthority::new(BTreeMap::new(), allow_check());

    // Port 9 (discard) on loopback: refused at once, never touches the network.
    let app = mock_builder()
        .plugin(tauri_plugin_update_gate::init(config(
            "http://127.0.0.1:9/",
        )))
        .build(context)
        .unwrap();
    let webview = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
        .build()
        .unwrap();

    let response = get_ipc_response(
        &webview,
        InvokeRequest {
            cmd: CHECK.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: if cfg!(any(windows, target_os = "android")) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .unwrap(),
            body: InvokeBody::default(),
            headers: tauri::http::HeaderMap::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .unwrap()
    .deserialize::<serde_json::Value>()
    .unwrap();

    assert_eq!(
        response,
        serde_json::json!({
            "state": "ok",
            "latestVersion": null,
            "message": null,
            "url": null,
        })
    );
}
