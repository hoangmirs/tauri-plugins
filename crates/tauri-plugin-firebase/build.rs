use std::path::{Path, PathBuf};
use std::process::Command;

const COMMANDS: &[&str] = &[
    "mode",
    "set_defaults",
    "fetch",
    "activate",
    "get_all",
    "log_event",
    "set_collection",
];

/// Firebase Analytics ships as prebuilt static frameworks. Swift Package
/// Manager builds them next to the plugin's own library, but the Rust link
/// only takes that library, so `_OBJC_CLASS_$_FIRAnalytics` stays undefined.
const FRAMEWORKS: &[&str] = &["FirebaseAnalytics", "GoogleAppMeasurement"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .ios_path("ios")
        .build();

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("ios") {
        link_frameworks();
    }
}

/// Links each framework's archive into the crate, cut to the target's
/// architecture: rustc cannot read a universal archive. The system
/// libraries are the ones Firebase's own `linkerSettings` name, which the
/// Rust link also never sees.
fn link_frameworks() {
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let sim = std::env::var("TARGET").is_ok_and(|t| t.ends_with("-sim") || t.starts_with("x86_64"));
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => "x86_64",
        _ => "arm64",
    };
    let products = out
        .join("swift-rs/tauri-plugin-firebase/out/Products")
        .join(if sim {
            "Release-iphonesimulator"
        } else {
            "Release-iphoneos"
        });
    for name in FRAMEWORKS {
        let binary = products.join(format!("{name}.framework/{name}"));
        let lib = out.join(format!("lib{name}.a"));
        thin(&binary, arch, &lib);
        println!("cargo:rustc-link-lib=static={name}");
    }
    println!("cargo:rustc-link-search=native={}", out.display());
    for lib in ["sqlite3", "c++", "z"] {
        println!("cargo:rustc-link-lib={lib}");
    }
    println!("cargo:rustc-link-lib=framework=StoreKit");
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
