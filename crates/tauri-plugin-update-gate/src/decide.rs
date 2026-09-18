//! The gate's decision is a pure function: given the document and the
//! running version, there is no reason to touch the network or the
//! filesystem. Keeping it pure is what lets the interesting logic be
//! tested without a server.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// The document as read from the update server, parsed loosely: every field
/// is optional so a document the app doesn't fully recognise yet still opens
/// the gate instead of failing to parse.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Document {
    pub min_version: Option<String>,
    pub latest_version: Option<String>,
    pub message: HashMap<String, String>,
    pub url: HashMap<String, Option<String>>,
}

/// Where the running copy stands relative to the document's versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Runs the latest known version, or newer.
    Ok,
    /// Runs an older version, but not below `minVersion`.
    Optional,
    /// Runs a version below `minVersion`.
    Forced,
}

/// The gate's verdict, shaped for direct serialisation to the app.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gate {
    pub state: State,
    pub latest_version: Option<String>,
    pub message: Option<String>,
    pub url: Option<String>,
}

/// Decides whether `running` may still play, against `doc`. Fails open: a
/// document field that doesn't parse as a version is treated as if it were
/// absent rather than as grounds to lock someone out, because a malformed
/// document is a server mistake, not the running app's fault.
#[must_use]
pub fn decide(doc: &Document, running: &str, lang: &str, platform: &str) -> Gate {
    let latest_version = doc.latest_version.clone();

    let parse = |v: &Option<String>| -> Result<Option<semver::Version>, semver::Error> {
        v.as_deref().map(semver::Version::parse).transpose()
    };

    let (Ok(running), Ok(min), Ok(latest)) = (
        semver::Version::parse(running),
        parse(&doc.min_version),
        parse(&doc.latest_version),
    ) else {
        return Gate {
            state: State::Ok,
            latest_version,
            message: None,
            url: None,
        };
    };

    let state = if min.is_some_and(|min| running < min) {
        State::Forced
    } else if latest.is_some_and(|latest| running < latest) {
        State::Optional
    } else {
        State::Ok
    };

    let message = doc
        .message
        .get(lang)
        .or_else(|| doc.message.get("en"))
        .cloned();
    let url = doc.url.get(platform).cloned().flatten();

    Gate {
        state,
        latest_version,
        message,
        url,
    }
}

#[cfg(test)]
mod tests {
    use super::{decide, Document, Gate, State};

    fn doc(s: &str) -> Document {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn below_min_is_forced() {
        let g = decide(
            &doc(r#"{"minVersion":"1.2.0","latestVersion":"1.4.0"}"#),
            "1.1.9",
            "vi",
            "ios",
        );
        assert!(matches!(g.state, State::Forced));
    }

    #[test]
    fn between_min_and_latest_is_optional() {
        let g = decide(
            &doc(r#"{"minVersion":"1.2.0","latestVersion":"1.4.0"}"#),
            "1.3.0",
            "vi",
            "ios",
        );
        assert!(matches!(g.state, State::Optional));
    }

    #[test]
    fn newest_is_ok() {
        let g = decide(
            &doc(r#"{"minVersion":"1.2.0","latestVersion":"1.4.0"}"#),
            "1.4.0",
            "vi",
            "ios",
        );
        assert!(matches!(g.state, State::Ok));
    }

    #[test]
    fn a_prerelease_is_older_than_its_release() {
        let g = decide(
            &doc(r#"{"minVersion":"1.2.0"}"#),
            "1.2.0-alpha.1",
            "vi",
            "ios",
        );
        assert!(matches!(g.state, State::Forced));
    }

    #[test]
    fn an_unreadable_version_opens_the_gate() {
        let g = decide(
            &doc(r#"{"minVersion":"not a version"}"#),
            "1.0.0",
            "vi",
            "ios",
        );
        assert!(matches!(g.state, State::Ok));
        let g = decide(&doc(r#"{"minVersion":"9.0.0"}"#), "nonsense", "vi", "ios");
        assert!(matches!(g.state, State::Ok));
    }

    #[test]
    fn an_empty_document_opens_the_gate() {
        let g = decide(&doc("{}"), "1.0.0", "vi", "ios");
        assert!(matches!(g.state, State::Ok));
        assert!(g.message.is_none());
    }

    #[test]
    fn it_picks_the_language_and_falls_back_to_english() {
        let d = doc(r#"{"minVersion":"2.0.0","message":{"vi":"Cập nhật nha","en":"Update"}}"#);
        assert_eq!(
            decide(&d, "1.0.0", "vi", "ios").message.as_deref(),
            Some("Cập nhật nha")
        );
        assert_eq!(
            decide(&d, "1.0.0", "fr", "ios").message.as_deref(),
            Some("Update")
        );
    }

    #[test]
    fn it_picks_the_url_for_this_platform_and_tolerates_null() {
        let d = doc(
            r#"{"minVersion":"2.0.0","url":{"ios":"https://apps.apple.com/x","android":null}}"#,
        );
        assert_eq!(
            decide(&d, "1.0.0", "vi", "ios").url.as_deref(),
            Some("https://apps.apple.com/x")
        );
        assert_eq!(decide(&d, "1.0.0", "vi", "android").url, None);
        assert_eq!(decide(&d, "1.0.0", "vi", "macos").url, None);
    }

    #[test]
    fn gate_serialises_to_camel_case_with_lowercase_state() {
        let g = Gate {
            state: State::Optional,
            latest_version: Some("1.4.0".to_string()),
            message: Some("Update".to_string()),
            url: Some("https://example.com".to_string()),
        };
        let json = serde_json::to_value(&g).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "state": "optional",
                "latestVersion": "1.4.0",
                "message": "Update",
                "url": "https://example.com",
            })
        );
    }
}
