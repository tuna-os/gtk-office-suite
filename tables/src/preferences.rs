// preferences.rs — Tables preferences dialog.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pattern: LibreOffice sc/ ScAppOptions (Calc preferences).
// Uses suite-common preferences builders for GNOME HIG compliance.

use libadwaita as adw;
use adw::prelude::*;
use gtk4::{gio, prelude::*};

pub struct TablesPreferences {
    pub window: adw::PreferencesDialog,
}

impl TablesPreferences {
    pub fn new(settings: &gio::Settings) -> Self {
        let prefs = suite_common::make_preferences_window();

        // ── General page ─────────────────────────────────────────────
        let general = suite_common::make_preferences_page("General", "emblem-system-symbolic");

        let sheet_group = suite_common::make_preferences_group("New Spreadsheet", "Defaults for new sheets");
        let rows_row = adw::SpinRow::builder()
            .title("Default Rows")
            .subtitle("Number of rows in new spreadsheets")
            .adjustment(&gtk4::Adjustment::new(100.0, 10.0, 10000.0, 10.0, 100.0, 0.0))
            .build();
        settings.bind("default-rows", &rows_row, "value").build();
        sheet_group.add(&rows_row);

        let cols_row = adw::SpinRow::builder()
            .title("Default Columns")
            .subtitle("Number of columns in new spreadsheets")
            .adjustment(&gtk4::Adjustment::new(26.0, 1.0, 702.0, 1.0, 5.0, 0.0))
            .build();
        settings.bind("default-cols", &cols_row, "value").build();
        sheet_group.add(&cols_row);
        general.add(&sheet_group);

        let save_group = suite_common::make_preferences_group("Auto-Save", "Automatic save settings");
        let autosave_switch = adw::SwitchRow::builder()
            .title("Enable Auto-Save")
            .build();
        settings.bind("auto-save", &autosave_switch, "active").build();
        save_group.add(&autosave_switch);

        let interval_row = adw::SpinRow::builder()
            .title("Auto-Save Interval")
            .subtitle("Seconds between auto-saves")
            .adjustment(&gtk4::Adjustment::new(60.0, 10.0, 3600.0, 10.0, 60.0, 0.0))
            .build();
        settings.bind("auto-save-interval", &interval_row, "value").build();
        save_group.add(&interval_row);
        general.add(&save_group);

        // ── Appearance page ──────────────────────────────────────────
        let appearance = suite_common::make_preferences_page("Appearance", "preferences-desktop-symbolic");

        let grid_group = suite_common::make_preferences_group("Grid", "Grid appearance");
        let grid_color_row = adw::EntryRow::builder()
            .title("Grid Line Color")
            .subtitle("Hex color for grid lines (e.g., #D9D9D9)")
            .text("#D9D9D9")
            .build();
        grid_group.add(&grid_color_row);
        appearance.add(&grid_group);

        prefs.add(&general);
        prefs.add(&appearance);
        TablesPreferences { window: prefs }
    }

    pub fn present(&self, parent: &adw::ApplicationWindow) {
        self.window.present(Some(parent));
    }
}
