//! The vault's rules, over a `Native` that holds the key and the stored text.
//! The rules run the same on every platform and the tests run them on a fake.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};

/// The longest key, in bytes.
pub const KEY_MAX: usize = 64;
/// The largest value, in bytes. The same limit as the `keep` plugin.
pub const VALUE_MAX: usize = 64 * 1024;

const ENVELOPE_VERSION: u32 = 1;
const MESSAGE_PREFIX: &[u8] = b"vault/1\0";

/// A key is 1 to `KEY_MAX` letters, digits, `.`, `-` or `_`.
///
/// # Errors
/// Says why a key is refused.
pub fn check_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.len() > KEY_MAX {
        return Err(format!("a key is 1 to {KEY_MAX} characters"));
    }
    if !key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    {
        return Err("a key is letters, digits, '.', '-' or '_'".into());
    }
    Ok(())
}

/// A value is at most `VALUE_MAX` bytes of text.
///
/// # Errors
/// Says why a value is refused.
pub fn check_value(value: &str) -> Result<(), String> {
    if value.len() > VALUE_MAX {
        return Err(format!("a value is at most {VALUE_MAX} bytes"));
    }
    Ok(())
}

/// The bytes a signature covers. The key name is inside them, so a signature
/// cannot move from one key to another.
#[must_use]
pub fn message(key: &str, value: &str) -> Vec<u8> {
    [MESSAGE_PREFIX, key.as_bytes(), b"\0", value.as_bytes()].concat()
}

/// What the platform stores for one key.
#[derive(Serialize, Deserialize)]
pub struct Envelope {
    pub v: u32,
    pub value: String,
    /// Base64. Absent when the device cannot make a key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
}

/// What the platform holds under a key.
pub enum Stored {
    Missing,
    Text(String),
    /// Something is there that the platform cannot read as text, such as a
    /// directory where the file should be.
    Unreadable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Nothing is stored under the key.
    Empty,
    /// The signature matches, with a key the device made.
    Verified,
    /// A value was stored and its key is gone. The vault signed it again.
    Restored,
    /// A value is stored and the vault cannot verify it.
    Tampered,
    /// The device cannot make a key. The value has no signature.
    Unsigned,
}

/// Where the signing key lives, from the safest place to the weakest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Enclave,
    Strongbox,
    Hardware,
    Software,
}

/// The platform's side: a store for text and a signing key. A primitive that
/// fails answers `Err`, which the vault never turns into `Tampered`.
#[async_trait::async_trait]
pub trait Native: Send + Sync {
    async fn load(&self, key: &str) -> Result<Stored, String>;
    async fn save(&self, key: &str, text: &str) -> Result<(), String>;
    /// `Ok(None)` when no key exists. That case is never an error.
    async fn key_level(&self) -> Result<Option<Level>, String>;
    /// Makes the key and answers its level. When a key already exists, answers
    /// that key's level and never replaces it. `Ok(None)` when the device
    /// cannot make a key.
    async fn make_key(&self) -> Result<Option<Level>, String>;
    async fn sign(&self, msg: &[u8]) -> Result<Vec<u8>, String>;
    /// Answers `Ok(false)` for any signature the system cannot parse or check
    /// against the key, such as one with a bad encoding. Answers `Err` only
    /// when the key store itself fails, for example when the key cannot be
    /// loaded. An `Err` here would let a person with root turn `Tampered` into
    /// an error by breaking the stored signature.
    async fn verify(&self, msg: &[u8], sig: &[u8]) -> Result<bool, String>;
}

#[derive(Serialize)]
pub struct Read {
    pub value: Option<String>,
    pub state: State,
}

fn read_as(value: Option<String>, state: State) -> Read {
    Read { value, state }
}

fn parse(text: &str) -> Option<Envelope> {
    serde_json::from_str::<Envelope>(text)
        .ok()
        .filter(|e| e.v == ENVELOPE_VERSION)
}

async fn save_signed(n: &dyn Native, key: &str, value: &str) -> Result<(), String> {
    let sig = n.sign(&message(key, value)).await?;
    save_envelope(n, key, value, Some(STANDARD.encode(sig))).await
}

async fn save_envelope(
    n: &dyn Native,
    key: &str,
    value: &str,
    sig: Option<String>,
) -> Result<(), String> {
    let envelope = Envelope {
        v: ENVELOPE_VERSION,
        value: value.to_owned(),
        sig,
    };
    let text = serde_json::to_string(&envelope).map_err(|e| e.to_string())?;
    n.save(key, &text).await
}

/// Reads the value under `key` and says whether it can be trusted.
///
/// A stored value that cannot be verified is `Tampered`, never an error, so
/// that breaking the stored value cannot make the app fall back to a weaker
/// store. An error means a platform call failed.
///
/// # Errors
/// A `Native` call failed.
pub async fn read(n: &dyn Native, key: &str) -> Result<Read, String> {
    let text = match n.load(key).await? {
        Stored::Missing => return Ok(read_as(None, State::Empty)),
        Stored::Unreadable => return Ok(read_as(None, State::Tampered)),
        Stored::Text(text) => text,
    };
    let Some(envelope) = parse(&text) else {
        return Ok(read_as(Some(text), State::Tampered));
    };
    let Envelope { value, sig, .. } = envelope;

    let state = if n.key_level().await?.is_none() {
        if n.make_key().await?.is_some() {
            // The key exists before the value is signed again. When `sign` or
            // `save` then fails, this read is an error and the next read is
            // `Tampered`: the stored signature belongs to the lost key.
            save_signed(n, key, &value).await?;
            State::Restored
        } else if sig.is_none() {
            State::Unsigned
        } else {
            State::Tampered
        }
    } else {
        let sig = sig.and_then(|s| STANDARD.decode(s).ok());
        match sig {
            Some(sig) if n.verify(&message(key, &value), &sig).await? => State::Verified,
            _ => State::Tampered,
        }
    };
    Ok(read_as(Some(value), state))
}

/// Stores `value` under `key`, signed when the device has a key or can make
/// one.
///
/// The key exists before the value is signed. When `sign` or `save` then
/// fails on a first write, the write is an error and the key stays.
///
/// # Errors
/// A `Native` call failed.
pub async fn write(n: &dyn Native, key: &str, value: &str) -> Result<State, String> {
    let has_key = match n.key_level().await? {
        Some(_) => true,
        None => n.make_key().await?.is_some(),
    };
    if has_key {
        save_signed(n, key, value).await?;
        Ok(State::Verified)
    } else {
        save_envelope(n, key, value, None).await?;
        Ok(State::Unsigned)
    }
}
