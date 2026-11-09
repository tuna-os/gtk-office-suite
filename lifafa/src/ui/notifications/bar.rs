//! The status bar itself: folding it and the panel, and placing it at the
//! top or the foot of the window.

use super::*;

impl NotificationCenter {
    pub(super) fn on_toggle_panel(&mut self) {
        self.panel_open = !self.panel_open;
        if !self.panel_open {
            // The bar's X is also console mode's exit.
            self.console_open = false;
            self.console_theme = false;
        }
    }

    pub(super) fn on_toggle_bar(&mut self) {
        if self.shown() {
            self.panel_open = false;
            self.transient_visible = false;
            self.console_open = false;
            self.console_theme = false;
        } else {
            self.panel_open = true;
        }
    }

    pub(super) fn on_set_at_top(&mut self, on: bool) {
        self.top.set(on);
        self.place();
    }
}
