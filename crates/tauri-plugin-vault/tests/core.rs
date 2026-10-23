//! The vault's states, over a native that keeps everything in memory.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use tauri_plugin_vault::core::{
    check_key, check_value, message, read, write, Level, Native, State, Stored, KEY_MAX, VALUE_MAX,
};

#[derive(Default)]
struct Inner {
    saved: HashMap<String, String>,
    unreadable: HashSet<String>,
    key: Option<Level>,
    cannot_make_key: bool,
    failing: HashSet<&'static str>,
}

#[derive(Default)]
struct Fake(Mutex<Inner>);

impl Fake {
    fn saved(&self, key: &str) -> String {
        self.0.lock().unwrap().saved[key].clone()
    }

    fn put(&self, key: &str, text: &str) {
        self.0.lock().unwrap().saved.insert(key.into(), text.into());
    }

    fn lose_key(&self) {
        self.0.lock().unwrap().key = None;
    }

    fn with(&self, change: impl FnOnce(&mut Inner)) {
        change(&mut self.0.lock().unwrap());
    }
}

fn fake_signature(msg: &[u8]) -> Vec<u8> {
    [b"sig:".as_slice(), msg].concat()
}

#[async_trait::async_trait]
impl Native for Fake {
    async fn load(&self, key: &str) -> Result<Stored, String> {
        let inner = self.0.lock().unwrap();
        if inner.failing.contains("load") {
            return Err("load failed".into());
        }
        if inner.unreadable.contains(key) {
            return Ok(Stored::Unreadable);
        }
        Ok(inner
            .saved
            .get(key)
            .map_or(Stored::Missing, |t| Stored::Text(t.clone())))
    }

    async fn save(&self, key: &str, text: &str) -> Result<(), String> {
        self.put(key, text);
        Ok(())
    }

    async fn key_level(&self) -> Result<Option<Level>, String> {
        let inner = self.0.lock().unwrap();
        if inner.failing.contains("key_level") {
            return Err("key level failed".into());
        }
        Ok(inner.key)
    }

    async fn make_key(&self) -> Result<Option<Level>, String> {
        let mut inner = self.0.lock().unwrap();
        if inner.cannot_make_key {
            return Ok(None);
        }
        inner.key = Some(Level::Software);
        Ok(inner.key)
    }

    async fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, String> {
        Ok(fake_signature(msg))
    }

    async fn verify(&self, msg: &[u8], sig: &[u8]) -> Result<bool, String> {
        if self.0.lock().unwrap().failing.contains("verify") {
            return Err("verify failed".into());
        }
        Ok(sig == fake_signature(msg))
    }
}

fn edit_value(fake: &Fake, key: &str, value: &str) {
    let mut envelope: serde_json::Value = serde_json::from_str(&fake.saved(key)).unwrap();
    envelope["value"] = value.into();
    fake.put(key, &envelope.to_string());
}

#[tokio::test]
async fn nothing_stored_reads_empty() {
    let fake = Fake::default();
    let got = read(&fake, "wallet").await.unwrap();
    assert_eq!(got.state, State::Empty);
    assert_eq!(got.value, None);
}

#[tokio::test]
async fn a_written_value_reads_back_verified() {
    let fake = Fake::default();
    assert_eq!(
        write(&fake, "wallet", r#"{"stars":5}"#).await.unwrap(),
        State::Verified
    );
    let got = read(&fake, "wallet").await.unwrap();
    assert_eq!(got.state, State::Verified);
    assert_eq!(got.value.as_deref(), Some(r#"{"stars":5}"#));
}

#[tokio::test]
async fn a_changed_value_reads_tampered() {
    let fake = Fake::default();
    write(&fake, "wallet", "5").await.unwrap();
    edit_value(&fake, "wallet", "5000");
    let got = read(&fake, "wallet").await.unwrap();
    assert_eq!(got.state, State::Tampered);
    assert_eq!(got.value.as_deref(), Some("5000"));
}

#[tokio::test]
async fn a_value_moved_to_another_key_reads_tampered() {
    let fake = Fake::default();
    write(&fake, "a", "5").await.unwrap();
    fake.put("b", &fake.saved("a"));
    let got = read(&fake, "b").await.unwrap();
    assert_eq!(got.state, State::Tampered);
    assert_eq!(got.value.as_deref(), Some("5"));
}

#[tokio::test]
async fn a_broken_shape_reads_tampered() {
    let fake = Fake::default();
    write(&fake, "wallet", "5").await.unwrap();

    fake.put("wallet", "not json");
    let got = read(&fake, "wallet").await.unwrap();
    assert_eq!(got.state, State::Tampered);
    assert_eq!(got.value.as_deref(), Some("not json"));

    fake.put("wallet", r#"{"v":2,"value":"5","sig":"AAAA"}"#);
    assert_eq!(read(&fake, "wallet").await.unwrap().state, State::Tampered);

    fake.put("wallet", r#"{"v":1,"value":"5"}"#);
    assert_eq!(read(&fake, "wallet").await.unwrap().state, State::Tampered);

    fake.put("wallet", r#"{"v":1,"value":"5","sig":"not base64!"}"#);
    assert_eq!(read(&fake, "wallet").await.unwrap().state, State::Tampered);

    fake.with(|i| {
        i.unreadable.insert("wallet".into());
    });
    let got = read(&fake, "wallet").await.unwrap();
    assert_eq!(got.state, State::Tampered);
    assert_eq!(got.value, None);
}

#[tokio::test]
async fn a_value_without_its_key_reads_restored_and_is_signed_again() {
    let fake = Fake::default();
    write(&fake, "wallet", "5").await.unwrap();
    fake.lose_key();

    let got = read(&fake, "wallet").await.unwrap();
    assert_eq!(got.state, State::Restored);
    assert_eq!(got.value.as_deref(), Some("5"));

    let again = read(&fake, "wallet").await.unwrap();
    assert_eq!(again.state, State::Verified);
    assert_eq!(again.value.as_deref(), Some("5"));
}

#[tokio::test]
async fn a_device_without_a_key_stores_unsigned() {
    let fake = Fake::default();
    fake.with(|i| i.cannot_make_key = true);
    assert_eq!(write(&fake, "wallet", "5").await.unwrap(), State::Unsigned);
    assert!(!fake.saved("wallet").contains("sig"));
    let got = read(&fake, "wallet").await.unwrap();
    assert_eq!(got.state, State::Unsigned);
    assert_eq!(got.value.as_deref(), Some("5"));
}

#[tokio::test]
async fn a_signed_value_on_a_device_that_lost_its_key_for_good_reads_tampered() {
    let fake = Fake::default();
    write(&fake, "wallet", "5").await.unwrap();
    fake.lose_key();
    fake.with(|i| i.cannot_make_key = true);
    assert_eq!(read(&fake, "wallet").await.unwrap().state, State::Tampered);
}

#[tokio::test]
async fn a_failing_primitive_is_an_error_not_tampered() {
    let fake = Fake::default();
    write(&fake, "wallet", "5").await.unwrap();

    fake.with(|i| {
        i.failing.insert("load");
    });
    assert!(read(&fake, "wallet").await.is_err());
    fake.with(|i| {
        i.failing.remove("load");
    });

    fake.with(|i| {
        i.failing.insert("key_level");
    });
    assert!(read(&fake, "wallet").await.is_err());
    fake.with(|i| {
        i.failing.remove("key_level");
    });

    fake.with(|i| {
        i.failing.insert("verify");
    });
    assert!(read(&fake, "wallet").await.is_err());
}

#[test]
fn the_message_names_the_key() {
    assert_ne!(message("a", "x"), message("b", "x"));
    assert!(message("a", "x").starts_with(b"vault/1\0"));
    assert_eq!(message("a", "x"), b"vault/1\0a\0x".to_vec());
}

#[test]
fn keys_and_values_follow_keeps_rules() {
    assert!(check_key("rachoi.wallet").is_ok());
    assert!(check_key("a-b_c.1").is_ok());
    assert!(check_key("").is_err());
    assert!(check_key(&"k".repeat(KEY_MAX + 1)).is_err());
    assert!(check_key("has space").is_err());
    assert!(check_key("slash/key").is_err());

    assert!(check_value(&"v".repeat(VALUE_MAX)).is_ok());
    assert!(check_value(&"v".repeat(VALUE_MAX + 1)).is_err());
}
