//! Fetches the document over HTTP, keeps it briefly so every check doesn't
//! hit the network, and never lets a failure lock anyone out. The fetch
//! itself is behind the `Fetch` trait so tests can hand back canned
//! responses instead of touching a socket.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use log::warn;

use crate::decide::{decide, Document, Gate};

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

/// Fetches over HTTP with `reqwest`. A non-2xx status is treated as a
/// failure, same as a transport error: the caller only cares whether it got
/// a usable body.
struct ReqwestFetch(reqwest::Client);

#[async_trait]
impl Fetch for ReqwestFetch {
    async fn get(&self, url: &str) -> Result<String, String> {
        let response = self
            .0
            .get(url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        response.text().await.map_err(|e| e.to_string())
    }
}

/// Fetches the document, caches a successfully parsed copy for `Config::ttl`,
/// and turns any failure into an open gate rather than an error the caller
/// has to handle.
pub struct Source {
    config: Config,
    fetcher: Box<dyn Fetch>,
    cache: Mutex<Option<(Instant, Document)>>,
}

impl Source {
    /// Builds a `Source` that fetches for real, over `reqwest`, honouring
    /// `Config::timeout`.
    #[must_use]
    pub fn new(config: Config) -> Self {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .unwrap_or_default();
        Self::with(config, ReqwestFetch(client))
    }

    /// Builds a `Source` around any `Fetch`, real or canned. Tests use this
    /// to swap in a canned fetcher.
    pub fn with(config: Config, fetcher: impl Fetch + 'static) -> Self {
        Self {
            config,
            fetcher: Box::new(fetcher),
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

    /// Fetches and parses the document, caching it on success only.
    async fn fetch_and_cache(&self) -> Result<Document, String> {
        let body = self.fetcher.get(&self.config.url).await?;
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

    use super::{Config, Fetch, Source};
    use crate::decide::State;

    struct Canned(Result<String, String>, Arc<AtomicUsize>);

    #[async_trait::async_trait]
    impl Fetch for Canned {
        async fn get(&self, _url: &str) -> Result<String, String> {
            self.1.fetch_add(1, Ordering::SeqCst);
            self.0.clone()
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
}
