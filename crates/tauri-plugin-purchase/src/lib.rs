//! `tauri-plugin-purchase`: one-time purchases through the phone's own store,
//! `StoreKit` 2 on iOS and the Play Billing Library on Android.
//!
//! Four commands, each answering JSON:
//!
//! - `products { ids }` → `{ products: [{ id, price }] }`, the price as the
//!   store writes it in the player's currency
//! - `buy { id }` → `{ state, proof? }`, where `state` is `bought`,
//!   `cancelled`, `pending` or `failed`, and `proof` comes with `bought`
//! - `owned { ids }` → `{ owned: [{ id, proof }] }`, what this store account
//!   owns now. A refunded purchase is not in it
//! - `restore { ids }` → the same as `owned`, after asking the store to bring
//!   back past purchases
//!
//! `proof` is what a server checks with the store: on iOS the transaction
//! Apple signed (its JWS), on Android the Play purchase token. The plugin
//! never decides on its own that something is bought, and an app should not
//! either: a server that asks the store is the one check a phone cannot fake.
//!
//! On a desktop every command answers an error.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_purchase);

#[cfg(mobile)]
struct Native<R: Runtime>(tauri::plugin::PluginHandle<R>);

/// Async, so waiting on the store (a purchase sheet can stay open for
/// minutes) never holds the main thread.
#[cfg(mobile)]
async fn ask<R: Runtime>(
    app: &tauri::AppHandle<R>,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    let handle = app.state::<Native<R>>().0.clone();
    handle
        .run_mobile_plugin_async::<serde_json::Value>(method, args)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(mobile)]
#[tauri::command]
async fn products<R: Runtime>(
    app: tauri::AppHandle<R>,
    ids: Vec<String>,
) -> Result<serde_json::Value, String> {
    ask(&app, "products", serde_json::json!({ "ids": ids })).await
}

#[cfg(mobile)]
#[tauri::command]
async fn buy<R: Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
) -> Result<serde_json::Value, String> {
    ask(&app, "buy", serde_json::json!({ "id": id })).await
}

#[cfg(mobile)]
#[tauri::command]
async fn owned<R: Runtime>(
    app: tauri::AppHandle<R>,
    ids: Vec<String>,
) -> Result<serde_json::Value, String> {
    ask(&app, "owned", serde_json::json!({ "ids": ids })).await
}

#[cfg(mobile)]
#[tauri::command]
async fn restore<R: Runtime>(
    app: tauri::AppHandle<R>,
    ids: Vec<String>,
) -> Result<serde_json::Value, String> {
    ask(&app, "restore", serde_json::json!({ "ids": ids })).await
}

#[cfg(not(mobile))]
const NO_STORE: &str = "Purchases need the App Store or Google Play, on iOS or Android";

#[cfg(not(mobile))]
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn products(ids: Vec<String>) -> Result<serde_json::Value, String> {
    let _ = ids;
    Err(NO_STORE.into())
}

#[cfg(not(mobile))]
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn buy(id: String) -> Result<serde_json::Value, String> {
    let _ = id;
    Err(NO_STORE.into())
}

#[cfg(not(mobile))]
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn owned(ids: Vec<String>) -> Result<serde_json::Value, String> {
    let _ = ids;
    Err(NO_STORE.into())
}

#[cfg(not(mobile))]
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn restore(ids: Vec<String>) -> Result<serde_json::Value, String> {
    let _ = ids;
    Err(NO_STORE.into())
}

/// Registers the plugin: the Swift side on iOS, the Kotlin side on Android,
/// and commands that answer an error everywhere else.
///
/// Register it at launch. On iOS it then finishes purchases that complete
/// while the app is closed, such as one a parent approved later, and on
/// Android it acknowledges each purchase, which Play refunds after three days
/// without one.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("purchase")
        .invoke_handler(tauri::generate_handler![products, buy, owned, restore])
        .setup(|_app, _api| {
            #[cfg(mobile)]
            {
                use tauri::Manager;
                #[cfg(target_os = "android")]
                let handle =
                    _api.register_android_plugin("dev.hoangmirs.purchase", "PurchasePlugin")?;
                #[cfg(target_os = "ios")]
                let handle = _api.register_ios_plugin(init_plugin_purchase)?;
                _app.manage(Native(handle));
            }
            Ok(())
        })
        .build()
}
