// SPDX-License-Identifier: GPL-3.0-or-later
//
// ToastManager — shared toast notification system wrapping AdwToastOverlay.
// Used by Letters, Tables, Decks for consistent save/error/action feedback.

use gtk4::{self as gtk, prelude::*};
use libadwaita as adw;

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
        let toast = adw::Toast::builder()
            .title(message)
            .timeout(3)
            .build();
        self.overlay.add_toast(toast);
    }

    /// Show a "Saved" toast with the file name.
    pub fn show_saved_toast(&self, file_name: &str) {
        let msg = format!("Saved \u{201C}{}\u{201D}", file_name);
        self.show_toast(&msg);
    }

    /// Show an error toast.
    pub fn show_error_toast(&self, message: &str) {
        let toast = adw::Toast::builder()
            .title(message)
            .timeout(5)
            .build();
        self.overlay.add_toast(toast);
    }

    /// Get the underlying overlay widget (for embedding in layout).
    pub fn widget(&self) -> &adw::ToastOverlay {
        &self.overlay
    }
}
