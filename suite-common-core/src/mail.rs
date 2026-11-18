// mail.rs — what "Send by email" hands to the user's mail client.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The suite's mail client is Lifafa (tuna-os/lifafa), but the apps never
// name it: they compose a `MailDraft` and give it to the desktop's mail
// handler (the XDG Email portal, or a `mailto:` URI where the portal is
// missing). Whichever client is the default receives it, and Lifafa is the
// default on a suite install. This module is the GTK-free half: the draft,
// its subject and body wording, and the RFC 6068 `mailto:` encoding.

use std::path::{Path, PathBuf};

/// A message to compose, with the document attached.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MailDraft {
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
    pub attachments: Vec<PathBuf>,
}

impl MailDraft {
    /// A draft that sends `file` named `document` from the app `app_label`.
    pub fn for_document(app_label: &str, document: &str, file: PathBuf) -> Self {
        MailDraft {
            to: Vec::new(),
            subject: document.to_string(),
            body: format!("Attached is {document}, sent from {app_label}.\n"),
            attachments: vec![file],
        }
    }

    /// A draft that sends several documents at once (INTEGRATION-PLAN A1):
    /// `files` are `(display name, path)` pairs. One document reads exactly as
    /// [`MailDraft::for_document`]; several are named in the subject (the first
    /// and a count) and listed in the body, one per line.
    pub fn for_documents(app_label: &str, files: &[(String, PathBuf)]) -> Self {
        match files {
            [] => MailDraft::default(),
            [(name, path)] => MailDraft::for_document(app_label, name, path.clone()),
            [(first, _), rest @ ..] => {
                let mut body = format!("Attached are {} documents, sent from {app_label}:\n\n", files.len());
                for (name, _) in files {
                    body.push_str(&format!("- {name}\n"));
                }
                MailDraft {
                    to: Vec::new(),
                    subject: format!("{first} and {} more", rest.len()),
                    body,
                    attachments: files.iter().map(|(_, path)| path.clone()).collect(),
                }
            }
        }
    }

    /// The `mailto:` URI for this draft (RFC 6068). A `mailto:` URI cannot
    /// carry an attachment, so this is the fallback where the portal is not
    /// there; the body then names the file instead.
    pub fn mailto_uri(&self) -> String {
        let mut body = self.body.clone();
        for file in &self.attachments {
            if let Some(name) = file.file_name().and_then(|n| n.to_str()) {
                body.push_str(&format!("\n(Attach {name} before sending.)"));
            }
        }
        let recipients: Vec<String> = self.to.iter().map(|a| encode(a, true)).collect();
        let mut uri = format!("mailto:{}", recipients.join(","));
        let mut sep = '?';
        for (key, value) in [("subject", self.subject.as_str()), ("body", body.as_str())] {
            if value.is_empty() {
                continue;
            }
            uri.push(sep);
            uri.push_str(key);
            uri.push('=');
            uri.push_str(&encode(value, false));
            sep = '&';
        }
        uri
    }
}

/// Percent-encode `text` for a `mailto:` URI. `keep_at` leaves `@` alone,
/// which a recipient address needs and a subject or body must escape.
fn encode(text: &str, keep_at: bool) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        let plain = byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~')
            || (keep_at && byte == b'@');
        if plain {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// A name for the temporary copy of an unsaved document: the title with
/// anything a file name cannot hold replaced, and the format's extension.
pub fn attachment_name(title: &str, extension: &str) -> String {
    let stem: String = title
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') { c } else { '_' })
        .collect();
    let stem = stem.trim_matches(|c| c == '.' || c == ' ');
    let stem = if stem.is_empty() { "Untitled" } else { stem };
    let extension = extension.trim_start_matches('.');
    if stem.to_ascii_lowercase().ends_with(&format!(".{}", extension.to_ascii_lowercase())) {
        stem.to_string()
    } else {
        format!("{stem}.{extension}")
    }
}

/// A new directory to write an attachment into before the mail client is
/// handed it: reachable by this user only, under a name nobody can guess.
/// A directory named after the app and its process id in the shared temp
/// directory was neither: another user could read the staged document, or
/// create the directory first and choose where the write landed (#2229, as
/// #268 was for pictures). Like the directory it replaces it is not
/// removed, because the mail client may read the attachment long after the
/// app that staged it has gone.
pub fn staging_dir() -> std::io::Result<PathBuf> {
    staging_dir_in(&std::env::temp_dir())
}

fn staging_dir_in(base: &Path) -> std::io::Result<PathBuf> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("office-mail-");
    // tempfile creates directories with the process umask, not 0700.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    Ok(builder.tempdir_in(base)?.keep())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_attachment_is_staged_in_a_private_directory_of_its_own() {
        let base = tempfile::tempdir().unwrap();
        let (a, b) = (staging_dir_in(base.path()).unwrap(), staging_dir_in(base.path()).unwrap());
        assert_ne!(a, b, "two sends share a directory");
        for dir in [&a, &b] {
            assert_eq!(dir.parent().unwrap(), base.path());
            assert!(dir.is_dir());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(dir).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o700, "staging directory mode {mode:o}");
            }
        }
    }

    #[test]
    fn mailto_escapes_subject_and_body() {
        let draft = MailDraft {
            to: vec!["ada@example.org".into()],
            subject: "Q3 & Q4 plan".into(),
            body: "Hi\nthere".into(),
            attachments: vec![],
        };
        assert_eq!(
            draft.mailto_uri(),
            "mailto:ada@example.org?subject=Q3%20%26%20Q4%20plan&body=Hi%0Athere"
        );
    }

    #[test]
    fn mailto_without_recipient_or_body_is_bare() {
        let draft = MailDraft { subject: "x".into(), ..Default::default() };
        assert_eq!(draft.mailto_uri(), "mailto:?subject=x");
        assert_eq!(MailDraft::default().mailto_uri(), "mailto:");
    }

    #[test]
    fn mailto_encodes_non_ascii_as_utf8() {
        let draft = MailDraft { subject: "Café".into(), ..Default::default() };
        assert_eq!(draft.mailto_uri(), "mailto:?subject=Caf%C3%A9");
    }

    #[test]
    fn mailto_names_the_file_it_cannot_attach() {
        let draft = MailDraft::for_document("Letters", "Report.docx", PathBuf::from("/tmp/Report.docx"));
        assert!(draft.mailto_uri().contains("Attach%20Report.docx%20before%20sending"));
    }

    #[test]
    fn for_document_attaches_the_file() {
        let draft = MailDraft::for_document("Tables", "Budget.xlsx", PathBuf::from("/x/Budget.xlsx"));
        assert_eq!(draft.subject, "Budget.xlsx");
        assert_eq!(draft.attachments, vec![PathBuf::from("/x/Budget.xlsx")]);
        assert!(draft.body.contains("sent from Tables"));
    }

    #[test]
    fn for_documents_with_one_file_is_for_document() {
        let one = [("Budget.xlsx".to_string(), PathBuf::from("/x/Budget.xlsx"))];
        assert_eq!(
            MailDraft::for_documents("Tables", &one),
            MailDraft::for_document("Tables", "Budget.xlsx", PathBuf::from("/x/Budget.xlsx"))
        );
    }

    #[test]
    fn for_documents_names_the_first_and_counts_the_rest() {
        let files = [
            ("Report.docx".to_string(), PathBuf::from("/x/Report.docx")),
            ("Budget.xlsx".to_string(), PathBuf::from("/x/Budget.xlsx")),
            ("Deck.pptx".to_string(), PathBuf::from("/x/Deck.pptx")),
        ];
        let draft = MailDraft::for_documents("Letters", &files);
        assert_eq!(draft.subject, "Report.docx and 2 more");
        assert_eq!(draft.attachments.len(), 3);
        assert_eq!(draft.attachments[2], PathBuf::from("/x/Deck.pptx"));
        assert!(draft.body.contains("Attached are 3 documents, sent from Letters"));
        assert!(draft.body.contains("- Budget.xlsx\n"));
    }

    #[test]
    fn for_documents_with_none_is_empty() {
        assert_eq!(MailDraft::for_documents("Decks", &[]), MailDraft::default());
    }

    #[test]
    fn several_attachments_are_each_named_in_the_mailto_fallback() {
        let files = [
            ("A.docx".to_string(), PathBuf::from("/x/A.docx")),
            ("B.docx".to_string(), PathBuf::from("/x/B.docx")),
        ];
        let uri = MailDraft::for_documents("Letters", &files).mailto_uri();
        assert!(uri.contains("Attach%20A.docx%20before%20sending"));
        assert!(uri.contains("Attach%20B.docx%20before%20sending"));
    }

    #[test]
    fn attachment_name_is_a_safe_file_name() {
        assert_eq!(attachment_name("Q3/Q4: plan", "docx"), "Q3_Q4_ plan.docx");
        assert_eq!(attachment_name("  ", "xlsx"), "Untitled.xlsx");
        assert_eq!(attachment_name("Deck.pptx", ".pptx"), "Deck.pptx");
        assert_eq!(attachment_name("..hidden..", "odp"), "hidden.odp");
    }
}
