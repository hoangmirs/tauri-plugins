//! The one task that owns the queue, and the `Handle` commands use to
//! reach it. Commands never touch the queue file: they send a `Msg` over a
//! channel, and the worker handles each one in turn, so a capture, a
//! flush, and a clear can never interleave on disk. The opt-out is the one
//! thing both sides hold, as a shared flag, because it has to take effect
//! the moment the command returns, not when the worker gets round to it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::{Map, Value};
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
    /// The opt-out flag changed. The flag, not the message, is the truth:
    /// the worker clears the queue if the install is opted out by the time
    /// it looks, so toggles that arrive out of order still end right.
    OptOutChanged,
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

/// The longest event name a log line will carry.
const LOGGED_NAME_LEN: usize = 64;

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
    opted_out: Arc<AtomicBool>,
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
            opted_out: Arc::new(AtomicBool::new(opted_out)),
        };
        worker.clear_if_opted_out();
        worker
    }

    /// A `Handle` sharing this worker's opt-out flag, sending on `tx`.
    pub(crate) fn handle_for(&self, tx: Sender<Msg>) -> Handle {
        Handle {
            tx,
            dir: self.store.as_ref().map(|store| store.dir.clone()),
            opted_out: Arc::clone(&self.opted_out),
            toggling: Mutex::new(()),
            warned: AtomicBool::new(false),
        }
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
            Msg::OptOutChanged => self.clear_if_opted_out(),
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

    fn is_opted_out(&self) -> bool {
        self.opted_out.load(Ordering::SeqCst)
    }

    async fn capture(&mut self, event: &Event) {
        // A capture sent just before an opt-out is still dropped here.
        if self.is_opted_out() {
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

    fn clear_if_opted_out(&self) {
        if !self.is_opted_out() {
            return;
        }
        if let Some(store) = &self.store {
            if store.queue.is_empty() {
                return;
            }
            if let Err(e) = store.queue.clear() {
                log::warn!("could not clear the analytics queue: {e}");
            }
        }
    }

    /// Sends the queue unless there is nothing to send, the install opted
    /// out, or a recent failure is still being waited out. The blocking
    /// send runs on its own thread so a panic there comes back as a
    /// `JoinError`, which counts as one more failure rather than the end of
    /// the worker. It checks the opt-out flag between batches, so an
    /// opt-out mid-flush lets at most the batch on the wire land.
    async fn try_flush(&mut self) {
        if self.is_opted_out() {
            self.clear_if_opted_out();
            return;
        }
        let Some(store) = &self.store else {
            return;
        };
        if store.queue.is_empty() || !self.backoff.ready_in().is_zero() {
            return;
        }

        let queue = Arc::clone(&store.queue);
        let post = Arc::clone(&self.post);
        let host = Arc::clone(&self.host);
        let api_key = Arc::clone(&self.api_key);
        let base = Arc::clone(&self.base);
        let stop = Arc::clone(&self.opted_out);
        let sent = tauri::async_runtime::spawn_blocking(move || {
            crate::send::flush(&queue, post.as_ref(), &host, &api_key, &base, &stop)
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
        self.clear_if_opted_out();
    }
}

/// What the commands hold: the way to the worker, the shared opt-out flag,
/// and the data dir to persist that flag in.
pub(crate) struct Handle {
    tx: Sender<Msg>,
    dir: Option<PathBuf>,
    opted_out: Arc<AtomicBool>,
    /// Serialises opt-out toggles, so two in quick succession leave the
    /// flag and its file the way the later one set them.
    toggling: Mutex<()>,
    /// Set once a capture has been dropped for want of a worker, so a busy
    /// or dead worker costs one log line, not one per event.
    warned: AtomicBool,
}

impl Handle {
    /// Queues `name` unless the install is opted out. Never blocks: a full
    /// channel or a dead worker drops the event.
    pub(crate) fn capture(&self, name: &str, properties: Map<String, Value>) {
        if self.is_opted_out() {
            return;
        }
        let Some(event) = Event::new(name, properties) else {
            log::warn!(
                "dropped an event with an unusable name: {:?}",
                name.chars().take(LOGGED_NAME_LEN).collect::<String>()
            );
            return;
        };
        if self.tx.try_send(Msg::Capture(event)).is_err()
            && !self.warned.swap(true, Ordering::Relaxed)
        {
            log::warn!("analytics is not keeping up; dropping events until it does");
        }
    }

    /// Asks for a flush without waiting for it.
    pub(crate) fn flush(&self) {
        let _ = self.tx.try_send(Msg::Flush(None));
    }

    /// Takes effect before it returns: the flag is set (so capture stops
    /// and an in-flight flush stops after its current batch) and saved to
    /// disk here, not in the worker. The worker is only told so it can
    /// clear the queue; if the message cannot be sent, the worker still
    /// sees the flag at its next flush attempt, and the next start clears
    /// the queue for an opt-out it finds on disk.
    pub(crate) fn set_opt_out(&self, out: bool) {
        {
            let _toggling = self.toggling.lock().unwrap_or_else(PoisonError::into_inner);
            self.opted_out.store(out, Ordering::SeqCst);
            if let Some(dir) = &self.dir {
                if let Err(e) = crate::identity::set_opted_out(dir, out) {
                    log::warn!("could not save the analytics opt-out: {e}");
                }
            }
        }
        let _ = self.tx.try_send(Msg::OptOutChanged);
    }

    pub(crate) fn is_opted_out(&self) -> bool {
        self.opted_out.load(Ordering::SeqCst)
    }

    /// See [`flush_before_exit`].
    pub(crate) fn flush_before_exit(&self, limit: Duration) {
        flush_before_exit(&self.tx, limit);
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
    use super::{flush_before_exit, Handle, Msg, Settings, Worker};
    use crate::event::Event;
    use crate::queue::{Limits, Queue};
    use crate::send::Post;
    use serde_json::{Map, Value};
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::{Duration, Instant};
    use tauri::async_runtime::Receiver;

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

    /// Holds its first post on the wire until the test lets it go, saying
    /// when it gets there; later posts go straight through.
    struct BlockingPost {
        posts: Mutex<usize>,
        on_the_wire: Mutex<mpsc::Sender<()>>,
        release: Mutex<mpsc::Receiver<()>>,
    }

    impl Post for BlockingPost {
        fn post(&self, _url: &str, _body: &Value) -> Result<(), String> {
            let first = {
                let mut posts = self.posts.lock().unwrap();
                *posts += 1;
                *posts == 1
            };
            if first {
                self.on_the_wire.lock().unwrap().send(()).unwrap();
                self.release.lock().unwrap().recv().unwrap();
            }
            Ok(())
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

    /// A worker over `dir` with its command-side handle, and the receiving
    /// end for tests that pump messages by hand.
    fn setup(dir: Option<&Path>, post: Arc<dyn Post>) -> (Worker, Handle, Receiver<Msg>) {
        let worker = Worker::new(settings(), dir.map(Path::to_path_buf), post);
        let (tx, rx) = tauri::async_runtime::channel(1_024);
        let handle = worker.handle_for(tx);
        (worker, handle, rx)
    }

    fn worker(dir: &Path, post: &Arc<FakePost>) -> (Worker, Handle, Receiver<Msg>) {
        let post: Arc<dyn Post> = post.clone();
        setup(Some(dir), post)
    }

    /// Hands every message the commands have sent so far to the worker.
    async fn pump(worker: &mut Worker, rx: &mut Receiver<Msg>) {
        while let Ok(msg) = rx.try_recv() {
            worker.handle(msg).await;
        }
    }

    /// The queue file the worker owns, opened here only for inspection.
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

    #[tokio::test]
    async fn twenty_captures_send_exactly_one_batch_of_twenty() {
        let dir = tempfile::tempdir().unwrap();
        let post = FakePost::scripted(vec![]);
        let (mut worker, _, _) = worker(dir.path(), &post);

        capture(&mut worker, 20).await;

        assert_eq!(post.batch_sizes(), vec![20]);
        assert!(queue(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn fewer_than_twenty_wait_for_a_flush() {
        let dir = tempfile::tempdir().unwrap();
        let post = FakePost::scripted(vec![]);
        let (mut worker, _, _) = worker(dir.path(), &post);

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
        let (mut worker, _, _) = worker(dir.path(), &post);

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
        let (mut worker, handle, mut rx) = worker(dir.path(), &post);

        capture(&mut worker, 3).await;
        handle.set_opt_out(true);
        pump(&mut worker, &mut rx).await;
        assert!(queue(dir.path()).is_empty());
        assert!(crate::identity::opted_out(dir.path()));
        assert!(handle.is_opted_out());

        for i in 0..25 {
            handle.capture(&format!("e{i}"), Map::new());
        }
        capture(&mut worker, 25).await;
        pump(&mut worker, &mut rx).await;
        worker.handle(Msg::Flush(None)).await;
        assert!(queue(dir.path()).is_empty());
        assert!(post.batch_sizes().is_empty());

        handle.set_opt_out(false);
        pump(&mut worker, &mut rx).await;
        assert!(!crate::identity::opted_out(dir.path()));
        assert!(!handle.is_opted_out());
        handle.capture("a", Map::new());
        handle.capture("b", Map::new());
        pump(&mut worker, &mut rx).await;
        assert_eq!(queue(dir.path()).len(), 2);
    }

    #[tokio::test]
    async fn a_capture_right_after_opting_out_is_never_queued() {
        let dir = tempfile::tempdir().unwrap();
        let post = FakePost::scripted(vec![]);
        let (mut worker, handle, mut rx) = worker(dir.path(), &post);

        // Sent before the opt-out, handled after it: dropped by the worker.
        handle.capture("before", Map::new());
        handle.set_opt_out(true);
        // Sent after: dropped by the command.
        handle.capture("after", Map::new());
        pump(&mut worker, &mut rx).await;
        worker.handle(Msg::Flush(None)).await;

        assert!(queue(dir.path()).is_empty());
        assert!(post.batch_sizes().is_empty());
    }

    #[tokio::test]
    async fn an_opt_out_on_disk_is_honoured_at_start_and_leaves_nothing_queued() {
        let dir = tempfile::tempdir().unwrap();
        queue(dir.path()).push(&event("left-over")).unwrap();
        crate::identity::set_opted_out(dir.path(), true).unwrap();
        let post = FakePost::scripted(vec![]);

        let (mut worker, handle, _) = worker(dir.path(), &post);

        assert!(handle.is_opted_out());
        assert!(queue(dir.path()).is_empty());
        capture(&mut worker, 1).await;
        assert!(queue(dir.path()).is_empty());
    }

    #[test]
    fn opting_out_mid_flush_takes_effect_at_once_and_stops_after_the_batch_on_the_wire() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..250 {
            queue(dir.path()).push(&event(&format!("e{i}"))).unwrap();
        }
        let (on_the_wire, arrived) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let post = Arc::new(BlockingPost {
            posts: Mutex::new(0),
            on_the_wire: Mutex::new(on_the_wire),
            release: Mutex::new(released),
        });
        let post_dyn: Arc<dyn Post> = post.clone();
        let (worker, handle, rx) = setup(Some(dir.path()), post_dyn);
        tauri::async_runtime::spawn(worker.run(rx, Duration::from_secs(3_600)));

        // The start-up flush is now holding batch 1 on the wire.
        arrived.recv_timeout(Duration::from_secs(5)).unwrap();

        handle.set_opt_out(true);
        assert!(handle.is_opted_out(), "answered while the post is blocked");
        assert!(crate::identity::opted_out(dir.path()), "persisted already");
        handle.capture("after", Map::new());

        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !queue(dir.path()).is_empty() {
            assert!(Instant::now() < deadline, "the queue was never cleared");
            std::thread::sleep(Duration::from_millis(5));
        }
        // One more round trip through the worker, so nothing is in flight.
        handle.flush_before_exit(Duration::from_secs(5));
        assert_eq!(*post.posts.lock().unwrap(), 1);
        assert!(queue(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn a_flush_while_backing_off_makes_no_request() {
        let dir = tempfile::tempdir().unwrap();
        let post = FakePost::scripted(vec![Outcome::Err]);
        let (mut worker, _, _) = worker(dir.path(), &post);

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
        let (mut worker, _, _) = worker(dir.path(), &post);

        capture(&mut worker, 20).await;
        assert_eq!(post.batch_sizes(), vec![20]);
        assert_eq!(queue(dir.path()).len(), 20, "events kept");

        worker.handle(Msg::Flush(None)).await;
        assert_eq!(post.batch_sizes(), vec![20], "backoff engaged");

        // Still working.
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
        let (mut worker, handle, mut rx) = setup(None, post_dyn);

        capture(&mut worker, 25).await;
        worker.handle(Msg::Flush(None)).await;
        assert!(post.batch_sizes().is_empty());

        handle.set_opt_out(true);
        pump(&mut worker, &mut rx).await;
        assert!(handle.is_opted_out());
    }

    #[tokio::test]
    async fn running_flushes_once_at_start_and_again_on_each_tick() {
        let dir = tempfile::tempdir().unwrap();
        queue(dir.path()).push(&event("from-last-run")).unwrap();
        let post = FakePost::scripted(vec![]);
        let (worker, _, _) = worker(dir.path(), &post);
        let (tx, rx) = tauri::async_runtime::channel(16);
        tokio::spawn(worker.run(rx, Duration::from_millis(20)));

        // Answered only after the start-up flush has finished.
        let (done, finished) = mpsc::channel();
        tx.send(Msg::Flush(Some(done))).await.unwrap();
        tokio::task::spawn_blocking(move || finished.recv().unwrap())
            .await
            .unwrap();
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
        let (mut worker, handle, mut rx) = worker(dir.path(), &post);
        // Handles messages but does no start-up flush, so the one post
        // below can only have come from the exit.
        tauri::async_runtime::spawn(async move {
            while let Some(msg) = rx.recv().await {
                worker.handle(msg).await;
            }
        });

        handle.flush_before_exit(Duration::from_secs(2));

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
