//! The read of an app's two settings files. It runs at plugin start, before
//! the webview, and again when JS asks.

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

const MAX_BODY: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Files {
    pub global: Option<String>,
    pub rules: Option<String>,
    pub country: Option<String>,
}

/// A file's body and the named header's value.
pub type Got = Result<(String, Option<String>), String>;

/// One GET. Real HTTP in an app, a fake in tests.
pub trait Fetch: Send + Sync {
    /// # Errors
    ///
    /// Returns an error for a transport failure, a status other than 2xx, or a
    /// body over the cap.
    fn get(&self, url: &str, header: &str) -> Got;
}

/// `<base>/<stage>/global.json` and `<base>/<stage>/v<n>.json`.
#[must_use]
pub fn urls(base: &str, stage: &str, key_version: u32) -> (String, String) {
    let root = format!("{}/{stage}", base.trim_end_matches('/'));
    (
        format!("{root}/global.json"),
        format!("{root}/v{key_version}.json"),
    )
}

/// Two capital letters. Cloudflare sends `XX` for an unknown country and `T1` for Tor.
#[must_use]
pub fn country_of(header: Option<&str>) -> Option<String> {
    header
        .filter(|h| h.len() == 2 && h.bytes().all(|b| b.is_ascii_uppercase()))
        .filter(|h| *h != "XX" && *h != "T1")
        .map(str::to_owned)
}

pub struct Reader {
    urls: (String, String),
    header: String,
    fetch: Arc<dyn Fetch>,
}

impl Reader {
    #[must_use]
    pub fn new(urls: (String, String), header: String, timeout: Duration) -> Self {
        Self::with(urls, header, Arc::new(UreqFetch::new(timeout)))
    }

    #[must_use]
    pub fn with(urls: (String, String), header: String, fetch: Arc<dyn Fetch>) -> Self {
        Self {
            urls,
            header,
            fetch,
        }
    }

    /// Both files at once. A file that fails gives `None` and leaves the other.
    /// Blocks: call it off the async workers.
    #[must_use]
    pub fn read(&self) -> Files {
        let (global, rules) = std::thread::scope(|s| {
            let g = s.spawn(|| self.fetch.get(&self.urls.0, &self.header));
            let r = s.spawn(|| self.fetch.get(&self.urls.1, &self.header));
            (
                g.join().unwrap_or_else(|_| Err("panicked".into())),
                r.join().unwrap_or_else(|_| Err("panicked".into())),
            )
        });
        let (global, global_country) = split(global);
        let (rules, rules_country) = split(rules);
        Files {
            global,
            rules,
            country: global_country.or(rules_country),
        }
    }
}

fn split(r: Got) -> (Option<String>, Option<String>) {
    match r {
        Ok((body, header)) => (Some(body), country_of(header.as_deref())),
        Err(e) => {
            log::debug!("remote-settings: a settings file did not read: {e}");
            (None, None)
        }
    }
}

/// TLS is checked against the bundled Mozilla roots, not the OS: the platform
/// verifier needs JNI on Android, which a plugin with no native code cannot set up.
struct UreqFetch(ureq::Agent);

impl UreqFetch {
    fn new(timeout: Duration) -> Self {
        Self(
            ureq::Agent::config_builder()
                .timeout_global(Some(timeout))
                .build()
                .into(),
        )
    }
}

impl Fetch for UreqFetch {
    fn get(&self, url: &str, header: &str) -> Got {
        let mut response = self.0.get(url).call().map_err(|e| e.to_string())?;
        let country = response
            .headers()
            .get(header)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let declared = response.body().content_length();
        let body = read_capped(declared, response.body_mut().as_reader(), MAX_BODY)?;
        Ok((body, country))
    }
}

fn read_capped(declared: Option<u64>, body: impl Read, cap: u64) -> Result<String, String> {
    if declared.is_some_and(|n| n > cap) {
        return Err(format!("the file is over the {cap}-byte cap"));
    }
    let mut bytes = Vec::new();
    body.take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > cap {
        return Err(format!("the file runs past the {cap}-byte cap"));
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{country_of, urls, Fetch, Files, Got, Reader};
    use std::sync::{Arc, Mutex};

    struct Fake {
        seen: Mutex<Vec<String>>,
        answer: fn(&str) -> Got,
    }

    impl Fetch for Fake {
        fn get(&self, url: &str, _header: &str) -> Got {
            self.seen.lock().unwrap().push(url.to_owned());
            (self.answer)(url)
        }
    }

    fn reader(answer: fn(&str) -> Got) -> (Reader, Arc<Fake>) {
        let fake = Arc::new(Fake {
            seen: Mutex::new(Vec::new()),
            answer,
        });
        let r = Reader::with(
            urls("https://settings.example.com/appa/", "production", 3),
            "x-country".into(),
            fake.clone(),
        );
        (r, fake)
    }

    #[test]
    fn the_urls_are_built_from_base_stage_and_key_version() {
        assert_eq!(
            urls("https://settings.example.com/appa/", "production", 3),
            (
                "https://settings.example.com/appa/production/global.json".to_owned(),
                "https://settings.example.com/appa/production/v3.json".to_owned()
            )
        );
        assert_eq!(
            urls("https://settings.example.com", "staging", 1).0,
            "https://settings.example.com/staging/global.json"
        );
    }

    #[test]
    fn the_country_header_is_read_and_xx_is_none() {
        assert_eq!(country_of(Some("VN")).as_deref(), Some("VN"));
        assert_eq!(country_of(Some("XX")), None);
        assert_eq!(country_of(Some("T1")), None);
        assert_eq!(country_of(Some("vn")), None);
        assert_eq!(country_of(Some("VNM")), None);
        assert_eq!(country_of(None), None);
    }

    #[test]
    fn both_files_and_the_country_are_read() {
        let (r, fake) = reader(|url| {
            Ok((
                if url.ends_with("global.json") {
                    "{}"
                } else {
                    "{\"encryptedFeatures\":\"x\"}"
                }
                .to_owned(),
                Some("VN".to_owned()),
            ))
        });
        assert_eq!(
            r.read(),
            Files {
                global: Some("{}".into()),
                rules: Some("{\"encryptedFeatures\":\"x\"}".into()),
                country: Some("VN".into())
            }
        );
        assert_eq!(fake.seen.lock().unwrap().len(), 2);
    }

    #[test]
    fn one_failed_file_leaves_the_other() {
        let (r, _) = reader(|url| {
            if url.ends_with("global.json") {
                Err("offline".into())
            } else {
                Ok(("{}".into(), Some("US".into())))
            }
        });
        assert_eq!(
            r.read(),
            Files {
                global: None,
                rules: Some("{}".into()),
                country: Some("US".into())
            }
        );
    }

    #[test]
    fn the_files_serialize_for_js() {
        let json = serde_json::to_value(Files {
            global: None,
            rules: Some("r".into()),
            country: None,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "global": null, "rules": "r", "country": null })
        );
    }
}
