//! `tauri-plugin-remote-settings`: reads an app's two settings files at start,
//! before the webview runs, and keeps a small store and an install id. The JS
//! package evaluates the rules, so web and app evaluate them with the same code.

mod read;
mod store;

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::async_runtime::JoinHandle;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{AppHandle, Manager, Runtime, State};

use read::{Files, Reader};
use store::Store;

const PLATFORM: &str = if cfg!(target_os = "ios") {
    "ios"
} else if cfg!(target_os = "android") {
    "android"
} else if cfg!(target_os = "macos") {
    "macos"
} else if cfg!(target_os = "windows") {
    "windows"
} else if cfg!(target_os = "linux") {
    "linux"
} else {
    "unknown"
};

/// Where the app's files are: `<base_url>/<stage>/global.json` and
/// `<base_url>/<stage>/v<key_version>.json`.
#[derive(Debug, Clone)]
pub struct Config {
    base_url: String,
    stage: String,
    key_version: u32,
    country_header: String,
    timeout: Duration,
    store_dir: Option<PathBuf>,
}

impl Config {
    #[must_use]
    pub fn new(base_url: impl Into<String>, stage: impl Into<String>, key_version: u32) -> Self {
        Self {
            base_url: base_url.into(),
            stage: stage.into(),
            key_version,
            country_header: "x-country".into(),
            timeout: Duration::from_secs(5),
            store_dir: None,
        }
    }

    /// The response header that holds the user's country. Default `x-country`.
    #[must_use]
    pub fn country_header(mut self, name: impl Into<String>) -> Self {
        self.country_header = name.into();
        self
    }

    /// How long one file read may take, connect to last byte. Default 5 seconds.
    #[must_use]
    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }

    /// Another folder for the store. Without it, the plugin uses its own folder
    /// in the app's data folder.
    #[must_use]
    pub fn store_dir(mut self, dir: PathBuf) -> Self {
        self.store_dir = Some(dir);
        self
    }
}

/// A value that is given once, then never again.
struct Once<T>(Mutex<Option<T>>);

impl<T> Once<T> {
    fn new(v: T) -> Self {
        Self(Mutex::new(Some(v)))
    }

    fn take(&self) -> Option<T> {
        self.0.lock().ok()?.take()
    }
}

struct Plugin {
    config: Config,
    reader: std::sync::Arc<Reader>,
    store: Store,
    prefetch: Once<JoinHandle<Files>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigAnswer {
    base_url: String,
    stage: String,
    key_version: u32,
    country_header: String,
    platform: &'static str,
    version: String,
}

#[must_use]
pub fn init<R: Runtime>(settings: Config) -> TauriPlugin<R> {
    Builder::new("remote-settings")
        .invoke_handler(tauri::generate_handler![
            config, prefetched, read, get, set, install_id
        ])
        .setup(move |app, _api| {
            let dir = if let Some(d) = &settings.store_dir {
                d.clone()
            } else {
                let base = if cfg!(target_os = "android") {
                    app.path().app_data_dir()
                } else {
                    app.path().app_local_data_dir()
                };
                store::dir_for(&base?, cfg!(target_os = "android"))
            };
            let reader = std::sync::Arc::new(Reader::new(
                read::urls(&settings.base_url, &settings.stage, settings.key_version),
                settings.country_header.clone(),
                settings.timeout,
            ));
            let first = reader.clone();
            let prefetch = tauri::async_runtime::spawn_blocking(move || first.read());
            app.manage(Plugin {
                config: settings.clone(),
                reader,
                store: Store::new(dir),
                prefetch: Once::new(prefetch),
            });
            Ok(())
        })
        .build()
}

// Tauri requires a Result of an async command that borrows State. None of
// these fail: a failure reads as null for JS.

#[tauri::command]
async fn config<R: Runtime>(
    app: AppHandle<R>,
    plugin: State<'_, Plugin>,
) -> Result<ConfigAnswer, String> {
    let c = &plugin.config;
    Ok(ConfigAnswer {
        base_url: c.base_url.clone(),
        stage: c.stage.clone(),
        key_version: c.key_version,
        country_header: c.country_header.clone(),
        platform: PLATFORM,
        version: app.package_info().version.to_string(),
    })
}

/// The read started at plugin start, once. Later calls give null.
#[tauri::command]
async fn prefetched(plugin: State<'_, Plugin>) -> Result<Option<Files>, String> {
    let Some(handle) = plugin.prefetch.take() else {
        return Ok(None);
    };
    Ok(handle.await.ok())
}

#[tauri::command]
async fn read(plugin: State<'_, Plugin>) -> Result<Files, String> {
    let reader = plugin.reader.clone();
    Ok(tauri::async_runtime::spawn_blocking(move || reader.read())
        .await
        .unwrap_or(Files {
            global: None,
            rules: None,
            country: None,
        }))
}

#[tauri::command]
async fn get(plugin: State<'_, Plugin>, name: String) -> Result<Option<String>, String> {
    Ok(plugin.store.get(&name))
}

#[tauri::command]
async fn set(plugin: State<'_, Plugin>, name: String, text: String) -> Result<(), String> {
    if let Err(e) = plugin.store.set(&name, &text) {
        log::warn!("remote-settings: could not keep {name}: {e}");
    }
    Ok(())
}

#[tauri::command]
async fn install_id(
    plugin: State<'_, Plugin>,
    seed: Option<String>,
) -> Result<Option<String>, String> {
    Ok(plugin.store.install_id(seed.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::Once;

    #[test]
    fn prefetched_is_given_once() {
        let once = Once::new(5);
        assert_eq!(once.take(), Some(5));
        assert_eq!(once.take(), None);
    }
}
