//! The six commands. Each one asks the native side, and answers its failure
//! value when there is no native side or the native side fails.

use serde_json::Value;
use tauri::Runtime;

use crate::slot::{Settings, Slot};

#[cfg(mobile)]
pub(crate) struct Native<R: Runtime>(pub(crate) tauri::plugin::PluginHandle<R>);

/// The native answer, or `None` on a desktop or on any error.
#[allow(clippy::unused_async)]
async fn call<R: Runtime>(app: &tauri::AppHandle<R>, method: &str, args: Value) -> Option<Value> {
    #[cfg(mobile)]
    {
        use tauri::Manager;
        app.state::<Native<R>>()
            .0
            .run_mobile_plugin_async::<Value>(method, args)
            .await
            .ok()
    }
    #[cfg(not(mobile))]
    {
        let _ = (app, method, args);
        None
    }
}

/// The `result` text of a native answer when it is one of `allowed`, else
/// `fallback`.
fn result_of(answer: Option<&Value>, allowed: &[&str], fallback: &str) -> String {
    answer
        .and_then(|v| v.get("result"))
        .and_then(Value::as_str)
        .filter(|r| allowed.contains(r))
        .unwrap_or(fallback)
        .to_string()
}

#[tauri::command]
pub(crate) async fn init<R: Runtime>(app: tauri::AppHandle<R>, settings: Settings) {
    call(
        &app,
        "init",
        serde_json::to_value(settings).unwrap_or_default(),
    )
    .await;
}

/// A desktop shows no ads, so no law asks it for consent.
#[tauri::command]
pub(crate) async fn consent<R: Runtime>(app: tauri::AppHandle<R>) -> String {
    let fallback = if cfg!(mobile) {
        "failed"
    } else {
        "not_required"
    };
    let answer = call(&app, "consent", Value::Null).await;
    result_of(
        answer.as_ref(),
        &["obtained", "not_required", "failed"],
        fallback,
    )
}

#[tauri::command]
pub(crate) async fn show_native<R: Runtime>(app: tauri::AppHandle<R>, slot: Slot) {
    call(
        &app,
        "showNative",
        serde_json::to_value(slot).unwrap_or_default(),
    )
    .await;
}

#[tauri::command]
pub(crate) async fn hide_native<R: Runtime>(app: tauri::AppHandle<R>) {
    call(&app, "hideNative", Value::Null).await;
}

#[tauri::command]
pub(crate) async fn load_rewarded<R: Runtime>(app: tauri::AppHandle<R>, unit_id: String) -> String {
    let answer = call(
        &app,
        "loadRewarded",
        serde_json::json!({ "unitId": unit_id }),
    )
    .await;
    result_of(answer.as_ref(), &["ready", "failed"], "failed")
}

#[tauri::command]
pub(crate) async fn show_rewarded<R: Runtime>(app: tauri::AppHandle<R>) -> String {
    let answer = call(&app, "showRewarded", Value::Null).await;
    result_of(answer.as_ref(), &["earned", "closed", "failed"], "failed")
}

#[cfg(test)]
mod tests {
    use super::result_of;
    use serde_json::json;

    #[test]
    fn a_result_outside_the_allowed_list_reads_as_the_fallback() {
        assert_eq!(
            result_of(Some(&json!({ "result": "earned" })), &["earned"], "failed"),
            "earned"
        );
        assert_eq!(
            result_of(Some(&json!({ "result": "maybe" })), &["earned"], "failed"),
            "failed"
        );
        assert_eq!(result_of(None, &["earned"], "failed"), "failed");
    }
}
