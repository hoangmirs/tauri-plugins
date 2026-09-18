//! Who the install is, and whether it said not to be watched: a v4 UUID
//! made once and kept in a file, and a marker file for opting out. Both
//! live as plain files rather than anything richer because there is
//! nothing else to store, and a missing or broken file should always be
//! recoverable without help — analytics must never be the reason the app
//! fails to start.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use uuid::Uuid;

/// Holds the install's anonymous id: a v4 UUID as trimmed text.
const ID_FILE: &str = "posthog-id";

/// Its mere presence means the install opted out; contents don't matter.
const OPT_OUT_FILE: &str = "posthog-opt-out";

/// The install's anonymous id, made once and remembered in `dir`. A
/// missing, unreadable, or garbage id file is treated the same: a fresh id
/// is generated and (best-effort) saved in its place. If `dir` can't be
/// written to, the fresh id is still returned so the caller always has
/// something to tag this run's events with — it just won't be the same id
/// next run, and the failure is logged rather than surfaced, since
/// analytics must never break the app.
#[must_use]
pub fn install_id(dir: &Path) -> String {
    let path = dir.join(ID_FILE);
    if let Ok(contents) = fs::read_to_string(&path) {
        if let Ok(uuid) = Uuid::parse_str(contents.trim()) {
            return uuid.to_string();
        }
    }

    let id = Uuid::new_v4().to_string();
    if let Err(e) = write_id(&path, &id) {
        log::warn!("could not save a new install id to {}: {e}", path.display());
    }
    id
}

/// Writes `id` through a sibling temp file and a rename, the way the queue
/// does, so a crash mid-write never leaves a half-written id behind.
fn write_id(path: &Path, id: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = path.with_extension("tmp");
    let mut tmp = File::create(&tmp_path)?;
    tmp.write_all(id.as_bytes())?;
    tmp.flush()?;
    drop(tmp);

    fs::rename(&tmp_path, path)
}

/// Whether this install has opted out of analytics. Any reason the marker
/// can't be read — it's missing, `dir` doesn't exist, permissions — reads
/// as "not opted out"; opting out is a deliberate act the plugin only
/// recognizes once it can actually see the marker.
#[must_use]
pub fn opted_out(dir: &Path) -> bool {
    dir.join(OPT_OUT_FILE).exists()
}

/// Persists whether this install has opted out, by creating or removing
/// the marker file.
///
/// # Errors
///
/// Returns an error if `dir` can't be created when opting out, or if the
/// marker file can't be written or removed.
pub fn set_opted_out(dir: &Path, out: bool) -> std::io::Result<()> {
    let path = dir.join(OPT_OUT_FILE);
    if out {
        fs::create_dir_all(dir)?;
        File::create(&path)?;
        Ok(())
    } else {
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{install_id, opted_out, set_opted_out};
    use std::fs;
    use uuid::Uuid;

    #[test]
    fn install_id_is_a_v4_uuid() {
        let dir = tempfile::tempdir().unwrap();
        let id = install_id(dir.path());
        assert_eq!(Uuid::parse_str(&id).unwrap().get_version_num(), 4);
    }

    #[test]
    fn the_same_dir_gives_the_same_id_twice() {
        let dir = tempfile::tempdir().unwrap();
        let first = install_id(dir.path());
        let second = install_id(dir.path());
        assert_eq!(first, second);
    }

    #[test]
    fn the_id_survives_being_read_back_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let id = install_id(dir.path());

        let saved = fs::read_to_string(dir.path().join("posthog-id")).unwrap();
        assert_eq!(saved.trim(), id);

        // A fresh call, as if from a new process, still finds it.
        assert_eq!(install_id(dir.path()), id);
    }

    #[test]
    fn two_dirs_get_two_ids() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        assert_ne!(install_id(a.path()), install_id(b.path()));
    }

    #[test]
    fn a_garbage_id_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("posthog-id"), "not-a-uuid").unwrap();

        let id = install_id(dir.path());
        assert_eq!(Uuid::parse_str(&id).unwrap().get_version_num(), 4);

        // The replacement was saved, not just returned for this call.
        assert_eq!(install_id(dir.path()), id);
    }

    #[test]
    fn an_unwritable_dir_still_returns_a_fresh_id_for_this_run() {
        let root = tempfile::tempdir().unwrap();
        // A regular file standing where a directory is expected makes
        // `create_dir_all` fail, portably, without touching permissions.
        let blocker = root.path().join("blocker");
        fs::write(&blocker, b"not a directory").unwrap();
        let unwritable_dir = blocker.join("sub");

        let id = install_id(&unwritable_dir);
        assert_eq!(Uuid::parse_str(&id).unwrap().get_version_num(), 4);
        assert!(!unwritable_dir.join("posthog-id").exists());
    }

    #[test]
    fn opted_out_is_false_by_default() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!opted_out(dir.path()));
    }

    #[test]
    fn opting_out_and_back_in_persists() {
        let dir = tempfile::tempdir().unwrap();

        set_opted_out(dir.path(), true).unwrap();
        assert!(opted_out(dir.path()));

        set_opted_out(dir.path(), false).unwrap();
        assert!(!opted_out(dir.path()));
    }

    #[test]
    fn opting_out_twice_and_in_when_never_out_are_both_fine() {
        let dir = tempfile::tempdir().unwrap();

        set_opted_out(dir.path(), true).unwrap();
        set_opted_out(dir.path(), true).unwrap();
        assert!(opted_out(dir.path()));

        set_opted_out(dir.path(), false).unwrap();
        set_opted_out(dir.path(), false).unwrap();
        assert!(!opted_out(dir.path()));
    }
}
