// SPDX-License-Identifier: GPL-3.0-or-later
//! Hand a [`MailDraft`] to the desktop's mail client (Lifafa on a suite
//! install). The XDG Email portal carries attachments, so it is tried first;
//! where there is no portal, a `mailto:` URI opens the default handler with a
//! note to attach the file.

use gtk4::{self as gtk, gio, glib, prelude::*};
pub use suite_common_core::mail::{attachment_name, staging_dir, MailDraft};

/// How a draft reached the mail client.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Delivery {
    /// Opened through the Email portal, attachments included.
    Portal,
    /// Opened as a `mailto:` URI; the file must be attached by hand.
    Mailto,
}

/// Open `draft` in the mail client and call `done` with how it went.
pub fn compose(window: &impl IsA<gtk::Window>, draft: MailDraft, done: impl FnOnce(Result<Delivery, String>) + 'static) {
    // Test hook: with SUITE_MAIL_SPOOL=<dir> the draft is written there
    // (draft.txt, and the attachments beside it) instead of being sent, so a
    // GUI test can read what the mail client would have been handed.
    if let Some(spool) = std::env::var_os("SUITE_MAIL_SPOOL") {
        done(spool_draft(std::path::Path::new(&spool), &draft).map(|()| Delivery::Portal));
        return;
    }
    let window = window.as_ref().clone();
    glib::spawn_future_local(async move {
        let result = match portal_compose(&draft).await {
            Ok(()) => Ok(Delivery::Portal),
            Err(portal_error) => {
                let launcher = gtk::UriLauncher::new(&draft.mailto_uri());
                launcher
                    .launch_future(Some(&window))
                    .await
                    .map(|()| Delivery::Mailto)
                    .map_err(|e| format!("{portal_error}; mailto: {e}"))
            }
        };
        done(result);
    });
}

async fn portal_compose(draft: &MailDraft) -> Result<(), String> {
    let bus = gio::bus_get_future(gio::BusType::Session).await.map_err(|e| e.to_string())?;
    let files: Vec<std::fs::File> = draft
        .attachments
        .iter()
        .map(std::fs::File::open)
        .collect::<Result<_, _>>()
        .map_err(|e| format!("attachment: {e}"))?;
    let fds = gio::UnixFDList::new();
    let mut indexes = Vec::new();
    for file in &files {
        indexes.push(fds.append(file).map_err(|e| e.to_string())?);
    }
    let options = glib::VariantDict::new(None);
    options.insert("addresses", draft.to.clone());
    options.insert("subject", draft.subject.as_str());
    options.insert("body", draft.body.as_str());
    if !indexes.is_empty() {
        options.insert_value("attachment_fds", &indexes.to_variant());
    }
    let parameters = ("", options.end()).to_variant();
    bus.call_with_unix_fd_list_future(
        Some("org.freedesktop.portal.Desktop"),
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Email",
        "ComposeEmail",
        Some(&parameters),
        None,
        gio::DBusCallFlags::NONE,
        30_000,
        Some(&fds),
    )
    .await
    .map(|_| ())
    .map_err(|e| format!("portal: {e}"))
}

/// Write `draft` into `dir`: its fields as `draft.txt`, its files copied in.
fn spool_draft(dir: &std::path::Path, draft: &MailDraft) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut names = Vec::new();
    for file in &draft.attachments {
        let name = file.file_name().ok_or("attachment has no name")?;
        std::fs::copy(file, dir.join(name)).map_err(|e| e.to_string())?;
        names.push(name.to_string_lossy().to_string());
    }
    let text = format!("to: {}\nsubject: {}\nattachments: {}\n\n{}", draft.to.join(", "), draft.subject, names.join(", "), draft.body);
    std::fs::write(dir.join("draft.txt"), text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spooling_copies_the_attachment_and_writes_the_fields() {
        let dir = std::env::temp_dir().join(format!("mail-spool-test-{}", std::process::id()));
        let src = dir.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("Report.docx"), b"PK").unwrap();
        let draft = MailDraft::for_document("Letters", "Report.docx", src.join("Report.docx"));
        spool_draft(&dir.join("out"), &draft).unwrap();
        assert_eq!(std::fs::read(dir.join("out/Report.docx")).unwrap(), b"PK");
        let text = std::fs::read_to_string(dir.join("out/draft.txt")).unwrap();
        assert!(text.contains("subject: Report.docx") && text.contains("attachments: Report.docx"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
