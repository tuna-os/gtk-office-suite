// SPDX-License-Identifier: GPL-3.0-or-later
//
// Alert dialogs shared by the three apps: reporting a failure, and asking
// before a dirty document is replaced.

use crate::i18n;
use gtk4 as gtk;
use libadwaita as adw;
use adw::prelude::*;

/// Tell the user an operation failed, in a dialog they must acknowledge.
///
/// All three apps had places that reported a failure with `eprintln!` and
/// nothing else — most consequentially a document that would not open, where
/// the user is left looking at an empty window with no indication that
/// anything went wrong (#447). stderr is not a user interface: under a
/// Flatpak or a desktop launcher nobody ever sees it.
///
/// `heading` should name the failure in words that read as a failure; the
/// GUI journeys match on phrases like "could not" / "cannot open" / "failed
/// to", and an accessible name that avoids all of them is invisible to them.
pub fn show_error_dialog(parent: Option<&adw::ApplicationWindow>, heading: &str, body: &str) {
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .body(body)
        .build();
    dialog.add_response("ok", &i18n("OK"));
    dialog.set_default_response(Some("ok"));
    dialog.present(parent);
}

/// Run `replace`, which swaps the window's document for another one (Open,
/// New, a template, a file handed over by the file manager), but ask first
/// when the current document has unsaved changes. Tables and Decks hold one
/// document per window, and every one of those paths used to replace a
/// dirty document without a word, losing the edits.
///
/// Cancel is the default and the close response: dismissing the dialog
/// keeps the document. `kind` names the document ("workbook",
/// "presentation") in the question.
pub fn confirm_discarding(parent: &adw::ApplicationWindow, dirty: bool, kind: &str, replace: impl FnOnce() + 'static) {
    if !dirty {
        replace();
        return;
    }
    let dialog = adw::AlertDialog::new(
        Some(&i18n("Discard unsaved changes?")),
        Some(&i18n("This %s has unsaved changes. Opening another one replaces it, and the changes will be lost.").replace("%s", &i18n(kind))),
    );
    dialog.add_response("cancel", &i18n("_Cancel"));
    dialog.add_response("discard", &i18n("_Discard"));
    dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.choose(Some(parent), None::<&gtk::gio::Cancellable>, move |response| {
        if response == "discard" {
            replace();
        }
    });
}
