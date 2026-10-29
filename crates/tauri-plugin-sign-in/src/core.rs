//! What the plugin decides without a phone: which providers a platform shows,
//! the checks on each command's arguments, the error a native rejection
//! becomes, and the answer the app gets.

use std::fmt;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Serialize;
use serde_json::{json, Map, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Ios,
    Android,
    Other,
}

impl Platform {
    #[must_use]
    pub const fn here() -> Self {
        if cfg!(target_os = "ios") {
            Self::Ios
        } else if cfg!(target_os = "android") {
            Self::Android
        } else {
            Self::Other
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Available {
    pub google: bool,
    pub apple: bool,
}

#[must_use]
pub const fn available(platform: Platform) -> Available {
    match platform {
        Platform::Ios => Available {
            google: true,
            apple: true,
        },
        Platform::Android => Available {
            google: true,
            apple: false,
        },
        Platform::Other => Available {
            google: false,
            apple: false,
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Code {
    Cancelled,
    Unavailable,
    NoAccount,
    Failed,
}

/// What a command rejects with: `{ code, message }` in the webview.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Error {
    pub code: Code,
    pub message: String,
}

impl Error {
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self::new(Code::Failed, message)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// The error a rejection from the Swift or Kotlin side becomes. A code outside
/// the four reads as `failed`, so the app never gets a word it does not know.
#[must_use]
pub fn rejected(code: Option<&str>, message: Option<&str>) -> Error {
    let code = match code {
        Some("cancelled") => Code::Cancelled,
        Some("unavailable") => Code::Unavailable,
        Some("no-account") => Code::NoAccount,
        _ => Code::Failed,
    };
    let message = message
        .filter(|m| !m.is_empty())
        .unwrap_or("the sign-in failed");
    Error::new(code, message)
}

const GOOGLE_CLIENT_SUFFIX: &str = ".apps.googleusercontent.com";

/// The part of a Google OAuth client id before `.apps.googleusercontent.com`.
fn client_prefix<'a>(id: &'a str, name: &str) -> Result<&'a str, Error> {
    id.strip_suffix(GOOGLE_CLIENT_SUFFIX)
        .filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        .ok_or_else(|| Error::failed(format!("{name} is not a Google OAuth client id")))
}

pub const NONCE_MAX: usize = 256;

/// # Errors
///
/// A nonce that is empty, longer than [`NONCE_MAX`], or has a character
/// outside printable ASCII.
pub fn check_nonce(nonce: &str) -> Result<(), Error> {
    if nonce.is_empty() || nonce.len() > NONCE_MAX || !nonce.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Error::failed(format!(
            "nonce must be 1 to {NONCE_MAX} printable ASCII characters"
        )));
    }
    Ok(())
}

fn unavailable(provider: &str) -> Error {
    Error::new(
        Code::Unavailable,
        format!("{provider} sign-in has no native sheet on this platform"),
    )
}

/// The arguments the native side of `google` gets. On iOS they add the iOS
/// client id and the URL scheme Google sends the browser back to, which is
/// that client id reversed.
///
/// # Errors
///
/// A platform with no Google sheet, a client id that is not Google's, a bad
/// nonce, or iOS without `ios_client_id`.
pub fn google_args(
    platform: Platform,
    client_id: &str,
    ios_client_id: Option<&str>,
    nonce: Option<&str>,
    silent: bool,
) -> Result<Value, Error> {
    client_prefix(client_id, "clientId")?;
    if let Some(nonce) = nonce {
        check_nonce(nonce)?;
    }
    let ios = match (platform, ios_client_id) {
        (Platform::Ios, None) => return Err(Error::failed("google on iOS needs iosClientId")),
        (Platform::Ios, Some(id)) => Some((id, client_prefix(id, "iosClientId")?)),
        _ => None,
    };
    if !available(platform).google {
        return Err(unavailable("Google"));
    }
    let mut args = Map::new();
    args.insert("clientId".into(), client_id.into());
    args.insert("silent".into(), silent.into());
    if let Some(nonce) = nonce {
        args.insert("nonce".into(), nonce.into());
    }
    if let Some((id, prefix)) = ios {
        args.insert("iosClientId".into(), id.into());
        args.insert(
            "scheme".into(),
            format!("com.googleusercontent.apps.{prefix}").into(),
        );
    }
    Ok(Value::Object(args))
}

/// # Errors
///
/// A bad nonce, or a platform with no Apple sheet.
pub fn apple_args(platform: Platform, nonce: &str) -> Result<Value, Error> {
    check_nonce(nonce)?;
    if !available(platform).apple {
        return Err(unavailable("Apple"));
    }
    Ok(json!({ "nonce": nonce }))
}

fn non_empty<'a>(answer: &'a Value, field: &str) -> Result<&'a str, Error> {
    answer
        .get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::failed(format!("the sign-in answered no {field}")))
}

/// The `nonce` claim of a JWT, read without checking its signature: the
/// service checks the token, the plugin only tells the app what it carries.
fn nonce_claim(jwt: &str) -> Result<Option<String>, Error> {
    let not_jwt = || Error::failed("the id token is not a JWT");
    let mut parts = jwt.split('.');
    let (Some(_), Some(payload), Some(_), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(not_jwt());
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .map_err(|_| not_jwt())?;
    let claims: Value = serde_json::from_slice(&bytes).map_err(|_| not_jwt())?;
    if !claims.is_object() {
        return Err(not_jwt());
    }
    Ok(claims
        .get("nonce")
        .and_then(Value::as_str)
        .map(str::to_owned))
}

/// What `google` answers: `{ idToken, nonce? }`. `nonce` is the raw nonce the
/// token carries, so the app sends the service that one. A token refreshed for
/// a silent iOS sign-in carries none, and then the answer has none.
///
/// # Errors
///
/// No id token, a token that is not a JWT, or a token whose nonce is not the
/// one asked for.
pub fn google_answer(answer: &Value, nonce: Option<&str>) -> Result<Value, Error> {
    let id_token = non_empty(answer, "idToken")?;
    match (nonce, nonce_claim(id_token)?) {
        (Some(asked), Some(carried)) if asked == carried => {
            Ok(json!({ "idToken": id_token, "nonce": asked }))
        }
        (Some(_), Some(_)) => Err(Error::failed("the id token carries another nonce")),
        _ => Ok(json!({ "idToken": id_token })),
    }
}

/// What `apple` answers: `{ idToken, authorizationCode }`.
///
/// # Errors
///
/// An answer without either field.
pub fn apple_answer(answer: &Value) -> Result<Value, Error> {
    let id_token = non_empty(answer, "idToken")?;
    let code = non_empty(answer, "authorizationCode")?;
    Ok(json!({ "idToken": id_token, "authorizationCode": code }))
}
