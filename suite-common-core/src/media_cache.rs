//! Where pictures unpacked from a document live while it is open (#455).
//!
//! A reader that meets an embedded image has to hand the model a *path*:
//! the models store `String` paths, and the renderers and writers read the
//! bytes back from there later. So the bytes have to outlive the parse, and
//! nothing owns them afterwards.
//!
//! The readers used to write each picture with `NamedTempFile::new()` and
//! then `keep()` it (#271, fixing the predictable names of #268). `keep()`
//! turns delete-on-drop off, so every picture of every open left a file in
//! `/tmp` that nothing ever removed: one file per image *per open*.
//!
//! This module keeps the security properties of #271 and bounds the growth:
//!
//! - **One private directory per process.** Created exclusively under an
//!   unpredictable name, with mode 0700, so no other user can
//!   pre-create, read or symlink anything inside it. Files inside it still
//!   get unpredictable names; nothing about a path comes from the document.
//! - **Identical bytes are stored once per process.** Reopening a document,
//!   or a picture repeated across slides, reuses the file already written.
//!   A hash only picks candidates; the bytes are compared before a file is
//!   reused, so a collision costs a second file, never the wrong picture.
//! - **A dead process's directory is removed by the next one.** The owning
//!   process holds an advisory lock on `.owner.lock` inside its directory
//!   for as long as it runs; the kernel drops it when the process dies,
//!   however it dies. The first picture a later process unpacks sweeps
//!   every `office-media-*` directory in the temp dir that this user owns
//!   and that nobody holds that lock on. This is the same "ask, don't
//!   guess" liveness test `atomic_save` uses for stranded save temporaries
//!   and `AutosaveSlot::claim` uses for crashed windows' snapshots.
//!
//! A directory is not deleted at exit: the paths are the model's for as
//! long as the process lives, and a GTK app does not reliably run
//! destructors on the way out. The sweep is what bounds the total.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Prefix of every per-process directory. The sweep only ever looks at
/// entries carrying it.
pub const DIR_PREFIX: &str = "office-media-";

/// The lock file whose advisory lock says "this directory's owner is alive".
const LOCK_NAME: &str = ".owner.lock";

/// One process's unpacked pictures.
pub struct MediaCache {
    dir: PathBuf,
    /// Held, locked, for the cache's lifetime. `None` when the filesystem
    /// could not lock: the directory is then never swept, which is the safe
    /// direction to fail in.
    _lock: Option<fs::File>,
    by_hash: HashMap<u64, Vec<PathBuf>>,
}

impl MediaCache {
    /// Create a fresh private directory under `base`, first sweeping any
    /// directory there that a dead process left behind.
    pub fn new_in(base: &Path) -> io::Result<MediaCache> {
        sweep_orphans_in(base);
        let mut builder = tempfile::Builder::new();
        builder.prefix(DIR_PREFIX);
        // tempfile creates directories with the process umask (0755 in
        // practice), not 0700 — measured, not assumed — so ask for it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(fs::Permissions::from_mode(0o700));
        }
        let dir = builder.tempdir_in(base)?.keep();
        let lock = claim(&dir);
        Ok(MediaCache { dir, _lock: lock, by_hash: HashMap::new() })
    }

    /// The directory this cache writes into.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Store `bytes` and return a path that holds exactly them for as long
    /// as this cache lives. Identical bytes return the same path.
    pub fn persist(&mut self, bytes: &[u8]) -> io::Result<PathBuf> {
        let key = hash(bytes);
        if let Some(candidates) = self.by_hash.get(&key) {
            for path in candidates {
                // Something outside this process could only have changed a
                // file here by being this user, but a reused path must hold
                // these bytes, so check rather than assume.
                if fs::read(path).is_ok_and(|on_disk| on_disk == bytes) {
                    return Ok(path.clone());
                }
            }
        }
        let mut tmp = tempfile::Builder::new().prefix("img-").tempfile_in(&self.dir)?;
        tmp.write_all(bytes)?;
        let (_, path) = tmp.keep().map_err(|e| e.error)?;
        self.by_hash.entry(key).or_default().push(path.clone());
        Ok(path)
    }
}

impl Drop for MediaCache {
    fn drop(&mut self) {
        // The process-wide cache is never dropped; this is for caches with
        // a shorter life (tests, tools), which should not leave a directory
        // for the next sweep to find.
        let _ = fs::remove_dir_all(&self.dir);
    }
}

static PROCESS_CACHE: Mutex<Option<MediaCache>> = Mutex::new(None);

/// Store an unpacked picture in this process's cache under the system temp
/// directory and return its path. See the module docs.
pub fn persist(bytes: &[u8]) -> io::Result<PathBuf> {
    let mut guard = PROCESS_CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_none() {
        // Retried on the next call if it fails: a temp dir that was
        // briefly unwritable must not disable pictures for the session.
        *guard = Some(MediaCache::new_in(&std::env::temp_dir())?);
    }
    guard.as_mut().expect("initialised above").persist(bytes)
}

/// The process cache's directory, if a picture has been unpacked yet.
pub fn process_dir() -> Option<PathBuf> {
    let guard = PROCESS_CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.as_ref().map(|cache| cache.dir.clone())
}

fn hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// Create and lock the directory's lock file.
///
/// The lock file is created under another name, locked, and only then
/// renamed into place: `flock` belongs to the open file, so the lock
/// survives the rename, and a sweep can therefore never find a
/// `.owner.lock` that exists but is not yet locked and mistake a
/// directory being born for one whose owner died.
fn claim(dir: &Path) -> Option<fs::File> {
    let pending = dir.join(".owner.lock.new");
    let file = fs::OpenOptions::new().write(true).create_new(true).open(&pending).ok()?;
    if !matches!(crate::autosave::try_lock_exclusive(&file), Ok(true)) {
        let _ = fs::remove_file(&pending);
        return None;
    }
    fs::rename(&pending, dir.join(LOCK_NAME)).ok()?;
    Some(file)
}

/// Remove every cache directory under `base` whose owner is gone.
///
/// Deliberately narrow, because it deletes things: only directories (not
/// symlinks) named with `DIR_PREFIX`, only ones this user owns, and only
/// ones whose lock file exists and could be locked — which the kernel
/// allows only once the owner has died. A directory with no lock file, or
/// a lock that cannot be asked about, is left alone. Errors are ignored,
/// so a sweep can never stop a picture from opening.
///
/// Off Unix there is no advisory lock to ask, so nothing is swept.
pub fn sweep_orphans_in(base: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let Ok(entries) = fs::read_dir(base) else { return };
        // SAFETY: geteuid has no preconditions and cannot fail.
        let me = unsafe { libc::geteuid() };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if !name.to_str().is_some_and(|n| n.starts_with(DIR_PREFIX)) {
                continue;
            }
            // `DirEntry::metadata` does not follow symlinks.
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_dir() || meta.uid() != me {
                continue;
            }
            let dir = entry.path();
            let Ok(lock) = fs::OpenOptions::new().read(true).write(true).open(dir.join(LOCK_NAME))
            else {
                continue;
            };
            if matches!(crate::autosave::try_lock_exclusive(&lock), Ok(true)) {
                let _ = fs::remove_dir_all(&dir);
            }
        }
    }
    #[cfg(not(unix))]
    let _ = base;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files_in(dir: &Path) -> Vec<PathBuf> {
        fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| !p.file_name().unwrap().to_string_lossy().starts_with(".owner"))
            .collect()
    }

    #[test]
    fn the_same_bytes_are_stored_once() {
        let base = tempfile::tempdir().unwrap();
        let mut cache = MediaCache::new_in(base.path()).unwrap();
        let first = cache.persist(b"a picture").unwrap();
        for _ in 0..20 {
            assert_eq!(cache.persist(b"a picture").unwrap(), first, "reopening grew the cache");
        }
        assert_eq!(files_in(cache.dir()).len(), 1);
        assert_eq!(fs::read(&first).unwrap(), b"a picture");
    }

    #[test]
    fn different_bytes_get_different_files() {
        let base = tempfile::tempdir().unwrap();
        let mut cache = MediaCache::new_in(base.path()).unwrap();
        let a = cache.persist(b"one").unwrap();
        let b = cache.persist(b"two").unwrap();
        assert_ne!(a, b);
        assert_eq!(fs::read(&a).unwrap(), b"one");
        assert_eq!(fs::read(&b).unwrap(), b"two");
    }

    #[test]
    fn a_reused_path_always_holds_the_requested_bytes() {
        // Stands in for a hash collision: a candidate whose bytes differ
        // must not be handed back.
        let base = tempfile::tempdir().unwrap();
        let mut cache = MediaCache::new_in(base.path()).unwrap();
        let first = cache.persist(b"original").unwrap();
        fs::write(&first, b"tampered").unwrap();
        let again = cache.persist(b"original").unwrap();
        assert_ne!(again, first);
        assert_eq!(fs::read(&again).unwrap(), b"original");
    }

    #[test]
    fn stored_files_are_inside_the_cache_and_not_named_by_content() {
        let base = tempfile::tempdir().unwrap();
        let mut cache = MediaCache::new_in(base.path()).unwrap();
        let path = cache.persist(b"x").unwrap();
        assert_eq!(path.parent().unwrap(), cache.dir());
        assert!(cache.dir().starts_with(base.path()));
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(!name.contains(&format!("{:x}", hash(b"x"))), "name derived from content: {name}");
    }

    #[cfg(unix)]
    #[test]
    fn the_directory_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir().unwrap();
        let cache = MediaCache::new_in(base.path()).unwrap();
        let mode = fs::metadata(cache.dir()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "cache dir mode {mode:o}");
    }

    #[cfg(unix)]
    #[test]
    fn a_dead_owners_directory_is_swept_and_a_live_ones_is_kept() {
        let base = tempfile::tempdir().unwrap();

        // Dead: a lock file nobody holds, as a crashed process leaves it.
        let dead = base.path().join(format!("{DIR_PREFIX}dead"));
        fs::create_dir(&dead).unwrap();
        fs::write(dead.join(LOCK_NAME), b"").unwrap();
        fs::write(dead.join("img-leftover"), b"old picture").unwrap();

        // Live: a cache that is still open. flock conflicts between two
        // open files even inside one process, which is what makes this a
        // faithful stand-in for another running window.
        let live = MediaCache::new_in(base.path()).unwrap();

        // No lock file at all: cannot prove the owner is gone, so kept.
        let unknown = base.path().join(format!("{DIR_PREFIX}unknown"));
        fs::create_dir(&unknown).unwrap();

        // Not ours by name.
        let other = base.path().join("something-else");
        fs::create_dir(&other).unwrap();
        fs::write(other.join(LOCK_NAME), b"").unwrap();

        let _next = MediaCache::new_in(base.path()).unwrap();

        assert!(!dead.exists(), "a dead process's pictures were left behind");
        assert!(live.dir().exists(), "a live process's pictures were deleted");
        assert!(unknown.exists(), "a directory with no owner lock was deleted");
        assert!(other.exists(), "a directory without the prefix was touched");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_with_the_prefix_is_not_followed() {
        let base = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        fs::write(target.path().join(LOCK_NAME), b"").unwrap();
        fs::write(target.path().join("precious"), b"keep me").unwrap();
        std::os::unix::fs::symlink(target.path(), base.path().join(format!("{DIR_PREFIX}link")))
            .unwrap();

        sweep_orphans_in(base.path());

        assert_eq!(fs::read(target.path().join("precious")).unwrap(), b"keep me");
    }

    #[test]
    fn dropping_a_cache_removes_its_directory() {
        let base = tempfile::tempdir().unwrap();
        let dir = {
            let mut cache = MediaCache::new_in(base.path()).unwrap();
            cache.persist(b"x").unwrap();
            cache.dir().to_path_buf()
        };
        assert!(!dir.exists());
    }

    #[test]
    fn the_process_cache_reuses_paths() {
        let a = persist(b"process-wide picture").unwrap();
        let b = persist(b"process-wide picture").unwrap();
        assert_eq!(a, b);
        let dir = process_dir().expect("the cache exists once something was stored");
        assert!(a.starts_with(&dir));
        assert!(dir.starts_with(std::env::temp_dir()));
    }
}
