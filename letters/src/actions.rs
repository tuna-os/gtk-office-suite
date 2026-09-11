// actions.rs — formatting, structured editing, and macro actions for Letters.
// SPDX-License-Identifier: GPL-3.0-or-later

use gtk4::{self as gtk, glib, prelude::*};
use libadwaita as adw;

use crate::dialogs::active_buffer;

/// Register all text formatting tags with the buffer's tag table.
pub fn register_formatting_tags(buffer: &gtk::TextBuffer) {
    let tag_table = buffer.tag_table();
    let tags: &[(&str, &[(&str, &glib::Value)])] = &[
        ("bold", &[]),
        ("italic", &[]),
        ("underline", &[]),
        ("strikethrough", &[]),
        ("highlight", &[]),
        ("code", &[]),
        ("h1", &[]),
        ("h2", &[]),
        ("h3", &[]),
        ("h4", &[]),
        ("h5", &[]),
        ("h6", &[]),
        ("blockquote", &[]),
        ("align-left", &[]),
        ("align-center", &[]),
        ("align-right", &[]),
        ("align-justify", &[]),
        ("line-spacing-1.0", &[]),
        ("line-spacing-1.15", &[]),
        ("line-spacing-1.5", &[]),
        ("line-spacing-2.0", &[]),
        ("search-match", &[]),
        ("search-current", &[]),
        (crate::bridge::PAGE_BREAK_TAG, &[]),
    ];

    for &(name, _) in tags {
        if tag_table.lookup(name).is_none() {
            let tag = match name {
                "bold" => gtk::TextTag::builder().name(name).weight(700).build(),
                "italic" => gtk::TextTag::builder().name(name).style(gtk4::pango::Style::Italic).build(),
                "underline" => gtk::TextTag::builder().name(name).underline(gtk4::pango::Underline::Single).build(),
                "strikethrough" => gtk::TextTag::builder().name(name).strikethrough(true).build(),
                "highlight" => gtk::TextTag::builder().name(name).background("#fce94f").build(),
                "code" => gtk::TextTag::builder().name(name).family("monospace").background("#f0f0f0").build(),
                "h1" => gtk::TextTag::builder().name(name).weight(700).scale(1.6).build(),
                "h2" => gtk::TextTag::builder().name(name).weight(700).scale(1.4).build(),
                "h3" => gtk::TextTag::builder().name(name).weight(700).scale(1.2).build(),
                "h4" => gtk::TextTag::builder().name(name).weight(700).scale(1.1).build(),
                "h5" => gtk::TextTag::builder().name(name).weight(700).scale(1.0).build(),
                "h6" => gtk::TextTag::builder().name(name).weight(700).scale(0.9).build(),
                "blockquote" => gtk::TextTag::builder().name(name).left_margin(24).style(gtk4::pango::Style::Italic).build(),
                // A page break has no text of its own, so it has to be
                // visible as space above the paragraph it starts — the
                // paragraph tag *is* the break (see bridge.rs), and an
                // invisible one would leave the user with no way to see
                // or remove what they inserted.
                crate::bridge::PAGE_BREAK_TAG => gtk::TextTag::builder()
                    .name(name)
                    .pixels_above_lines(28)
                    .paragraph_background("#e6e6e6")
                    .build(),
                "align-left" => gtk::TextTag::builder().name(name).justification(gtk::Justification::Left).build(),
                "align-center" => gtk::TextTag::builder().name(name).justification(gtk::Justification::Center).build(),
                "align-right" => gtk::TextTag::builder().name(name).justification(gtk::Justification::Right).build(),
                "align-justify" => gtk::TextTag::builder().name(name).justification(gtk::Justification::Fill).build(),
                "line-spacing-1.0" => gtk::TextTag::builder().name(name).pixels_inside_wrap(0).pixels_below_lines(0).build(),
                "line-spacing-1.15" => gtk::TextTag::builder().name(name).pixels_inside_wrap(3).pixels_below_lines(3).build(),
                "line-spacing-1.5" => gtk::TextTag::builder().name(name).pixels_inside_wrap(8).pixels_below_lines(8).build(),
                "line-spacing-2.0" => gtk::TextTag::builder().name(name).pixels_inside_wrap(16).pixels_below_lines(16).build(),
                "search-match" => gtk::TextTag::builder().name(name).background("#fff3bf").build(),
                "search-current" => gtk::TextTag::builder().name(name).background("#f59f00").foreground("#000000").build(),
                _ => gtk::TextTag::builder().name(name).build(),
            };
            tag_table.add(&tag);
        }
    }
}

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

/// Toggle the cursor's paragraph between body text and a list item.
///
/// Model only: this used to insert a literal "\u{2022} " bullet into the
/// buffer *and* set the list kind on paragraph 0. The bridge renders a
/// list item's marker itself ("- " / "N. "), so the inserted bullet
/// survived capture as document text and the editor showed "- \u{2022} item"
/// — while the paragraph the user was actually on kept its old style.
pub fn toggle_list(tv: &adw::TabView, kind: &str) {
    let Some(buf) = active_buffer(tv) else { return };
    let list_kind = match kind {
        "bullet" => letters_core::ListKind::Bullet,
        "numbered" => letters_core::ListKind::Numbered,
        _ => return,
    };
    crate::bridge::apply_structured_edit(&buf, |editor| {
        editor.toggle_list_at_cursor(list_kind);
    });
}

/// Register formatting actions, accelerators, and palette labels.
type FormatHandler = fn(&adw::TabView);

pub fn register_formatting_actions(tv: &adw::TabView, app: &adw::Application) {
    let pairs: &[(&str, FormatHandler)] = &[
        ("bold", |tv| toggle_tag(tv, "bold")),
        ("italic", |tv| toggle_tag(tv, "italic")),
        ("underline", |tv| toggle_tag(tv, "underline")),
        ("strikethrough", |tv| toggle_tag(tv, "strikethrough")),
        ("highlight", |tv| toggle_tag(tv, "highlight")),
    ];
    for (name, handler) in pairs {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new(name, None);
        a.connect_activate(move |_, _| handler(&tv));
        app.add_action(&a);
    }

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

    // Alignment
    let align_names: &[&str] = &["align-left", "align-center", "align-right", "align-justify"];
    for name in align_names {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new(name, None);
        let name = *name;
        a.connect_activate(move |_, _| {
            if let Some(buf) = active_buffer(&tv) {
                let bounds = buf.selection_bounds();
                let (anchor, _) = bounds.unwrap_or_else(|| (buf.start_iter(), buf.start_iter()));
                let mut line_start = anchor;
                line_start.backward_line();
                let mut line_end = anchor;
                line_end.forward_line();
                for an in &["align-left", "align-center", "align-right", "align-justify"] {
                    if let Some(at) = buf.tag_table().lookup(an) {
                        buf.remove_tag(&at, &line_start, &line_end);
                    }
                }
                if name != "align-left" {
                    if let Some(tag) = buf.tag_table().lookup(name) {
                        buf.apply_tag(&tag, &line_start, &line_end);
                    }
                }
            }
        });
        app.add_action(&a);
    }

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

/// Register structured editing actions: tables, list indentation, restart numbering, page breaks.
/// The paragraph-level edits the menu offers, all relative to the caret.
#[derive(Clone, Copy)]
enum ParagraphOp {
    Indent,
    Outdent,
    RestartNumbering,
    TogglePageBreak,
}

/// The table edits the menu offers, all relative to the cursor's cell.
#[derive(Clone, Copy)]
enum TableOp {
    InsertRowAbove,
    InsertRowBelow,
    DeleteRow,
    InsertColLeft,
    InsertColRight,
    DeleteCol,
}

pub fn register_structured_actions(tv: &adw::TabView, app: &adw::Application) {
    // ── Table insertion ──
    {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new("insert-table", None);
        a.connect_activate(move |_, _| {
            if let Some(buf) = active_buffer(&tv) {
                // One operation on the document, rendered back by the
                // bridge. Writing the pipe grid straight into the buffer
                // *and* calling the editor — which is what this did — put
                // the table in twice: once as literal text the model read
                // back as prose, once as real cells appended at the end of
                // the document (#438).
                crate::bridge::apply_structured_edit(&buf, |editor| {
                    editor.insert_table(3, 3);
                });
            }
        });
        app.add_action(&a);
    }

    // ── Table row/column operations ──
    // Each targets the table the cursor is in and does nothing elsewhere.
    // They used to edit the buffer text by hand *and* call the editor with
    // a hardcoded table id of 1 and a hardcoded row/column of 0, so they
    // rewrote whichever table happened to be first in the document while
    // also inserting a literal "| New Cell 1 |" line wherever the caret
    // was — including between a header and its delimiter row.
    for (name, op) in [
        ("table-insert-row-above", TableOp::InsertRowAbove),
        ("table-insert-row-below", TableOp::InsertRowBelow),
        ("table-delete-row", TableOp::DeleteRow),
        ("table-insert-col-left", TableOp::InsertColLeft),
        ("table-insert-col-right", TableOp::InsertColRight),
        ("table-delete-col", TableOp::DeleteCol),
    ] {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new(name, None);
        a.connect_activate(move |_, _| {
            if let Some(buf) = active_buffer(&tv) {
                crate::bridge::apply_structured_edit(&buf, |editor| {
                    let _ = match op {
                        TableOp::InsertRowAbove => editor.insert_row_at_cursor(false),
                        TableOp::InsertRowBelow => editor.insert_row_at_cursor(true),
                        TableOp::DeleteRow => editor.delete_row_at_cursor(),
                        TableOp::InsertColLeft => editor.insert_col_at_cursor(false),
                        TableOp::InsertColRight => editor.insert_col_at_cursor(true),
                        TableOp::DeleteCol => editor.delete_col_at_cursor(),
                    };
                });
            }
        });
        app.add_action(&a);
    }

    // ── List nesting, numbering and page breaks ──
    // All cursor-relative. Each of these used to pass a hardcoded
    // paragraph index of 0 — editing the first paragraph of the document
    // — while separately inserting or deleting literal indent and "---"
    // text at the caret.
    for (name, op) in [
        ("list-indent", ParagraphOp::Indent),
        ("list-outdent", ParagraphOp::Outdent),
        ("list-restart-numbering", ParagraphOp::RestartNumbering),
        ("insert-page-break", ParagraphOp::TogglePageBreak),
    ] {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new(name, None);
        a.connect_activate(move |_, _| {
            if let Some(buf) = active_buffer(&tv) {
                crate::bridge::apply_structured_edit(&buf, |editor| {
                    let _ = match op {
                        ParagraphOp::Indent => editor.indent_list_at_cursor(),
                        ParagraphOp::Outdent => editor.outdent_list_at_cursor(),
                        ParagraphOp::RestartNumbering => editor.restart_numbering_at_cursor(),
                        ParagraphOp::TogglePageBreak => editor.toggle_page_break_at_cursor(),
                    };
                });
            }
        });
        app.add_action(&a);
    }

    app.set_accels_for_action("app.insert-page-break", &["<Primary>Return"]);
    app.set_accels_for_action("app.list-indent", &["<Primary>bracketright"]);
    app.set_accels_for_action("app.list-outdent", &["<Primary>bracketleft"]);
}

/// Connect list auto-continuation on Enter for a TextView.
pub fn connect_list_continuation(editor: &gtk::TextView, buf: &gtk::TextBuffer) {
    let buf = buf.clone();
    let ctrl = gtk::EventControllerKey::new();
    ctrl.connect_key_pressed(move |_, key, _code, _state| {
        if key == gtk::gdk::Key::Return || key == gtk::gdk::Key::KP_Enter {
            let bounds = buf.selection_bounds();
            let (ins, _) = bounds.unwrap_or((buf.start_iter(), buf.start_iter()));
            let mut line_start = ins;
            line_start.backward_line();
            let mut line_end = ins;
            line_end.forward_line();
            let line = buf.text(&line_start, &line_end, false);
            let trimmed = line.trim_start();

            // Bullet list continuation
            if trimmed.starts_with("\u{2022}") || trimmed.starts_with("- ") {
                let indent = line.len() - trimmed.len();
                let marker = "\u{2022} ";
                let after_marker = trimmed
                    .strip_prefix("\u{2022}").or_else(|| trimmed.strip_prefix("- "))
                    .unwrap_or("").trim_start();
                if after_marker.is_empty() {
                    return glib::Propagation::Proceed;
                }
                let prefix = format!("{}{}", " ".repeat(indent), marker);
                buf.insert(&mut line_end, &prefix);
                return glib::Propagation::Stop;
            }

            // Numbered list continuation
            if trimmed.starts_with(|c: char| c.is_ascii_digit()) && trimmed.contains(". ") {
                let num_str: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
                let after_num = &trimmed[num_str.len()..];
                let rest = after_num.strip_prefix(". ").unwrap_or("");
                if let Ok(n) = num_str.parse::<usize>() {
                    if rest.is_empty() {
                        return glib::Propagation::Proceed;
                    }
                    let indent = line.len() - trimmed.len();
                    let new_prefix = format!("{}{}. ", " ".repeat(indent), n + 1);
                    buf.insert(&mut line_end, &new_prefix);
                    return glib::Propagation::Stop;
                }
            }
        }
        glib::Propagation::Proceed
    });
    editor.add_controller(ctrl);
}

/// Connect Markdown inline macro expansion on space / punctuation.
pub fn connect_markdown_macros(buf: &gtk::TextBuffer) {
    let buf_c = buf.clone();
    buf.connect_insert_text(move |b, pos, text| {
        if text != " " && text != "\n" { return; }
        let offset = pos.offset();
        if offset < 2 { return; }

        let mut line_start = *pos;
        line_start.backward_line();
        let before = b.text(&line_start, pos, false);

        if let Some(inner) = extract_md_pattern(&before, "**", "**") {
            apply_md_pattern(b, &before, "**", inner, "bold");
        } else if let Some(inner) = extract_md_pattern(&before, "_", "_") {
            apply_md_pattern(b, &before, "_", inner, "italic");
        } else if let Some(inner) = extract_md_pattern(&before, "~~", "~~") {
            apply_md_pattern(b, &before, "~~", inner, "strikethrough");
        } else if let Some(inner) = extract_md_pattern(&before, "==", "==") {
            apply_md_pattern(b, &before, "==", inner, "highlight");
        } else if let Some(inner) = extract_md_pattern(&before, "`", "`") {
            apply_md_pattern(b, &before, "`", inner, "code");
        }
    });
    let _ = buf_c;
}

fn extract_md_pattern<'a>(before: &'a str, open: &str, close: &str) -> Option<&'a str> {
    if !before.ends_with(close) { return None; }
    let without_close = &before[..before.len() - close.len()];
    let open_pos = without_close.rfind(open)?;
    let inner = &without_close[open_pos + open.len()..];
    if inner.is_empty() || inner.starts_with(' ') || inner.ends_with(' ') {
        return None;
    }
    Some(inner)
}

fn apply_md_pattern(buf: &gtk::TextBuffer, before: &str, delimiter: &str, inner: &str, tag_name: &str) {
    let del_len = delimiter.len() * 2 + inner.len();
    let bounds = buf.selection_bounds();
    let (mut end, _) = bounds.unwrap_or((buf.start_iter(), buf.start_iter()));
    let mut start = end;
    start.backward_chars(del_len as i32);
    if start < end {
        buf.begin_user_action();
        buf.delete(&mut start, &mut end);
        buf.insert(&mut start, inner);
        if let Some(tag) = buf.tag_table().lookup(tag_name) {
            let mut tag_end = start;
            tag_end.forward_chars(inner.chars().count() as i32);
            buf.apply_tag(&tag, &start, &tag_end);
        }
        buf.end_user_action();
    }
    let _ = before;
}
