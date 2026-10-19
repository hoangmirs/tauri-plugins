//! `build.rs` finds Google's prebuilt archives in the xcframeworks Swift
//! Package Manager downloads, one slice for a simulator and one for a device.

// The anchor helpers are tested in the Firebase crate, which owns the same file.
#[allow(dead_code)]
#[path = "../build_support.rs"]
mod build_support;

use std::fs;
use std::path::Path;

fn touch(root: &Path, rel: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, b"!<arch>\n").unwrap();
}

const SIM: &str = "artifacts/swift-package-manager-google-mobile-ads/GoogleMobileAds/GoogleMobileAds.xcframework/ios-arm64_x86_64-simulator/GoogleMobileAds.framework/GoogleMobileAds";
const DEVICE: &str = "artifacts/swift-package-manager-google-mobile-ads/GoogleMobileAds/GoogleMobileAds.xcframework/ios-arm64/GoogleMobileAds.framework/GoogleMobileAds";

#[test]
fn the_simulator_slice_is_found_for_a_simulator() {
    let dir = tempfile::tempdir().unwrap();
    for rel in [SIM, DEVICE] {
        touch(dir.path(), rel);
    }
    let found = build_support::framework_binary(dir.path(), "GoogleMobileAds", true).unwrap();
    assert_eq!(found, dir.path().join(SIM));
}

#[test]
fn the_device_slice_is_found_for_a_device() {
    let dir = tempfile::tempdir().unwrap();
    for rel in [SIM, DEVICE] {
        touch(dir.path(), rel);
    }
    let found = build_support::framework_binary(dir.path(), "GoogleMobileAds", false).unwrap();
    assert_eq!(found, dir.path().join(DEVICE));
}
