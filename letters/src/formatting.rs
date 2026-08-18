// SPDX-License-Identifier: GPL-3.0-or-later
//
// formatting.rs — GtkTextTag definitions, text styling, and structured editing actions for Letters.

use gtk4::{self as gtk, glib, prelude::*};
use libadwaita as adw;
use crate::list_controller::toggle_list;

// ── Active buffer and widget helpers ─────────────────────────────────

/// Walk from a TabPage child through PageContainer → ScrolledWindow → TextView.
pub fn get_textview(widget: &impl IsA<gtk::Widget>) -> Option<gtk::TextView> {
    fn find(w: &gtk::Widget) -> Option<gtk::TextView> {
        if let Ok(tv) = w.clone().downcast::<gtk::TextView>() {
            return Some(tv);
        }
        let mut child = w.first_child();
        while let Some(c) = child {
            if let Some(tv) = find(&c) {
                return Some(tv);
            }
            child = c.next_sibling();
        }
        None
    }
    find(widget.as_ref().upcast_ref::<gtk::Widget>())
}

pub fn active_buffer(tv: &adw::TabView) -> Option<gtk::TextBuffer> {
    tv.selected_page()
        .and_then(|p| get_textview(&p.child()))
        .map(|tv| tv.buffer())
}

/// Apply a named GtkTextTag to the current selection or cursor position.
pub fn apply_tag_to_active(tv: &adw::TabView, tag_name: &str) {
    if let Some(buf) = active_buffer(tv) {
        if let Some(tag) = buf.tag_table().lookup(tag_name) {
            let sel = buf.selection_bounds();
            if let Some((start, end)) = sel {
                buf.apply_tag(&tag, &start, &end);
            }
        }
    }
}

/// Toggle a named GtkTextTag on the current selection.
pub fn toggle_tag(tv: &adw::TabView, tag_name: &str) {
    if let Some(buf) = active_buffer(tv) {
        if let Some(tag) = buf.tag_table().lookup(tag_name) {
            let sel = buf.selection_bounds();
            if let Some((start, end)) = sel {
                let tags_at_cursor = start.tags();
                let has = tags_at_cursor.iter().any(|t| t.name().as_deref() == Some(tag_name));
                if has {
                    buf.remove_tag(&tag, &start, &end);
                } else {
                    buf.apply_tag(&tag, &start, &end);
                }
            }
        }
    }
}

pub fn toggle_inline_bold(tv: &adw::TabView)  { toggle_tag(tv, "bold"); }
pub fn toggle_inline_italic(tv: &adw::TabView) { toggle_tag(tv, "italic"); }
pub fn toggle_inline_underline(tv: &adw::TabView) { toggle_tag(tv, "underline"); }
pub fn toggle_inline_strikethrough(tv: &adw::TabView) { toggle_tag(tv, "strikethrough"); }
pub fn toggle_highlight(tv: &adw::TabView) { toggle_tag(tv, "highlight"); }

// ── TextTag registration ────────────────────────────────────────────────

pub fn register_formatting_tags(buffer: &gtk::TextBuffer) {
    let tb = buffer.tag_table();
    macro_rules! add { ($tag:expr) => { tb.add(&$tag); }}
    add!(gtk::TextTag::builder().name("bold").weight(700).build());
    add!(gtk::TextTag::builder().name("italic").style(gtk4::pango::Style::Italic).build());
    add!(gtk::TextTag::builder().name("underline").underline(gtk4::pango::Underline::Single).build());
    add!(gtk::TextTag::builder().name("strikethrough").strikethrough(true).build());
    add!(gtk::TextTag::builder().name("highlight").background("#FFFF00").build());
    add!(gtk::TextTag::builder().name("h1").scale(2.0).weight(700).build());
    add!(gtk::TextTag::builder().name("h2").scale(1.5).weight(700).build());
    add!(gtk::TextTag::builder().name("h3").scale(1.17).weight(700).build());
    add!(gtk::TextTag::builder().name("h4").scale(1.0).weight(700).build());
    add!(gtk::TextTag::builder().name("h5").scale(0.83).weight(700).build());
    add!(gtk::TextTag::builder().name("h6").scale(0.67).weight(700).build());
    add!(gtk::TextTag::builder().name("h-title").scale(2.36).weight(700).build());
    add!(gtk::TextTag::builder().name("h-subtitle").scale(1.36).weight(400).foreground("#666666").build());
    add!(gtk::TextTag::builder().name("normal").build());
    // Line spacing tags
    add!(gtk::TextTag::builder().name("line-spacing-1.0").pixels_inside_wrap(0).pixels_above_lines(0).pixels_below_lines(0).build());
    add!(gtk::TextTag::builder().name("line-spacing-1.15").pixels_inside_wrap(2).pixels_above_lines(0).pixels_below_lines(0).build());
    add!(gtk::TextTag::builder().name("line-spacing-1.5").pixels_inside_wrap(6).pixels_above_lines(2).pixels_below_lines(2).build());
    add!(gtk::TextTag::builder().name("line-spacing-2.0").pixels_inside_wrap(12).pixels_above_lines(4).pixels_below_lines(4).build());
    add!(gtk::TextTag::builder().name("code").family("Monospace").background("#F0F0F0").foreground("#333333").build());
    add!(gtk::TextTag::builder().name("blockquote").left_margin(40).style(gtk4::pango::Style::Italic).foreground("#666666").build());
    // Alignment tags
    add!(gtk::TextTag::builder().name("align-left").justification(gtk::Justification::Left).build());
    add!(gtk::TextTag::builder().name("align-center").justification(gtk::Justification::Center).build());
    add!(gtk::TextTag::builder().name("align-right").justification(gtk::Justification::Right).build());
    add!(gtk::TextTag::builder().name("align-justify").justification(gtk::Justification::Fill).build());
    // Font size tags
    add!(gtk::TextTag::builder().name("font-larger").scale(1.2).build());
    add!(gtk::TextTag::builder().name("font-smaller").scale(0.833).build());
    // Search highlight tags
    add!(gtk::TextTag::builder().name("search-match").background("#FFFF00").build());
    add!(gtk::TextTag::builder().name("search-current").background("#FF9800").build());
}

// ── Formatting & Structured Editing Actions ───────────────────────────

pub fn register_formatting_actions(tv: &adw::TabView, app: &adw::Application) {
    // Inline formatting
    type ToggleHandler = fn(&adw::TabView);
    let pairs: &[(&str, ToggleHandler)] = &[
        ("bold", toggle_inline_bold),
        ("italic", toggle_inline_italic),
        ("underline", toggle_inline_underline),
        ("strikethrough", toggle_inline_strikethrough),
        ("highlight", toggle_highlight),
    ];
    for (name, handler) in pairs {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new(name, None);
        a.connect_activate(move |_, _| handler(&tv));
        app.add_action(&a);
    }

    // Shortcuts for B/I/U
    app.set_accels_for_action("app.bold", &["<Primary>b"]);
    app.set_accels_for_action("app.italic", &["<Primary>i"]);
    app.set_accels_for_action("app.underline", &["<Primary>u"]);

    // Lists
    {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new("bullet-list", None);
        a.connect_activate(move |_, _| { toggle_list(&tv, "bullet"); });
        app.add_action(&a);
    }
    {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new("numbered-list", None);
        a.connect_activate(move |_, _| { toggle_list(&tv, "numbered"); });
        app.add_action(&a);
    }
    app.set_accels_for_action("app.bullet-list", &["<Primary><Shift>8"]);
    app.set_accels_for_action("app.numbered-list", &["<Primary><Shift>7"]);

    // Structured List Indentation (nesting / un-nesting)
    {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new("list-increase-indent", None);
        a.connect_activate(move |_, _| {
            if let Some(buf) = active_buffer(&tv) {
                crate::bridge::adjust_list_indent(&buf, true);
            }
        });
        app.add_action(&a);
    }
    {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new("list-decrease-indent", None);
        a.connect_activate(move |_, _| {
            if let Some(buf) = active_buffer(&tv) {
                crate::bridge::adjust_list_indent(&buf, false);
            }
        });
        app.add_action(&a);
    }
    app.set_accels_for_action("app.list-increase-indent", &["<Primary>bracketright"]);
    app.set_accels_for_action("app.list-decrease-indent", &["<Primary>bracketleft"]);

    // Structured Table Actions
    {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new("table-insert-row", None);
        a.connect_activate(move |_, _| {
            if let Some(buf) = active_buffer(&tv) {
                crate::bridge::table_insert_rows(&buf, 1, 1, 1);
            }
        });
        app.add_action(&a);
    }
    {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new("table-insert-col", None);
        a.connect_activate(move |_, _| {
            if let Some(buf) = active_buffer(&tv) {
                crate::bridge::table_insert_cols(&buf, 1, 1, 1);
            }
        });
        app.add_action(&a);
    }
    {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new("table-delete-row", None);
        a.connect_activate(move |_, _| {
            if let Some(buf) = active_buffer(&tv) {
                crate::bridge::table_delete_rows(&buf, 1, 0, 1);
            }
        });
        app.add_action(&a);
    }
    {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new("table-delete-col", None);
        a.connect_activate(move |_, _| {
            if let Some(buf) = active_buffer(&tv) {
                crate::bridge::table_delete_cols(&buf, 1, 0, 1);
            }
        });
        app.add_action(&a);
    }

    // Alignment
    let align_names: &[&str] = &["align-left", "align-center", "align-right", "align-justify"];
    for name in align_names {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new(name, None);
        let name = *name;
        a.connect_activate(move |_, _| {
            if let Some(buf) = active_buffer(&tv) {
                let bounds = buf.selection_bounds();
                let (anchor, _) = bounds.unwrap_or_else(|| {
                    (buf.start_iter(), buf.start_iter())
                });
                let mut line_start = anchor;
                line_start.backward_line();
                let mut line_end = anchor;
                line_end.forward_line();
                for an in &["align-left", "align-center", "align-right", "align-justify"] {
                    if let Some(at) = buf.tag_table().lookup(an) {
                        buf.remove_tag(&at, &line_start, &line_end);
                    }
                }
                if let Some(tag) = buf.tag_table().lookup(name) {
                    buf.apply_tag(&tag, &line_start, &line_end);
                }
            }
        });
        app.add_action(&a);
    }
    app.set_accels_for_action("app.align-left", &["<Primary>l"]);
    app.set_accels_for_action("app.align-center", &["<Primary>e"]);
    app.set_accels_for_action("app.align-right", &["<Primary>r"]);
    app.set_accels_for_action("app.align-justify", &["<Primary>j"]);

    // Font size
    {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new("increase-font", None);
        a.connect_activate(move |_, _| {
            if let Some(buf) = active_buffer(&tv) {
                if let Some(tag) = buf.tag_table().lookup("font-larger") {
                    let sel = buf.selection_bounds();
                    if let Some((start, end)) = sel {
                        buf.apply_tag(&tag, &start, &end);
                    }
                }
            }
        });
        app.add_action(&a);
    }
    {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new("decrease-font", None);
        a.connect_activate(move |_, _| {
            if let Some(buf) = active_buffer(&tv) {
                if let Some(tag) = buf.tag_table().lookup("font-smaller") {
                    let sel = buf.selection_bounds();
                    if let Some((start, end)) = sel {
                        buf.apply_tag(&tag, &start, &end);
                    }
                }
            }
        });
        app.add_action(&a);
    }
    app.set_accels_for_action("app.increase-font", &["<Primary><Shift>greater"]);
    app.set_accels_for_action("app.decrease-font", &["<Primary><Shift>less"]);

    // Styles
    let styles: &[(&str, &str)] = &[
        ("style-p", ""),
        ("style-h1", "h1"), ("style-h2", "h2"), ("style-h3", "h3"),
        ("style-h4", "h4"), ("style-h5", "h5"), ("style-h6", "h6"),
        ("style-code", "code"), ("style-quote", "blockquote"),
    ];
    for (action_name, tag_name) in styles {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new(action_name, None);
        let tag_name = *tag_name;
        a.connect_activate(move |_, _| {
            if !tag_name.is_empty() {
                apply_tag_to_active(&tv, tag_name);
            }
        });
        app.add_action(&a);
    }
}

// ── Fragment & Readout Helpers ────────────────────────────────────────

pub fn insert_fragment(buf: &gtk::TextBuffer, frag: &letters_core::fragment::Fragment) {
    use letters_core::fragment::Fragment;
    match frag {
        Fragment::Text(paras) => {
            for (i, p) in paras.iter().enumerate() {
                if i > 0 {
                    buf.insert_at_cursor("\n");
                }
                for run in &p.runs {
                    let mut tags: Vec<&str> = Vec::new();
                    if run.style.bold { tags.push("bold"); }
                    if run.style.italic { tags.push("italic"); }
                    if run.style.underline { tags.push("underline"); }
                    if run.style.strikethrough { tags.push("strikethrough"); }
                    if run.style.highlight { tags.push("highlight"); }
                    if run.style.code { tags.push("code"); }
                    let mut iter = buf.iter_at_mark(&buf.get_insert());
                    if tags.is_empty() {
                        buf.insert(&mut iter, &run.text);
                    } else {
                        buf.insert_with_tags_by_name(&mut iter, &run.text, &tags);
                    }
                }
            }
        }
        Fragment::Grid(_) => {
            buf.insert_at_cursor(&frag.to_plain());
        }
    }
}

pub fn update_word_count(buf: &gtk::TextBuffer, wc: &gtk4::Label) {
    let text = buf.text(&buf.start_iter(), &buf.end_iter(), false);
    let n = text.split_whitespace().count();
    wc.set_text(&format!("{} words", n));
}

pub fn connect_word_count(buf: &gtk::TextBuffer, wc: &gtk4::Label) {
    let wc = wc.clone();
    buf.connect_changed(move |b| update_word_count(b, &wc));
}

pub fn update_style_readout(buf: &gtk::TextBuffer, label: &gtk4::Label) {
    let mut iter = buf.iter_at_mark(&buf.get_insert());
    let mut tags = iter.tags();
    if tags.is_empty() && iter.backward_char() {
        tags = iter.tags();
    }
    let names: Vec<String> = tags
        .iter()
        .filter_map(|t| t.name().map(|n| n.to_string()))
        .collect();
    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    label.set_text(&letters_core::model::style_readout(&refs));
}

pub fn connect_style_readout(buf: &gtk::TextBuffer, label: &gtk4::Label) {
    let l = label.clone();
    buf.connect_notify_local(Some("cursor-position"), move |b, _| {
        update_style_readout(b, &l);
    });
    let l = label.clone();
    buf.connect_changed(move |b| update_style_readout(b, &l));
    let l = label.clone();
    buf.connect_apply_tag(move |b, _, _, _| {
        let b = b.clone();
        let l = l.clone();
        glib::idle_add_local_once(move || update_style_readout(&b, &l));
    });
    let l = label.clone();
    buf.connect_remove_tag(move |b, _, _, _| {
        let b = b.clone();
        let l = l.clone();
        glib::idle_add_local_once(move || update_style_readout(&b, &l));
    });
}
