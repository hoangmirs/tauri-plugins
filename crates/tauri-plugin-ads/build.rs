#[path = "build_support.rs"]
mod build_support;

use std::path::{Path, PathBuf};
use std::process::Command;

/// `register_listener` and `remove_listener` are the core's own commands for a
/// mobile plugin's events. Listing them lets a webview hear `native`.
const COMMANDS: &[&str] = &[
    "register_listener",
    "remove_listener",
    "init",
    "consent",
    "show_native",
    "hide_native",
    "load_rewarded",
    "show_rewarded",
    "privacy_options",
    "show_privacy_options",
    "set_muted",
];

/// Google ships the SDK and its consent form as prebuilt static frameworks.
/// Swift Package Manager builds them next to the plugin's own library, but
/// the Rust link only takes that library, so their symbols stay undefined.
const FRAMEWORKS: &[&str] = &["GoogleMobileAds", "UserMessagingPlatform"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .ios_path("ios")
        .build();

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let mut anchors = Vec::new();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("ios") {
        anchors = link_frameworks(&out);
    }
    std::fs::write(
        out.join("anchors.rs"),
        build_support::anchors_source(&anchors),
    )
    .expect("write anchors.rs");
}

/// Objects the app links without `-ObjC` lose their Objective-C categories,
/// and the SDK then stops the app with "unrecognized selector". `-ObjC` on
/// the app also loads every plugin's own copy of Tauri's Swift API, which
/// fails with duplicate symbols. So the crate refers to one symbol in each
/// of Google's objects instead, which loads those objects and no others.
const SKIP: &[&str] = &["Tauri.o", "SwiftRs.o"];

fn anchors_of(lib: &Path) -> Vec<String> {
    let nm = Command::new("nm")
        .arg("-gUm")
        .arg(lib)
        .output()
        .unwrap_or_else(|e| panic!("cannot run nm on {}: {e}", lib.display()));
    build_support::anchors(&String::from_utf8_lossy(&nm.stdout), SKIP)
}

/// Links each framework's archive into the crate, cut to the target's
/// architecture: rustc cannot read a universal archive.
fn link_frameworks(out: &Path) -> Vec<String> {
    let sim = std::env::var("TARGET").is_ok_and(|t| t.ends_with("-sim") || t.starts_with("x86_64"));
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => "x86_64",
        _ => "arm64",
    };
    let root = out.join("swift-rs");
    let swift_lib = build_support::find_file(&root, "libtauri-plugin-ads.a", 8)
        .unwrap_or_else(|| panic!("no libtauri-plugin-ads.a under {}", root.display()));
    // The plugin's own object needs no anchor: `ios_plugin_binding!` already
    // refers to it, with a signature an anchor would clash with.
    let mut anchors: Vec<String> = anchors_of(&swift_lib)
        .into_iter()
        .filter(|a| a != "init_plugin_ads")
        .collect();
    for name in FRAMEWORKS {
        let binary = build_support::framework_binary(&root, name, sim).unwrap_or_else(|| {
            panic!(
                "{name}.xcframework has no iOS slice under {}",
                root.display()
            )
        });
        let lib = out.join(format!("lib{name}.a"));
        thin(&binary, arch, &lib);
        anchors.extend(anchors_of(&lib));
        println!("cargo:rustc-link-lib=static={name}");
    }
    println!("cargo:rustc-link-search=native={}", out.display());
    anchors.sort();
    anchors.dedup();
    anchors
}

fn thin(binary: &Path, arch: &str, lib: &Path) {
    let fat = Command::new("lipo")
        .arg(binary)
        .args(["-thin", arch, "-output"])
        .arg(lib)
        .status()
        .is_ok_and(|s| s.success());
    if !fat {
        std::fs::copy(binary, lib)
            .unwrap_or_else(|e| panic!("cannot copy {}: {e}", binary.display()));
    }
}
