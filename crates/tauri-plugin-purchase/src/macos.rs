//! The Mac's store: the `StoreKit` 2 code in `ios/Sources/Store.swift`,
//! reached through `macos/Bridge.swift` over the C ABI.
//!
//! A mobile plugin's events go through the core's `register_listener`. A
//! desktop plugin has no core for that, so this module answers those two
//! commands itself, with the same arguments.

// The C ABI to Swift needs `unsafe`. Each block says what it relies on.
#![allow(unsafe_code)]

use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::{Mutex, Once};

use serde_json::Value;
use tauri::ipc::Channel;
use tokio::sync::oneshot;

type Reply = Result<Value, String>;
type Done = extern "C" fn(*mut c_void, bool, *const c_char);
type Arrived = extern "C" fn(*const c_char);

extern "C" {
    fn tauri_plugin_purchase_call(
        method: *const c_char,
        args: *const c_char,
        ctx: *mut c_void,
        done: Done,
    );
    fn tauri_plugin_purchase_listen(arrived: Arrived);
}

/// One store command. Swift copies both strings before it returns, and calls
/// `done` once when the store answers.
pub async fn ask(method: &str, args: Value) -> Reply {
    let method = CString::new(method).map_err(|e| e.to_string())?;
    let args = CString::new(args.to_string()).map_err(|e| e.to_string())?;
    let (tx, rx) = oneshot::channel::<Reply>();
    let ctx = Box::into_raw(Box::new(tx)).cast::<c_void>();
    // SAFETY: both strings are NUL-terminated and live until the call returns,
    // and `Bridge.swift` copies them first. It hands `ctx` back to `done`
    // exactly once, which takes the box back.
    unsafe { tauri_plugin_purchase_call(method.as_ptr(), args.as_ptr(), ctx, done) };
    rx.await
        .unwrap_or_else(|_| Err("The store did not answer".into()))
}

extern "C" fn done(ctx: *mut c_void, ok: bool, body: *const c_char) {
    // SAFETY: `ctx` is the box `ask` leaked, and Swift passes it back once.
    let tx = unsafe { Box::from_raw(ctx.cast::<oneshot::Sender<Reply>>()) };
    let reply = text(body).and_then(|text| {
        if ok {
            serde_json::from_str(&text).map_err(|e| e.to_string())
        } else {
            Err(text)
        }
    });
    // The command was dropped, so nobody waits for the answer.
    let _ = tx.send(reply);
}

fn text(body: *const c_char) -> Result<String, String> {
    if body.is_null() {
        return Err("The store answered nothing".into());
    }
    // SAFETY: Swift passes a NUL-terminated string that lives for this call.
    Ok(unsafe { CStr::from_ptr(body) }
        .to_string_lossy()
        .into_owned())
}

static LISTENERS: Mutex<Vec<(String, Channel<Value>)>> = Mutex::new(Vec::new());
static LISTEN: Once = Once::new();

/// Starts `StoreKit`'s transaction updates, once per process.
pub fn listen() {
    // SAFETY: `arrived` is a plain function and lives as long as the process.
    LISTEN.call_once(|| unsafe { tauri_plugin_purchase_listen(arrived) });
}

extern "C" fn arrived(body: *const c_char) {
    let Ok(payload) =
        text(body).and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()))
    else {
        return;
    };
    send("arrived", &payload);
}

fn send(event: &str, payload: &Value) {
    let Ok(listeners) = LISTENERS.lock() else {
        return;
    };
    for (name, channel) in listeners.iter() {
        if name == event {
            // A closed webview's channel fails. The others still hear it.
            let _ = channel.send(payload.clone());
        }
    }
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn register_listener(event: String, handler: Channel<Value>) -> Result<(), String> {
    LISTENERS
        .lock()
        .map_err(|e| e.to_string())?
        .push((event, handler));
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn remove_listener(event: String, channel_id: u32) -> Result<(), String> {
    LISTENERS
        .lock()
        .map_err(|e| e.to_string())?
        .retain(|(name, channel)| !(name == &event && channel.id() == channel_id));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(rx: oneshot::Receiver<Reply>) -> Reply {
        rx.blocking_recv().expect("done sends")
    }

    fn ctx() -> (*mut c_void, oneshot::Receiver<Reply>) {
        let (tx, rx) = oneshot::channel::<Reply>();
        (Box::into_raw(Box::new(tx)).cast::<c_void>(), rx)
    }

    #[test]
    fn an_answer_comes_back_as_json_and_a_failure_as_its_message() {
        let (c, rx) = ctx();
        done(c, true, c"{\"owned\":[]}".as_ptr());
        assert_eq!(wait(rx), Ok(serde_json::json!({ "owned": [] })));

        let (c, rx) = ctx();
        done(c, false, c"owned needs ids".as_ptr());
        assert_eq!(wait(rx), Err("owned needs ids".into()));

        let (c, rx) = ctx();
        done(c, true, c"not json".as_ptr());
        assert!(wait(rx).is_err(), "an answer that is not JSON is an error");
    }

    #[test]
    fn an_answer_to_a_dropped_command_goes_nowhere() {
        let (c, rx) = ctx();
        drop(rx);
        done(c, true, c"{}".as_ptr());
    }
}
