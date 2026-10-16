//! `build.rs` finds Firebase's prebuilt archives in the xcframeworks Swift
//! Package Manager downloads. Their slices have the same names whatever the
//! build configuration, so a debug build finds them too.

#[path = "../build_support.rs"]
mod build_support;

use std::fs;
use std::path::Path;

fn touch(root: &Path, rel: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, b"!<arch>\n").unwrap();
}

const SIM: &str = "artifacts/firebase-ios-sdk/FirebaseAnalytics/FirebaseAnalytics.xcframework/ios-arm64_x86_64-simulator/FirebaseAnalytics.framework/FirebaseAnalytics";
const DEVICE: &str = "artifacts/firebase-ios-sdk/FirebaseAnalytics/FirebaseAnalytics.xcframework/ios-arm64/FirebaseAnalytics.framework/FirebaseAnalytics";
const MAC: &str = "artifacts/firebase-ios-sdk/FirebaseAnalytics/FirebaseAnalytics.xcframework/ios-arm64_x86_64-maccatalyst/FirebaseAnalytics.framework/FirebaseAnalytics";

#[test]
fn the_simulator_slice_is_found_for_a_simulator() {
    let dir = tempfile::tempdir().unwrap();
    for rel in [SIM, DEVICE, MAC] {
        touch(dir.path(), rel);
    }
    let found = build_support::framework_binary(dir.path(), "FirebaseAnalytics", true).unwrap();
    assert_eq!(found, dir.path().join(SIM));
}

#[test]
fn the_device_slice_is_found_for_a_device() {
    let dir = tempfile::tempdir().unwrap();
    for rel in [SIM, DEVICE, MAC] {
        touch(dir.path(), rel);
    }
    let found = build_support::framework_binary(dir.path(), "FirebaseAnalytics", false).unwrap();
    assert_eq!(found, dir.path().join(DEVICE));
}

#[test]
fn a_missing_framework_is_none() {
    let dir = tempfile::tempdir().unwrap();
    touch(dir.path(), DEVICE);
    assert!(build_support::framework_binary(dir.path(), "GoogleAppMeasurement", false).is_none());
}

const NM: &str = "
/out/libtauri-plugin-firebase.a(Tauri.o):
0000000000001000 T _$s5Tauri6InvokeC7resolveyyF

/out/libtauri-plugin-firebase.a(GoogleUtilities-NSData.o):
0000000000000010 S _GULNSDataZlibErrorDomain
0000000000000020 T _GULNSDataZlibOther

/out/libtauri-plugin-firebase.a(FBLPromises.o):
0000000000000040 D _OBJC_CLASS_$_FBLPromise
0000000000000050 T _FBLWaitForPromisesWithTimeout

/out/libtauri-plugin-firebase.a(Empty.o):

/out/libtauri-plugin-firebase.a(SwiftRs.o):
0000000000000060 T _$s7SwiftRs8SRStringC
";

#[test]
fn one_symbol_is_kept_for_each_object_but_the_skipped_ones() {
    let anchors = build_support::anchors(NM, &["Tauri.o", "SwiftRs.o"]);
    assert_eq!(
        anchors,
        vec![
            "GULNSDataZlibErrorDomain".to_string(),
            "OBJC_CLASS_$_FBLPromise".to_string()
        ]
    );
}

#[test]
fn the_plugin_library_is_found_at_any_depth() {
    let dir = tempfile::tempdir().unwrap();
    touch(
        dir.path(),
        "tauri-plugin-firebase/out/Products/Debug-iphonesimulator/libtauri-plugin-firebase.a",
    );
    let found = build_support::find_file(dir.path(), "libtauri-plugin-firebase.a", 8).unwrap();
    assert!(found.ends_with("Debug-iphonesimulator/libtauri-plugin-firebase.a"));
}

#[test]
fn the_source_refers_to_each_anchor_by_its_link_name() {
    let src =
        build_support::anchors_source(&["GULNSDataZlibErrorDomain".into(), "$s5Other".into()]);
    assert!(src.contains("#[link_name = \"GULNSDataZlibErrorDomain\"]\n    fn anchor_0();"));
    assert!(src.contains("#[link_name = \"$s5Other\"]\n    fn anchor_1();"));
    assert!(src.contains("&[\n    anchor_0,\n    anchor_1,\n];"));
}

#[test]
fn no_anchors_is_an_empty_list() {
    assert!(build_support::anchors_source(&[]).contains("= &[\n];"));
}

#[test]
fn apple_nm_member_headers_are_read() {
    let nm = "\nTauri.o:\n0000000000001000 T _$s5Tauri6InvokeC\n\nGoogleUtilities-NSData.o:\n0000000000000a68 S _GULNSDataZlibErrorDomain\n";
    assert_eq!(
        build_support::anchors(nm, &["Tauri.o"]),
        vec!["GULNSDataZlibErrorDomain".to_string()]
    );
}
