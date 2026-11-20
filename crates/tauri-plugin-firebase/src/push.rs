//! Push messages: the device's token, and the silent pushes the native side
//! kept while the app's code did not run. On a desktop, and on a phone without
//! its Firebase file, there is no token and the queue is empty.

use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(mobile)]
use tauri::Manager;
use tauri::{AppHandle, Runtime};

/// One silent push: its action, its data, and when it came, in milliseconds
/// since 1970.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PushData {
    pub action: String,
    #[serde(default)]
    pub data: Value,
    pub at: u64,
}

/// The push API of one app.
pub struct Push<R: Runtime> {
    #[cfg_attr(not(mobile), allow(dead_code))]
    app: AppHandle<R>,
}

pub trait PushExt<R: Runtime> {
    fn push(&self) -> Push<R>;
}

impl<R: Runtime> PushExt<R> for AppHandle<R> {
    fn push(&self) -> Push<R> {
        Push { app: self.clone() }
    }
}

impl<R: Runtime> Push<R> {
    /// The device's push token. `None` when the system denies notifications,
    /// or when no token comes in time.
    pub async fn token(&self) -> Option<String> {
        self.call("pushToken")
            .await
            .and_then(|v| v.get("token").and_then(Value::as_str).map(str::to_string))
    }

    pub async fn delete_token(&self) {
        self.call("deletePushToken").await;
    }

    /// The silent pushes kept since the last call, oldest first. The call
    /// empties the queue.
    pub async fn take_data(&self) -> Vec<PushData> {
        let Some(Value::Array(entries)) = self
            .call("takePushData")
            .await
            .and_then(|mut v| v.get_mut("entries").map(Value::take))
        else {
            return Vec::new();
        };
        entries
            .into_iter()
            .filter_map(|e| serde_json::from_value(e).ok())
            .collect()
    }

    #[cfg_attr(not(mobile), allow(clippy::unused_async))]
    async fn call(&self, method: &str) -> Option<Value> {
        #[cfg(mobile)]
        {
            let native = self.app.try_state::<crate::Native<R>>()?;
            native.call(method, Value::Null).await
        }
        #[cfg(not(mobile))]
        {
            let _ = method;
            None
        }
    }
}

#[tauri::command]
pub(crate) async fn push_token<R: Runtime>(app: AppHandle<R>) -> Option<String> {
    app.push().token().await
}

#[tauri::command]
pub(crate) async fn delete_push_token<R: Runtime>(app: AppHandle<R>) {
    app.push().delete_token().await;
}

#[tauri::command]
pub(crate) async fn take_push_data<R: Runtime>(app: AppHandle<R>) -> Vec<PushData> {
    app.push().take_data().await
}
