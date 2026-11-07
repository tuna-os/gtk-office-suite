// SPDX-License-Identifier: GPL-3.0-or-later
//
// collab_dialogs.rs — the Share and Join action dialogs, the join code alert
// and the Resolve alert of RFC-0005 5.1, on suite_common::dialogs.
//
// A form of two or more fields is an action dialog with entry rows; a refusal
// or a choice between named outcomes is an alert (docs/GNOME-GUIDELINES.md §2).

use std::rc::Rc;

use adw::prelude::{AdwDialogExt as _, AlertDialogExt as _, EntryRowExt as _, PreferencesGroupExt as _};
use gtk4::prelude::*;
use gtk4::{self as gtk};
use letters_core::collab::{CollabError, Replica};
use libadwaita as adw;
use suite_common::i18n;

use crate::collab_actions::Host;
use crate::collab_logic::{people, share_refusal};
use crate::collab_session::{JoinForm, ShareForm};

fn non_empty(text: gtk::glib::GString) -> Option<String> {
    let t = text.trim();
    (!t.is_empty()).then(|| t.to_owned())
}

/// An entry row, shown in error state until something is typed.
fn missing(row: &adw::EntryRow) {
    row.add_css_class("error");
    row.grab_focus();
    let r = row.clone();
    row.connect_changed(move |_| r.remove_css_class("error"));
}

/// Share This Document…: refused up front when the document holds what a
/// shared document cannot, before anything is asked or connected.
pub(crate) fn share(host: &Rc<Host>) {
    let Some((buf, shared)) = host.selected() else { return };
    if shared.is_some() {
        return;
    }
    let doc = crate::bridge::document_of(&buf);
    if let Err(CollabError::Unsupported(what)) = Replica::check_document(&doc) {
        host.alert(&i18n("Cannot Share This Document"), &share_refusal(what));
        return;
    }
    let server = adw::EntryRow::builder().title(i18n("Server")).build();
    let people_row = adw::EntryRow::builder().title(i18n("People (public keys)")).build();
    let ca = adw::EntryRow::builder().title(i18n("Certificate Authority File (optional)")).build();
    let group = adw::PreferencesGroup::builder().description(i18n("Each person's key is shown in their Join dialog. Only the people listed can open the document.")).build();
    for row in [&server, &people_row, &ca] {
        group.add(row);
    }
    let suite_common::dialogs::ActionDialog { dialog, action } = suite_common::dialogs::action_dialog(
        &i18n("Share This Document"),
        &i18n("_Share"),
        460,
        &suite_common::dialogs::form_body(&[group.upcast_ref()]),
    );
    let first = server.clone();
    {
        let (host, dialog, buf) = (host.clone(), dialog.clone(), buf.clone());
        action.connect_clicked(move |button| {
            let Some(server_text) = non_empty(server.text()) else { return missing(&server) };
            let form = ShareForm { server: server_text, ca_file: non_empty(ca.text()), people: people(&people_row.text()) };
            button.set_sensitive(false);
            let session = host.new_session();
            session.bind(&buf);
            let (button, dialog, host) = (button.clone(), dialog.clone(), host.clone());
            session.share(
                form,
                Box::new(move |_, result| {
                    button.set_sensitive(true);
                    match result {
                        Ok(invite) => {
                            dialog.close();
                            invite_dialog(&host, &invite);
                        }
                        Err(e) => host.alert(&i18n("Could Not Share"), &e),
                    }
                }),
            );
        });
    }
    dialog.present(host.window().as_ref());
    dialog.set_focus(Some(&first));
}

/// The join code, shown for the person who shared to hand on.
fn invite_dialog(host: &Rc<Host>, invite: &str) {
    let dialog = adw::AlertDialog::new(Some(&i18n("Sharing Started")), Some(invite));
    dialog.add_response("copy", &i18n("_Copy Join Code"));
    dialog.add_response("close", &i18n("_Close"));
    dialog.set_default_response(Some("close"));
    dialog.set_close_response("close");
    {
        let (host, code) = (host.clone(), invite.to_owned());
        dialog.connect_response(Some("copy"), move |_, _| {
            if let Some(window) = host.window() {
                window.clipboard().set_text(&code);
                host.toast(&i18n("Join code copied"));
            }
        });
    }
    dialog.present(host.window().as_ref());
}

/// Join a Shared Document…: the server, the person's own key (which the
/// owner adds to People before sharing: shown once the server is entered), and
/// the join code.
pub(crate) fn join(host: &Rc<Host>) {
    let session = host.new_session();
    let server = adw::EntryRow::builder().title(i18n("Server")).build();
    let key = adw::EntryRow::builder().title(i18n("Your Key")).build();
    key.set_editable(false);
    let copy = gtk::Button::from_icon_name("edit-copy-symbolic");
    copy.set_tooltip_text(Some(&i18n("Copy Key")));
    copy.update_property(&[gtk::accessible::Property::Label(&i18n("Copy Key"))]);
    copy.set_valign(gtk::Align::Center);
    copy.add_css_class("flat");
    key.add_suffix(&copy);
    let ca = adw::EntryRow::builder().title(i18n("Certificate Authority File (optional)")).build();
    let code = adw::EntryRow::builder().title(i18n("Join Code")).build();
    let group = adw::PreferencesGroup::builder().description(i18n("Give your key to the person who shares the document: they add it to People before sharing.")).build();
    for row in [&server, &key, &ca, &code] {
        group.add(row);
    }
    let suite_common::dialogs::ActionDialog { dialog, action } = suite_common::dialogs::action_dialog(
        &i18n("Join a Shared Document"),
        &i18n("_Join"),
        460,
        &suite_common::dialogs::form_body(&[group.upcast_ref()]),
    );
    {
        let k = key.clone();
        copy.connect_clicked(move |b| {
            let text = k.text();
            if !text.is_empty() {
                b.clipboard().set_text(&text);
            }
        });
    }
    // Enter in the server row connects, so the key can be read before the
    // owner has a join code to give.
    {
        let (session, host, key, ca) = (session.clone(), host.clone(), key.clone(), ca.clone());
        server.connect_entry_activated(move |row| {
            let Some(address) = non_empty(row.text()) else { return };
            let (key, host) = (key.clone(), host.clone());
            session.ensure_connected(
                address,
                non_empty(ca.text()),
                Box::new(move |_, result| match result {
                    Ok(principal) => key.set_text(&principal),
                    Err(e) => host.alert(&i18n("Could Not Connect"), &e),
                }),
            );
        });
    }
    let first = server.clone();
    {
        let (session, host, dialog) = (session.clone(), host.clone(), dialog.clone());
        action.connect_clicked(move |button| {
            let Some(code_text) = non_empty(code.text()) else { return missing(&code) };
            let form = JoinForm { server: non_empty(server.text()), ca_file: non_empty(ca.text()), code: code_text };
            button.set_sensitive(false);
            let (button, dialog, host) = (button.clone(), dialog.clone(), host.clone());
            session.join(
                form,
                Box::new(move |_, result| {
                    button.set_sensitive(true);
                    match result {
                        Ok(_) => {
                            dialog.close();
                        }
                        // A join that fails leaves the dialog open, to fix the code or the server.
                        Err(e) => host.alert(&i18n("Could Not Join"), &e),
                    }
                }),
            );
        });
    }
    {
        // A connection nobody used ends with the dialog.
        let session = session.clone();
        dialog.connect_closed(move |_| session.drop_idle_connection());
    }
    dialog.present(host.window().as_ref());
    dialog.set_focus(Some(&first));
}

/// Resolve…: the two versions, and what to do about them (RFC-0005 2.5).
pub(crate) fn resolve(host: &Rc<Host>) {
    let Some(session) = host.selected_session() else { return };
    let body = format!(
        "{}\n\n{}",
        i18n("Someone saved a version that this document does not include. Choose which to keep."),
        session.heads_text().unwrap_or_default()
    );
    let dialog = adw::AlertDialog::builder().heading(suite_common::i18n("Two Versions of This Document")).body(body).build();
    dialog.add_response("cancel", &i18n("_Cancel"));
    dialog.add_response("open-both", &i18n("_Open Both"));
    dialog.add_response("keep-theirs", &i18n("Keep _Theirs"));
    dialog.add_response("keep-mine", &i18n("_Keep Mine"));
    dialog.set_response_appearance("keep-mine", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("keep-mine"));
    dialog.set_close_response("cancel");
    let theirs = session.has_other_head();
    dialog.set_response_enabled("keep-theirs", theirs);
    dialog.set_response_enabled("open-both", theirs);
    dialog.connect_response(None, move |_, response| match response {
        "keep-mine" => session.keep_mine(),
        "keep-theirs" => session.keep_theirs(),
        "open-both" => session.open_both(),
        _ => {}
    });
    dialog.present(host.window().as_ref());
}
