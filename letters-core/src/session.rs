// session.rs — canonical per-tab document identity for Letters.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// First slice of issue #103 for Letters. Letters' actual document
// *content* model is a GtkTextBuffer (rich text + formatting tags), which
// has no GTK-free representation today — that's a larger design question
// (mirror buffer state into a GTK-free AST kept in sync, or accept
// GtkTextBuffer itself as the content boundary) than a single slice
// should resolve. What *is* cleanly GTK-free, and was living directly in
// `letters/src/window.rs` despite having no GTK dependency in any of its
// field types, is per-tab session identity: which file a tab is backed
// by, whether it's mid-close-after-save, and its autosave slot. Moved
// here so it's canonical, testable, and outside the app crate.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use suite_common_core::autosave::AutosaveSlot;

pub struct DocumentSession {
    pub file: Option<PathBuf>,
    pub closing_after_save: bool,
    pub autosave_slot: Rc<AutosaveSlot>,
}

/// A committed user save may still need a recovery-cleanup warning. Cleanup
/// failure does not undo a successful file write or change its destination.
pub struct SaveCommit {
    pub recovery_warning: Option<String>,
}

impl DocumentSession {
    pub fn new(autosave_slot: Rc<AutosaveSlot>) -> Self {
        Self {
            file: None,
            closing_after_save: false,
            autosave_slot,
        }
    }

    pub fn file_name(&self) -> Option<String> {
        self.file
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
    }

    /// Commit identity and retire the recovery checkpoint only after the
    /// supplied writer succeeds. The GTK adapter marks its buffer clean only
    /// after this returns Ok; on Err all session/recovery state stays intact.
    pub fn save_to(
        &mut self,
        path: PathBuf,
        write: impl FnOnce(&Path) -> Result<(), String>,
    ) -> Result<SaveCommit, String> {
        write(&path)?;
        self.file = Some(path);
        Ok(SaveCommit {
            recovery_warning: self.autosave_slot.clear().err(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn slot() -> Rc<AutosaveSlot> {
        Rc::new(AutosaveSlot::new(
            std::env::temp_dir(),
            "test-doc".to_string(),
        ))
    }

    #[test]
    fn new_session_has_no_file() {
        let s = DocumentSession::new(slot());
        assert!(s.file.is_none());
        assert!(!s.closing_after_save);
        assert_eq!(s.file_name(), None);
    }

    #[test]
    fn file_name_extracts_basename() {
        let mut s = DocumentSession::new(slot());
        s.file = Some(Path::new("/tmp/some/dir/report.docx").to_path_buf());
        assert_eq!(s.file_name(), Some("report.docx".to_string()));
    }

    #[test]
    fn failed_save_keeps_original_identity_and_recovery_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let slot = Rc::new(AutosaveSlot::new(dir.path(), "document"));
        let meta = suite_common_core::autosave::SnapshotMeta {
            original_path: Some(PathBuf::from("original.odt")),
            kind: "json".into(),
        };
        slot.write(b"unsaved styled document", &meta).unwrap();
        let mut session = DocumentSession::new(slot.clone());
        session.file = meta.original_path.clone();
        let result = session.save_to(PathBuf::from("new.docx"), |path| {
            assert_eq!(path, Path::new("new.docx"));
            Err("disk full".into())
        });
        assert!(result.is_err());
        assert_eq!(session.file, meta.original_path);
        assert_eq!(
            slot.read(),
            Some((b"unsaved styled document".to_vec(), meta))
        );
    }

    #[test]
    fn successful_save_commits_identity_and_retires_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let slot = Rc::new(AutosaveSlot::new(dir.path(), "document"));
        slot.write(
            b"draft",
            &suite_common_core::autosave::SnapshotMeta {
                original_path: None,
                kind: "json".into(),
            },
        )
        .unwrap();
        let mut session = DocumentSession::new(slot.clone());
        let destination = dir.path().join("saved.odt");
        let result = session
            .save_to(destination.clone(), |path| {
                std::fs::write(path, b"saved content").map_err(|e| e.to_string())
            })
            .unwrap();
        assert!(result.recovery_warning.is_none());
        assert_eq!(session.file, Some(destination.clone()));
        assert_eq!(std::fs::read(destination).unwrap(), b"saved content");
        assert!(slot.read().is_none());
    }

    #[test]
    fn recovery_cleanup_failure_is_reported_after_successful_save() {
        let dir = tempfile::tempdir().unwrap();
        let slot = Rc::new(AutosaveSlot::new(dir.path(), "document"));
        // A nonempty directory in the snapshot slot deterministically makes
        // cleanup fail, including when tests run as root.
        std::fs::create_dir(dir.path().join("document.snapshot")).unwrap();
        let mut session = DocumentSession::new(slot);
        let result = session
            .save_to(PathBuf::from("saved.odt"), |_| Ok(()))
            .unwrap();
        assert!(result.recovery_warning.is_some());
        assert_eq!(session.file, Some(PathBuf::from("saved.odt")));
    }
}
