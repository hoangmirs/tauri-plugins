//! Firebase gives every Remote Config value as text, whatever its type in the
//! console. The defaults are kept as JSON and read back the same way, so an
//! app reads a default exactly as it reads a value from Firebase.

use serde::Serialize;
use serde_json::{Map, Value};

/// One value as the app reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub value: String,
    pub source: &'static str,
}

/// A default as Firebase would give it: `true` reads `"true"`, `15` reads
/// `"15"`, and an object reads as its JSON text.
#[must_use]
pub fn as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The defaults in the shape `get_all` answers with.
#[must_use]
pub fn defaults_view(d: &Map<String, Value>) -> Map<String, Value> {
    d.iter()
        .map(|(k, v)| {
            let entry = Entry {
                value: as_text(v),
                source: "default",
            };
            (
                k.clone(),
                serde_json::to_value(entry).unwrap_or(Value::Null),
            )
        })
        .collect()
}

/// The defaults as text, for the native SDKs, which take text only here.
#[must_use]
pub fn as_texts(d: &Map<String, Value>) -> Map<String, Value> {
    d.iter()
        .map(|(k, v)| (k.clone(), Value::String(as_text(v))))
        .collect()
}
