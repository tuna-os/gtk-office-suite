// SPDX-License-Identifier: GPL-3.0-or-later
//
// Toasts, the same in every suite app (INTEGRATION-PLAN C1): `show` puts one
// on a window's AdwToastOverlay, `show_near` finds the overlay from any
// widget inside it, and how long a toast stays is one rule, `timeout_for`,
// rather than each call site's own number. App code calls these instead of
// building `adw::Toast`s itself (tests/test_chrome_contract.py).

use gtk4::{self as gtk, prelude::*};
use libadwaita as adw;

/// Seconds a toast stays: long enough to read. A short note ("Rows
/// frozen") goes in 3; a sentence ("Place the pivot table outside the
/// selection, which it would overwrite") gets 5.
pub fn timeout_for(text: &str) -> u32 {
    if text.chars().count() <= 40 {
        3
    } else {
        5
    }
}

/// Show `text` as a toast on `overlay`.
pub fn show(overlay: &adw::ToastOverlay, text: &str) {
    let toast = adw::Toast::new(text);
    toast.set_timeout(timeout_for(text));
    overlay.add_toast(toast);
}

/// Show `text` on the toast overlay `widget` sits in. Returns false when it
/// is in none (a widget not yet in a window), so the caller can say it some
/// other way.
pub fn show_near(widget: &impl IsA<gtk::Widget>, text: &str) -> bool {
    match widget.ancestor(adw::ToastOverlay::static_type()).and_then(|w| w.downcast::<adw::ToastOverlay>().ok()) {
        Some(overlay) => {
            show(&overlay, text);
            true
        }
        None => false,
    }
}

/// Manages toast notifications via an AdwToastOverlay.
pub struct ToastManager {
    overlay: adw::ToastOverlay,
}

impl ToastManager {
    /// Create a ToastManager wrapping the given content widget.
    pub fn new(content: &impl IsA<gtk::Widget>) -> Self {
        let overlay = adw::ToastOverlay::new();
        overlay.set_child(Some(content));
        ToastManager { overlay }
    }

    /// Show a simple toast message.
    pub fn show_toast(&self, message: &str) {
        show(&self.overlay, message);
    }

    /// Show a "Saved" toast with the file name.
    pub fn show_saved_toast(&self, file_name: &str) {
        let msg = format!("Saved \u{201C}{}\u{201D}", file_name);
        self.show_toast(&msg);
    }

    /// Show an error toast.
    pub fn show_error_toast(&self, message: &str) {
        show(&self.overlay, message);
    }

    /// Get the underlying overlay widget (for embedding in layout).
    pub fn widget(&self) -> &adw::ToastOverlay {
        &self.overlay
    }
}

#[cfg(test)]
mod tests {
    use super::timeout_for;

    #[test]
    fn a_short_note_stays_three_seconds_and_a_sentence_five() {
        assert_eq!(timeout_for("Rows frozen"), 3);
        assert_eq!(timeout_for("Join code copied"), 3);
        assert_eq!(timeout_for("Place the pivot table outside the selection, which it would overwrite"), 5);
    }

    #[test]
    fn length_is_counted_in_characters_not_bytes() {
        // 40 characters, more than 40 bytes.
        assert_eq!(timeout_for(&"é".repeat(40)), 3);
        assert_eq!(timeout_for(&"é".repeat(41)), 5);
    }
}
