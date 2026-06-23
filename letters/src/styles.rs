// SPDX-License-Identifier: GPL-3.0-or-later
//
// styles.rs — Named styles system with inheritance for Letters.
// Maps Style definitions to GtkTextTags for application.
// When a style is modified, its TextTag properties update → all instances auto-update.

use gtk4::{self as gtk, prelude::*};
use std::collections::HashMap;

/// A named document style with formatting properties.
#[derive(Clone, Debug)]
pub struct Style {
    pub name: String,
    pub base_style: Option<String>,
    pub font: Option<String>,
    pub size_pt: Option<f64>,
    pub weight: Option<i32>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub color: Option<String>,
    pub background: Option<String>,
    pub alignment: Option<String>,
    pub scale: Option<f64>,
}

/// Ordered list of all style names, for toolbar dropdown.
pub fn style_names() -> Vec<&'static str> {
    vec![
        "Normal",
        "Title",
        "Subtitle",
        "Heading 1",
        "Heading 2",
        "Heading 3",
        "Heading 4",
        "Heading 5",
        "Heading 6",
        "Code",
        "Blockquote",
    ]
}

/// A collection of named styles with inheritance resolution.
pub struct StyleSheet {
    styles: HashMap<String, Style>,
}

impl StyleSheet {
    /// Create a StyleSheet with default document styles.
    pub fn default_styles() -> Self {
        let mut sheet = StyleSheet { styles: HashMap::new() };

        // Normal — base style
        sheet.add(Style { name: "Normal".into(), base_style: None,
            font: Some("Sans".into()), size_pt: Some(11.0), weight: Some(400),
            italic: None, underline: None, color: None, background: None, alignment: None, scale: None });

        // Title
        sheet.add(Style { name: "Title".into(), base_style: Some("Normal".into()),
            font: None, size_pt: Some(26.0), weight: Some(700), italic: None, underline: None,
            color: None, background: None, alignment: Some("center".into()), scale: None });

        // Subtitle
        sheet.add(Style { name: "Subtitle".into(), base_style: Some("Normal".into()),
            font: None, size_pt: Some(15.0), weight: Some(400), italic: None, underline: None,
            color: Some("#666666".into()), background: None, alignment: Some("center".into()), scale: None });

        // Headings 1-6
        for (level, scale, size) in [(1, 2.0, 22.0), (2, 1.5, 16.5), (3, 1.17, 12.9),
            (4, 1.0, 11.0), (5, 0.83, 9.1), (6, 0.67, 7.4)] {
            sheet.add(Style { name: format!("Heading {}", level), base_style: Some("Normal".into()),
                font: None, size_pt: Some(size), weight: Some(700), italic: None, underline: None,
                color: None, background: None, alignment: None, scale: Some(scale) });
        }

        // Code
        sheet.add(Style { name: "Code".into(), base_style: Some("Normal".into()),
            font: Some("Monospace".into()), size_pt: Some(10.0), weight: Some(400),
            italic: None, underline: None, color: Some("#333333".into()),
            background: Some("#F0F0F0".into()), alignment: None, scale: None });

        // Blockquote
        sheet.add(Style { name: "Blockquote".into(), base_style: Some("Normal".into()),
            font: None, size_pt: None, weight: Some(400), italic: Some(true), underline: None,
            color: Some("#666666".into()), background: None, alignment: None, scale: None });

        sheet
    }

    /// Add or replace a style. Does not update TextTags.
    pub fn add(&mut self, style: Style) {
        self.styles.insert(style.name.clone(), style);
    }

    /// Modify a style and sync its properties to all TextTags using it.
    /// Automatically updates all text tagged with this style.
    pub fn modify_and_sync<F: FnOnce(&mut Style)>(&mut self, name: &str, tag_table: &gtk::TextTagTable, f: F) -> bool {
        if let Some(style) = self.styles.get_mut(name) {
            f(style);
            // Re-resolve from stored data (clone to avoid borrow conflict)
            let style_clone = style.clone();
            drop(style); // end mutable borrow
            let resolved = self.resolve_style(&style_clone);
            let tag_name = style_to_tag_name(name);
            if !tag_name.is_empty() {
                if let Some(tag) = tag_table.lookup(tag_name) {
                    sync_style_to_tag(&resolved, &tag);
                }
            }
            true
        } else {
            false
        }
    }

    /// Get a style by name (resolved through inheritance).
    pub fn get(&self, name: &str) -> Option<Style> {
        self.styles.get(name).map(|s| self.resolve_style(s))
    }

    /// Resolve inheritance for a style reference.
    fn resolve_style(&self, style: &Style) -> Style {
        let mut resolved = style.clone();
        let mut current = style.base_style.clone();
        while let Some(base_name) = current {
            if let Some(base) = self.styles.get(&base_name) {
                if resolved.font.is_none() { resolved.font = base.font.clone(); }
                if resolved.size_pt.is_none() { resolved.size_pt = base.size_pt; }
                if resolved.weight.is_none() { resolved.weight = base.weight; }
                if resolved.italic.is_none() { resolved.italic = base.italic; }
                if resolved.underline.is_none() { resolved.underline = base.underline; }
                if resolved.color.is_none() { resolved.color = base.color.clone(); }
                if resolved.background.is_none() { resolved.background = base.background.clone(); }
                if resolved.alignment.is_none() { resolved.alignment = base.alignment.clone(); }
                if resolved.scale.is_none() { resolved.scale = base.scale; }
                current = base.base_style.clone();
            } else { break; }
        }
        resolved
    }

    /// List all style names.
    pub fn names(&self) -> Vec<String> {
        self.styles.keys().cloned().collect()
    }

    /// Ensure the given TextTagTable has tags for all styles in this sheet.
    /// Creates missing tags and updates existing ones to match StyleSheet definitions.
    pub fn sync_to_tag_table(&self, tag_table: &gtk::TextTagTable) {
        for name in style_names() {
            let tag_name = style_to_tag_name(name);
            if tag_name.is_empty() { continue; }
            if let Some(resolved) = self.get(name) {
                if let Some(existing) = tag_table.lookup(tag_name) {
                    sync_style_to_tag(&resolved, &existing);
                } else {
                    let tag = build_tag(&resolved, tag_name);
                    tag_table.add(&tag);
                }
            }
        }
    }
}

// ── Style → TextTag mapping ──────────────────────────────────────────

pub fn style_to_tag_name(style_name: &str) -> &str {
    match style_name {
        "Heading 1" => "h1", "Heading 2" => "h2", "Heading 3" => "h3",
        "Heading 4" => "h4", "Heading 5" => "h5", "Heading 6" => "h6",
        "Title" => "h-title", "Subtitle" => "h-subtitle",
        "Code" => "code", "Blockquote" => "blockquote",
        "Normal" => "normal",
        _ => "",
    }
}

fn build_tag(style: &Style, name: &str) -> gtk::TextTag {
    let mut builder = gtk::TextTag::builder().name(name);
    if let Some(f) = &style.font { builder = builder.family(f); }
    if let Some(s) = style.size_pt { builder = builder.size_points(s); }
    if let Some(w) = style.weight { builder = builder.weight(w); }
    if let Some(true) = style.italic { builder = builder.style(gtk4::pango::Style::Italic); }
    if let Some(true) = style.underline { builder = builder.underline(gtk4::pango::Underline::Single); }
    if let Some(c) = &style.color {
        if let Ok(rgba) = parse_hex_color(c) {
            builder = builder.foreground_rgba(&rgba);
        }
    }
    if let Some(bg) = &style.background {
        if let Ok(rgba) = parse_hex_color(bg) {
            builder = builder.background_rgba(&rgba);
        }
    }
    if let Some(a) = &style.alignment {
        match a.as_str() {
            "left" => builder = builder.justification(gtk::Justification::Left),
            "center" => builder = builder.justification(gtk::Justification::Center),
            "right" => builder = builder.justification(gtk::Justification::Right),
            "justify" => builder = builder.justification(gtk::Justification::Fill),
            _ => {}
        }
    }
    if let Some(s) = style.scale { builder = builder.scale(s); }
    builder.build()
}

fn sync_style_to_tag(style: &Style, tag: &gtk::TextTag) {
    if let Some(f) = &style.font { tag.set_family(Some(f)); }
    if let Some(s) = style.size_pt { tag.set_size_points(s); }
    tag.set_weight(style.weight.unwrap_or(400));
    tag.set_style(if style.italic.unwrap_or(false) { gtk4::pango::Style::Italic } else { gtk4::pango::Style::Normal });
    tag.set_underline(if style.underline.unwrap_or(false) { gtk4::pango::Underline::Single } else { gtk4::pango::Underline::None });
    if let Some(c) = &style.color {
        if let Ok(rgba) = parse_hex_color(c) { tag.set_foreground_rgba(Some(&rgba)); }
    }
    if let Some(bg) = &style.background {
        if let Ok(rgba) = parse_hex_color(bg) { tag.set_background_rgba(Some(&rgba)); }
    }
    if let Some(_a) = &style.alignment {
        // Alignment tags are separate — use existing align-* tags
    }
    if let Some(s) = style.scale { tag.set_scale(s); }
}

/// Ensure StyleSheet tags are present in a buffer's tag table.
/// Call this once per buffer before applying styles.
pub fn ensure_tags_synced(sheet: &StyleSheet, tag_table: &gtk::TextTagTable) {
    sheet.sync_to_tag_table(tag_table);
}
pub fn apply_style(buf: &gtk::TextBuffer, sheet: &StyleSheet, style_name: &str) {
    let (start, end) = buf.selection_bounds()
        .unwrap_or_else(|| {
            let mut s = buf.cursor_position();
            let mut line_start = buf.iter_at_offset(s);
            line_start.backward_line();
            let mut line_end = buf.iter_at_offset(s);
            if !line_end.ends_line() { line_end.forward_to_line_end(); }
            (line_start, line_end)
        });

    buf.begin_user_action();
    // Remove ALL paragraph-level style tags
    let all_style_tags = ["h1", "h2", "h3", "h4", "h5", "h6", "h-title", "h-subtitle", "code", "blockquote", "normal"];
    for tn in all_style_tags {
        if let Some(t) = buf.tag_table().lookup(tn) { buf.remove_tag(&t, &start, &end); }
    }
    // Apply the selected style tag (Normal clears all, leaving plain text)
    let tag_name = style_to_tag_name(style_name);
    if !tag_name.is_empty() {
        if let Some(tag) = buf.tag_table().lookup(tag_name) {
            buf.apply_tag(&tag, &start, &end);
        }
    }
    buf.end_user_action();
}

// ── Helpers ────────────────────────────────────────────────────────────

fn parse_hex_color(hex: &str) -> Result<gtk4::gdk::RGBA, ()> {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 { return Err(()); }
    let r = u8::from_str_radix(&h[0..2], 16).map_err(|_| ())?;
    let g = u8::from_str_radix(&h[2..4], 16).map_err(|_| ())?;
    let b = u8::from_str_radix(&h[4..6], 16).map_err(|_| ())?;
    Ok(gtk4::gdk::RGBA::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0))
}
