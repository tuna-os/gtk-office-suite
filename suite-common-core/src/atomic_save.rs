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
    let file_name = path
        .file_name()
        .ok_or_else(|| "save path has no file name".to_string())?
        .to_string_lossy();
    let tmp_path = dir.join(format!(".{file_name}.tmp-{}", std::process::id()));

    let write_result = (|| -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp_path)?;
        f.write_all(bytes)?;
        f.sync_all()
    })();

    if let Err(e) = write_result {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("Failed to write file: {e}"));
    }

    fs::rename(&tmp_path, path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        format!("Failed to finalize save: {e}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(leftovers.is_empty(), "temp file was not cleaned up: {leftovers:?}");
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

        assert!(result.is_err(), "expected the write to fail under a read-only directory");
        assert_eq!(fs::read(&path).unwrap(), b"original content");
    }
}
