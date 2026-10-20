//! The plugin's small store: a few named text files for JS, and the install
//! id. JS decides what is good enough to keep. Rust only keeps what it is
//! given, in a folder of its own.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use uuid::Uuid;

const ID_FILE: &str = "install-id";

/// The store's folder under the app's data folder. On Android, Tauri gives the
/// app's `dataDir`, and Android leaves `no_backup` out of its automatic backup:
/// a restore must never bring another install's id to a new phone.
#[must_use]
pub fn dir_for(base: &Path, android: bool) -> PathBuf {
    if android {
        base.join("no_backup").join("remote-settings")
    } else {
        base.join("remote-settings")
    }
}

/// A name JS may use: 1 to 40 of `a-z`, `0-9` and `-`, so no name reaches
/// outside the store's folder.
#[must_use]
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Each async command runs on its own task, so two writes of one name can
/// overlap. The lock keeps one rename from landing inside another write.
pub struct Store {
    dir: Option<PathBuf>,
    lock: Mutex<()>,
}

static TEMP: AtomicU64 = AtomicU64::new(0);

impl Store {
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir: Some(dir),
            lock: Mutex::new(()),
        }
    }

    /// A store that keeps nothing, for when the app's data folder is unknown:
    /// the app still starts, on its defaults.
    #[must_use]
    pub fn without_folder() -> Self {
        Self {
            dir: None,
            lock: Mutex::new(()),
        }
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<String> {
        if !valid_name(name) {
            return None;
        }
        fs::read_to_string(self.dir.as_ref()?.join(name)).ok()
    }

    /// Writes through a temp file and a rename, so a crash mid-write never
    /// leaves half a file.
    ///
    /// # Errors
    ///
    /// Returns an error for a refused name, or when the folder or file cannot
    /// be written.
    pub fn set(&self, name: &str, text: &str) -> std::io::Result<()> {
        let Some(dir) = self.dir.as_ref().filter(|_| valid_name(name)) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "refused name, or no folder",
            ));
        };
        let _held = self
            .lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        write_atomic(&dir.join(name), text)
    }

    /// The first good id: the kept one, else `seed`, else a new one. None when
    /// the id cannot be kept: an id made new at each launch would move the
    /// user between test groups.
    #[must_use]
    pub fn install_id(&self, seed: Option<&str>) -> Option<String> {
        if let Some(kept) = self.get(ID_FILE) {
            if let Ok(id) = Uuid::parse_str(kept.trim()) {
                return Some(id.to_string());
            }
        }
        let id = seed
            .and_then(|s| Uuid::parse_str(s.trim()).ok())
            .filter(|u| u.get_version_num() == 4)
            .unwrap_or_else(Uuid::new_v4)
            .to_string();
        match self.set(ID_FILE, &id) {
            Ok(()) => Some(id),
            Err(e) => {
                log::warn!("remote-settings: could not keep the install id: {e}");
                None
            }
        }
    }
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let n = TEMP.fetch_add(1, Ordering::Relaxed);
    let tmp_path = path.with_extension(format!("{}.{n}.tmp", std::process::id()));
    let mut tmp = File::create(&tmp_path)?;
    tmp.write_all(text.as_bytes())?;
    tmp.sync_all()?;
    drop(tmp);
    fs::rename(&tmp_path, path)
}

#[cfg(test)]
mod tests {
    use super::{dir_for, valid_name, Store};
    use std::path::PathBuf;

    const SEED: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";

    #[test]
    fn a_name_with_a_slash_or_dots_is_refused() {
        assert!(valid_name("state"));
        assert!(valid_name("install-id"));
        assert!(!valid_name("../x"));
        assert!(!valid_name("a/b"));
        assert!(!valid_name("a.b"));
        assert!(!valid_name(""));
        assert!(!valid_name("State"));
        assert!(!valid_name(&"a".repeat(41)));
    }

    #[test]
    fn a_refused_name_reads_nothing_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("s"));
        assert!(store.set("../escape", "x").is_err());
        assert_eq!(store.get("../escape"), None);
        assert!(!dir.path().join("escape").exists());
    }

    #[test]
    fn set_then_get_gives_the_text_back() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("s"));
        assert_eq!(store.get("state"), None);
        store.set("state", "hello").unwrap();
        assert_eq!(store.get("state").as_deref(), Some("hello"));
        store.set("state", "again").unwrap();
        assert_eq!(store.get("state").as_deref(), Some("again"));
    }

    #[test]
    fn the_install_id_is_kept_across_calls() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("s"));
        let id = store.install_id(None).unwrap();
        assert!(uuid::Uuid::parse_str(&id).is_ok());
        assert_eq!(store.install_id(None).as_deref(), Some(id.as_str()));
        assert_eq!(
            Store::new(dir.path().join("s")).install_id(Some(SEED)),
            Some(id)
        );
    }

    #[test]
    fn a_seed_id_is_kept_when_there_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("s"));
        assert_eq!(store.install_id(Some(SEED)).as_deref(), Some(SEED));
        assert_eq!(store.install_id(None).as_deref(), Some(SEED));
    }

    #[test]
    fn a_bad_seed_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("s"));
        let id = store.install_id(Some("not-a-uuid")).unwrap();
        assert_ne!(id, "not-a-uuid");
        assert!(uuid::Uuid::parse_str(&id).is_ok());
    }

    #[test]
    fn an_unwritable_folder_gives_no_install_id() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a-file");
        std::fs::write(&file, "x").unwrap();
        let store = Store::new(file.join("s"));
        assert_eq!(store.install_id(None), None);
    }

    #[test]
    fn writers_at_the_same_time_never_mix_their_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = std::sync::Arc::new(Store::new(dir.path().join("s")));
        let texts: Vec<String> = (0..8).map(|i| format!("{i}").repeat(20_000)).collect();
        std::thread::scope(|s| {
            for t in &texts {
                let store = store.clone();
                s.spawn(move || {
                    for _ in 0..50 {
                        store.set("state", t).unwrap();
                    }
                });
            }
        });
        let kept = store.get("state").unwrap();
        assert!(texts.contains(&kept));
        let left: Vec<_> = std::fs::read_dir(dir.path().join("s")).unwrap().collect();
        assert_eq!(left.len(), 1);
    }

    #[test]
    fn a_store_with_no_folder_keeps_nothing_and_gives_no_id() {
        let store = Store::without_folder();
        assert!(store.set("state", "x").is_err());
        assert_eq!(store.get("state"), None);
        assert_eq!(store.install_id(None), None);
    }

    #[test]
    fn android_keeps_the_store_out_of_its_backup() {
        let base = PathBuf::from("/data/user/0/app");
        assert_eq!(
            dir_for(&base, true),
            PathBuf::from("/data/user/0/app/no_backup/remote-settings")
        );
        assert_eq!(
            dir_for(&base, false),
            PathBuf::from("/data/user/0/app/remote-settings")
        );
    }
}
