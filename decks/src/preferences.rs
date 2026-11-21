// preferences.rs — Decks preferences dialog.
use libadwaita as adw;
use adw::prelude::*;
use gtk4::{gdk, gio};

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
        page.add(&presentation_group(settings));
        prefs.add(&page);
        DecksPreferences { window: prefs }
    }
}

/// The displays a show can go on, as the system lists them: "Display 2 —
/// DELL U2720Q". The index is what the setting stores.
fn display_names() -> Vec<String> {
    let Some(display) = gdk::Display::default() else { return vec![] };
    let monitors = display.monitors();
    (0..monitors.n_items())
        .filter_map(|i| monitors.item(i).and_downcast::<gdk::Monitor>())
        .enumerate()
        .map(|(i, m)| {
            let what = m.description().or_else(|| m.model()).or_else(|| m.connector()).map(|s| s.to_string());
            match what {
                Some(what) if !what.is_empty() => format!("Display {} — {what}", i + 1),
                _ => format!("Display {}", i + 1),
            }
        })
        .collect()
}

/// Presentation ▸ Presentation Display: Automatic (the second display
/// when there is one) or a display by name (ADR 0004, "explicit external
/// display selection"). A chosen display that is later unplugged is shown
/// as such and the show falls back to automatic.
fn presentation_group(settings: &gio::Settings) -> adw::PreferencesGroup {
    let group = suite_common::make_preferences_group("Presentation", "Where a slide show goes");
    let chosen = settings.int("presentation-display");
    let mut items = vec!["Automatic".to_string()];
    items.extend(display_names());
    // Chosen before and not connected now: still the setting, and listed
    // as such (the item's position is the display's index).
    while chosen >= 0 && items.len() <= chosen as usize + 1 {
        items.push(format!("Display {} (not connected)", items.len()));
    }
    let model = gtk4::StringList::new(&items.iter().map(String::as_str).collect::<Vec<_>>());
    let row = adw::ComboRow::builder()
        .title("Presentation Display")
        .subtitle("The display the audience sees; the presenter display goes on another")
        .model(&model)
        .selected((chosen + 1).max(0) as u32)
        .build();
    let s = settings.clone();
    row.connect_selected_notify(move |row| {
        s.set_int("presentation-display", row.selected() as i32 - 1)
            .unwrap_or_else(|e| eprintln!("GSettings write failed: {}", e));
    });
    group.add(&row);
    group
}
