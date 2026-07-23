// SPDX-License-Identifier: GPL-3.0-or-later
//
// Preferences window for Letters — following the original Python Letters pattern.

use libadwaita as adw;
use adw::prelude::*;
use gtk4::{gio, prelude::*};

const FORMAT_NAMES: &[&str] = &["ODT (OpenDocument)", "DOCX (Office Open XML)",
    "Markdown", "HTML", "Plain Text", "RTF (Rich Text)"];
/// Extensions matching FORMAT_NAMES 1:1 — index into either with the same
/// selection, and this is what's persisted to the `default-format` key
/// (a stable string, not an index that would shift if the list reorders).
const FORMAT_EXTENSIONS: &[&str] = &["odt", "docx", "md", "html", "txt", "rtf"];

fn format_index_for_extension(ext: &str) -> u32 {
    FORMAT_EXTENSIONS.iter().position(|e| *e == ext).unwrap_or(0) as u32
}

pub struct LettersPreferences {
    pub window: adw::PreferencesDialog,
}

impl LettersPreferences {
    pub fn new(settings: &gio::Settings) -> Self {
        let prefs = suite_common::make_preferences_window();

        // ── General page ─────────────────────────────────────────────────
        let general = suite_common::make_preferences_page("General", "emblem-system-symbolic");

        let doc_group = suite_common::make_preferences_group("Document", "Default save format");
        let format_names = gtk4::StringList::new(FORMAT_NAMES);
        let format_row = adw::ComboRow::builder()
            .title("Default format")
            .subtitle("Preselected filter in the Save As dialog for a new document")
            .model(&format_names)
            .selected(format_index_for_extension(&settings.string("default-format")))
            .build();
        {
            let s = settings.clone();
            format_row.connect_selected_notify(move |row| {
                if let Some(ext) = FORMAT_EXTENSIONS.get(row.selected() as usize) {
                    s.set_string("default-format", ext)
                        .unwrap_or_else(|e| eprintln!("GSettings write failed: {}", e));
                }
            });
        }
        doc_group.add(&format_row);
        general.add(&doc_group);

        let editor_group = suite_common::make_preferences_group("Editor", "Text editing preferences");
        let font_row = adw::EntryRow::builder()
            .title("Font family")
            .text(settings.string("font").as_str())
            .build();
        {
            let s = settings.clone();
            font_row.connect_changed(move |row| {
                s.set_string("font", &row.text())
                    .unwrap_or_else(|e| eprintln!("GSettings write failed: {}", e));
            });
        }
        editor_group.add(&font_row);
        let font_note = adw::ActionRow::builder()
            .subtitle("Applies to documents opened or created after this change")
            .build();
        font_note.add_css_class("dim-label");
        editor_group.add(&font_note);

        let margin_row = adw::SpinRow::builder()
            .title("Editor margin")
            .subtitle("Not yet implemented — coming soon")
            .adjustment(&gtk4::Adjustment::new(16.0, 0.0, 50.0, 1.0, 5.0, 0.0))
            .sensitive(false)
            .build();
        editor_group.add(&margin_row);
        general.add(&editor_group);

        prefs.add(&general);

        // ── Editing page ─────────────────────────────────────────────────
        let editing = suite_common::make_preferences_page("Editing", "input-keyboard-symbolic");

        let tools_group = suite_common::make_preferences_group("Tools", "Editing tools");
        let spell_row = adw::SwitchRow::builder()
            .title("Spell checking")
            .subtitle("Enable built-in spell checking")
            .active(settings.boolean("spell-check-enabled"))
            .build();
        {
            let s = settings.clone();
            spell_row.connect_active_notify(move |row| {
                s.set_boolean("spell-check-enabled", row.is_active())
                    .unwrap_or_else(|e| eprintln!("GSettings write failed: {}", e));
            });
        }
        tools_group.add(&spell_row);

        let auto_save_row = adw::SpinRow::builder()
            .title("Auto-save interval")
            .subtitle("Set to 0 to disable auto-save")
            .adjustment(&gtk4::Adjustment::new(0.0, 0.0, 3600.0, 30.0, 60.0, 0.0))
            .build();
        tools_group.add(&auto_save_row);
        editing.add(&tools_group);

        prefs.add(&editing);

        // ── Appearance page ──────────────────────────────────────────────
        let appearance = suite_common::make_preferences_page("Appearance", "preferences-desktop-symbolic");

        let layout_group = suite_common::make_preferences_group("Layout", "Window layout");
        let toolbar_row = adw::SwitchRow::builder()
            .title("Show toolbar")
            .subtitle("Show the formatting toolbar")
            .active(settings.boolean("show-toolbar"))
            .build();
        {
            let s = settings.clone();
            toolbar_row.connect_active_notify(move |row| {
                s.set_boolean("show-toolbar", row.is_active())
                    .unwrap_or_else(|e| eprintln!("GSettings write failed: {}", e));
            });
        }
        layout_group.add(&toolbar_row);
        appearance.add(&layout_group);

        prefs.add(&appearance);

        LettersPreferences { window: prefs }
    }
}
