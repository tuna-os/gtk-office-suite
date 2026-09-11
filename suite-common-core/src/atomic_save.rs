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
        tempfile::Builder::new().prefix(".office-save-").tempfile_in(dir)
    )
    .map_err(|e| format!("Failed to create save file: {e}"))?;
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes the one test that changes the process working directory.
    static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
