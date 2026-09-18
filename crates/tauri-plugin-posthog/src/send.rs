//! Sends the queue's events to `PostHog` in batches, through a `Post` seam
//! so tests never touch a network, and tracks how long to wait after a
//! failed send before trying again.

use std::io::Read;
use std::time::{Duration, Instant};

use crate::event::{batch_body, Base, Event};
use crate::queue::Queue;

/// The most events a single request carries.
pub const BATCH: usize = 100;

/// How long a single request is allowed to run before it's treated as a
/// failure.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// The most of a response body `UreqPost` will read. `PostHog`'s `/batch`
/// response is a tiny JSON object; anything past this is not worth reading,
/// and never worth logging.
const MAX_RESPONSE_BODY: u64 = 4 * 1024;

/// How long the first retry waits after a failed send.
const INITIAL_BACKOFF: Duration = Duration::from_secs(30);

/// The longest a retry ever waits, however many failures in a row.
const MAX_BACKOFF: Duration = Duration::from_secs(600);

/// Sends one already-built request body to `url`, blocking until it either
/// succeeds or fails. Implemented for real with `ureq`, and with a fake in
/// tests so `flush` and `Backoff` can be tested with no network.
pub trait Post: Send + Sync {
    /// # Errors
    ///
    /// Returns an error, as a human-readable string, for a transport
    /// failure, a non-2xx response, or anything else that keeps the batch
    /// from being accepted. Never panics: a failure here is reported, not
    /// unwound.
    fn post(&self, url: &str, body: &serde_json::Value) -> Result<(), String>;
}

/// Sends over HTTP with `ureq`, verifying TLS against the bundled Mozilla
/// roots rather than the OS: the platform verifier needs JNI set up on
/// Android, which a plugin with no native code cannot do, and panics
/// without it. A non-2xx status is a failure, same as a transport error,
/// because `ureq`'s default `http_status_as_error` is left on.
pub struct UreqPost(ureq::Agent);

impl UreqPost {
    /// Every request, connect to last byte, is bounded by
    /// [`REQUEST_TIMEOUT`]. Building the agent cannot fail, so there is no
    /// fallback client that could forget the timeout.
    #[must_use]
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .build()
            .into();
        Self(agent)
    }
}

impl Default for UreqPost {
    fn default() -> Self {
        Self::new()
    }
}

impl Post for UreqPost {
    fn post(&self, url: &str, body: &serde_json::Value) -> Result<(), String> {
        let payload = serde_json::to_string(body).map_err(|e| e.to_string())?;
        let mut response = self
            .0
            .post(url)
            .header("Content-Type", "application/json")
            .send(&payload)
            .map_err(|e| e.to_string())?;

        // The response body is read, capped, and thrown away: a success
        // doesn't need it, and it must never end up in a log line.
        let mut buf = Vec::new();
        let _ = std::io::copy(
            &mut response.body_mut().as_reader().take(MAX_RESPONSE_BODY),
            &mut buf,
        );

        Ok(())
    }
}

/// Builds the `/batch` endpoint URL for `host`, exactly one slash between
/// them whether `host` ends with a trailing slash or not.
#[must_use]
pub fn batch_url(host: &str) -> String {
    format!("{}/batch/", host.trim_end_matches('/'))
}

/// Sends everything currently in `queue`, in batches of at most [`BATCH`]
/// events, to `{host}/batch/` (via `batch_url`). Stops and returns `Err` on
/// the first failed batch, leaving it and everything after it queued; each
/// batch that posted successfully is removed from the queue before the next
/// one is sent. Returns the total number of events sent on success.
///
/// # Errors
///
/// Returns the underlying error, as a string, from a failed `post` call, or
/// from a failed `drop_front` after a post that did succeed — in the latter
/// case the events stay queued and will be resent, which `PostHog`
/// de-duplicates by `uuid`.
pub fn flush(
    queue: &Queue,
    post: &dyn Post,
    host: &str,
    api_key: &str,
    base: &Base,
) -> Result<usize, String> {
    let url = batch_url(host);
    let mut sent = 0usize;

    loop {
        let batch: Vec<Event> = queue.peek(BATCH);
        if batch.is_empty() {
            return Ok(sent);
        }

        let body = batch_body(api_key, base, &batch);
        post.post(&url, &body)?;

        queue
            .drop_front(batch.len())
            .map_err(|e| format!("sent a batch but could not remove it from the queue: {e}"))?;

        sent += batch.len();
    }
}

/// Tracks how long to wait before the next retry after a failed send: 30 s
/// on the first failure, doubling on each one after that, capped at 10
/// minutes. Any success resets it back to no wait at all.
///
/// The clock is `Instant`-based so tests can check `ready_in` without
/// sleeping, by passing a `now` of their own choosing to `ready_in_at`.
pub struct Backoff {
    /// The wait that the *next* `failed()` call will return and remember.
    next: Duration,
    /// When the most recent failure happened, and how long it decided to
    /// wait — `None` once `succeeded()` has reset it, or before any
    /// failure at all.
    waiting_since: Option<(Instant, Duration)>,
}

impl Backoff {
    /// A fresh backoff: no failures yet, so `ready_in()` is zero.
    #[must_use]
    pub fn new() -> Self {
        Backoff {
            next: INITIAL_BACKOFF,
            waiting_since: None,
        }
    }

    /// Records a failure now, and returns how long to wait before trying
    /// again. The wait doubles each call, capped at [`MAX_BACKOFF`].
    pub fn failed(&mut self) -> Duration {
        let wait = self.next;
        self.waiting_since = Some((Instant::now(), wait));
        self.next = (self.next * 2).min(MAX_BACKOFF);
        wait
    }

    /// Resets the backoff: the next failure will wait
    /// [`INITIAL_BACKOFF`] again, and `ready_in()` is zero until then.
    pub fn succeeded(&mut self) {
        self.next = INITIAL_BACKOFF;
        self.waiting_since = None;
    }

    /// How much longer to wait, measured from now, before the next attempt
    /// should be made. Zero once the last recorded wait has elapsed, or
    /// when there has been no failure since the last success.
    #[must_use]
    pub fn ready_in(&self) -> Duration {
        self.ready_in_at(Instant::now())
    }

    /// As `ready_in`, but measured from `now` rather than the real clock,
    /// so tests can check it without sleeping.
    #[must_use]
    pub fn ready_in_at(&self, now: Instant) -> Duration {
        let Some((since, wait)) = self.waiting_since else {
            return Duration::ZERO;
        };
        let elapsed = now.saturating_duration_since(since);
        wait.saturating_sub(elapsed)
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{batch_url, flush, Backoff, Post};
    use crate::event::Base;
    use crate::queue::{Limits, Queue};
    use serde_json::{Map, Value};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    fn base() -> Base {
        Base {
            distinct_id: "install-1".to_string(),
            lib_version: "0.1.0-alpha.1",
            app_version: "1.2.3".to_string(),
            os: "macOS",
            platform: "macos",
        }
    }

    fn push_n(queue: &Queue, n: usize) {
        for i in 0..n {
            let event = crate::event::Event::new(&format!("e{i}"), Map::new()).unwrap();
            queue.push(&event).unwrap();
        }
    }

    /// A `Post` that records every body it was asked to send, and replays a
    /// scripted sequence of results, one per call. Panics if called more
    /// times than the script provides for — that would mean `flush` sent
    /// more requests than the test expected.
    struct FakePost {
        results: Mutex<Vec<Result<(), String>>>,
        bodies: Mutex<Vec<Value>>,
    }

    impl FakePost {
        fn new(results: Vec<Result<(), String>>) -> Self {
            FakePost {
                results: Mutex::new(results),
                bodies: Mutex::new(Vec::new()),
            }
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
            let mut results = self.results.lock().unwrap();
            assert!(
                !results.is_empty(),
                "flush made more requests than the test scripted"
            );
            results.remove(0)
        }
    }

    #[test]
    fn flush_sends_everything_in_batches_of_at_most_100_and_empties_the_queue() {
        let dir = tempfile::tempdir().unwrap();
        let queue = Queue::open(dir.path().join("queue.jsonl"), Limits::default());
        push_n(&queue, 250);

        let post = FakePost::new(vec![Ok(()), Ok(()), Ok(())]);
        let sent = flush(&queue, &post, "https://us.i.posthog.com", "k", &base()).unwrap();

        assert_eq!(sent, 250);
        assert_eq!(post.batch_sizes(), vec![100, 100, 50]);
        assert!(queue.is_empty());
    }

    #[test]
    fn a_failed_post_stops_the_flush_and_keeps_that_batch_and_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let queue = Queue::open(dir.path().join("queue.jsonl"), Limits::default());
        push_n(&queue, 250);

        let post = FakePost::new(vec![Ok(()), Err("boom".to_string())]);
        let err = flush(&queue, &post, "https://us.i.posthog.com", "k", &base()).unwrap_err();

        assert_eq!(err, "boom");
        assert_eq!(queue.len(), 150);
        assert_eq!(post.batch_sizes(), vec![100, 100]);
    }

    #[test]
    fn an_empty_queue_makes_no_request() {
        let dir = tempfile::tempdir().unwrap();
        let queue = Queue::open(dir.path().join("queue.jsonl"), Limits::default());

        let post = FakePost::new(vec![]);
        let sent = flush(&queue, &post, "https://us.i.posthog.com", "k", &base()).unwrap();

        assert_eq!(sent, 0);
        assert!(post.bodies.lock().unwrap().is_empty());
    }

    #[test]
    fn batch_url_is_exactly_one_slash_between_host_and_batch() {
        assert_eq!(
            batch_url("https://us.i.posthog.com"),
            "https://us.i.posthog.com/batch/"
        );
        assert_eq!(
            batch_url("https://us.i.posthog.com/"),
            "https://us.i.posthog.com/batch/"
        );
    }

    #[test]
    fn backoff_starts_ready() {
        let backoff = Backoff::new();
        assert_eq!(backoff.ready_in(), Duration::ZERO);
    }

    #[test]
    fn backoff_doubles_then_caps() {
        let mut backoff = Backoff::new();
        assert_eq!(backoff.failed(), Duration::from_secs(30));
        assert_eq!(backoff.failed(), Duration::from_secs(60));
        assert_eq!(backoff.failed(), Duration::from_secs(120));
        assert_eq!(backoff.failed(), Duration::from_secs(240));
        assert_eq!(backoff.failed(), Duration::from_secs(480));
        assert_eq!(backoff.failed(), Duration::from_secs(600));
        assert_eq!(backoff.failed(), Duration::from_secs(600));
    }

    #[test]
    fn backoff_succeeded_resets_to_zero() {
        let mut backoff = Backoff::new();
        backoff.failed();
        backoff.failed();
        backoff.succeeded();
        assert_eq!(backoff.ready_in(), Duration::ZERO);
        assert_eq!(backoff.failed(), Duration::from_secs(30));
    }

    #[test]
    fn ready_in_counts_down_from_the_last_failure() {
        // `now` is taken *before* `failed()` records its own instant, so
        // that instant is always a little later than `now` and
        // `ready_in_at(now)` reports the full wait with no wall-clock
        // flakiness — `saturating_duration_since` floors the (negative)
        // elapsed time at zero for it, same as for every `now` before the
        // failure was recorded.
        let now = Instant::now();
        let mut backoff = Backoff::new();
        let wait = backoff.failed();
        assert_eq!(wait, Duration::from_secs(30));

        assert_eq!(backoff.ready_in_at(now), Duration::from_secs(30));
        assert_eq!(
            backoff.ready_in_at(now.checked_sub(Duration::from_secs(10)).unwrap()),
            Duration::from_secs(30)
        );
        assert_eq!(
            backoff.ready_in_at(now + Duration::from_secs(999)),
            Duration::ZERO
        );

        // Ten seconds in, about twenty should remain — "about" because
        // `failed()` recorded its own instant a hair after `now`, so the
        // true remaining wait is at least (never less than) twenty seconds.
        let ten_seconds_later = backoff.ready_in_at(now + Duration::from_secs(10));
        assert!(ten_seconds_later >= Duration::from_secs(20));
        assert!(ten_seconds_later <= Duration::from_secs(20) + Duration::from_millis(500));
    }
}
