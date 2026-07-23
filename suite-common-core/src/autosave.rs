// autosave.rs — Crash-recovery snapshot lifecycle, shared across Letters,
// Tables, and Decks.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// This module owns only the *lifecycle*: where a snapshot lives, how it's
// written/read/cleared, and how a leftover snapshot is recognized at next
// launch. It knows nothing about document formats — callers hand it opaque
// bytes (already serialized by the app's own `*-core` write path) and get
// opaque bytes back.
//
// There is deliberately no pidfile or lock file. A snapshot's mere presence
// at launch, before this session has written anything, IS the crash signal:
// a clean save or an explicit discard always clears its slot, so anything
// still there was orphaned by a save that never happened.

use crate::atomic_save::atomic_write_bytes;
use std::fs;
use std::path::{Path, PathBuf};

/// Metadata persisted alongside a snapshot so recovery can reassociate it
/// with the original document — or offer it as a recovered "Untitled"
/// document if it was never saved anywhere.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotMeta {
    pub original_path: Option<PathBuf>,
    /// App-defined hint (e.g. a format extension) describing how to
    /// interpret the snapshot bytes. Opaque to this module.
    pub kind: String,
}

/// One durable autosave slot rooted at `state_dir`, for a document
/// identified by `doc_id`. `doc_id` must be stable for the life of the
/// in-memory document but does not need to be predictable across restarts —
/// recovery works by scanning `state_dir` for whatever is there, not by
/// recomputing the id that made it.
pub struct AutosaveSlot {
    state_dir: PathBuf,
    doc_id: String,
}

const DATA_SUFFIX: &str = ".snapshot";
const META_SUFFIX: &str = ".snapshot.meta";

impl AutosaveSlot {
    pub fn new(state_dir: impl Into<PathBuf>, doc_id: impl Into<String>) -> Self {
        Self { state_dir: state_dir.into(), doc_id: doc_id.into() }
    }

    fn data_path(&self) -> PathBuf {
        self.state_dir.join(format!("{}{DATA_SUFFIX}", self.doc_id))
    }

    fn meta_path(&self) -> PathBuf {
        self.state_dir.join(format!("{}{META_SUFFIX}", self.doc_id))
    }

    /// Write (or overwrite) the snapshot atomically, creating `state_dir`
    /// if it doesn't exist yet. Never call this after `mark_clean()` on a
    /// real save without first calling `clear()` — a snapshot represents
    /// *unsaved* state; leaving a stale one around after a save would
    /// falsely trigger recovery next launch.
    pub fn write(&self, bytes: &[u8], meta: &SnapshotMeta) -> Result<(), String> {
        fs::create_dir_all(&self.state_dir)
            .map_err(|e| format!("Cannot create {}: {e}", self.state_dir.display()))?;
        atomic_write_bytes(&self.data_path(), bytes)?;
        let path_line = meta
            .original_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let meta_bytes = format!("{path_line}\n{}", meta.kind);
        atomic_write_bytes(&self.meta_path(), meta_bytes.as_bytes())
    }

    /// Remove the snapshot — call this on a successful real save and on an
    /// explicit discard. Missing files are not an error: nothing to do.
    pub fn clear(&self) -> Result<(), String> {
        for p in [self.data_path(), self.meta_path()] {
            if p.exists() {
                fs::remove_file(&p).map_err(|e| format!("Cannot remove {}: {e}", p.display()))?;
            }
        }
        Ok(())
    }

    /// Read back the snapshot bytes and metadata, if both files are present
    /// and well-formed.
    pub fn read(&self) -> Option<(Vec<u8>, SnapshotMeta)> {
        let bytes = fs::read(self.data_path()).ok()?;
        let meta_raw = fs::read_to_string(self.meta_path()).ok()?;
        let mut lines = meta_raw.splitn(2, '\n');
        let path_line = lines.next().unwrap_or_default();
        let kind = lines.next().unwrap_or_default().to_string();
        let original_path = if path_line.is_empty() { None } else { Some(PathBuf::from(path_line)) };
        Some((bytes, SnapshotMeta { original_path, kind }))
    }
}

/// Scan `state_dir` for snapshots left behind by a crash — call this once
/// at app launch, before the session's own autosave timer has run. Returns
/// the `doc_id` of each complete (data + meta) snapshot found; a data file
/// with no matching meta file (write interrupted mid-way through the two
/// atomic writes) is skipped rather than offered for recovery, since there's
/// no way to tell the caller what format it's in.
pub fn find_orphaned_snapshots(state_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(state_dir) else { return Vec::new() };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter_map(|name| name.strip_suffix(DATA_SUFFIX).map(str::to_string))
        .filter(|doc_id| state_dir.join(format!("{doc_id}{META_SUFFIX}")).exists())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_read_round_trips_bytes_and_meta() {
        let dir = tempfile::tempdir().unwrap();
        let slot = AutosaveSlot::new(dir.path(), "doc-1");
        let meta = SnapshotMeta { original_path: Some(PathBuf::from("/home/x/report.xlsx")), kind: "xlsx".into() };
        slot.write(b"snapshot bytes", &meta).unwrap();

        let (bytes, read_meta) = slot.read().expect("snapshot should be readable");
        assert_eq!(bytes, b"snapshot bytes");
        assert_eq!(read_meta, meta);
    }

    #[test]
    fn write_creates_state_dir_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nested/state");
        let slot = AutosaveSlot::new(&nested, "doc-1");
        slot.write(b"x", &SnapshotMeta { original_path: None, kind: "md".into() }).unwrap();
        assert!(nested.exists());
    }

    #[test]
    fn unnamed_document_snapshot_has_no_original_path() {
        let dir = tempfile::tempdir().unwrap();
        let slot = AutosaveSlot::new(dir.path(), "doc-1");
        slot.write(b"x", &SnapshotMeta { original_path: None, kind: "md".into() }).unwrap();
        let (_, meta) = slot.read().unwrap();
        assert_eq!(meta.original_path, None);
    }

    #[test]
    fn clear_removes_both_files_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let slot = AutosaveSlot::new(dir.path(), "doc-1");
        slot.write(b"x", &SnapshotMeta { original_path: None, kind: "md".into() }).unwrap();
        slot.clear().unwrap();
        assert!(slot.read().is_none());
        // Clearing an already-clear slot must not error.
        slot.clear().unwrap();
    }

    #[test]
    fn find_orphaned_snapshots_lists_doc_ids_with_a_complete_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        AutosaveSlot::new(dir.path(), "a").write(b"1", &SnapshotMeta { original_path: None, kind: "md".into() }).unwrap();
        AutosaveSlot::new(dir.path(), "b").write(b"2", &SnapshotMeta { original_path: None, kind: "xlsx".into() }).unwrap();

        let mut found = find_orphaned_snapshots(dir.path());
        found.sort();
        assert_eq!(found, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn find_orphaned_snapshots_ignores_data_file_missing_its_meta() {
        let dir = tempfile::tempdir().unwrap();
        // Simulate a write interrupted between the two atomic writes.
        fs::write(dir.path().join("half.snapshot"), b"partial").unwrap();
        assert!(find_orphaned_snapshots(dir.path()).is_empty());
    }

    #[test]
    fn find_orphaned_snapshots_on_missing_dir_is_empty_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        assert_eq!(find_orphaned_snapshots(&missing), Vec::<String>::new());
    }

    #[test]
    fn clearing_after_a_real_save_leaves_no_orphan_for_next_launch() {
        let dir = tempfile::tempdir().unwrap();
        let slot = AutosaveSlot::new(dir.path(), "doc-1");
        slot.write(b"dirty state", &SnapshotMeta { original_path: None, kind: "md".into() }).unwrap();
        // ... a real save happens here, then:
        slot.clear().unwrap();
        assert!(find_orphaned_snapshots(dir.path()).is_empty());
    }
}
