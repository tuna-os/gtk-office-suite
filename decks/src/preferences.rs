// preferences.rs — Decks preferences dialog.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pattern: LibreOffice Impress presentation preferences.
// Uses suite-common preferences builders for GNOME HIG compliance.

use libadwaita as adw;
use adw::prelude::*;
use gtk4::{gio, prelude::*};

pub struct DecksPreferences {
    pub window: adw::PreferencesDialog,
}

impl DecksPreferences {
    pub fn new(settings: &gio::Settings) -> Self {
        let prefs = suite_common::make_preferences_window();

        // ── General page ─────────────────────────────────────────────
        let general = suite_common::make_preferences_page("General", "emblem-system-symbolic");

        let slide_group = suite_common::make_preferences_group("New Presentation", "Defaults for new decks");
        let width_row = adw::SpinRow::builder()
            .title("Slide Width")
            .subtitle("Default slide width in pixels (standard: 960)")
            .adjustment(&gtk4::Adjustment::new(960.0, 320.0, 3840.0, 10.0, 100.0, 0.0))
            .build();
        slide_group.add(&width_row);

        let height_row = adw::SpinRow::builder()
            .title("Slide Height")
            .subtitle("Default slide height in pixels (standard: 540)")
            .adjustment(&gtk4::Adjustment::new(540.0, 180.0, 2160.0, 10.0, 100.0, 0.0))
            .build();
        slide_group.add(&height_row);
        general.add(&slide_group);

        // ── Editing page ─────────────────────────────────────────────
        let editing = suite_common::make_preferences_page("Editing", "input-keyboard-symbolic");

        let snap_group = suite_common::make_preferences_group("Snap to Grid", "Grid snapping during object drag");
        let snap_switch = adw::SwitchRow::builder()
            .title("Enable Snap to Grid")
            .subtitle("Objects snap to grid when dragged")
            .active(true)
            .build();
        snap_group.add(&snap_switch);

        let grid_spacing_row = adw::SpinRow::builder()
            .title("Grid Spacing")
            .subtitle("Grid spacing in pixels (slide coordinates)")
            .adjustment(&gtk4::Adjustment::new(20.0, 5.0, 100.0, 5.0, 10.0, 0.0))
            .build();
        snap_group.add(&grid_spacing_row);
        editing.add(&snap_group);

        let transition_group = suite_common::make_preferences_group("Transitions", "Slide transition defaults");
        let speed_row = adw::SpinRow::builder()
            .title("Transition Speed")
            .subtitle("Default transition duration in milliseconds")
            .adjustment(&gtk4::Adjustment::new(400.0, 100.0, 2000.0, 50.0, 100.0, 0.0))
            .build();
        transition_group.add(&speed_row);
        editing.add(&transition_group);

        prefs.add(&general);
        prefs.add(&editing);
        DecksPreferences { window: prefs }
    }

    pub fn present(&self, parent: &adw::ApplicationWindow) {
        self.window.present(Some(parent));
    }
}
