//! `tauri-plugin-posthog`: sends anonymous analytics events to `PostHog`
//! from a queue on disk.

mod event;
mod identity;
mod queue;
mod send;
mod worker;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value};
use tauri::async_runtime::Sender;
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, RunEvent, Runtime, State as Managed};

use event::Event;
use queue::Limits;
use worker::{Msg, Settings, Worker};

const US_HOST: &str = "https://us.i.posthog.com";
const EU_HOST: &str = "https://eu.i.posthog.com";

/// How many messages may wait for the worker while it is busy sending. A
/// capture that finds it full is dropped rather than waited on.
const CHANNEL: usize = 1_024;

/// The longest exit ever waits for a last flush.
const EXIT_WAIT: Duration = Duration::from_secs(2);

/// Which `PostHog` project to send to, and where. Everything else — when to
/// flush, how much to keep on disk — has the plan's defaults and is not
/// configurable yet.
pub struct Config {
    api_key: String,
    host: String,
    flush_at: usize,
    flush_every: Duration,
    limits: Limits,
}

impl Config {
    /// Starts from a project API key (`phc_…`) on `PostHog`'s US cloud.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            host: US_HOST.to_string(),
            flush_at: 20,
            flush_every: Duration::from_secs(30),
            limits: Limits::default(),
        }
    }

    /// Sends to `PostHog`'s EU cloud.
    #[must_use]
    pub fn eu(self) -> Self {
        self.host(EU_HOST)
    }

    /// Sends to `PostHog`'s US cloud, the default.
    #[must_use]
    pub fn us(self) -> Self {
        self.host(US_HOST)
    }

    /// Sends to a self-hosted instance or a reverse proxy, e.g.
    /// `https://posthog.example.com`.
    #[must_use]
    pub fn host(mut self, url: impl Into<String>) -> Self {
        self.host = url.into();
        self
    }
}

/// What the commands hold: the way to the worker, and enough to answer on
/// their own if the worker is gone.
struct Handle {
    tx: Sender<Msg>,
    dir: Option<PathBuf>,
    /// Set once a capture has been dropped for want of a worker, so a busy
    /// or dead worker costs one log line, not one per event.
    warned: AtomicBool,
}

impl Handle {
    fn capture(&self, name: &str, properties: Map<String, Value>) {
        let Some(event) = Event::new(name, properties) else {
            log::warn!(
                "dropped an event with an unusable name: {:?}",
                shorten(name)
            );
            return;
        };
        if self.tx.try_send(Msg::Capture(event)).is_err()
            && !self.warned.swap(true, Ordering::Relaxed)
        {
            log::warn!("analytics is not keeping up; dropping events until it does");
        }
    }

    async fn set_opt_out(&self, out: bool) {
        if self.tx.send(Msg::SetOptOut(out)).await.is_ok() {
            return;
        }
        // No worker to clear the queue, but the choice itself must stick;
        // the next start clears the queue for an opt-out it finds on disk.
        if let Some(dir) = &self.dir {
            if let Err(e) = identity::set_opted_out(dir, out) {
                log::warn!("could not save the analytics opt-out: {e}");
            }
        }
    }

    async fn is_opted_out(&self) -> bool {
        let (reply, answer) = tokio::sync::oneshot::channel();
        if self.tx.send(Msg::IsOptedOut(reply)).await.is_ok() {
            if let Ok(out) = answer.await {
                return out;
            }
        }
        self.dir.as_deref().is_some_and(identity::opted_out)
    }
}

/// An event name as it may appear in a log line: bounded, whatever was
/// passed.
fn shorten(name: &str) -> String {
    name.chars().take(64).collect()
}

/// Resolves and creates the app's data directory, or says once why
/// analytics will be off for this run.
fn data_dir<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<PathBuf> {
    let dir = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(e) => {
            log::warn!("no app data directory, so analytics is off for this run: {e}");
            return None;
        }
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!(
            "could not create {}, so analytics is off for this run: {e}",
            dir.display()
        );
        return None;
    }
    Some(dir)
}

/// Registers the `posthog` commands, starts the worker that owns the queue,
/// and asks it for a last flush when the app exits.
#[must_use]
pub fn init<R: Runtime>(config: Config) -> TauriPlugin<R> {
    Builder::new("posthog")
        .invoke_handler(tauri::generate_handler![
            capture,
            flush,
            set_opt_out,
            is_opted_out
        ])
        .setup(move |app, _api| {
            let dir = data_dir(app);
            let settings = Settings {
                api_key: config.api_key,
                host: config.host,
                flush_at: config.flush_at,
                limits: config.limits,
                app_version: app.package_info().version.to_string(),
            };
            let (tx, rx) = tauri::async_runtime::channel(CHANNEL);
            let worker = Worker::new(settings, dir.clone(), Arc::new(send::UreqPost::new()));
            tauri::async_runtime::spawn(worker.run(rx, config.flush_every));
            app.manage(Handle {
                tx,
                dir,
                warned: AtomicBool::new(false),
            });
            Ok(())
        })
        .on_event(|app, event| {
            if let RunEvent::Exit = event {
                if let Some(handle) = app.try_state::<Handle>() {
                    worker::flush_before_exit(&handle.tx, EXIT_WAIT);
                }
            }
        })
        .build()
}

/// Queues `event` with the caller's `properties`. Returns `Result` only
/// because Tauri requires it of an async command that borrows `State`; a
/// bad name, an oversized event, or a busy worker drops the event and logs,
/// so this never returns `Err`.
#[tauri::command]
async fn capture(
    handle: Managed<'_, Handle>,
    event: String,
    properties: Option<Map<String, Value>>,
) -> Result<(), String> {
    handle.capture(&event, properties.unwrap_or_default());
    Ok(())
}

/// Asks for a flush and returns without waiting for the network, so a
/// caller is never held up by it. Never returns `Err`.
#[tauri::command]
async fn flush(handle: Managed<'_, Handle>) -> Result<(), String> {
    let _ = handle.tx.try_send(Msg::Flush(None));
    Ok(())
}

/// Opts the install out (clearing anything queued) or back in, and
/// remembers it across launches. Never returns `Err`.
#[tauri::command]
async fn set_opt_out(handle: Managed<'_, Handle>, out: bool) -> Result<(), String> {
    handle.set_opt_out(out).await;
    Ok(())
}

/// Whether the install has opted out. Never returns `Err`.
#[tauri::command]
async fn is_opted_out(handle: Managed<'_, Handle>) -> Result<bool, String> {
    Ok(handle.is_opted_out().await)
}
