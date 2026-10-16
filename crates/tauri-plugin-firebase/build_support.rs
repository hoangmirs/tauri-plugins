//! Shared by `build.rs` and `tests/frameworks.rs`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The archive of `name` for an iOS device or simulator, from the
/// `<name>.xcframework` Swift Package Manager downloaded under `root`.
///
/// The built `Products/<Config>-<platform>` folder is not used: its name
/// follows Cargo's `DEBUG` and the Xcode version, so a path to it holds on
/// one machine only.
pub fn framework_binary(root: &Path, name: &str, sim: bool) -> Option<PathBuf> {
    let xcframework = find_dir(root, &format!("{name}.xcframework"), 6)?;
    std::fs::read_dir(&xcframework)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|slice| slice.starts_with("ios-") && slice.ends_with("-simulator") == sim)
        .filter(|slice| !slice.ends_with("-maccatalyst"))
        .map(|slice| {
            xcframework
                .join(slice)
                .join(format!("{name}.framework/{name}"))
        })
        .find(|binary| binary.is_file())
}

fn find_dir(root: &Path, wanted: &str, depth: usize) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    let mut dirs = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if entry.file_name() == wanted {
            return Some(path);
        }
        dirs.push(path);
    }
    if depth == 0 {
        return None;
    }
    dirs.sort();
    dirs.iter().find_map(|d| find_dir(d, wanted, depth - 1))
}

/// One defined global symbol from each object in `nm -gU` output, skipping
/// the objects named in `skip`. A reference to any symbol of an object makes
/// the linker load all of it, Objective-C categories included.
pub fn anchors(nm: &str, skip: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut member: Option<&str> = None;
    let mut taken = true;
    for line in nm.lines().map(str::trim) {
        // A member header is `lib.a(Member.o):` in GNU nm and `Member.o:` in
        // Apple's. A symbol line never ends with a colon.
        if let Some(head) = line.strip_suffix(':') {
            let head = head.strip_suffix(')').unwrap_or(head);
            let name = head.rsplit_once('(').map_or(head, |(_, m)| m);
            member = Some(name);
            taken = skip.contains(&name);
            continue;
        }
        if taken || member.is_none() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let (Some(_addr), Some(kind), Some(name)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if matches!(kind, "T" | "D" | "S") {
            out.push(name.strip_prefix('_').unwrap_or(name).to_string());
            taken = true;
        }
    }
    out
}

/// The first file named `wanted` under `root`, searched to `depth` levels.
pub fn find_file(root: &Path, wanted: &str, depth: usize) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    let mut dirs = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        } else if entry.file_name() == wanted {
            return Some(path);
        }
    }
    if depth == 0 {
        return None;
    }
    dirs.sort();
    dirs.iter().find_map(|d| find_file(d, wanted, depth - 1))
}

/// Rust source that refers to each anchor, so the linker keeps every object
/// they live in.
pub fn anchors_source(anchors: &[String]) -> String {
    let mut src = String::from("extern \"C\" {\n");
    for (i, name) in anchors.iter().enumerate() {
        let _ = writeln!(src, "    #[link_name = \"{name}\"]\n    fn anchor_{i}();");
    }
    src.push_str("}\n\npub(crate) const ANCHORS: &[unsafe extern \"C\" fn()] = &[\n");
    for i in 0..anchors.len() {
        let _ = writeln!(src, "    anchor_{i},");
    }
    src.push_str("];\n");
    src
}
