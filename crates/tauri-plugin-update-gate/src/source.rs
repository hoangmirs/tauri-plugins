//! Fetches the document over HTTP, keeps it briefly so every check doesn't
//! hit the network, and never lets a failure lock anyone out. The fetch
//! itself is behind the `Fetch` trait so tests can hand back canned
//! responses instead of touching a socket.

use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use log::warn;

use crate::decide::{decide, Document, Gate};

/// The most of a response body the gate will read. The document is a few
/// short strings; anything this large is not it, and reading on would only
/// spend a phone's memory and data on a mistake.
const MAX_BODY: u64 = 64 * 1024;

/// Where to fetch the document from, how long to trust a cached copy, and
/// how long to wait for the network before giving up.
pub struct Config {
    url: String,
    ttl: Duration,
    timeout: Duration,
}

impl Config {
    /// Starts from a URL with the defaults: a 6-hour cache and a 5-second
    /// network timeout.
    pub fn url(u: impl Into<String>) -> Self {
        Self {
            url: u.into(),
            ttl: Duration::from_secs(6 * 60 * 60),
            timeout: Duration::from_secs(5),
        }
    }

    /// Overrides how long a fetched document is trusted before it is
    /// fetched again.
    #[must_use]
    pub fn ttl(mut self, d: Duration) -> Self {
        self.ttl = d;
        self
    }

    /// Overrides how long the fetch waits for a response.
    #[must_use]
    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }
}

/// Fetches the document body as text. Implemented by a real HTTP client in
/// production and by a canned fake in tests, so `Source`'s caching and
/// fail-open behaviour can be tested without a network.
#[async_trait]
pub trait Fetch: Send + Sync {
    async fn get(&self, url: &str) -> Result<String, String>;
}

/// Fetches over HTTP with `ureq`, verifying TLS against the bundled Mozilla
/// roots rather than the OS: the platform verifier needs JNI set up on
/// Android, which a plugin with no native code cannot do, and panics without
/// it. A non-2xx status is a failure, same as a transport error.
struct UreqFetch(ureq::Agent);

impl UreqFetch {
    /// Every request, connect to last byte, is bounded by `timeout`. Building
    /// the agent cannot fail, so there is no fallback client that could
    /// forget the timeout.
    fn new(timeout: Duration) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .build()
            .into();
        Self(agent)
    }
}

#[async_trait]
impl Fetch for UreqFetch {
    async fn get(&self, url: &str) -> Result<String, String> {
        // ureq blocks, so the request runs off the async workers.
        let agent = self.0.clone();
        let url = url.to_owned();
        tauri::async_runtime::spawn_blocking(move || {
            let mut response = agent.get(&url).call().map_err(|e| e.to_string())?;
            let declared = response.body().content_length();
            read_capped(declared, response.body_mut().as_reader(), MAX_BODY)
        })
        .await
        .map_err(|e| e.to_string())?
    }
}

/// Reads `body` as UTF-8, but no more than `cap` bytes of it: a declared
/// length over the cap is refused before a byte is read, and a body that
/// runs past the cap anyway (no length, or a wrong one) is refused as soon
/// as it does.
fn read_capped(declared: Option<u64>, body: impl Read, cap: u64) -> Result<String, String> {
    if let Some(n) = declared.filter(|&n| n > cap) {
        return Err(format!(
            "document declares {n} bytes, over the {cap}-byte cap"
        ));
    }
    let mut bytes = Vec::new();
    body.take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > cap {
        return Err(format!("document runs past the {cap}-byte cap"));
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

/// Fetches the document, caches a successfully parsed copy for `Config::ttl`,
/// and turns any failure into an open gate rather than an error the caller
/// has to handle.
pub struct Source {
    config: Config,
    fetcher: Arc<dyn Fetch>,
    cache: Mutex<Option<(Instant, Document)>>,
}

impl Source {
    /// Builds a `Source` that fetches for real, over `ureq`, honouring
    /// `Config::timeout`.
    #[must_use]
    pub fn new(config: Config) -> Self {
        let fetcher = UreqFetch::new(config.timeout);
        Self::with(config, fetcher)
    }

    /// Builds a `Source` around any `Fetch`, real or canned. Tests use this
    /// to swap in a canned fetcher.
    pub fn with(config: Config, fetcher: impl Fetch + 'static) -> Self {
        Self {
            config,
            fetcher: Arc::new(fetcher),
            cache: Mutex::new(None),
        }
    }

    /// Decides whether `running` may still play. Reuses the cached document
    /// while it's within the TTL; otherwise fetches, parses, and caches a
    /// fresh one. Any failure along the way — network, HTTP status, or
    /// malformed JSON — is logged and answered with an open gate instead of
    /// being handed back to the caller.
    pub async fn gate(&self, running: &str, lang: &str, platform: &str) -> Gate {
        if let Some(doc) = self.cached() {
            return decide(&doc, running, lang, platform);
        }

        match self.fetch_and_cache().await {
            Ok(doc) => decide(&doc, running, lang, platform),
            Err(e) => {
                warn!("update-gate: {e}");
                decide(&Document::default(), running, lang, platform)
            }
        }
    }

    /// Returns the cached document if it's still within the TTL.
    fn cached(&self) -> Option<Document> {
        let cache = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache.as_ref().and_then(|(at, doc)| {
            (Instant::now().saturating_duration_since(*at) < self.config.ttl).then(|| doc.clone())
        })
    }

    /// Fetches and parses the document, caching it on success only. The
    /// fetch runs as its own task so that a panic anywhere in the network
    /// stack comes back as an error here, and so as an open gate, instead
    /// of unwinding through the command and leaving the app unanswered.
    async fn fetch_and_cache(&self) -> Result<Document, String> {
        let fetcher = Arc::clone(&self.fetcher);
        let url = self.config.url.clone();
        let body = tauri::async_runtime::spawn(async move { fetcher.get(&url).await })
            .await
            .map_err(|e| format!("fetch did not finish: {e}"))??;
        let doc: Document = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        *self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((Instant::now(), doc.clone()));
        Ok(doc)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::{read_capped, Config, Fetch, Source};
    use crate::decide::State;

    struct Canned(Result<String, String>, Arc<AtomicUsize>);

    #[async_trait::async_trait]
    impl Fetch for Canned {
        async fn get(&self, _url: &str) -> Result<String, String> {
            self.1.fetch_add(1, Ordering::SeqCst);
            self.0.clone()
        }
    }

    struct Panics;

    #[async_trait::async_trait]
    impl Fetch for Panics {
        async fn get(&self, _url: &str) -> Result<String, String> {
            panic!("the network stack fell over");
        }
    }

    fn n() -> Arc<AtomicUsize> {
        Arc::new(AtomicUsize::new(0))
    }

    #[tokio::test]
    async fn a_network_error_opens_the_gate() {
        let s = Source::with(Config::url("http://x"), Canned(Err("offline".into()), n()));
        assert!(matches!(
            s.gate("1.0.0", "vi", "ios").await.state,
            State::Ok
        ));
    }

    #[tokio::test]
    async fn a_fetch_that_panics_still_opens_the_gate() {
        let s = Source::with(Config::url("http://x"), Panics);
        assert!(matches!(
            s.gate("1.0.0", "vi", "ios").await.state,
            State::Ok
        ));
    }

    #[tokio::test]
    async fn malformed_json_opens_the_gate() {
        let s = Source::with(
            Config::url("http://x"),
            Canned(Ok("<html>404</html>".into()), n()),
        );
        assert!(matches!(
            s.gate("1.0.0", "vi", "ios").await.state,
            State::Ok
        ));
    }

    #[tokio::test]
    async fn it_fetches_once_inside_the_ttl() {
        let calls = n();
        let s = Source::with(
            Config::url("http://x"),
            Canned(Ok(r#"{"minVersion":"2.0.0"}"#.into()), calls.clone()),
        );
        assert!(matches!(
            s.gate("1.0.0", "vi", "ios").await.state,
            State::Forced
        ));
        assert!(matches!(
            s.gate("1.0.0", "vi", "ios").await.state,
            State::Forced
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn it_fetches_again_once_the_ttl_has_passed() {
        let calls = n();
        let s = Source::with(
            Config::url("http://x").ttl(std::time::Duration::ZERO),
            Canned(Ok(r#"{"minVersion":"2.0.0"}"#.into()), calls.clone()),
        );
        s.gate("1.0.0", "vi", "ios").await;
        s.gate("1.0.0", "vi", "ios").await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_body_within_the_cap_is_read_whole() {
        let body = r#"{"minVersion":"2.0.0"}"#;
        assert_eq!(read_capped(None, body.as_bytes(), 64).unwrap(), body);
        assert_eq!(read_capped(Some(4), &b"abcd"[..], 4).unwrap(), "abcd");
    }

    #[test]
    fn a_declared_length_over_the_cap_is_refused_before_reading() {
        struct Unread;
        impl std::io::Read for Unread {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                panic!("read a body its header already ruled out");
            }
        }
        assert!(read_capped(Some(5), Unread, 4).is_err());
    }

    #[test]
    fn a_body_that_outgrows_the_cap_is_refused() {
        // No content-length, or one that understates the body.
        assert!(read_capped(None, &b"abcde"[..], 4).is_err());
        assert!(read_capped(Some(1), &b"abcde"[..], 4).is_err());
    }

    #[test]
    fn a_body_that_is_not_utf8_is_refused() {
        assert!(read_capped(None, &[0xff, 0xfe][..], 4).is_err());
    }
}
