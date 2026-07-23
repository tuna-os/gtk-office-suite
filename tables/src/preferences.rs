// preferences.rs — Tables preferences dialog.
use libadwaita as adw;
use adw::prelude::*;
use gtk4::{gio, prelude::*};

pub struct TablesPreferences {
    pub window: adw::PreferencesDialog,
}

impl TablesPreferences {
    pub fn new(settings: &gio::Settings) -> Self {
        let prefs = suite_common::make_preferences_window();
        let page = suite_common::make_preferences_page("General", "emblem-system-symbolic");
        let group = suite_common::make_preferences_group("Grid", "Spreadsheet appearance");
        let row = adw::SwitchRow::builder()
            .title("Show gridlines")
            .subtitle("Show cell gridlines in the spreadsheet grid")
            .active(settings.boolean("show-gridlines"))
            .build();
        {
            let s = settings.clone();
            row.connect_active_notify(move |row| {
                s.set_boolean("show-gridlines", row.is_active())
                    .unwrap_or_else(|e| eprintln!("GSettings write failed: {}", e));
            });
        }
        group.add(&row);
        page.add(&group);
        prefs.add(&page);
        TablesPreferences { window: prefs }
    }
}
