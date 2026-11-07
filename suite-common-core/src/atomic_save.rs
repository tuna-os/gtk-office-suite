// atomic_save.rs — Crash-safe file writes shared across Letters, Tables, and Decks.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Every app format writer should produce a complete in-memory buffer first
// (no streaming a ZipWriter straight into the destination — a rename before
// the central directory is flushed leaves a corrupt archive) and hand it to
// `atomic_write_bytes`, which never touches the destination path until the
// full write has succeeded.

use std::fs;
use std::io::Write;
use std::path::Path;

/// Test-only fault injection at each transaction boundary.
///
/// The durability promises in this module are all of the form "if it fails
/// *here*, the destination still holds X" — and every one of them used to
/// be a comment. This makes them executable: a test arms a boundary, the
/// write fails exactly there, and the test asserts what the caller was
/// promised (#442, `docs/readiness-2026-09/crash-stress.md`).
///
/// Compiled only under `cfg(test)`, so a release build contains no hook,
/// no branch and no atomic to read. The armed boundary is thread-local
/// because the test suite runs in parallel and a process-wide switch would
/// make one test's fault another test's flake.
#[cfg(test)]
pub(crate) mod fault {
    use std::cell::Cell;

    /// The points at which `atomic_write_bytes` can fail, in the order it
    /// reaches them. Everything before `Persist` leaves the destination
    /// untouched; `SyncDir` is the one boundary that fails *after* the new
    /// bytes are in place.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Boundary {
        CreateTemp,
        Permissions,
        WriteData,
        SyncData,
        Persist,
        SyncDir,
    }

    /// Every boundary, for a test that wants to sweep them all.
    pub const ALL: [Boundary; 6] = [
        Boundary::CreateTemp,
        Boundary::Permissions,
        Boundary::WriteData,
        Boundary::SyncData,
        Boundary::Persist,
        Boundary::SyncDir,
    ];

    thread_local! {
        /// The armed boundary and how many more times it must be reached
        /// before it fails. A countdown rather than a flag because one
        /// logical transaction can span several writes — a snapshot is
        /// bytes *and* metadata — and "fail the second commit" is the
        /// interesting case there.
        static ARMED: Cell<Option<(Boundary, u32)>> = const { Cell::new(None) };
    }

    /// Arm `boundary` for this thread until the returned guard drops, so a
    /// panicking assertion cannot leave the fault armed for the next test
    /// on the same thread.
    pub fn arm(boundary: Boundary) -> Guard {
        arm_nth(boundary, 1)
    }

    /// Fail the `nth` (1-based) arrival at `boundary` and let the earlier
    /// ones through.
    pub fn arm_nth(boundary: Boundary, nth: u32) -> Guard {
        assert!(nth >= 1, "arrivals are 1-based");
        ARMED.with(|a| a.set(Some((boundary, nth))));
        Guard
    }

    pub struct Guard;

    impl Drop for Guard {
        fn drop(&mut self) {
            ARMED.with(|a| a.set(None));
        }
    }

    /// An `io::Error`, not a string, so an injected fault flows through
    /// the same `map_err` the real failure would: the caller-facing
    /// wording is part of the promise at some boundaries, and a test that
    /// asserted on an injection-specific message would not be testing it.
    pub(super) fn check(boundary: Boundary) -> std::io::Result<()> {
        ARMED.with(|a| match a.get() {
            Some((armed, 1)) if armed == boundary => {
                a.set(None);
                Err(std::io::Error::other(format!("injected fault at {boundary:?}")))
            }
            Some((armed, remaining)) if armed == boundary => {
                a.set(Some((armed, remaining - 1)));
                Ok(())
            }
            _ => Ok(()),
        })
    }
}

/// Run `op`, but fail first when a test has armed this boundary. Outside
/// `cfg(test)` this expands to `op` and nothing else.
#[cfg(test)]
macro_rules! fail_point {
    ($boundary:expr, $op:expr) => {
        fault::check($boundary).and_then(|()| $op)
    };
}

#[cfg(not(test))]
macro_rules! fail_point {
    ($boundary:expr, $op:expr) => {
        $op
    };
}

/// The prefix every save temporary carries. Named because two places need
/// to agree on it: the writer that creates them and the sweep that clears
/// the ones a crash stranded.
pub(crate) const TEMP_PREFIX: &str = ".office-save-";

/// A floor on how new a temporary can be and still be swept.
///
/// This is not the liveness test — `flock` below is — it closes the gap
/// *before* the lock exists. A save creates its temporary and then locks
/// it, and in between the file is on disk with nothing holding it, which
/// is exactly what a stranded one looks like. A second window sweeping at
/// that instant would delete a temporary the first window is about to
/// fill. A minute is enormous next to the microseconds between those two
/// syscalls, and it only delays cleaning up after a crash by however long
/// it takes the next save to come along.
const SWEEP_RACE_FLOOR: std::time::Duration = std::time::Duration::from_secs(60);

/// Remove save temporaries a crash stranded in `dir`.
///
/// `atomic_write_bytes` writes to a temporary and renames it over the
/// destination, and `tempfile` removes that temporary on drop — which
/// covers every *error*. It covers no *kill*: SIGKILL, an OOM kill and the
/// power going out run no destructor, so the temporary survives with up to
/// a whole document's worth of bytes in it, in the user's own document
/// directory, named with a leading dot so they are unlikely ever to see
/// it. Nothing removed them, so they accumulated one per crashed save,
/// forever.
///
/// The fault-injection sweep in this module could not have found that: it
/// makes the write *fail*, and a failing write unwinds and drops the
/// temporary. Only a real kill leaves one behind. Measured — SIGKILL
/// during a 600 MiB save left a 78 MiB `.office-save-…` beside an intact
/// destination — and recorded in
/// `docs/readiness-2026-09/recovery.md`.
///
/// Whether a temporary is stranded is asked, not guessed: every live save
/// holds an advisory lock on its temporary for as long as it is writing,
/// and the kernel releases that lock when the process dies however it
/// dies. So a temporary nobody can lock is one nobody owns. That is the
/// same mechanism `AutosaveSlot::claim` uses to tell a crashed window's
/// snapshot from a live one's.
///
/// Deliberately narrow, because this deletes files in a directory the user
/// owns: only regular files, not symlinks and not directories; only names
/// carrying the prefix *this* module writes; only ones nothing holds a
/// lock on; and only ones past `SWEEP_RACE_FLOOR`. Anything it cannot
/// establish, it leaves — including a lock it could not ask about. Errors
/// are ignored throughout, so an unreadable or read-only directory cannot
/// fail the save that just succeeded.
fn sweep_stranded_temps(dir: &Path, now: std::time::SystemTime) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name.starts_with(TEMP_PREFIX) {
            continue;
        }
        // `DirEntry::metadata` does not traverse, so this is the entry
        // itself: a symlink carrying the prefix is not a temporary we
        // wrote, and `remove_file` would unlink it happily.
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let past_the_floor = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age >= SWEEP_RACE_FLOOR);
        if !past_the_floor {
            continue;
        }
        let path = entry.path();
        let Ok(handle) = fs::OpenOptions::new().read(true).write(true).open(&path) else {
            continue;
        };
        // Ok(true): nothing held it, so nothing owns it. Ok(false): a live
        // save does. Err: could not ask, so this is not ours to delete.
        if matches!(crate::autosave::try_lock_exclusive(&handle), Ok(true)) {
            let _ = fs::remove_file(&path);
        }
    }
}

/// Whether a save of `path` should hold at `stage` for a GUI journey to
/// kill the real app there (#1217). Only in test mode
/// (`GTK_OFFICE_TEST_MODE`, which already gates the apps' other test-only
/// actions), and only when `GTK_OFFICE_TEST_SAVE_PAUSE` names this stage and
/// this file as `<stage>:<file name>`, so an autosave in the same process
/// never stalls.
///
/// `atomic_save::fault` arms every boundary, but in-process and in tests
/// only: it can fail a write, not kill a process, and a kill is the case
/// that runs no error path and no destructor. Racing a real save with a
/// kill reaches the window only for a document whose write is slow, and a
/// save builds its bytes first, so the temporary exists for milliseconds.
fn test_pause_requested(test_mode: bool, spec: Option<&str>, stage: &str, path: &Path) -> bool {
    let Some((at, name)) = spec.and_then(|spec| spec.split_once(':')) else {
        return false;
    };
    test_mode && at == stage && path.file_name().is_some_and(|file| file == name)
}

/// How long a requested pause holds: far longer than a journey needs to see
/// the temporary and kill the app.
const TEST_PAUSE: std::time::Duration = std::time::Duration::from_secs(20);

fn test_pause(stage: &str, path: &Path) {
    let spec = std::env::var("GTK_OFFICE_TEST_SAVE_PAUSE").ok();
    if test_pause_requested(std::env::var_os("GTK_OFFICE_TEST_MODE").is_some(), spec.as_deref(), stage, path) {
        std::thread::sleep(TEST_PAUSE);
    }
}

/// Identity and content stamp of a destination file, captured when a document
/// is opened or saved so a later save can tell whether something else
/// replaced or rewrote it in between (SH-002).
///
/// On Unix this is device + inode + mtime (nanosecond resolution) + size.
/// The inode is what catches "unlinked and recreated" — e.g. another window's
/// atomic save, which renames a new file over the path and so always yields a
/// new inode. Size and mtime catch in-place rewrites. On other platforms only
/// mtime and size are compared, which is weaker: a same-size rewrite within
/// the filesystem's mtime granularity is invisible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileVersion {
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
    mtime: Option<std::time::SystemTime>,
    size: u64,
}

impl FileVersion {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        FileVersion {
            #[cfg(unix)]
            dev: metadata.dev(),
            #[cfg(unix)]
            ino: metadata.ino(),
            mtime: metadata.modified().ok(),
            size: metadata.len(),
        }
    }

    /// The current version of the file at `path` (following symlinks, like
    /// the save itself does when it inspects the destination), or `None`
    /// when nothing is there.
    pub fn capture(path: &Path) -> std::io::Result<Option<FileVersion>> {
        match fs::metadata(path) {
            Ok(metadata) => Ok(Some(FileVersion::from_metadata(&metadata))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Why a save did not (fully) succeed, structured so a caller can tell the
/// three cases apart without parsing text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    /// The destination no longer matches the version the caller expected.
    /// Nothing was written; the destination and its directory are untouched.
    PreconditionFailed,
    /// The save failed before the new bytes replaced the destination, which
    /// still holds whatever it held before.
    FailedBeforeCommit(String),
    /// The new bytes *are* at the destination, but syncing the directory
    /// entry failed, so durability is unconfirmed. The caller must not treat
    /// the old content as current, nor the document as durably saved.
    CommittedWithDirSyncFailure(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::PreconditionFailed => {
                f.write_str("The file was changed on disk since it was opened; it was not overwritten")
            }
            SaveError::FailedBeforeCommit(msg) | SaveError::CommittedWithDirSyncFailure(msg) => {
                f.write_str(msg)
            }
        }
    }
}

impl std::error::Error for SaveError {}

/// Write `bytes` to `path` atomically: write to a temporary file in the same
/// directory, flush and sync it to disk, then rename it over the
/// destination. `rename` within one filesystem is atomic, so a reader can
/// never observe a partially-written file, and a failure at any point before
/// the rename leaves the original untouched.
///
/// No precondition is checked: this overwrites whatever is at `path`. Use
/// [`atomic_write_bytes_conditional`] to refuse overwriting a file that
/// changed since it was opened.
pub fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    atomic_write_impl(path, bytes, None)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Like [`atomic_write_bytes`], but only replaces the destination if it still
/// has the `expected` version; `None` means the destination is expected not
/// to exist. On success returns the version of the file just written, taken
/// from the temporary before the rename (rename preserves inode, mtime and
/// size), so it describes what this call wrote even if something else
/// replaces the path a moment later.
///
/// On [`SaveError::PreconditionFailed`] the destination was not modified and
/// the temporary has been removed.
///
/// **Residual race.** The version is checked immediately before the rename,
/// but check-then-rename is not atomic: POSIX offers no compare-and-rename,
/// so an external writer that changes or replaces the destination in the
/// window between the check and the rename is silently overwritten. This
/// narrows the window from "the whole time the document was open" to
/// microseconds; it does not close it. Closing it needs cooperative locking
/// between writers, which this function does not provide.
pub fn atomic_write_bytes_conditional(
    path: &Path,
    bytes: &[u8],
    expected: Option<&FileVersion>,
) -> Result<FileVersion, SaveError> {
    atomic_write_impl(path, bytes, Some(expected))
}

/// `precondition`: `None` = unconditional; `Some(expected)` = the destination
/// must currently be `expected` (`None` inside = absent).
fn atomic_write_impl(
    path: &Path,
    bytes: &[u8],
    precondition: Option<Option<&FileVersion>>,
) -> Result<FileVersion, SaveError> {
    use SaveError::FailedBeforeCommit;
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    path.file_name()
        .ok_or_else(|| FailedBeforeCommit("save path has no file name".to_string()))?;
    // Exclusive creation prevents concurrent saves or stale symlinks from
    // sharing/truncating a temporary file. Drop cleans up only our own file.
    let mut tmp = fail_point!(
        fault::Boundary::CreateTemp,
        tempfile::Builder::new().prefix(TEMP_PREFIX).tempfile_in(dir)
    )
    .map_err(|e| FailedBeforeCommit(format!("Failed to create save file: {e}")))?;
    // Claim the temporary for as long as this write lasts, so another
    // window's sweep can tell it apart from one a crash stranded. The
    // kernel drops this when the process does, however it does, which is
    // the whole point — a lock that outlived its owner would strand
    // temporaries just as permanently as no lock at all. Best-effort: a
    // filesystem that cannot lock must not fail the save, it only means a
    // sweep will wait out the race floor instead of asking.
    let _claim = crate::autosave::try_lock_exclusive(tmp.as_file());
    match fs::metadata(path) {
        Ok(metadata) => fail_point!(
            fault::Boundary::Permissions,
            tmp.as_file().set_permissions(metadata.permissions())
        )
        .map_err(|e| FailedBeforeCommit(format!("Failed to preserve file permissions: {e}")))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(FailedBeforeCommit(format!("Failed to inspect destination: {e}"))),
    }
    test_pause("created", path);
    fail_point!(fault::Boundary::WriteData, tmp.write_all(bytes))
        .map_err(|e| FailedBeforeCommit(format!("Failed to write file: {e}")))?;
    fail_point!(fault::Boundary::SyncData, tmp.as_file().sync_all())
        .map_err(|e| FailedBeforeCommit(format!("Failed to sync file: {e}")))?;
    test_pause("written", path);
    // The version this save produces. Rename keeps inode, mtime and size, and
    // nothing writes to the temporary after the sync above.
    let written = tmp
        .as_file()
        .metadata()
        .map(|m| FileVersion::from_metadata(&m))
        .map_err(|e| FailedBeforeCommit(format!("Failed to inspect saved file: {e}")))?;
    // As late as possible: after the slow work (write + fsync, and any pause
    // a journey holds), immediately before the rename. See the residual-race
    // note on `atomic_write_bytes_conditional`. A mismatch returns here and
    // `tmp` drops, removing the temporary; the destination is never touched.
    if let Some(expected) = precondition {
        let current = FileVersion::capture(path)
            .map_err(|e| FailedBeforeCommit(format!("Failed to inspect destination: {e}")))?;
        if current.as_ref() != expected {
            return Err(SaveError::PreconditionFailed);
        }
    }
    fail_point!(fault::Boundary::Persist, Ok(()))
        .and_then(|()| tmp.persist(path).map_err(std::io::Error::from))
        .map_err(|e| FailedBeforeCommit(format!("Failed to finalize save: {e}")))?;
    // Persist the directory entry as well as the data before reporting a
    // durable save. If this fails, the new file is present but durability
    // isn't confirmed; callers must retain dirty state/recovery data.
    #[cfg(unix)]
    fail_point!(
        fault::Boundary::SyncDir,
        fs::File::open(dir).and_then(|dir| dir.sync_all())
    )
    .map_err(|e| {
        SaveError::CommittedWithDirSyncFailure(format!(
            "File replaced, but failed to sync its directory: {e}"
        ))
    })?;
    // Only once the save has succeeded: a sweep is housekeeping, and doing
    // it first would spend time and risk on the path that still has the
    // user's document to save.
    sweep_stranded_temps(dir, std::time::SystemTime::now());
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pause is asked for by stage and file name, and only in test mode.
    #[test]
    fn a_test_pause_needs_test_mode_its_stage_and_its_file() {
        let path = Path::new("/docs/large.xlsx");
        assert!(test_pause_requested(true, Some("written:large.xlsx"), "written", path));
        assert!(!test_pause_requested(false, Some("written:large.xlsx"), "written", path), "outside test mode");
        assert!(!test_pause_requested(true, Some("written:large.xlsx"), "created", path), "another stage");
        assert!(!test_pause_requested(true, Some("written:other.xlsx"), "written", path), "another file");
        assert!(!test_pause_requested(true, Some("written"), "written", path), "no file named");
        assert!(!test_pause_requested(true, None, "written", path));
    }

    /// Serializes the one test that changes the process working directory.
    static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Backdate a file past the race floor without waiting a minute.
    fn backdate(path: &Path, by: std::time::Duration) {
        let when = std::time::SystemTime::now() - by;
        let handle = fs::OpenOptions::new().write(true).open(path).unwrap();
        handle.set_times(fs::FileTimes::new().set_modified(when)).unwrap();
    }

    /// Plant something that looks exactly like what a crash leaves: a
    /// prefixed file, old enough to be past the race floor, held by
    /// nobody.
    fn plant_stranded(dir: &Path, suffix: &str) -> std::path::PathBuf {
        let p = dir.join(format!("{TEMP_PREFIX}{suffix}"));
        fs::write(&p, b"a document's worth of bytes, in spirit").unwrap();
        backdate(&p, SWEEP_RACE_FLOOR * 2);
        p
    }

    /// A crash during a save strands its temporary, and nothing used to
    /// remove it.
    ///
    /// This is the case the fault-injection sweep below cannot reach. It
    /// makes the write *fail*, and a failing write unwinds and drops the
    /// temporary — so "no temporary left behind" was true of every error
    /// and false of every kill. Measured before this existed: SIGKILL
    /// during a 600 MiB save left a 78 MiB `.office-save-J6zNaJ` next to
    /// the document, with the destination intact. The atomicity promise
    /// held; the cleanup promise did not.
    #[test]
    fn a_stranded_temporary_is_swept_by_the_next_save() {
        let dir = tempfile::tempdir().unwrap();
        let stranded = plant_stranded(dir.path(), "aBcDeF");

        let path = dir.path().join("doc.txt");
        atomic_write_bytes(&path, b"new document").unwrap();

        assert!(!stranded.exists(), "a crash's leftover temporary was kept forever");
        assert_eq!(fs::read(&path).unwrap(), b"new document", "the save itself must still land");
    }

    /// And a temporary a live save is still filling must survive — which
    /// is the assertion the lock exists for, so the test holds a real
    /// lock rather than relying on the file being new.
    ///
    /// Getting this wrong in the other direction is the worse bug: a
    /// second window's sweep deleting the temporary the first window is
    /// mid-write on would turn a tidy-up into data loss. Backdating the
    /// file past the race floor is what makes the lock the only thing
    /// standing between the sweep and that file — without it the floor
    /// alone would pass this test, while protecting nothing a minute
    /// later.
    #[cfg(unix)]
    #[test]
    fn a_temporary_a_live_save_holds_is_not_swept() {
        let dir = tempfile::tempdir().unwrap();
        let live = plant_stranded(dir.path(), "zYxWvU");
        let held = fs::OpenOptions::new().read(true).write(true).open(&live).unwrap();
        assert!(
            matches!(crate::autosave::try_lock_exclusive(&held), Ok(true)),
            "precondition: this test has to actually hold the lock",
        );

        atomic_write_bytes(&dir.path().join("doc.txt"), b"new document").unwrap();

        assert!(
            live.exists(),
            "the sweep deleted a temporary a concurrent save was still writing",
        );
        drop(held);
    }

    /// The window between creating a temporary and locking it is real: for
    /// those microseconds the file is on disk holding no lock, which is
    /// indistinguishable from stranded. The race floor is what covers it,
    /// so a temporary nobody holds but which is *new* is still left alone.
    #[test]
    fn a_brand_new_unlocked_temporary_is_not_swept() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = dir.path().join(format!("{TEMP_PREFIX}justBorn"));
        fs::write(&fresh, b"created, not yet locked").unwrap();

        atomic_write_bytes(&dir.path().join("doc.txt"), b"new document").unwrap();

        assert!(
            fresh.exists(),
            "the sweep took a temporary created in the instant before its lock",
        );
    }

    /// Something else's old dotfile in the same directory is not ours to
    /// delete, however unlocked and however old.
    #[test]
    fn the_sweep_only_touches_our_own_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let others = [".other-editor-swap", "notes.txt", ".hidden-but-not-ours"];
        for name in others {
            let p = dir.path().join(name);
            fs::write(&p, b"not ours").unwrap();
            backdate(&p, SWEEP_RACE_FLOOR * 2);
        }

        atomic_write_bytes(&dir.path().join("doc.txt"), b"new document").unwrap();

        for name in others {
            assert!(dir.path().join(name).exists(), "the sweep deleted {name}");
        }
    }

    /// Something that carries the prefix but is not a file we wrote — and
    /// the save must succeed regardless, since a sweep that cannot remove
    /// something must never fail a save that already landed.
    ///
    /// The symlink is the case that makes `is_file` load-bearing, and it
    /// took a mutation to notice: dropping the check does *not* change
    /// what happens to a directory, because `remove_file` refuses one
    /// anyway. It changes what happens to a symlink, which `remove_file`
    /// will happily unlink. `DirEntry::metadata` does not traverse, so the
    /// check sees the link itself rather than its target.
    ///
    /// The clock is injected rather than the entries backdated, because a
    /// symlink's own mtime cannot be set through `std` — and with a
    /// real-time sweep the race floor would skip the link for being fresh,
    /// which is how this test would have passed without testing anything.
    #[cfg(unix)]
    #[test]
    fn prefixed_things_we_did_not_write_are_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("someones-file.txt");
        fs::write(&target, b"must survive").unwrap();
        let link = dir.path().join(format!("{TEMP_PREFIX}link"));
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let decoy = dir.path().join(format!("{TEMP_PREFIX}directory"));
        fs::create_dir(&decoy).unwrap();
        fs::write(decoy.join("inside"), b"someone's data").unwrap();

        // Far enough ahead that every entry is past the race floor, so
        // what is being tested is the kind check and nothing else.
        sweep_stranded_temps(dir.path(), std::time::SystemTime::now() + SWEEP_RACE_FLOOR * 2);

        assert!(link.is_symlink(), "the sweep unlinked a symlink it did not write");
        assert_eq!(fs::read(&target).unwrap(), b"must survive");
        assert!(decoy.is_dir(), "the sweep removed a directory it did not write");
        assert_eq!(fs::read(decoy.join("inside")).unwrap(), b"someone's data");

        let path = dir.path().join("doc.txt");
        atomic_write_bytes(&path, b"new document").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new document");
    }

    /// A save holds a lock on its temporary *while it is writing data* —
    /// the property the sweep reads. Asserted from outside, because the
    /// whole mechanism rests on another process being unable to take it.
    ///
    /// "While it is writing data" is the careful part, and the first
    /// version of this test got it wrong. It locked any temporary the
    /// moment one appeared, which asserts that a temporary is never
    /// lockable at all — and that is not what the code promises. The order
    /// is: create the temporary, *then* lock it. Between those two
    /// statements the file is on disk holding no lock, and a watcher that
    /// pounces on sight can win that window. It did, on a CI runner, at
    /// test 497 of 912 in a full parallel workspace run, while passing a
    /// dozen times in a row locally — load decides who wins.
    ///
    /// That window is not a bug to close; it is the window
    /// `SWEEP_RACE_FLOOR` exists for. So the test waits for evidence that
    /// the save has got past it: a non-zero length. `write_all` runs after
    /// the lock is taken, so any non-empty temporary is already locked, and
    /// a lock acquired on one is a real failure rather than a race.
    #[cfg(unix)]
    #[test]
    fn a_save_in_progress_holds_its_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<bool>::new()));
        let watcher = {
            let (dir, seen) = (dir.path().to_path_buf(), seen.clone());
            std::thread::spawn(move || {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                while std::time::Instant::now() < deadline {
                    for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
                        if !entry.file_name().to_string_lossy().starts_with(TEMP_PREFIX) {
                            continue;
                        }
                        // Empty means the save may not have reached its
                        // lock yet; that is the documented window, so it
                        // is not evidence either way. Keep looking.
                        if entry.metadata().map(|m| m.len()).unwrap_or(0) == 0 {
                            continue;
                        }
                        // The save may have persisted between the listing
                        // and this open, which is also not evidence.
                        if let Ok(f) =
                            fs::OpenOptions::new().read(true).write(true).open(entry.path())
                        {
                            let got = crate::autosave::try_lock_exclusive(&f);
                            seen.lock().unwrap().push(matches!(got, Ok(true)));
                            return;
                        }
                    }
                }
            })
        };
        // Big enough that the write dominates, so the watcher has time to
        // catch the temporary with bytes in it.
        let big = vec![b'x'; 64 * 1024 * 1024];
        atomic_write_bytes(&dir.path().join("doc.bin"), &big).unwrap();
        watcher.join().unwrap();

        let attempts = seen.lock().unwrap().clone();
        // Nothing observed makes this inconclusive, not passing — say so
        // rather than reporting a pass nobody earned.
        assert!(
            !attempts.is_empty(),
            "the watcher never saw a temporary with bytes in it, so this \
             proved nothing; raise the buffer size if this recurs",
        );
        assert!(
            attempts.iter().all(|locked| !locked),
            "a temporary was lockable while its save was writing data, so \
             the sweep cannot tell a live save from a crashed one",
        );
    }

    #[cfg(unix)]
    #[test]
    fn save_does_not_follow_a_stale_temporary_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.txt");
        let unrelated = dir.path().join("unrelated.txt");
        fs::write(&unrelated, b"must survive").unwrap();
        let stale = dir
            .path()
            .join(format!(".doc.txt.tmp-{}", std::process::id()));
        std::os::unix::fs::symlink(&unrelated, &stale).unwrap();

        atomic_write_bytes(&path, b"new document").unwrap();

        assert_eq!(fs::read(&unrelated).unwrap(), b"must survive");
        assert_eq!(fs::read(&path).unwrap(), b"new document");
        assert!(
            stale.is_symlink(),
            "save must not remove a file it did not create"
        );
    }

    #[cfg(unix)]
    #[test]
    fn save_preserves_existing_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private.txt");
        fs::write(&path, b"private").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

        atomic_write_bytes(&path, b"still private").unwrap();

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn failed_commit_preserves_destination_and_cleans_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("destination");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("original.txt"), b"original").unwrap();

        assert!(atomic_write_bytes(&path, b"replacement").is_err());

        assert_eq!(fs::read(path.join("original.txt")).unwrap(), b"original");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn writes_bytes_and_cleans_up_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.txt");
        atomic_write_bytes(&path, b"hello").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"hello");
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp file was not cleaned up: {leftovers:?}"
        );
    }

    #[test]
    fn overwrites_existing_file_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.txt");
        atomic_write_bytes(&path, b"version one").unwrap();
        atomic_write_bytes(&path, b"version two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"version two");
    }

    /// Fail at every transaction boundary in turn and check what the
    /// caller was promised at that point.
    ///
    /// This replaces a test that made the write fail by chmod-ing the
    /// directory to 0555, which proves nothing when the suite runs as
    /// root — the write simply succeeded and the assertion never fired.
    /// Arming a boundary fails deterministically for any user, and covers
    /// five boundaries the chmod reached only one of.
    #[test]
    fn every_pre_commit_boundary_leaves_the_original_intact() {
        for boundary in fault::ALL {
            if boundary == fault::Boundary::SyncDir {
                continue; // commits before it fails; see the next test
            }
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("doc.txt");
            fs::write(&path, b"original content").unwrap();

            let result = {
                let _armed = fault::arm(boundary);
                atomic_write_bytes(&path, b"new content that never lands")
            };

            assert!(result.is_err(), "{boundary:?} must report the failure");
            assert_eq!(
                fs::read(&path).unwrap(),
                b"original content",
                "{boundary:?} must leave the destination untouched"
            );
            let leftovers: Vec<_> = fs::read_dir(dir.path())
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.file_name())
                .filter(|name| name != "doc.txt")
                .collect();
            assert!(
                leftovers.is_empty(),
                "{boundary:?} left a temporary file behind: {leftovers:?}"
            );
        }
    }

    /// The one boundary that fails *after* the bytes are in place. The
    /// promise there is different and easy to get wrong: the save is not
    /// durable, but the new content is what a reader sees, so the error
    /// must not claim the original survived. Callers keep the document
    /// dirty and the recovery snapshot alive on this error.
    #[cfg(unix)]
    #[test]
    fn a_failure_after_the_rename_says_so_rather_than_claiming_a_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.txt");
        fs::write(&path, b"original content").unwrap();

        let err = {
            let _armed = fault::arm(fault::Boundary::SyncDir);
            atomic_write_bytes(&path, b"new content").unwrap_err()
        };

        assert_eq!(
            fs::read(&path).unwrap(),
            b"new content",
            "the rename already happened; pretending otherwise would be a lie"
        );
        assert!(
            err.contains("File replaced"),
            "the error must say the file was replaced: {err}"
        );
    }

    /// Arming is per-thread and released on drop, so one test's injected
    /// fault can never surface inside another running beside it.
    #[test]
    fn an_armed_fault_does_not_outlive_its_guard() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.txt");
        {
            let _armed = fault::arm(fault::Boundary::WriteData);
            assert!(atomic_write_bytes(&path, b"never lands").is_err());
        }
        atomic_write_bytes(&path, b"lands").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"lands");
    }

    #[test]
    fn rename_over_directory_fails_and_cleans_up_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("existing-dir");
        std::fs::create_dir(&target).unwrap();

        let result = atomic_write_bytes(&target, b"data");
        assert!(
            result.is_err(),
            "renaming a file over a directory must fail"
        );

        // The destination directory is untouched and the temp file removed.
        assert!(target.is_dir(), "destination directory must be preserved");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp file was not cleaned up: {leftovers:?}"
        );
    }

    #[test]
    fn path_without_file_name_errors_before_any_write() {
        let err = atomic_write_bytes(Path::new("/"), b"data").unwrap_err();
        assert!(err.contains("no file name"), "got: {err}");
    }

    /// Saving is process-wide safe under concurrency, not just crash-safe.
    /// Two things could break here and neither shows up in a single-threaded
    /// test: two savers could pick the same temporary name and interleave
    /// their writes into it, and a reader could catch the destination
    /// mid-write. So writers race on one path while a reader watches, and
    /// every observation must be one whole payload — never a prefix, a
    /// blend, or an empty file.
    #[test]
    fn concurrent_savers_never_expose_a_partial_or_blended_file() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        const WRITERS: usize = 6;
        const ROUNDS: usize = 40;
        // Large enough that a non-atomic write could not plausibly land in
        // one operation, and distinct per writer so a blend is detectable.
        const SIZE: usize = 256 * 1024;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("contended.bin");
        let payloads: Vec<Vec<u8>> =
            (0..WRITERS).map(|w| vec![b'a' + w as u8; SIZE]).collect();
        fs::write(&path, &payloads[0]).unwrap();

        let done = Arc::new(AtomicBool::new(false));
        let reader = {
            let path = path.clone();
            let done = Arc::clone(&done);
            let payloads = payloads.clone();
            std::thread::spawn(move || {
                let mut observations = 0usize;
                while !done.load(Ordering::Relaxed) {
                    match fs::read(&path) {
                        Ok(seen) => {
                            assert!(
                                payloads.contains(&seen),
                                "a reader saw {} bytes that are not any writer's whole payload",
                                seen.len()
                            );
                            observations += 1;
                        }
                        // The destination always exists here; anything else
                        // is a real defect.
                        Err(e) => panic!("destination vanished mid-save: {e}"),
                    }
                }
                observations
            })
        };

        let writers: Vec<_> = (0..WRITERS)
            .map(|w| {
                let path = path.clone();
                let payload = payloads[w].clone();
                std::thread::spawn(move || {
                    for _ in 0..ROUNDS {
                        atomic_write_bytes(&path, &payload).unwrap();
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap();
        }
        done.store(true, Ordering::Relaxed);
        let observations = reader.join().unwrap();
        assert!(observations > 0, "the reader never got to look");

        // Nothing is left behind: the destination and nothing else.
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name()))
            .filter(|name| name != "contended.bin")
            .collect();
        assert!(leftovers.is_empty(), "temporary files survived: {leftovers:?}");
        assert!(payloads.contains(&fs::read(&path).unwrap()));
    }

    /// `atomic_write_bytes` resolves the temporary's directory from the
    /// destination, so a path with no parent component has to fall back to
    /// the working directory rather than trying to create a temporary in
    /// "". A bare file name is what a caller passes after `cd`-ing, and a
    /// document opened from a command line argument arrives exactly so.
    #[test]
    fn a_bare_relative_file_name_saves_into_the_working_directory() {
        // `set_current_dir` is process-wide, so this is the one test that
        // touches it and it holds the lock while it does.
        let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let bare = atomic_write_bytes(Path::new("bare.txt"), b"no parent");
        let nested = fs::create_dir(dir.path().join("sub"))
            .map_err(|e| e.to_string())
            .and_then(|()| atomic_write_bytes(Path::new("sub/nested.txt"), b"relative parent"));

        std::env::set_current_dir(&original).unwrap();

        bare.unwrap();
        nested.unwrap();
        assert_eq!(fs::read(dir.path().join("bare.txt")).unwrap(), b"no parent");
        assert_eq!(
            fs::read(dir.path().join("sub/nested.txt")).unwrap(),
            b"relative parent"
        );
    }

    /// Paths on Unix are bytes, not text. A file named by a byte sequence
    /// that is not valid UTF-8 — which a user can create, and which a
    /// non-UTF-8 locale or a file copied off a foreign filesystem produces
    /// routinely — must save like any other, and must not be lossily
    /// re-encoded into a *different* file on the way.
    #[cfg(unix)]
    #[test]
    fn a_non_utf8_path_saves_to_that_exact_path() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let dir = tempfile::tempdir().unwrap();
        // 0xFF is never valid UTF-8; `to_string_lossy` turns it into U+FFFD,
        // so a lossy round-trip would write to a neighbouring name instead.
        let name = OsStr::from_bytes(b"r\xffport.odt");
        let path = dir.path().join(name);

        atomic_write_bytes(&path, b"first").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"first");
        // Overwrite too: that is the path that reads the destination's
        // metadata and renames over it.
        atomic_write_bytes(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");

        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name()))
            .collect();
        assert_eq!(names, vec![name.to_os_string()], "saved beside the requested path");
    }

    // ---- SH-002: destination version precondition -------------------------

    /// Set a file's mtime explicitly, so a test never depends on the
    /// filesystem's timestamp granularity to see (or not see) a change.
    fn set_mtime(path: &Path, when: std::time::SystemTime) {
        let handle = fs::OpenOptions::new().write(true).open(path).unwrap();
        handle.set_times(fs::FileTimes::new().set_modified(when)).unwrap();
    }

    /// The directory holds the document and nothing else: a refused or failed
    /// save must not strand its temporary.
    fn assert_only_entry(dir: &Path, name: &str) {
        let names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.file_name()))
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from(name)], "stray entries in {dir:?}");
    }

    #[test]
    fn a_matching_version_saves_and_returns_the_new_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.txt");
        fs::write(&path, b"original").unwrap();
        let opened = FileVersion::capture(&path).unwrap().expect("file exists");

        let saved = atomic_write_bytes_conditional(&path, b"first save", Some(&opened)).unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"first save");
        assert_eq!(
            FileVersion::capture(&path).unwrap().as_ref(),
            Some(&saved),
            "the returned version must describe the file now at the path"
        );
        assert_ne!(saved, opened, "a new file must not look like the one it replaced");
        // The returned version is what the next save expects.
        atomic_write_bytes_conditional(&path, b"second save", Some(&saved)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second save");
        assert_only_entry(dir.path(), "doc.txt");
    }

    /// An external in-place rewrite that changes the size keeps the inode, so
    /// it is the size (and mtime) that must catch it.
    #[test]
    fn an_external_size_change_refuses_the_save_and_keeps_their_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.txt");
        fs::write(&path, b"original").unwrap();
        let opened = FileVersion::capture(&path).unwrap();

        fs::write(&path, b"someone else wrote a longer document").unwrap();
        let err = atomic_write_bytes_conditional(&path, b"mine", opened.as_ref()).unwrap_err();

        assert_eq!(err, SaveError::PreconditionFailed);
        assert_eq!(fs::read(&path).unwrap(), b"someone else wrote a longer document");
        assert_only_entry(dir.path(), "doc.txt");
    }

    /// Same size, same inode: only the mtime differs, and it is set
    /// explicitly so a coarse-grained clock cannot hide the change.
    #[test]
    fn an_external_same_size_rewrite_is_caught_by_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.txt");
        fs::write(&path, b"original").unwrap();
        let opened = FileVersion::capture(&path).unwrap();

        fs::write(&path, b"tampered").unwrap();
        set_mtime(&path, std::time::SystemTime::now() + std::time::Duration::from_secs(3600));
        let err = atomic_write_bytes_conditional(&path, b"mine", opened.as_ref()).unwrap_err();

        assert_eq!(err, SaveError::PreconditionFailed);
        assert_eq!(fs::read(&path).unwrap(), b"tampered");
        assert_only_entry(dir.path(), "doc.txt");
    }

    /// Another window's atomic save renames a new file over the path. Make
    /// the replacement identical in size and mtime so the inode is the only
    /// thing left to tell them apart. It is created while the original still
    /// exists, so the inode numbers cannot be recycled into a false match.
    #[cfg(unix)]
    #[test]
    fn a_replaced_inode_refuses_the_save_even_when_size_and_mtime_match() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.txt");
        fs::write(&path, b"original").unwrap();
        let opened = FileVersion::capture(&path).unwrap();
        let original_mtime = fs::metadata(&path).unwrap().modified().unwrap();

        let sibling = dir.path().join("other-window.tmp");
        fs::write(&sibling, b"replaced").unwrap();
        set_mtime(&sibling, original_mtime);
        fs::rename(&sibling, &path).unwrap();
        let now = FileVersion::capture(&path).unwrap();
        assert_ne!(now, opened, "precondition: the replacement must differ");

        let err = atomic_write_bytes_conditional(&path, b"mine", opened.as_ref()).unwrap_err();

        assert_eq!(err, SaveError::PreconditionFailed);
        assert_eq!(fs::read(&path).unwrap(), b"replaced");
        assert_only_entry(dir.path(), "doc.txt");
    }

    #[test]
    fn expecting_no_file_saves_only_when_the_destination_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.txt");
        assert_eq!(FileVersion::capture(&path).unwrap(), None);

        // Absent and expected absent: creates it.
        let created = atomic_write_bytes_conditional(&path, b"created", None).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"created");
        assert_eq!(FileVersion::capture(&path).unwrap(), Some(created));

        // Present but expected absent: someone created it first.
        let err = atomic_write_bytes_conditional(&path, b"clobber", None).unwrap_err();
        assert_eq!(err, SaveError::PreconditionFailed);
        assert_eq!(fs::read(&path).unwrap(), b"created");
        assert_only_entry(dir.path(), "doc.txt");
    }

    #[test]
    fn a_deleted_destination_refuses_a_save_that_expected_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.txt");
        fs::write(&path, b"original").unwrap();
        let opened = FileVersion::capture(&path).unwrap();
        fs::remove_file(&path).unwrap();

        let err = atomic_write_bytes_conditional(&path, b"mine", opened.as_ref()).unwrap_err();

        assert_eq!(err, SaveError::PreconditionFailed);
        assert!(!path.exists(), "a refused save must not resurrect a deleted file");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0, "temporary left behind");
    }

    /// The unconditional wrapper keeps its behaviour: it overwrites a file
    /// that changed underneath it.
    #[test]
    fn the_unconditional_wrapper_still_overwrites_a_changed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.txt");
        fs::write(&path, b"original").unwrap();
        fs::write(&path, b"changed externally").unwrap();

        atomic_write_bytes(&path, b"mine").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"mine");
    }

    /// A pre-rename failure is `FailedBeforeCommit`, with the original kept.
    #[test]
    fn a_failure_before_the_rename_is_reported_as_not_committed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.txt");
        fs::write(&path, b"original").unwrap();
        let opened = FileVersion::capture(&path).unwrap();

        let err = {
            let _armed = fault::arm(fault::Boundary::SyncData);
            atomic_write_bytes_conditional(&path, b"never lands", opened.as_ref()).unwrap_err()
        };

        assert!(matches!(err, SaveError::FailedBeforeCommit(_)), "got {err:?}");
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert_eq!(FileVersion::capture(&path).unwrap(), opened, "destination must be untouched");
        assert_only_entry(dir.path(), "doc.txt");
    }

    /// The directory sync fails after the rename: the new bytes are at the
    /// destination and the outcome says so, distinctly from a failed save.
    #[cfg(unix)]
    #[test]
    fn a_dir_sync_failure_after_the_rename_is_reported_as_committed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.txt");
        fs::write(&path, b"original").unwrap();
        let opened = FileVersion::capture(&path).unwrap();

        let err = {
            let _armed = fault::arm(fault::Boundary::SyncDir);
            atomic_write_bytes_conditional(&path, b"new content", opened.as_ref()).unwrap_err()
        };

        match &err {
            SaveError::CommittedWithDirSyncFailure(msg) => {
                assert!(msg.contains("File replaced"), "message: {msg}");
            }
            other => panic!("expected CommittedWithDirSyncFailure, got {other:?}"),
        }
        assert_eq!(fs::read(&path).unwrap(), b"new content");
        assert!(err.to_string().contains("File replaced"), "Display must carry the message");
    }

}
