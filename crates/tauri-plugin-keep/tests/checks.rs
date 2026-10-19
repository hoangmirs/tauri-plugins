//! What the plugin accepts, the same on every platform.

use tauri_plugin_keep::{check_key, check_value, KEY_MAX, VALUE_MAX};

#[test]
fn a_short_plain_key_is_taken() {
    assert!(check_key("rachoi.kept").is_ok());
    assert!(check_key("a-b_c.1").is_ok());
}

#[test]
fn an_empty_long_or_odd_key_is_refused() {
    assert!(check_key("").is_err());
    assert!(check_key(&"k".repeat(KEY_MAX + 1)).is_err());
    assert!(check_key("has space").is_err());
    assert!(check_key("slash/key").is_err());
}

#[test]
fn a_value_up_to_the_limit_is_taken_and_a_larger_one_refused() {
    assert!(check_value(&"v".repeat(VALUE_MAX)).is_ok());
    assert!(check_value(&"v".repeat(VALUE_MAX + 1)).is_err());
}
