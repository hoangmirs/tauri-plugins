//! The gate's decision is a pure function: given the document and the
//! running version, there is no reason to touch the network or the
//! filesystem. Keeping it pure is what lets the interesting logic be
//! tested without a server.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// The document as read from the update server, parsed loosely: every field
/// is optional so a document the app doesn't fully recognise yet still opens
/// the gate instead of failing to parse.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Document {
    pub min_version: Option<String>,
    pub latest_version: Option<String>,
    pub message: HashMap<String, String>,
    pub url: HashMap<String, Option<String>>,
    /// Versions one platform holds apart from the rest. The stores release on
    /// different days: the floor can rise everywhere while iOS still waits in
    /// review. A `null` entry is the same as none.
    pub platforms: HashMap<String, Option<Override>>,
}

/// One platform's own versions. A field it leaves out comes from the top level.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Override {
    pub min_version: Option<String>,
    pub latest_version: Option<String>,
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
    let own = doc.platforms.get(platform).and_then(Option::as_ref);
    let min_version = own
        .and_then(|o| o.min_version.as_ref())
        .or(doc.min_version.as_ref());
    let latest_version = own
        .and_then(|o| o.latest_version.as_ref())
        .or(doc.latest_version.as_ref())
        .cloned();

    let parse = |v: Option<&String>| -> Result<Option<semver::Version>, semver::Error> {
        v.map(|v| semver::Version::parse(v)).transpose()
    };

    let (Ok(running), Ok(min), Ok(latest)) = (
        semver::Version::parse(running),
        parse(min_version),
        parse(latest_version.as_ref()),
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

    /// The top level raised to 1.3.0 everywhere, with iOS held at 1.2.0
    /// while its build waits in review.
    const HELD_BACK: &str = r#"{"minVersion":"1.3.0","latestVersion":"1.4.0",
        "platforms":{"ios":{"minVersion":"1.2.0"}}}"#;

    #[test]
    fn a_platform_can_hold_its_floor_below_the_rest() {
        let d = doc(HELD_BACK);
        assert_eq!(decide(&d, "1.2.5", "vi", "ios").state, State::Optional);
        assert_eq!(decide(&d, "1.2.5", "vi", "android").state, State::Forced);
    }

    #[test]
    fn a_platform_can_hold_its_offer_back_and_reports_its_own_latest() {
        let d = doc(r#"{"minVersion":"1.0.0","latestVersion":"1.4.0",
                "platforms":{"ios":{"latestVersion":"1.3.0"}}}"#);
        let ios = decide(&d, "1.3.0", "vi", "ios");
        assert_eq!(ios.state, State::Ok);
        assert_eq!(ios.latest_version.as_deref(), Some("1.3.0"));
        let android = decide(&d, "1.3.0", "vi", "android");
        assert_eq!(android.state, State::Optional);
        assert_eq!(android.latest_version.as_deref(), Some("1.4.0"));
    }

    #[test]
    fn an_override_leaves_the_field_it_does_not_name_to_the_top_level() {
        let d = doc(HELD_BACK);
        // iOS overrides only the floor, so the offer still comes from 1.4.0.
        assert_eq!(decide(&d, "1.3.0", "vi", "ios").state, State::Optional);
        assert_eq!(
            decide(&d, "1.3.0", "vi", "ios").latest_version.as_deref(),
            Some("1.4.0")
        );
    }

    #[test]
    fn a_null_or_empty_override_means_the_top_level() {
        for platforms in [r#"{"ios":null}"#, r#"{"ios":{}}"#] {
            let d = doc(&format!(
                r#"{{"minVersion":"1.3.0","platforms":{platforms}}}"#
            ));
            assert_eq!(decide(&d, "1.2.0", "vi", "ios").state, State::Forced);
        }
    }

    #[test]
    fn an_unreadable_override_opens_the_gate_on_that_platform_only() {
        let d = doc(r#"{"minVersion":"1.3.0",
                "platforms":{"ios":{"minVersion":"soon"}}}"#);
        assert_eq!(decide(&d, "1.2.0", "vi", "ios").state, State::Ok);
        assert_eq!(decide(&d, "1.2.0", "vi", "android").state, State::Forced);
    }
}
