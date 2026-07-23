// preferences.rs — Decks preferences dialog.
use libadwaita as adw;
use adw::prelude::*;
use gtk4::gio;

pub struct DecksPreferences {
    pub window: adw::PreferencesDialog,
}

impl DecksPreferences {
    pub fn new(settings: &gio::Settings) -> Self {
        let prefs = suite_common::make_preferences_window();
        let page = suite_common::make_preferences_page("General", "emblem-system-symbolic");
        let group = suite_common::make_preferences_group("Grid", "Canvas snapping");
        let row = adw::SwitchRow::builder()
            .title("Snap to grid")
            .subtitle("Snap dragged objects to the grid spacing")
            .active(settings.boolean("snap-to-grid"))
            .build();
        {
            let s = settings.clone();
            row.connect_active_notify(move |row| {
                s.set_boolean("snap-to-grid", row.is_active())
                    .unwrap_or_else(|e| eprintln!("GSettings write failed: {}", e));
            });
        }
        group.add(&row);
        page.add(&group);
        prefs.add(&page);
        DecksPreferences { window: prefs }
    }
}
