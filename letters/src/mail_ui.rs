//! mail_ui.rs — Send by Email: the tab's document written as a `.docx` and
//! attached to a new message in the desktop's mail client (Lifafa on a suite
//! install). The same file Save As would write, in a temporary directory.
//! "Send as PDF by Email" does the same with the PDF export (INTEGRATION-PLAN A1).
//! SPDX-License-Identifier: GPL-3.0-or-later

use adw::prelude::*;
use gtk4::gio;
use libadwaita as adw;

pub(crate) fn register(app: &adw::Application, window: &adw::ApplicationWindow, tab_view: &adw::TabView, header: &adw::HeaderBar) {
    if let Some(menu) = suite_common::menus::primary_menu(header.upcast_ref()) {
        // After Save and Print, before Preferences: unlabelled, like the others.
        menu.insert_section(
            1,
            None,
            &suite_common::menus::section(&[
                (&suite_common::i18n("Send by _Email…"), "app.send-by-email"),
                (&suite_common::i18n("Send as _PDF by Email…"), "app.send-as-pdf-by-email"),
            ]),
        );
    }
    // One action per format: the document as the app saves it, and as a PDF.
    for (action, extension, write) in [
        ("send-by-email", "docx", write_docx as WriteFn),
        ("send-as-pdf-by-email", "pdf", write_pdf as WriteFn),
    ] {
        let (tab_view, window) = (tab_view.clone(), window.clone());
        let act = gio::SimpleAction::new(action, None);
        act.connect_activate(move |_, _| {
            let Some(page) = tab_view.selected_page() else { return };
            let Some(buf) = crate::dialogs::active_buffer(&tab_view) else { return };
            let name = suite_common::mail::attachment_name(page.title().as_str(), extension);
            let dir = std::env::temp_dir().join(format!("letters-mail-{}", std::process::id()));
            let target = dir.join(&name);
            let written = std::fs::create_dir_all(&dir).map_err(|e| e.to_string()).and_then(|()| write(&buf, &target));
            let parent = window.clone();
            let fail = move |e: String| suite_common::show_error_dialog(Some(&parent), &suite_common::i18n("Could Not Send by Email"), &e);
            match written {
                Ok(()) => {
                    let draft = suite_common::mail::MailDraft::for_document("Letters", &name, target);
                    suite_common::mail::compose(&window, draft, move |result| {
                        if let Err(e) = result {
                            fail(e);
                        }
                    });
                }
                Err(e) => fail(e),
            }
        });
        app.add_action(&act);
    }
    suite_common::actions::register_labels(&[
        ("app.send-by-email", &suite_common::i18n("Send by Email…")),
        ("app.send-as-pdf-by-email", &suite_common::i18n("Send as PDF by Email…")),
    ]);
}

type WriteFn = fn(&gtk4::TextBuffer, &std::path::Path) -> Result<(), String>;

/// The `.docx` Save As would write.
fn write_docx(buf: &gtk4::TextBuffer, target: &std::path::Path) -> Result<(), String> {
    crate::bridge::save_buffer_to_file(buf, target).map(|_| ())
}

/// The PDF export: the document as Typst source, compiled in process.
fn write_pdf(buf: &gtk4::TextBuffer, target: &std::path::Path) -> Result<(), String> {
    let source = letters_core::typst_export::typst_source(&crate::bridge::document_of(buf));
    crate::engine::export_pdf(&source, &target.to_string_lossy())
}
