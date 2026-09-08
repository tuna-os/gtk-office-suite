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
    let mut tmp = tempfile::Builder::new()
        .prefix(".office-save-")
        .tempfile_in(dir)
        .map_err(|e| format!("Failed to create save file: {e}"))?;
    match fs::metadata(path) {
        Ok(metadata) => tmp
            .as_file()
            .set_permissions(metadata.permissions())
            .map_err(|e| format!("Failed to preserve file permissions: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Failed to inspect destination: {e}")),
    }
    tmp.write_all(bytes)
        .map_err(|e| format!("Failed to write file: {e}"))?;
    tmp.as_file()
        .sync_all()
        .map_err(|e| format!("Failed to sync file: {e}"))?;
    tmp.persist(path)
        .map_err(|e| format!("Failed to finalize save: {e}"))?;
    // Persist the directory entry as well as the data before reporting a
    // durable save. If this fails, the new file is present but durability
    // isn't confirmed; callers must retain dirty state/recovery data.
    #[cfg(unix)]
    fs::File::open(dir)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| format!("File replaced, but failed to sync its directory: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[cfg(unix)]
    #[test]
    fn failed_write_preserves_the_original_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.txt");
        atomic_write_bytes(&path, b"original content").unwrap();

        // Make the directory read-only so the temp-file create fails.
        let mut perms = fs::metadata(dir.path()).unwrap().permissions();
        let original_mode = perms.mode();
        perms.set_mode(0o555);
        fs::set_permissions(dir.path(), perms).unwrap();

        let result = atomic_write_bytes(&path, b"new content that never lands");

        // Restore write permission so the tempdir can clean itself up.
        let mut perms = fs::metadata(dir.path()).unwrap().permissions();
        perms.set_mode(original_mode);
        fs::set_permissions(dir.path(), perms).unwrap();

        assert!(
            result.is_err(),
            "expected the write to fail under a read-only directory"
        );
        assert_eq!(fs::read(&path).unwrap(), b"original content");
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
}
