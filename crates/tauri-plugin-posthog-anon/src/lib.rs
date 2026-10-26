//! `tauri-plugin-posthog-anon`: sends anonymous analytics events to `PostHog`
//! from a queue on disk.

mod event;
mod identity;
mod queue;
mod send;
mod worker;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, RunEvent, Runtime, State as Managed};

use queue::Limits;
use worker::{Handle, Settings, Worker};

const US_HOST: &str = "https://us.i.posthog.com";
const EU_HOST: &str = "https://eu.i.posthog.com";

/// How many messages may wait for the worker while it is busy sending. A
/// capture that finds it full is dropped rather than waited on.
const CHANNEL: usize = 1_024;

/// The longest exit ever waits for a last flush.
const EXIT_WAIT: Duration = Duration::from_secs(2);

/// The shortest wait [`Config::flush_every`] accepts. A zero period would
/// panic the worker's timer.
const MIN_FLUSH_EVERY: Duration = Duration::from_secs(1);

/// Which `PostHog` project to send to, where, and how often to send. How
/// much to keep on disk has the plan's defaults and is not configurable yet.
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

    /// Sends what is queued this often, 30 seconds by default, raised to 1
    /// second if shorter. A longer wait puts more events in each request.
    /// Twenty queued events, launch and exit still send at once.
    #[must_use]
    pub fn flush_every(mut self, every: Duration) -> Self {
        self.flush_every = every.max(MIN_FLUSH_EVERY);
        self
    }
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
    Builder::new("posthog-anon")
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
            let worker = Worker::new(settings, dir, Arc::new(send::UreqPost::new()));
            app.manage(worker.handle_for(tx));
            tauri::async_runtime::spawn(worker.run(rx, config.flush_every));
            Ok(())
        })
        .on_event(|app, event| {
            if let RunEvent::Exit = event {
                if let Some(handle) = app.try_state::<Handle>() {
                    handle.flush_before_exit(EXIT_WAIT);
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
    handle.flush();
    Ok(())
}

/// Opts the install out (clearing anything queued) or back in, and
/// remembers it across launches. In effect, and saved, before it returns;
/// it does not wait for the worker. Never returns `Err`.
#[tauri::command]
async fn set_opt_out(handle: Managed<'_, Handle>, out: bool) -> Result<(), String> {
    handle.set_opt_out(out);
    Ok(())
}

/// Whether the install has opted out. Never returns `Err`.
#[tauri::command]
async fn is_opted_out(handle: Managed<'_, Handle>) -> Result<bool, String> {
    Ok(handle.is_opted_out())
}

#[cfg(test)]
mod tests {
    use super::{Config, Duration};

    #[test]
    fn flush_every_defaults_to_30_seconds_and_takes_a_longer_wait() {
        assert_eq!(Config::new("k").flush_every, Duration::from_secs(30));
        assert_eq!(
            Config::new("k")
                .flush_every(Duration::from_secs(120))
                .flush_every,
            Duration::from_secs(120)
        );
    }

    #[test]
    fn flush_every_never_goes_under_one_second() {
        assert_eq!(
            Config::new("k").flush_every(Duration::ZERO).flush_every,
            Duration::from_secs(1)
        );
    }
}
