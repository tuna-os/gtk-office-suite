// autosave_notice.rs — tell the user when autosave stops working.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The decision of *when* to say something lives in
// `suite_common_core::autosave::AutosaveNotices`, which is GTK-free and
// tested. This is the adapter that turns its answers into toasts, so the
// three apps' autosave call sites each gain one line rather than a copy of
// the policy.

use std::cell::RefCell;
use std::rc::Rc;

use libadwaita as adw;

use suite_common_core::autosave::AutosaveNotices;

/// Shows autosave failures in a window's toast overlay, at most as often as
/// the policy allows.
pub struct AutosaveNotifier {
    notices: RefCell<AutosaveNotices>,
    overlay: adw::ToastOverlay,
}

impl AutosaveNotifier {
    pub fn new(overlay: &adw::ToastOverlay) -> Rc<Self> {
        Rc::new(Self {
            notices: RefCell::new(AutosaveNotices::new()),
            overlay: overlay.clone(),
        })
    }

    /// Feed this the result of every snapshot write, successes included: the
    /// policy needs the successes to know when to say autosave recovered,
    /// and stays silent about them otherwise.
    pub fn record(&self, outcome: Result<(), String>) {
        // The borrow ends before the toast is added: `add_toast` runs GTK
        // code that could re-enter this, and a live RefCell borrow across it
        // would panic at runtime rather than fail to compile.
        let message = {
            let mut notices = self.notices.borrow_mut();
            match outcome {
                Ok(()) => notices.succeeded(),
                Err(reason) => notices.failed(&reason),
            }
        };
        if let Some(message) = message {
            // Longer than the 3s default: this one asks the reader to go and
            // save their work, which takes longer than noticing a "Saved".
            let toast = adw::Toast::builder().title(&message).timeout(8).build();
            self.overlay.add_toast(toast);
        }
    }
}
