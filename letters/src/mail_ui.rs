//! mail_ui.rs — Send by Email: the tab's document written as a `.docx` and
//! attached to a new message in the desktop's mail client (Lifafa on a suite
//! install). The same file Save As would write, in a temporary directory.
//! SPDX-License-Identifier: GPL-3.0-or-later

use adw::prelude::*;
use gtk4::gio;
use libadwaita as adw;

pub(crate) fn register(app: &adw::Application, window: &adw::ApplicationWindow, tab_view: &adw::TabView, header: &adw::HeaderBar) {
    if let Some(menu) = suite_common::menus::primary_menu(header.upcast_ref()) {
        // After Save and Print, before Preferences: unlabelled, like the others.
        menu.insert_section(1, None, &suite_common::menus::section(&[(&suite_common::i18n("Send by _Email…"), "app.send-by-email")]));
    }
    let (tab_view, window) = (tab_view.clone(), window.clone());
    let act = gio::SimpleAction::new("send-by-email", None);
    act.connect_activate(move |_, _| {
        let Some(page) = tab_view.selected_page() else { return };
        let Some(buf) = crate::dialogs::active_buffer(&tab_view) else { return };
        let name = suite_common::mail::attachment_name(page.title().as_str(), "docx");
        let dir = std::env::temp_dir().join(format!("letters-mail-{}", std::process::id()));
        let target = dir.join(&name);
        let written = std::fs::create_dir_all(&dir)
            .map_err(|e| e.to_string())
            .and_then(|()| crate::bridge::save_buffer_to_file(&buf, &target).map(|_| ()));
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
    suite_common::actions::register_labels(&[("app.send-by-email", &suite_common::i18n("Send by Email…"))]);
}
