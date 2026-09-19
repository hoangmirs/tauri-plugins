//! The one payload both the queue and the flush path build: a `PostHog`
//! capture event, and the batch request body that carries a slice of them.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

/// The plugin's own name, as `PostHog`'s `$lib` property expects it.
const LIB_NAME: &str = "tauri-plugin-posthog-anon";

/// The longest event name `PostHog` will see from this plugin. Not a
/// `PostHog`-imposed limit, just a sanity bound against accidental payloads.
const MAX_EVENT_NAME_LEN: usize = 200;

/// Event names `PostHog` treats as tying an install to a person.
const IDENTITY_EVENTS: [&str; 4] = [
    "$identify",
    "$create_alias",
    "$merge_dangerously",
    "$groupidentify",
];

/// Properties `PostHog` reads as person or group data, whatever the event.
const IDENTITY_PROPERTIES: [&str; 5] = [
    "$set",
    "$set_once",
    "$unset",
    "$groups",
    "$anon_distinct_id",
];

/// A single analytics event, queued on disk until it is flushed. Carries
/// its own identity (`uuid`) and the moment it happened (`timestamp`) so
/// that queuing and retrying never changes either.
// `event` is PostHog's own name for the field, and the queue stores it as is.
#[allow(clippy::struct_field_names)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub uuid: String,
    pub event: String,
    pub timestamp: String,
    pub properties: Map<String, Value>,
}

impl Event {
    /// Builds an event stamped with a fresh v4 UUID and the current UTC
    /// time. Returns `None` when `name`, trimmed, is empty or longer than
    /// `MAX_EVENT_NAME_LEN` — callers pass event names as literals or
    /// simple identifiers, never free text, so this is a sanity check, not
    /// validation of user input — or is one of `PostHog`'s identity events,
    /// which would undo the anonymity; or, which no real clock does, when
    /// the time cannot be formatted.
    #[must_use]
    pub fn new(name: &str, properties: Map<String, Value>) -> Option<Event> {
        let name = name.trim();
        if name.is_empty()
            || name.chars().count() > MAX_EVENT_NAME_LEN
            || IDENTITY_EVENTS.contains(&name)
        {
            return None;
        }

        let timestamp = OffsetDateTime::now_utc().format(&Rfc3339).ok()?;

        Some(Event {
            uuid: Uuid::new_v4().to_string(),
            event: name.to_string(),
            timestamp,
            properties,
        })
    }
}

/// What the plugin knows about the install and the app, independent of any
/// single event: who (anonymously) is sending, and from where.
pub struct Base {
    pub distinct_id: String,
    pub lib_version: &'static str,
    pub app_version: String,
    pub os: &'static str,
    pub platform: &'static str,
}

/// Builds a `PostHog` `/batch` request body for `events`, keyed to `api_key`.
/// Each event's properties are `base`'s properties, then the event's own,
/// then `distinct_id` and `$process_person_profile: false` re-asserted last
/// — so a caller can add its own properties, or even shadow most of the
/// plugin's, but never unmask the anonymous install or opt an event into
/// person profiles. The worker already merged `base` in at capture, so
/// the capture-time values win here; merging again only fills in events
/// queued before that.
#[must_use]
pub fn batch_body(api_key: &str, base: &Base, events: &[Event]) -> Value {
    let batch: Vec<Value> = events
        .iter()
        .map(|event| {
            json!({
                "event": event.event,
                "uuid": event.uuid,
                "timestamp": event.timestamp,
                "properties": merged_properties(base, event),
            })
        })
        .collect();

    json!({
        "api_key": api_key,
        "batch": batch,
    })
}

/// Merges an event's properties onto `base`'s the way `batch_body` promises:
/// base properties first, then the caller's own (which may shadow most of
/// them) less any that would build a person, then `distinct_id` and
/// `$process_person_profile` re-asserted so neither can be overridden.
pub(crate) fn merged_properties(base: &Base, event: &Event) -> Map<String, Value> {
    let mut properties = Map::new();
    properties.insert("distinct_id".to_string(), base.distinct_id.clone().into());
    properties.insert("$lib".to_string(), LIB_NAME.into());
    properties.insert("$lib_version".to_string(), base.lib_version.into());
    properties.insert("$app_version".to_string(), base.app_version.clone().into());
    properties.insert("$os".to_string(), base.os.into());
    properties.insert("platform".to_string(), base.platform.into());
    properties.insert("$process_person_profile".to_string(), false.into());

    for (key, value) in &event.properties {
        if !IDENTITY_PROPERTIES.contains(&key.as_str()) {
            properties.insert(key.clone(), value.clone());
        }
    }

    properties.insert("distinct_id".to_string(), base.distinct_id.clone().into());
    properties.insert("$process_person_profile".to_string(), false.into());

    properties
}

#[cfg(test)]
mod tests {
    use super::{batch_body, Base, Event};
    use serde_json::Map;

    fn base() -> Base {
        Base {
            distinct_id: "install-1".to_string(),
            lib_version: "0.1.0-alpha.1",
            app_version: "1.2.3".to_string(),
            os: "iOS",
            platform: "ios",
        }
    }

    #[test]
    fn an_event_gets_a_uuid_and_a_utc_timestamp_when_it_is_made() {
        let e = Event::new("game_finished", Map::new()).unwrap();
        assert_eq!(uuid::Uuid::parse_str(&e.uuid).unwrap().get_version_num(), 4);
        assert!(e.timestamp.ends_with('Z'));
        assert!(time::OffsetDateTime::parse(
            &e.timestamp,
            &time::format_description::well_known::Rfc3339
        )
        .is_ok());
    }

    #[test]
    fn a_blank_or_overlong_name_is_refused() {
        assert!(Event::new("", Map::new()).is_none());
        assert!(Event::new("   ", Map::new()).is_none());
        assert!(Event::new(&"x".repeat(201), Map::new()).is_none());
        assert!(Event::new(&"x".repeat(200), Map::new()).is_some());
    }

    #[test]
    fn the_names_that_would_identify_a_person_are_refused() {
        for name in [
            "$identify",
            "$create_alias",
            "$merge_dangerously",
            "$groupidentify",
        ] {
            assert!(Event::new(name, Map::new()).is_none(), "{name}");
        }
        assert!(Event::new("$pageview", Map::new()).is_some());
    }

    #[test]
    fn the_properties_that_would_build_a_person_are_stripped() {
        let mut props = Map::new();
        for key in [
            "$set",
            "$set_once",
            "$unset",
            "$groups",
            "$anon_distinct_id",
        ] {
            props.insert(key.into(), serde_json::json!({ "name": "someone" }));
        }
        props.insert("game".into(), "caro".into());
        let body = batch_body("k", &base(), &[Event::new("x", props).unwrap()]);
        let p = body["batch"][0]["properties"].as_object().unwrap();
        for key in [
            "$set",
            "$set_once",
            "$unset",
            "$groups",
            "$anon_distinct_id",
        ] {
            assert!(!p.contains_key(key), "{key}");
        }
        assert_eq!(p["game"], "caro");
    }

    #[test]
    fn the_body_carries_the_key_and_every_event_with_its_own_uuid_and_time() {
        let a = Event::new("room_created", Map::new()).unwrap();
        let b = Event::new("game_started", Map::new()).unwrap();
        let body = batch_body("phc_test", &base(), &[a.clone(), b.clone()]);
        assert_eq!(body["api_key"], "phc_test");
        assert_eq!(body["batch"][0]["event"], "room_created");
        assert_eq!(body["batch"][0]["uuid"], a.uuid);
        assert_eq!(body["batch"][1]["timestamp"], b.timestamp);
    }

    #[test]
    fn every_event_is_anonymous_and_says_where_it_came_from() {
        let body = batch_body("k", &base(), &[Event::new("x", Map::new()).unwrap()]);
        let p = &body["batch"][0]["properties"];
        assert_eq!(p["distinct_id"], "install-1");
        assert_eq!(p["$process_person_profile"], false);
        assert_eq!(p["$lib"], "tauri-plugin-posthog-anon");
        assert_eq!(p["$app_version"], "1.2.3");
        assert_eq!(p["$os"], "iOS");
        assert_eq!(p["platform"], "ios");
    }

    #[test]
    fn a_caller_cannot_unmask_the_player_but_may_add_its_own_properties() {
        let mut props = Map::new();
        props.insert("distinct_id".into(), "someone@example.com".into());
        props.insert("$process_person_profile".into(), true.into());
        props.insert("game".into(), "caro".into());
        props.insert("$os".into(), "custom".into());
        let body = batch_body("k", &base(), &[Event::new("x", props).unwrap()]);
        let p = &body["batch"][0]["properties"];
        assert_eq!(p["distinct_id"], "install-1");
        assert_eq!(p["$process_person_profile"], false);
        assert_eq!(p["game"], "caro");
        assert_eq!(p["$os"], "custom");
    }

    #[test]
    fn an_event_round_trips_through_json_unchanged() {
        let e = Event::new("x", Map::new()).unwrap();
        let back: Event = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(back.uuid, e.uuid);
        assert_eq!(back.timestamp, e.timestamp);
    }
}
