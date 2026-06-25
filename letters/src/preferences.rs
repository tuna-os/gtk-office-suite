// SPDX-License-Identifier: GPL-3.0-or-later
//
// Preferences window for Letters — following the original Python Letters pattern.

use libadwaita as adw;
use adw::prelude::*;
use gtk4::{gio, prelude::*};

const FORMAT_NAMES: &[&str] = &["ODT (OpenDocument)", "DOCX (Office Open XML)",
    "Markdown", "HTML", "Plain Text", "RTF (Rich Text)"];

pub struct LettersPreferences {
    pub window: adw::PreferencesDialog,
}

impl LettersPreferences {
    pub fn new(settings: &gio::Settings) -> Self {
        let prefs = suite_common::make_preferences_window();

        // ── General page ─────────────────────────────────────────────────
        let general = suite_common::make_preferences_page("General", "emblem-system-symbolic");

        let doc_group = suite_common::make_preferences_group("Document", "Default save format");
        let format_row = adw::ComboRow::builder()
            .title("Default format")
            .subtitle("Default save format for new documents")
            .model(&gtk4::StringList::new(FORMAT_NAMES))
            .build();
        doc_group.add(&format_row);
        general.add(&doc_group);

        let editor_group = suite_common::make_preferences_group("Editor", "Text editing preferences");
        let font_row = adw::EntryRow::builder()
            .title("Font family")
            .build();
        editor_group.add(&font_row);

        let margin_row = adw::SpinRow::builder()
            .title("Editor margin")
            .subtitle("Left and right margin as percentage")
            .adjustment(&gtk4::Adjustment::new(16.0, 0.0, 50.0, 1.0, 5.0, 0.0))
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
            .active(true)
            .build();
        layout_group.add(&toolbar_row);
        appearance.add(&layout_group);

        prefs.add(&appearance);

        LettersPreferences { window: prefs }
    }
}
