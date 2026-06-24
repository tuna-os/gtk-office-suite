// preferences.rs — Decks preferences dialog.
use libadwaita as adw;
use adw::prelude::*;
use gtk4::{gio, prelude::*};

pub struct DecksPreferences {
    pub window: adw::PreferencesDialog,
}

impl DecksPreferences {
    pub fn new(_settings: &gio::Settings) -> Self {
        let prefs = suite_common::make_preferences_window();
        let page = suite_common::make_preferences_page("General", "emblem-system-symbolic");
        let group = suite_common::make_preferences_group("Grid", "Canvas snapping");
        let row = adw::SwitchRow::builder().title("Snap to Grid").active(true).build();
        group.add(&row);
        page.add(&group);
        prefs.add(&page);
        DecksPreferences { window: prefs }
    }
}
