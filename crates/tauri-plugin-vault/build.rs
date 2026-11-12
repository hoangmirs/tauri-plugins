const COMMANDS: &[&str] = &["read", "write"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .ios_path("ios")
        .build();
    // The target's, not this build script's: a universal Mac build runs this
    // once for each architecture, both on one host.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        mac::build();
    }
}

/// The Mac's Keychain code, compiled into a static library and linked.
///
/// This calls `swiftc` for the target's architecture rather than going through
/// `swift-rs`: on a macOS target `swift-rs` builds for the host's, so the other
/// half of a universal app would not link.
mod mac {
    use std::env;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const SOURCES: [&str; 2] = ["ios/Sources/Vault.swift", "macos/Bridge.swift"];
    const LIB: &str = "tauri_plugin_vault_keychain";
    /// The Mac's data protection keychain begins at macOS 10.15.
    const LOWEST: (u32, u32) = (10, 15);

    pub fn build() {
        for source in SOURCES {
            println!("cargo:rerun-if-changed={source}");
        }
        println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");

        let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
        let arch = match env::var("CARGO_CFG_TARGET_ARCH")
            .expect("cargo sets CARGO_CFG_TARGET_ARCH")
            .as_str()
        {
            "aarch64" => "arm64".to_owned(),
            arch => arch.to_owned(),
        };
        let sdk = xcrun(&["--sdk", "macosx", "--show-sdk-path"]);
        let swiftc = PathBuf::from(xcrun(&["--sdk", "macosx", "--find", "swiftc"]));

        let status = Command::new(&swiftc)
            .args(["-parse-as-library", "-emit-library", "-static"])
            .args([
                "-module-name",
                "TauriPluginVault",
                "-swift-version",
                "5",
                "-O",
            ])
            .args([
                "-target",
                &format!("{arch}-apple-macos{}", deployment_target()),
            ])
            .args(["-sdk", &sdk])
            .arg("-module-cache-path")
            .arg(out.join("swift-module-cache"))
            .arg("-o")
            .arg(out.join(format!("lib{LIB}.a")))
            .args(SOURCES)
            .status()
            .expect("swiftc runs. Install Xcode or its command line tools");
        assert!(status.success(), "swiftc failed on the Mac's vault code");

        println!("cargo:rustc-link-search=native={}", out.display());
        println!("cargo:rustc-link-lib=static={LIB}");
        println!("cargo:rustc-link-lib=framework=Security");
        // The Swift runtime the library asks for: the system's own, as the
        // SDK describes it, and the toolchain's static compatibility shims.
        println!("cargo:rustc-link-search=native={sdk}/usr/lib/swift");
        copy_shims(&swiftc, &out);
        // Below macOS 12 the linker names the concurrency runtime
        // `@rpath/libswift_Concurrency.dylib`, for a copy shipped in the app.
        // This finds the system's copy instead, for this crate's own tests
        // only: an app that uses the plugin targets macOS 12 or later.
        println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    }

    /// Only the shims, copied beside the library. Their folder also holds
    /// back-deployed runtime dylibs named `@rpath/...`. A search of that folder
    /// links the app to those instead of the system's, and the app then fails
    /// to start.
    fn copy_shims(swiftc: &Path, out: &Path) {
        let Some(toolchain) = swiftc.parent().and_then(Path::parent) else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(toolchain.join("lib/swift/macosx")) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("libswiftCompatibility") && name.ends_with(".a") {
                std::fs::copy(entry.path(), out.join(&*name)).expect("the shim copies");
            }
        }
    }

    /// The app's own lowest macOS, never below `LOWEST`. The Tauri CLI
    /// sets `MACOSX_DEPLOYMENT_TARGET` from `minimumSystemVersion`.
    fn deployment_target() -> String {
        let parse = |v: &str| -> Option<(u32, u32)> {
            let mut parts = v.trim().split('.');
            let major = parts.next()?.parse().ok()?;
            let minor = parts.next().map_or(Some(0), |m| m.parse().ok())?;
            Some((major, minor))
        };
        let asked = env::var("MACOSX_DEPLOYMENT_TARGET")
            .ok()
            .and_then(|v| parse(&v));
        let (major, minor) = match asked {
            Some(v) if v >= LOWEST => v,
            Some(v) => {
                println!(
                    "cargo:warning=tauri-plugin-vault needs macOS {}.{}, and the app targets {}.{}: the app may not open on the older Macs",
                    LOWEST.0, LOWEST.1, v.0, v.1
                );
                LOWEST
            }
            None => LOWEST,
        };
        format!("{major}.{minor}")
    }

    fn xcrun(args: &[&str]) -> String {
        let out = Command::new("xcrun")
            .args(args)
            .output()
            .expect("xcrun runs. Install Xcode or its command line tools");
        assert!(out.status.success(), "xcrun {} failed", args.join(" "));
        String::from_utf8(out.stdout)
            .expect("xcrun prints UTF-8")
            .trim()
            .to_owned()
    }
}
