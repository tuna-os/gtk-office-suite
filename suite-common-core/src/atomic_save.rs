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
const TEMP_PREFIX: &str = ".office-save-";

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

/// Write `bytes` to `path` atomically: write to a temporary file in the same
/// directory, flush and sync it to disk, then rename it over the
/// destination. `rename` within one filesystem is atomic, so a reader can
/// never observe a partially-written file, and a failure at any point before
/// the rename leaves the original untouched.
pub fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    path.file_name()
        .ok_or_else(|| "save path has no file name".to_string())?;
    // Exclusive creation prevents concurrent saves or stale symlinks from
    // sharing/truncating a temporary file. Drop cleans up only our own file.
    let mut tmp = fail_point!(
        fault::Boundary::CreateTemp,
        tempfile::Builder::new().prefix(TEMP_PREFIX).tempfile_in(dir)
    )
    .map_err(|e| format!("Failed to create save file: {e}"))?;
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
        .map_err(|e| format!("Failed to preserve file permissions: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Failed to inspect destination: {e}")),
    }
    fail_point!(fault::Boundary::WriteData, tmp.write_all(bytes))
        .map_err(|e| format!("Failed to write file: {e}"))?;
    fail_point!(fault::Boundary::SyncData, tmp.as_file().sync_all())
        .map_err(|e| format!("Failed to sync file: {e}"))?;
    fail_point!(fault::Boundary::Persist, Ok(()))
        .and_then(|()| tmp.persist(path).map_err(std::io::Error::from))
        .map_err(|e| format!("Failed to finalize save: {e}"))?;
    // Persist the directory entry as well as the data before reporting a
    // durable save. If this fails, the new file is present but durability
    // isn't confirmed; callers must retain dirty state/recovery data.
    #[cfg(unix)]
    fail_point!(
        fault::Boundary::SyncDir,
        fs::File::open(dir).and_then(|dir| dir.sync_all())
    )
    .map_err(|e| format!("File replaced, but failed to sync its directory: {e}"))?;
    // Only once the save has succeeded: a sweep is housekeeping, and doing
    // it first would spend time and risk on the path that still has the
    // user's document to save.
    sweep_stranded_temps(dir, std::time::SystemTime::now());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

}
