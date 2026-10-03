// SPDX-License-Identifier: GPL-3.0-or-later
//
// Preferences window for Letters — following the original Python Letters pattern.

use libadwaita as adw;
use adw::prelude::*;
use gtk4::gio;

/// The offered formats come from the writer itself, so this list cannot
/// promise something Letters has no writer for. It used to be hand-written
/// and included "HTML" and "RTF (Rich Text)": picking either pre-filled
/// `Untitled.html` / `Untitled.rtf` in Save As, and the save then wrote
/// Markdown bytes under that name (#436). HTML now has a real writer; RTF
/// has none anywhere in the suite, so it is no longer offered.
fn format_names() -> Vec<&'static str> {
    letters_core::save::SaveFormat::ALL.iter().map(|f| f.label()).collect()
}

/// The extension persisted to the `default-format` key — a stable string,
/// not an index that would shift if the list reorders.
fn format_extensions() -> Vec<&'static str> {
    letters_core::save::SaveFormat::ALL.iter().map(|f| f.extension()).collect()
}

fn format_index_for_extension(ext: &str) -> u32 {
    format_extensions().iter().position(|e| *e == ext).unwrap_or(0) as u32
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
        let format_names = gtk4::StringList::new(&format_names());
        let format_row = adw::ComboRow::builder()
            .title("Default format")
            .subtitle("Preselected filter in the Save As dialog for a new document")
            .model(&format_names)
            .selected(format_index_for_extension(&settings.string("default-format")))
            .build();
        {
            let s = settings.clone();
            format_row.connect_selected_notify(move |row| {
                if let Some(ext) = format_extensions().get(row.selected() as usize) {
                    s.set_string("default-format", ext)
                        .unwrap_or_else(|e| eprintln!("GSettings write failed: {}", e));
                }
            });
        }
        doc_group.add(&format_row);
        general.add(&doc_group);

        let editor_group = suite_common::make_preferences_group("Editor", "Text editing preferences");
        // A font picker, not free text: the font chooser lists only fonts
        // that exist, and the help text is the row's own subtitle rather
        // than a second, empty-looking row.
        let font_button = gtk4::FontDialogButton::new(Some(gtk4::FontDialog::new()));
        font_button.set_font_desc(&gtk4::pango::FontDescription::from_string(settings.string("font").as_str()));
        font_button.set_valign(gtk4::Align::Center);
        {
            let s = settings.clone();
            font_button.connect_font_desc_notify(move |b| {
                if let Some(desc) = b.font_desc() {
                    s.set_string("font", &desc.to_string())
                        .unwrap_or_else(|e| eprintln!("GSettings write failed: {}", e));
                }
            });
        }
        let font_row = adw::ActionRow::builder()
            .title("Body font")
            .subtitle("The font of new documents")
            .build();
        font_row.add_suffix(&font_button);
        font_row.set_activatable_widget(Some(&font_button));
        editor_group.add(&font_row);
        // An "Editor margin" row stood here, disabled and labelled "Not yet
        // implemented": a setting that does nothing is not shown.
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
        let appearance = suite_common::make_preferences_page("Appearance", "preferences-desktop-appearance-symbolic");

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

#[cfg(test)]
mod tests {
    use super::{format_extensions, format_index_for_extension, format_names};

    #[test]
    fn known_extensions_map_to_stable_indices() {
        // Indices index into both lists; the extension is what is
        // persisted to `default-format`.
        assert_eq!(format_index_for_extension("odt"), 0);
        assert_eq!(format_index_for_extension("docx"), 1);
        assert_eq!(format_index_for_extension("md"), 2);
        assert_eq!(format_index_for_extension("html"), 3);
        assert_eq!(format_index_for_extension("txt"), 4);
    }

    #[test]
    fn unknown_extension_falls_back_to_first_format() {
        assert_eq!(format_index_for_extension("pdf"), 0);
        assert_eq!(format_index_for_extension(""), 0);
    }

    /// RTF was offered here with no writer behind it anywhere in the
    /// suite, so choosing it pre-filled `Untitled.rtf` and the save wrote
    /// Markdown under that name (#436). A stale `default-format` of "rtf"
    /// now simply falls back to the first format.
    #[test]
    fn rtf_is_no_longer_offered_and_a_stale_setting_falls_back() {
        assert!(!format_extensions().contains(&"rtf"));
        assert_eq!(format_index_for_extension("rtf"), 0);
    }

    #[test]
    fn matching_is_exact_and_case_sensitive() {
        // Persisted values are lowercase; a shifted case must NOT silently
        // map to a different index (which would reorder the default format).
        assert_eq!(format_index_for_extension("DOCX"), 0);
        assert_eq!(format_index_for_extension("MD"), 0);
        assert_eq!(format_index_for_extension(" ODT"), 0);
    }

    #[test]
    fn format_tables_are_parallel() {
        assert_eq!(format_names().len(), format_extensions().len());
    }

    /// The point of building both lists from `SaveFormat::ALL`: anything
    /// this window offers has a writer that accepts that extension.
    #[test]
    fn every_offered_format_has_a_writer() {
        for extension in format_extensions() {
            assert_eq!(
                letters_core::save::SaveFormat::from_extension(extension)
                    .map(|format| format.extension()),
                Some(extension),
                "{extension} is offered but has no writer"
            );
        }
    }
}
