//! The argument checks, the error mapping and the answers, without a phone.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::{json, Value};
use tauri_plugin_sign_in::core::{
    apple_answer, apple_args, available, check_nonce, google_answer, google_args, rejected,
    Available, Code, Platform, NONCE_MAX,
};

const WEB: &str = "123-web.apps.googleusercontent.com";
const IOS: &str = "123-ios.apps.googleusercontent.com";

fn jwt(claims: &Value) -> String {
    let part = |v: &Value| URL_SAFE_NO_PAD.encode(serde_json::to_vec(v).unwrap());
    format!("{}.{}.sig", part(&json!({ "alg": "RS256" })), part(claims))
}

#[test]
fn each_platform_offers_its_own_sheets() {
    let both = Available {
        google: true,
        apple: true,
    };
    assert_eq!(available(Platform::Ios), both);
    assert_eq!(
        available(Platform::Android),
        Available {
            google: true,
            apple: false
        }
    );
    assert_eq!(
        available(Platform::Other),
        Available {
            google: false,
            apple: false
        }
    );
    assert_eq!(
        serde_json::to_value(both).unwrap(),
        json!({ "google": true, "apple": true })
    );
}

#[test]
fn a_native_rejection_keeps_a_known_code_and_reads_any_other_as_failed() {
    for (code, want) in [
        ("cancelled", Code::Cancelled),
        ("unavailable", Code::Unavailable),
        ("no-account", Code::NoAccount),
        ("failed", Code::Failed),
        ("1001", Code::Failed),
    ] {
        assert_eq!(rejected(Some(code), Some("m")).code, want, "{code}");
    }
    let none = rejected(None, None);
    assert_eq!(none.code, Code::Failed);
    assert!(
        !none.message.is_empty(),
        "a rejection always says something"
    );
    assert_eq!(rejected(Some("cancelled"), Some("")).message, none.message);
}

#[test]
fn an_error_reaches_the_webview_as_code_and_message() {
    let e = rejected(Some("no-account"), Some("no Google account chose this app"));
    assert_eq!(
        serde_json::to_value(&e).unwrap(),
        json!({ "code": "no-account", "message": "no Google account chose this app" })
    );
}

#[test]
fn a_nonce_is_one_to_256_printable_ascii_characters() {
    assert!(check_nonce("0123abcd").is_ok());
    assert!(check_nonce(&"a".repeat(NONCE_MAX)).is_ok());
    for bad in [
        String::new(),
        "a".repeat(NONCE_MAX + 1),
        "a b".into(),
        "é".into(),
    ] {
        let e = check_nonce(&bad).unwrap_err();
        assert_eq!(e.code, Code::Failed, "{bad:?}");
    }
}

#[test]
fn google_on_android_sends_the_web_client_id_the_nonce_and_silent() {
    let args = google_args(Platform::Android, WEB, Some(IOS), Some("n1"), true).unwrap();
    assert_eq!(
        args,
        json!({ "clientId": WEB, "nonce": "n1", "silent": true })
    );
    let args = google_args(Platform::Android, WEB, None, None, false).unwrap();
    assert_eq!(args, json!({ "clientId": WEB, "silent": false }));
}

#[test]
fn google_on_ios_needs_the_ios_client_id_and_sends_its_reversed_scheme() {
    let args = google_args(Platform::Ios, WEB, Some(IOS), Some("n1"), false).unwrap();
    assert_eq!(
        args,
        json!({
            "clientId": WEB,
            "iosClientId": IOS,
            "scheme": "com.googleusercontent.apps.123-ios",
            "nonce": "n1",
            "silent": false,
        })
    );
    let e = google_args(Platform::Ios, WEB, None, None, false).unwrap_err();
    assert_eq!(e.code, Code::Failed);
    assert!(e.message.contains("iosClientId"), "{}", e.message);
}

#[test]
fn google_refuses_a_client_id_that_is_not_googles() {
    for bad in [
        "",
        "123-web",
        ".apps.googleusercontent.com",
        "12 3.apps.googleusercontent.com",
        "x.apps.googleusercontent.com.evil",
    ] {
        let e = google_args(Platform::Android, bad, None, None, false).unwrap_err();
        assert_eq!(e.code, Code::Failed, "{bad:?}");
        assert!(e.message.contains("clientId"), "{}", e.message);
    }
    let e = google_args(Platform::Ios, WEB, Some("com.example.app"), None, false).unwrap_err();
    assert!(e.message.contains("iosClientId"), "{}", e.message);
}

#[test]
fn a_platform_without_a_sheet_checks_the_arguments_and_then_answers_unavailable() {
    let e = google_args(Platform::Other, WEB, None, Some("n1"), false).unwrap_err();
    assert_eq!(e.code, Code::Unavailable);
    let e = google_args(Platform::Other, "nope", None, None, false).unwrap_err();
    assert_eq!(e.code, Code::Failed, "a bad argument is reported first");
    assert_eq!(
        apple_args(Platform::Android, "n1").unwrap_err().code,
        Code::Unavailable
    );
    assert_eq!(
        apple_args(Platform::Other, "n1").unwrap_err().code,
        Code::Unavailable
    );
    assert_eq!(
        apple_args(Platform::Android, "").unwrap_err().code,
        Code::Failed
    );
}

#[test]
fn apple_on_ios_sends_the_raw_nonce_to_the_native_side() {
    assert_eq!(
        apple_args(Platform::Ios, "n1").unwrap(),
        json!({ "nonce": "n1" })
    );
}

#[test]
fn google_answers_the_nonce_the_token_carries() {
    let token = jwt(&json!({ "aud": WEB, "nonce": "n1" }));
    let answer = json!({ "idToken": token, "extra": 1 });
    assert_eq!(
        google_answer(&answer, Some("n1")).unwrap(),
        json!({ "idToken": token, "nonce": "n1" })
    );
    let e = google_answer(&answer, Some("n2")).unwrap_err();
    assert_eq!(e.code, Code::Failed, "a token from another sign-in");
    assert_eq!(
        google_answer(&answer, None).unwrap(),
        json!({ "idToken": token }),
        "no nonce asked, none answered"
    );
}

#[test]
fn a_refreshed_google_token_without_a_nonce_answers_none() {
    let token = jwt(&json!({ "aud": WEB }));
    assert_eq!(
        google_answer(&json!({ "idToken": token }), Some("n1")).unwrap(),
        json!({ "idToken": token })
    );
}

#[test]
fn google_refuses_an_answer_without_a_jwt() {
    for answer in [
        json!({}),
        json!({ "idToken": "" }),
        json!({ "idToken": 7 }),
        json!({ "idToken": "a.b" }),
        json!({ "idToken": "a.!!!.c" }),
        json!({ "idToken": format!("a.{}.c", URL_SAFE_NO_PAD.encode("[1]")) }),
    ] {
        let e = google_answer(&answer, None).unwrap_err();
        assert_eq!(e.code, Code::Failed, "{answer}");
    }
}

#[test]
fn apple_answers_the_token_and_the_code_and_refuses_an_answer_missing_either() {
    let answer = json!({ "idToken": "t", "authorizationCode": "c", "extra": 1 });
    assert_eq!(
        apple_answer(&answer).unwrap(),
        json!({ "idToken": "t", "authorizationCode": "c" })
    );
    for bad in [
        json!({ "idToken": "t" }),
        json!({ "authorizationCode": "c" }),
        json!({ "idToken": "", "authorizationCode": "c" }),
    ] {
        assert_eq!(apple_answer(&bad).unwrap_err().code, Code::Failed, "{bad}");
    }
}
