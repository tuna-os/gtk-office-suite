//! The two message dialogs written the same way all over the app: a notice
//! with only OK, and the question asked before something that cannot be
//! taken back.
//!
//! They are `adw::MessageDialog`, deprecated in libadwaita 1.6 for
//! `adw::AlertDialog`; that one is a sheet inside the window rather than a
//! window of its own, which would change how every dialog looks, so the
//! deprecation is allowed where a dialog is built.

use std::cell::RefCell;

use adw::prelude::*;

use crate::i18n::i18n;

/// Asks before something that cannot be undone: Cancel, which is also the
/// default and what closing the dialog means, or the destructive
/// `response` labelled `label`, which runs `on_confirm` once.
#[allow(deprecated)] // adw::MessageDialog, see the module comment
pub(crate) fn confirm_destructive(
    parent: Option<&gtk::Window>,
    heading: &str,
    body: &str,
    response: &'static str,
    label: &str,
    on_confirm: impl FnOnce() + 'static,
) {
    let dialog = adw::MessageDialog::new(parent, Some(heading), Some(body));
    dialog.add_response("cancel", &i18n("Cancel"));
    dialog.add_response(response, label);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.set_response_appearance(response, adw::ResponseAppearance::Destructive);
    let on_confirm = RefCell::new(Some(on_confirm));
    dialog.connect_response(None, move |_, resp| {
        if resp == response {
            if let Some(f) = on_confirm.borrow_mut().take() {
                f();
            }
        }
    });
    dialog.present();
}

/// A message with only an OK button, over `parent`.
#[allow(deprecated)] // adw::MessageDialog, see the module comment
pub(super) fn alert(parent: Option<&gtk::Window>, heading: &str, body: &str) {
    let dialog = adw::MessageDialog::new(parent, Some(heading), Some(body));
    dialog.add_response("ok", &i18n("OK"));
    dialog.present();
}
