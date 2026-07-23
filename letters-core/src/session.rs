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

use std::path::PathBuf;
use std::rc::Rc;

use suite_common_core::autosave::AutosaveSlot;

pub struct DocumentSession {
    pub file: Option<PathBuf>,
    pub closing_after_save: bool,
    pub autosave_slot: Rc<AutosaveSlot>,
}

impl DocumentSession {
    pub fn new(autosave_slot: Rc<AutosaveSlot>) -> Self {
        Self { file: None, closing_after_save: false, autosave_slot }
    }

    pub fn file_name(&self) -> Option<String> {
        self.file
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn slot() -> Rc<AutosaveSlot> {
        Rc::new(AutosaveSlot::new(std::env::temp_dir(), "test-doc".to_string()))
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
}
