// preferences.rs — Tables preferences dialog.
use libadwaita as adw;
use adw::prelude::*;
use gtk4::{gio, prelude::*};

pub struct TablesPreferences {
    pub window: adw::PreferencesDialog,
}

impl TablesPreferences {
    pub fn new(_settings: &gio::Settings) -> Self {
        let prefs = suite_common::make_preferences_window();
        let page = suite_common::make_preferences_page("General", "emblem-system-symbolic");
        let group = suite_common::make_preferences_group("Grid", "Spreadsheet appearance");
        let row = adw::SwitchRow::builder().title("Show Gridlines").active(true).build();
        group.add(&row);
        page.add(&group);
        prefs.add(&page);
        TablesPreferences { window: prefs }
    }
}
