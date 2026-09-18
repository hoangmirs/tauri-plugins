//! The one task that owns the queue. Commands never touch the queue file:
//! they send a `Msg` over a channel, and this worker handles each one in
//! turn, so a capture, a flush, and an opt-out can never interleave on
//! disk.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tauri::async_runtime::{Receiver, Sender};
use tokio::sync::mpsc::error::TrySendError;

use crate::event::{Base, Event};
use crate::queue::{Limits, Queue};
use crate::send::{Backoff, Post};

/// What a command asks the worker to do.
pub(crate) enum Msg {
    Capture(Event),
    /// Flush now if the backoff allows; `done` hears back once the attempt
    /// (or the decision not to make one) is over.
    Flush(Option<std::sync::mpsc::Sender<()>>),
    SetOptOut(bool),
    IsOptedOut(tokio::sync::oneshot::Sender<bool>),
}

/// Everything the worker needs that comes from `Config` and the app.
pub(crate) struct Settings {
    pub api_key: String,
    pub host: String,
    pub flush_at: usize,
    pub limits: Limits,
    pub app_version: String,
}

/// The queue file's name inside the app's data directory.
const QUEUE_FILE: &str = "posthog-queue.jsonl";

/// How often `flush_before_exit` looks again at a full channel.
const EXIT_RETRY: Duration = Duration::from_millis(10);

/// This build's OS, the way `PostHog` spells it in `$os`.
const OS: &str = if cfg!(target_os = "ios") {
    "iOS"
} else if cfg!(target_os = "android") {
    "Android"
} else if cfg!(target_os = "macos") {
    "macOS"
} else if cfg!(target_os = "windows") {
    "Windows"
} else if cfg!(target_os = "linux") {
    "Linux"
} else {
    "unknown"
};

/// This build's OS in lowercase, matching update-gate's `PLATFORM`.
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

/// The data directory and the queue inside it. Absent when the app's data
/// directory could not be resolved or created, which turns capture into a
/// no-op rather than an error.
struct Store {
    dir: PathBuf,
    queue: Arc<Queue>,
}

pub(crate) struct Worker {
    store: Option<Store>,
    post: Arc<dyn Post>,
    host: Arc<str>,
    api_key: Arc<str>,
    base: Arc<Base>,
    backoff: Backoff,
    flush_at: usize,
    opted_out: bool,
}

impl Worker {
    /// Reads the opt-out and the install id from `dir`. An opt-out found on
    /// disk also clears the queue, in case a clear after an earlier opt-out
    /// never happened, so nothing an opted-out user left behind is ever
    /// sent.
    pub(crate) fn new(settings: Settings, dir: Option<PathBuf>, post: Arc<dyn Post>) -> Self {
        let store = dir.map(|dir| Store {
            queue: Arc::new(Queue::open(dir.join(QUEUE_FILE), settings.limits)),
            dir,
        });
        let opted_out = store
            .as_ref()
            .is_some_and(|store| crate::identity::opted_out(&store.dir));
        let distinct_id = store
            .as_ref()
            .map(|store| crate::identity::install_id(&store.dir))
            .unwrap_or_default();

        let worker = Worker {
            store,
            post,
            host: settings.host.into(),
            api_key: settings.api_key.into(),
            base: Arc::new(Base {
                distinct_id,
                lib_version: env!("CARGO_PKG_VERSION"),
                app_version: settings.app_version,
                os: OS,
                platform: PLATFORM,
            }),
            backoff: Backoff::new(),
            flush_at: settings.flush_at,
            opted_out,
        };
        if worker.opted_out {
            worker.clear_queue();
        }
        worker
    }

    /// Handles one message to completion, flush included, before the next
    /// is looked at: that ordering is what keeps the queue file single-owner.
    pub(crate) async fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Capture(event) => self.capture(&event).await,
            Msg::Flush(done) => {
                self.try_flush().await;
                if let Some(done) = done {
                    let _ = done.send(());
                }
            }
            Msg::SetOptOut(out) => self.set_opt_out(out),
            Msg::IsOptedOut(reply) => {
                let _ = reply.send(self.opted_out);
            }
        }
    }

    /// Flushes once, then handles messages and flushes on every tick of
    /// `every` until every sender is gone.
    pub(crate) async fn run(mut self, mut rx: Receiver<Msg>, every: Duration) {
        self.try_flush().await;

        let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
        // A tick missed during a slow flush is not worth catching up on.
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(msg) => self.handle(msg).await,
                    None => return,
                },
                _ = tick.tick() => self.try_flush().await,
            }
        }
    }

    async fn capture(&mut self, event: &Event) {
        if self.opted_out {
            return;
        }
        let Some(store) = &self.store else {
            return;
        };
        match store.queue.push(event) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => {
                log::warn!(
                    "dropped event {:?}: larger than the queue can hold",
                    event.event
                );
                return;
            }
            Err(e) => {
                log::warn!("could not queue event {:?}: {e}", event.event);
                return;
            }
        }
        if store.queue.len() >= self.flush_at {
            self.try_flush().await;
        }
    }

    /// Persists first, so an opt-out survives even if the clear fails; the
    /// next start clears whatever this one could not.
    fn set_opt_out(&mut self, out: bool) {
        self.opted_out = out;
        let Some(store) = &self.store else {
            return;
        };
        if let Err(e) = crate::identity::set_opted_out(&store.dir, out) {
            log::warn!("could not save the analytics opt-out: {e}");
        }
        if out {
            self.clear_queue();
        }
    }

    fn clear_queue(&self) {
        if let Some(store) = &self.store {
            if let Err(e) = store.queue.clear() {
                log::warn!("could not clear the analytics queue: {e}");
            }
        }
    }

    /// Sends the queue unless there is nothing to send, the install opted
    /// out, or a recent failure is still being waited out. The blocking
    /// send runs on its own thread so a panic there comes back as a
    /// `JoinError`, which counts as one more failure rather than the end of
    /// the worker.
    async fn try_flush(&mut self) {
        let Some(store) = &self.store else {
            return;
        };
        if self.opted_out || store.queue.is_empty() || !self.backoff.ready_in().is_zero() {
            return;
        }

        let queue = Arc::clone(&store.queue);
        let post = Arc::clone(&self.post);
        let host = Arc::clone(&self.host);
        let api_key = Arc::clone(&self.api_key);
        let base = Arc::clone(&self.base);
        let sent = tauri::async_runtime::spawn_blocking(move || {
            crate::send::flush(&queue, post.as_ref(), &host, &api_key, &base)
        })
        .await;

        match sent {
            Ok(Ok(n)) => {
                self.backoff.succeeded();
                log::debug!("sent {n} analytics events");
            }
            Ok(Err(e)) => {
                let wait = self.backoff.failed();
                log::warn!("could not send analytics events, retrying in {wait:?}: {e}");
            }
            Err(e) => {
                let wait = self.backoff.failed();
                log::warn!("sending analytics events did not finish, retrying in {wait:?}: {e}");
            }
        }
    }
}

/// Asks the worker for one last flush and waits for it at most `limit`,
/// start to finish. Blocks the calling thread with plain `std` waits and no
/// runtime handle: the worker and its send run on the async runtime's own
/// threads, so nothing here holds anything they need. Whatever is unsent
/// when the time runs out stays on disk for the next launch.
pub(crate) fn flush_before_exit(tx: &Sender<Msg>, limit: Duration) {
    let deadline = std::time::Instant::now() + limit;
    let (done, finished) = std::sync::mpsc::channel();
    let mut msg = Msg::Flush(Some(done));
    loop {
        match tx.try_send(msg) {
            Ok(()) => break,
            Err(TrySendError::Closed(_)) => return,
            Err(TrySendError::Full(back)) => {
                if std::time::Instant::now() >= deadline {
                    return;
                }
                msg = back;
                std::thread::sleep(EXIT_RETRY);
            }
        }
    }
    let _ = finished.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()));
}

#[cfg(test)]
mod tests {
    use super::{flush_before_exit, Msg, Settings, Worker};
    use crate::event::Event;
    use crate::queue::{Limits, Queue};
    use crate::send::Post;
    use serde_json::{Map, Value};
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    enum Outcome {
        Ok,
        Err,
        Panic,
    }

    /// Records every body it is asked to post, and answers from a script;
    /// an exhausted script answers `Ok`.
    #[derive(Default)]
    struct FakePost {
        script: Mutex<VecDeque<Outcome>>,
        bodies: Mutex<Vec<Value>>,
    }

    impl FakePost {
        fn scripted(outcomes: Vec<Outcome>) -> Arc<Self> {
            Arc::new(FakePost {
                script: Mutex::new(outcomes.into()),
                bodies: Mutex::default(),
            })
        }

        fn batch_sizes(&self) -> Vec<usize> {
            self.bodies
                .lock()
                .unwrap()
                .iter()
                .map(|b| b["batch"].as_array().unwrap().len())
                .collect()
        }
    }

    impl Post for FakePost {
        fn post(&self, _url: &str, body: &Value) -> Result<(), String> {
            self.bodies.lock().unwrap().push(body.clone());
            let next = self.script.lock().unwrap().pop_front();
            match next.unwrap_or(Outcome::Ok) {
                Outcome::Ok => Ok(()),
                Outcome::Err => Err("refused".to_string()),
                Outcome::Panic => panic!("the network stack fell over"),
            }
        }
    }

    fn settings() -> Settings {
        Settings {
            api_key: "phc_test".to_string(),
            host: "https://us.i.posthog.com".to_string(),
            flush_at: 20,
            limits: Limits::default(),
            app_version: "1.2.3".to_string(),
        }
    }

    fn worker(dir: &Path, post: &Arc<FakePost>) -> Worker {
        let post: Arc<dyn Post> = post.clone();
        Worker::new(settings(), Some(dir.to_path_buf()), post)
    }

    /// The queue file the worker owns, opened read-only for inspection.
    fn queue(dir: &Path) -> Queue {
        Queue::open(dir.join("posthog-queue.jsonl"), Limits::default())
    }

    fn event(name: &str) -> Event {
        Event::new(name, Map::new()).unwrap()
    }

    async fn capture(worker: &mut Worker, n: usize) {
        for i in 0..n {
            worker.handle(Msg::Capture(event(&format!("e{i}")))).await;
        }
    }

    async fn opted_out(worker: &mut Worker) -> bool {
        let (tx, rx) = tokio::sync::oneshot::channel();
        worker.handle(Msg::IsOptedOut(tx)).await;
        rx.await.unwrap()
    }

    #[tokio::test]
    async fn twenty_captures_send_exactly_one_batch_of_twenty() {
        let dir = tempfile::tempdir().unwrap();
        let post = FakePost::scripted(vec![]);
        let mut worker = worker(dir.path(), &post);

        capture(&mut worker, 20).await;

        assert_eq!(post.batch_sizes(), vec![20]);
        assert!(queue(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn fewer_than_twenty_wait_for_a_flush() {
        let dir = tempfile::tempdir().unwrap();
        let post = FakePost::scripted(vec![]);
        let mut worker = worker(dir.path(), &post);

        capture(&mut worker, 5).await;
        assert!(post.batch_sizes().is_empty());
        assert_eq!(queue(dir.path()).len(), 5);

        worker.handle(Msg::Flush(None)).await;
        assert_eq!(post.batch_sizes(), vec![5]);
        assert!(queue(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn every_event_carries_the_install_and_the_app() {
        let dir = tempfile::tempdir().unwrap();
        let post = FakePost::scripted(vec![]);
        let mut worker = worker(dir.path(), &post);

        capture(&mut worker, 1).await;
        worker.handle(Msg::Flush(None)).await;

        let body = post.bodies.lock().unwrap()[0].clone();
        let props = &body["batch"][0]["properties"];
        assert_eq!(body["api_key"], "phc_test");
        assert_eq!(
            props["distinct_id"],
            crate::identity::install_id(dir.path())
        );
        assert_eq!(props["$app_version"], "1.2.3");
        assert_eq!(props["$lib_version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn opting_out_clears_the_queue_and_stops_capture_until_opting_back_in() {
        let dir = tempfile::tempdir().unwrap();
        let post = FakePost::scripted(vec![]);
        let mut worker = worker(dir.path(), &post);

        capture(&mut worker, 3).await;
        worker.handle(Msg::SetOptOut(true)).await;
        assert!(queue(dir.path()).is_empty());
        assert!(crate::identity::opted_out(dir.path()));
        assert!(opted_out(&mut worker).await);

        capture(&mut worker, 25).await;
        worker.handle(Msg::Flush(None)).await;
        assert!(queue(dir.path()).is_empty());
        assert!(post.batch_sizes().is_empty());

        worker.handle(Msg::SetOptOut(false)).await;
        assert!(!crate::identity::opted_out(dir.path()));
        assert!(!opted_out(&mut worker).await);
        capture(&mut worker, 2).await;
        assert_eq!(queue(dir.path()).len(), 2);
    }

    #[tokio::test]
    async fn an_opt_out_on_disk_is_honoured_at_start_and_leaves_nothing_queued() {
        let dir = tempfile::tempdir().unwrap();
        queue(dir.path()).push(&event("left-over")).unwrap();
        crate::identity::set_opted_out(dir.path(), true).unwrap();
        let post = FakePost::scripted(vec![]);

        let mut worker = worker(dir.path(), &post);

        assert!(opted_out(&mut worker).await);
        assert!(queue(dir.path()).is_empty());
        capture(&mut worker, 1).await;
        assert!(queue(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn a_flush_while_backing_off_makes_no_request() {
        let dir = tempfile::tempdir().unwrap();
        let post = FakePost::scripted(vec![Outcome::Err]);
        let mut worker = worker(dir.path(), &post);

        capture(&mut worker, 20).await;
        assert_eq!(post.batch_sizes(), vec![20]);
        assert_eq!(queue(dir.path()).len(), 20);

        worker.handle(Msg::Flush(None)).await;
        capture(&mut worker, 5).await;
        assert_eq!(post.batch_sizes(), vec![20], "no request while backing off");
        assert_eq!(queue(dir.path()).len(), 25);
    }

    #[tokio::test]
    async fn a_post_that_panics_counts_as_a_failure_and_the_worker_carries_on() {
        let dir = tempfile::tempdir().unwrap();
        let post = FakePost::scripted(vec![Outcome::Panic]);
        let mut worker = worker(dir.path(), &post);

        capture(&mut worker, 20).await;
        assert_eq!(post.batch_sizes(), vec![20]);
        assert_eq!(queue(dir.path()).len(), 20, "events kept");

        worker.handle(Msg::Flush(None)).await;
        assert_eq!(post.batch_sizes(), vec![20], "backoff engaged");

        // Still answering.
        assert!(!opted_out(&mut worker).await);
        capture(&mut worker, 1).await;
        assert_eq!(queue(dir.path()).len(), 21);
    }

    #[tokio::test]
    async fn an_event_too_large_for_the_queue_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let post = FakePost::scripted(vec![]);
        let mut settings = settings();
        settings.limits = Limits {
            max_events: 1_000,
            max_bytes: 512,
        };
        let post_dyn: Arc<dyn Post> = post.clone();
        let mut worker = Worker::new(settings, Some(dir.path().to_path_buf()), post_dyn);

        let mut props = Map::new();
        props.insert("blob".to_string(), Value::String("x".repeat(4096)));
        worker
            .handle(Msg::Capture(Event::new("huge", props).unwrap()))
            .await;
        capture(&mut worker, 1).await;

        assert_eq!(queue(dir.path()).len(), 1);
    }

    #[tokio::test]
    async fn with_no_data_dir_every_message_is_still_answered() {
        let post = FakePost::scripted(vec![]);
        let post_dyn: Arc<dyn Post> = post.clone();
        let mut worker = Worker::new(settings(), None, post_dyn);

        capture(&mut worker, 25).await;
        worker.handle(Msg::Flush(None)).await;
        assert!(post.batch_sizes().is_empty());

        worker.handle(Msg::SetOptOut(true)).await;
        assert!(opted_out(&mut worker).await);
    }

    #[tokio::test]
    async fn running_flushes_once_at_start_and_again_on_each_tick() {
        let dir = tempfile::tempdir().unwrap();
        queue(dir.path()).push(&event("from-last-run")).unwrap();
        let post = FakePost::scripted(vec![]);
        let worker = worker(dir.path(), &post);
        let (tx, rx) = tauri::async_runtime::channel(16);
        tokio::spawn(worker.run(rx, Duration::from_millis(20)));

        // Answered only after the start-up flush has finished.
        let (reply, answer) = tokio::sync::oneshot::channel();
        tx.send(Msg::IsOptedOut(reply)).await.unwrap();
        assert!(!answer.await.unwrap());
        assert_eq!(post.batch_sizes(), vec![1]);

        tx.send(Msg::Capture(event("later"))).await.unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while post.batch_sizes().len() < 2 {
            assert!(Instant::now() < deadline, "no tick flushed the queue");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(post.batch_sizes(), vec![1, 1]);
    }

    #[test]
    fn exit_waits_for_the_flush_to_finish() {
        let dir = tempfile::tempdir().unwrap();
        queue(dir.path()).push(&event("before-exit")).unwrap();
        let post = FakePost::scripted(vec![]);
        let (tx, mut rx) = tauri::async_runtime::channel(16);
        let mut worker = worker(dir.path(), &post);
        // Handles messages but does no start-up flush, so the one post
        // below can only have come from the exit.
        tauri::async_runtime::spawn(async move {
            while let Some(msg) = rx.recv().await {
                worker.handle(msg).await;
            }
        });

        flush_before_exit(&tx, Duration::from_secs(2));

        assert_eq!(post.batch_sizes(), vec![1]);
        assert!(queue(dir.path()).is_empty());
    }

    #[test]
    fn exit_gives_up_on_a_worker_that_never_answers() {
        let (tx, _rx) = tauri::async_runtime::channel(1);
        let started = Instant::now();

        flush_before_exit(&tx, Duration::from_millis(100));
        // A full channel waits out the same deadline, not a second one.
        flush_before_exit(&tx, Duration::from_millis(100));

        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn exit_does_not_wait_for_a_dead_worker() {
        let (tx, rx) = tauri::async_runtime::channel(1);
        drop(rx);
        let started = Instant::now();

        flush_before_exit(&tx, Duration::from_secs(2));

        assert!(started.elapsed() < Duration::from_millis(500));
    }
}
