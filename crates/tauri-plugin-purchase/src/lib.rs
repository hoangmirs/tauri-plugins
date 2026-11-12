//! `tauri-plugin-purchase`: one-time purchases through the device's own store,
//! `StoreKit` 2 on iOS and macOS and the Play Billing Library on Android.
//!
//! Seven commands, each answering JSON:
//!
//! - `products { ids }` → `{ products: [{ id, price }] }`, the price as the
//!   store writes it in the player's currency
//! - `buy { id, consumable? }` → `{ state, proof?, tx? }`, where `state` is
//!   `bought`, `cancelled`, `pending` or `failed`, and `proof` comes with
//!   `bought`. A consumable also answers `tx` and stays open: the store does
//!   not finish it until the app calls `finish`
//! - `owned { ids }` → `{ owned: [{ id, proof }] }`, what this store account
//!   owns now. A refunded purchase is not in it
//! - `restore { ids }` → the same as `owned`, after asking the store to bring
//!   back past purchases
//! - `finish { tx }` → `{}`, finishes a consumable on iOS and macOS and consumes
//!   it on Android, once the app has claimed it
//! - `unfinished { ids }` → `{ open: [{ id, tx }] }`, the consumables bought
//!   and not yet finished
//!
//! A consumable that completes later, such as a pending Android payment or an
//! Apple purchase cut off by a closed app, arrives as the `arrived` event with
//! `{ id, tx }`.
//!
//! `proof` is what a server checks with the store: on iOS and macOS the
//! transaction Apple signed (its JWS), on Android the Play purchase token. The plugin
//! never decides on its own that something is bought, and an app should not
//! either: a server that asks the store is the one check a phone cannot fake.
//!
//! On macOS only an app from the Mac App Store, or `TestFlight`, can buy. On
//! Windows and Linux every command answers an error.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

#[cfg(target_os = "macos")]
mod macos;

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

#[cfg(target_os = "macos")]
async fn ask<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    macos::ask(method, args).await
}

#[cfg(not(any(mobile, target_os = "macos")))]
const NO_STORE: &str = "Purchases need the App Store or Google Play, on iOS, Android or macOS";

// Async only to share the commands' shape with the platforms that wait.
#[cfg(not(any(mobile, target_os = "macos")))]
#[allow(clippy::unused_async)]
async fn ask<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    _method: &str,
    _args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    Err(NO_STORE.into())
}

#[tauri::command]
async fn products<R: Runtime>(
    app: tauri::AppHandle<R>,
    ids: Vec<String>,
) -> Result<serde_json::Value, String> {
    ask(&app, "products", serde_json::json!({ "ids": ids })).await
}

#[tauri::command]
async fn buy<R: Runtime>(
    app: tauri::AppHandle<R>,
    id: String,
    consumable: Option<bool>,
) -> Result<serde_json::Value, String> {
    ask(
        &app,
        "buy",
        serde_json::json!({ "id": id, "consumable": consumable.unwrap_or(false) }),
    )
    .await
}

#[tauri::command]
async fn owned<R: Runtime>(
    app: tauri::AppHandle<R>,
    ids: Vec<String>,
) -> Result<serde_json::Value, String> {
    ask(&app, "owned", serde_json::json!({ "ids": ids })).await
}

#[tauri::command]
async fn finish<R: Runtime>(
    app: tauri::AppHandle<R>,
    tx: String,
) -> Result<serde_json::Value, String> {
    ask(&app, "finish", serde_json::json!({ "tx": tx })).await
}

#[tauri::command]
async fn unfinished<R: Runtime>(
    app: tauri::AppHandle<R>,
    ids: Vec<String>,
) -> Result<serde_json::Value, String> {
    ask(&app, "unfinished", serde_json::json!({ "ids": ids })).await
}

#[tauri::command]
async fn restore<R: Runtime>(
    app: tauri::AppHandle<R>,
    ids: Vec<String>,
) -> Result<serde_json::Value, String> {
    ask(&app, "restore", serde_json::json!({ "ids": ids })).await
}

/// Registers the plugin: the Swift side on iOS and macOS, the Kotlin side on
/// Android, and commands that answer an error on Windows and Linux.
///
/// Register it at launch. On iOS and macOS it then finishes non-consumable
/// purchases that complete while the app is closed, such as one a parent
/// approved later, and on Android it acknowledges each non-consumable
/// purchase, which Play refunds after three days without one. A consumable
/// stays open until the app calls `finish`.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    let builder = Builder::new("purchase");
    // A mobile plugin's listeners belong to the core, so only the Mac lists
    // its own.
    #[cfg(target_os = "macos")]
    let builder = builder.invoke_handler(tauri::generate_handler![
        products,
        buy,
        owned,
        restore,
        finish,
        unfinished,
        macos::register_listener,
        macos::remove_listener
    ]);
    #[cfg(not(target_os = "macos"))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        products, buy, owned, restore, finish, unfinished
    ]);
    builder
        .setup(|app, api| {
            #[cfg(target_os = "macos")]
            macos::listen();
            #[cfg(mobile)]
            {
                use tauri::Manager;
                #[cfg(target_os = "android")]
                let handle =
                    api.register_android_plugin("dev.hoangmirs.purchase", "PurchasePlugin")?;
                #[cfg(target_os = "ios")]
                let handle = api.register_ios_plugin(init_plugin_purchase)?;
                app.manage(Native(handle));
            }
            #[cfg(not(mobile))]
            let _ = (app, api);
            Ok(())
        })
        .build()
}
