// attachment_edit.rs — the GTK-free half of "edit an attachment and put it
// back" (INTEGRATION-PLAN A2).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Lifafa writes an attachment to a temporary file, opens it in the right app
// and watches the file. This is the decision part: what to call the file, and
// whether what is on disk now is a finished edit worth offering to put back.
// The file watching, the temp directory and the "replace the attachment?"
// dialog are the shell half; they feed `observe` the file's bytes each time it
// changes and act on the answer.
//
// Editors save in steps (write a backup, write the file, rename), so a change
// only counts once the same new bytes are seen twice in a row.

/// What `observe` found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Observation {
    /// The file holds what the last accepted version held.
    Unchanged,
    /// Different from the accepted version, but possibly still being written.
    Settling,
    /// A finished edit: offer to replace the attachment with `replacement`.
    Edited,
}

/// One attachment being edited.
#[derive(Clone, Debug)]
pub struct EditSession {
    file_name: String,
    accepted: Vec<u8>,
    pending: Option<Vec<u8>>,
}

impl EditSession {
    /// Start editing an attachment named `name` whose bytes are `original`.
    pub fn start(name: &str, original: Vec<u8>) -> Self {
        Self { file_name: safe_file_name(name), accepted: original, pending: None }
    }

    /// The name to give the temporary file: the attachment's own name, so the
    /// editor sees the right extension, with anything that could leave the
    /// temp directory removed.
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// Look at the file's current bytes.
    pub fn observe(&mut self, current: &[u8]) -> Observation {
        if current == self.accepted.as_slice() {
            self.pending = None;
            return Observation::Unchanged;
        }
        if self.pending.as_deref() == Some(current) {
            return Observation::Edited;
        }
        self.pending = Some(current.to_vec());
        Observation::Settling
    }

    /// The edited bytes, once `observe` said `Edited`.
    pub fn replacement(&self) -> Option<&[u8]> {
        self.pending.as_deref()
    }

    /// The person agreed: the edited bytes are now the attachment, so only a
    /// further change is worth asking about again.
    pub fn accept(&mut self) {
        if let Some(bytes) = self.pending.take() {
            self.accepted = bytes;
        }
    }

    /// The person declined: stop asking about these bytes, but ask again if
    /// the file changes once more.
    pub fn decline(&mut self) {
        // Keep the declined bytes as the baseline for *asking*, not as the
        // attachment: nothing was replaced, so `accepted` is only the thing
        // observe compares against.
        self.accept();
    }
}

/// `name` as a single safe file name: no directory parts, no control
/// characters, never empty, never a dot-name that hides the file.
pub fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base.chars().filter(|c| !c.is_control()).collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim().to_string();
    if cleaned.is_empty() {
        "attachment".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_untouched_file_is_unchanged() {
        let mut s = EditSession::start("a.odt", b"one".to_vec());
        assert_eq!(s.observe(b"one"), Observation::Unchanged);
        assert_eq!(s.replacement(), None);
    }

    #[test]
    fn a_change_must_be_seen_twice_before_it_counts() {
        let mut s = EditSession::start("a.odt", b"one".to_vec());
        assert_eq!(s.observe(b"tw"), Observation::Settling);
        // The editor wrote more: still settling on the new bytes.
        assert_eq!(s.observe(b"two"), Observation::Settling);
        assert_eq!(s.observe(b"two"), Observation::Edited);
        assert_eq!(s.replacement(), Some(&b"two"[..]));
    }

    #[test]
    fn saving_back_to_the_original_cancels_a_pending_edit() {
        let mut s = EditSession::start("a.odt", b"one".to_vec());
        s.observe(b"two");
        assert_eq!(s.observe(b"one"), Observation::Unchanged);
        assert_eq!(s.replacement(), None);
    }

    #[test]
    fn accepting_makes_the_edit_the_baseline() {
        let mut s = EditSession::start("a.odt", b"one".to_vec());
        s.observe(b"two");
        s.observe(b"two");
        s.accept();
        assert_eq!(s.observe(b"two"), Observation::Unchanged);
        assert_eq!(s.observe(b"three"), Observation::Settling);
        assert_eq!(s.observe(b"three"), Observation::Edited);
    }

    #[test]
    fn declining_stops_asking_until_the_file_changes_again() {
        let mut s = EditSession::start("a.odt", b"one".to_vec());
        s.observe(b"two");
        s.observe(b"two");
        s.decline();
        assert_eq!(s.observe(b"two"), Observation::Unchanged);
        assert_eq!(s.observe(b"two!"), Observation::Settling);
    }

    #[test]
    fn the_temp_file_keeps_the_extension_and_cannot_escape() {
        assert_eq!(safe_file_name("report.docx"), "report.docx");
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name("C:\\Users\\me\\q.xlsx"), "q.xlsx");
        assert_eq!(safe_file_name(".hidden.odt"), "hidden.odt");
        assert_eq!(safe_file_name("a\nb.txt"), "ab.txt");
        assert_eq!(safe_file_name(""), "attachment");
        assert_eq!(safe_file_name("/"), "attachment");
        assert_eq!(EditSession::start("x/../y.ods", vec![]).file_name(), "y.ods");
    }
}
