//! The Mac's vault: the Keychain code in `ios/Sources/Vault.swift`, reached
//! through `macos/Bridge.swift` over the C ABI.

// The C ABI to Swift needs `unsafe`. Each block says what it relies on.
#![allow(unsafe_code)]

use std::ffi::{c_char, c_void, CStr, CString};
use std::marker::PhantomData;

use serde_json::Value;

type Answer = Option<Result<String, String>>;
type Done = extern "C" fn(*mut c_void, bool, *const c_char);

extern "C" {
    fn tauri_plugin_vault_call(
        method: *const c_char,
        args: *const c_char,
        ctx: *mut c_void,
        done: Done,
    );
}

/// The Swift side. `R` only lets the plugin name one managed type for every
/// runtime, as the phones' handle does.
pub struct Swift<R>(PhantomData<fn() -> R>);

impl<R> Swift<R> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

#[async_trait::async_trait]
impl<R: tauri::Runtime> crate::bridge::Ask for Swift<R> {
    /// A Keychain call can wait on the Secure Enclave, so it runs off the
    /// async threads.
    async fn ask(&self, method: &str, args: Value) -> Result<Value, String> {
        let method = method.to_owned();
        tauri::async_runtime::spawn_blocking(move || call(&method, &args))
            .await
            .map_err(|e| e.to_string())?
    }
}

fn call(method: &str, args: &Value) -> Result<Value, String> {
    let method = CString::new(method).map_err(|e| e.to_string())?;
    let args = CString::new(args.to_string()).map_err(|e| e.to_string())?;
    let mut answer: Answer = None;
    // SAFETY: both strings are NUL-terminated and live until the call returns.
    // `Bridge.swift` calls `done` once before it returns, so `answer` is still
    // alive when `done` writes it.
    unsafe {
        tauri_plugin_vault_call(
            method.as_ptr(),
            args.as_ptr(),
            std::ptr::from_mut(&mut answer).cast::<c_void>(),
            done,
        );
    }
    let text = answer.unwrap_or_else(|| Err("The Keychain did not answer".into()))?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

extern "C" fn done(ctx: *mut c_void, ok: bool, body: *const c_char) {
    // SAFETY: `ctx` is `call`'s `answer`, alive for the whole call.
    let answer = unsafe { &mut *ctx.cast::<Answer>() };
    let text = if body.is_null() {
        String::new()
    } else {
        // SAFETY: Swift passes a NUL-terminated string that lives for this call.
        unsafe { CStr::from_ptr(body) }
            .to_string_lossy()
            .into_owned()
    };
    *answer = Some(if ok { Ok(text) } else { Err(text) });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answered(ok: bool, body: &CStr) -> Answer {
        let mut answer: Answer = None;
        done(std::ptr::from_mut(&mut answer).cast(), ok, body.as_ptr());
        answer
    }

    #[test]
    fn an_answer_lands_in_the_caller_and_a_failure_keeps_its_message() {
        assert_eq!(
            answered(true, c"{\"level\":null}"),
            Some(Ok("{\"level\":null}".into()))
        );
        assert_eq!(
            answered(false, c"The Keychain refused the read (-34018)"),
            Some(Err("The Keychain refused the read (-34018)".into()))
        );
    }

    /// A test binary carries no App ID, as a dev build or a Developer ID
    /// download does not. The Keychain then finds nothing and refuses to write.
    #[test]
    fn a_mac_without_the_entitlement_reads_nothing_and_cannot_write() {
        let key = serde_json::json!({ "key": "test" });
        assert_eq!(call("load", &key), Ok(serde_json::json!({})));
        let refused = call("save", &serde_json::json!({ "key": "test", "text": "x" }));
        assert!(refused.is_err_and(|e| e.contains("-34018")));
    }
}
