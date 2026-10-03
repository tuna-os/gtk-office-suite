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
        ("superscript", &[]),
        ("subscript", &[]),
        ("h1", &[]),
        ("h2", &[]),
        ("h3", &[]),
        ("h4", &[]),
        ("h5", &[]),
        ("h6", &[]),
        ("blockquote", &[]),
        ("h-title", &[]),
        ("h-subtitle", &[]),
        ("code-block", &[]),
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
                // 58% size, as LibreOffice and Word draw super/subscript;
                // the rise is in Pango units (1/1024 pt), a third of an
                // 11 pt line up and a sixth down.
                "superscript" => gtk::TextTag::builder().name(name).scale(0.58).rise(4 * gtk4::pango::SCALE).build(),
                "subscript" => gtk::TextTag::builder().name(name).scale(0.58).rise(-2 * gtk4::pango::SCALE).build(),
                "h1" => gtk::TextTag::builder().name(name).weight(700).scale(1.6).build(),
                "h2" => gtk::TextTag::builder().name(name).weight(700).scale(1.4).build(),
                "h3" => gtk::TextTag::builder().name(name).weight(700).scale(1.2).build(),
                "h4" => gtk::TextTag::builder().name(name).weight(700).scale(1.1).build(),
                "h5" => gtk::TextTag::builder().name(name).weight(700).scale(1.0).build(),
                "h6" => gtk::TextTag::builder().name(name).weight(700).scale(0.9).build(),
                "blockquote" => gtk::TextTag::builder().name(name).left_margin(24).style(gtk4::pango::Style::Italic).build(),
                // The page's Title, Subtitle and code block looks
                // (letters_core::layout::Look); the document state rides on
                // the paragraph's `para:` tag, these only draw it.
                "h-title" => gtk::TextTag::builder().name(name).weight(700).scale(26.0 / 11.0).build(),
                "h-subtitle" => gtk::TextTag::builder().name(name).scale(15.0 / 11.0).foreground("#666666").build(),
                "code-block" => gtk::TextTag::builder().name(name).family("monospace").paragraph_background("#f0f0f0").build(),
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


pub fn toggle_tag(tv: &adw::TabView, tag_name: &str) {
    if let Some(buf) = active_buffer(tv) {
        toggle_tag_in(&buf, tag_name);
    }
}

/// Toggle `tag_name` over the selection, or, with nothing selected, for
/// the text typed next at the caret: Bold, then type, types bold.
///
/// On a tab's document the toggle is a model op (`Op::Mark`), one undo step
/// that the buffer then shows, rather than a tag applied to the buffer and
/// read back (#1202 stage 3). The first selected character decides: bold
/// already, the selection loses bold; otherwise it gains it.
pub(crate) fn toggle_tag_in(buf: &gtk::TextBuffer, tag_name: &str) {
    let Some((start, end)) = buf.selection_bounds() else {
        toggle_pending(buf, tag_name);
        return;
    };
    let (from, to) = (start.offset().max(0), end.offset().max(0));
    if let (Some(live), Some(key)) = (crate::live::of(buf), mark_key(tag_name)) {
        let mut m = live.borrow_mut();
        let (s, e) = (m.sequence_offset(buf, from as usize), m.sequence_offset(buf, to as usize));
        let first = letters_core::edit::slice(m.document(buf), s, e)
            .and_then(|paras| paras.into_iter().flat_map(|p| p.runs).find(|r| !r.text.is_empty()));
        let has = first.is_some_and(|r| {
            let mut style = r.style;
            style_flag(&mut style, tag_name).is_some_and(|f| *f)
        });
        let mut value = letters_core::RunStyle::default();
        if let Some(flag) = style_flag(&mut value, tag_name) {
            *flag = !has;
        }
        let applied = m.apply_user_ops(buf, &[letters_core::edit::Op::Mark { start: s, end: e, key, value }], false);
        drop(m);
        // Marks move no text, so the selection's offsets still hold; the
        // model's projection put the caret at the change's end.
        if applied {
            buf.select_range(&buf.iter_at_offset(from), &buf.iter_at_offset(to));
        }
        return;
    }
    let Some(tag) = buf.tag_table().lookup(tag_name) else { return };
    let has = start.tags().iter().any(|t| t.name().as_deref() == Some(tag_name));
    if has {
        buf.remove_tag(&tag, &start, &end);
    } else {
        buf.apply_tag(&tag, &start, &end);
    }
}

/// Align the paragraphs the selection touches, or the caret's paragraph,
/// as a model op (#1202 stage 3). It used to tag buffer lines from the line
/// *before* the anchor, so a neighbouring paragraph was aligned too, and
/// with nothing selected the anchor was the document's start rather than
/// the caret.
pub(crate) fn align_in(buf: &gtk::TextBuffer, name: &str) {
    let alignment = match name {
        "align-left" => letters_core::Alignment::Left,
        "align-center" => letters_core::Alignment::Center,
        "align-right" => letters_core::Alignment::Right,
        "align-justify" => letters_core::Alignment::Justify,
        _ => return,
    };
    let Some(live) = crate::live::of(buf) else { return };
    let (start, end) = buf.selection_bounds().unwrap_or_else(|| {
        let caret = buf.iter_at_mark(&buf.get_insert());
        (caret, caret)
    });
    let (from, to) = (start.offset().max(0), end.offset().max(0));
    let restyled = live.borrow_mut().restyle(buf, from as usize, to as usize, |style| letters_core::ParaStyle { alignment, ..style.clone() });
    // A paragraph style moves no text: put the caret or selection back
    // where the projection found it.
    if restyled {
        buf.select_range(&buf.iter_at_offset(from), &buf.iter_at_offset(to));
    }
}

/// The selection's buffer offsets, or the caret's twice.
fn caret_or_selection(buf: &gtk::TextBuffer) -> (usize, usize) {
    let (start, end) = buf.selection_bounds().unwrap_or_else(|| {
        let caret = buf.iter_at_mark(&buf.get_insert());
        (caret, caret)
    });
    (start.offset().max(0) as usize, end.offset().max(0) as usize)
}

/// Restyle the caret's or the selection's paragraphs with `f` as one model
/// op, putting the caret or selection back where it was.
fn restyle_here(buf: &gtk::TextBuffer, f: impl Fn(&letters_core::ParaStyle) -> letters_core::ParaStyle) -> bool {
    let Some(live) = crate::live::of(buf) else { return false };
    let (from, to) = caret_or_selection(buf);
    let restyled = live.borrow_mut().restyle(buf, from, to, f);
    if restyled {
        buf.select_range(&buf.iter_at_offset(from as i32), &buf.iter_at_offset(to as i32));
    }
    restyled
}

/// The line spacings Cycle Line Spacing steps through.
const LINE_SPACINGS: [f32; 4] = [1.0, 1.15, 1.5, 2.0];

/// Give the caret's or selection's paragraphs the line spacing after the
/// first one's, as a model op (#1202 stage 3), and return it. It used to
/// tag buffer lines from the line before the caret, so a neighbouring
/// paragraph changed too, and the model never saw the spacing: it was not
/// saved.
pub(crate) fn cycle_line_spacing_in(buf: &gtk::TextBuffer) -> Option<f32> {
    let live = crate::live::of(buf)?;
    let current = live.borrow_mut().paragraph_style_at(buf, caret_or_selection(buf).0)?.line_spacing;
    let next = *LINE_SPACINGS.iter().find(|s| **s > current + 0.01).unwrap_or(&LINE_SPACINGS[0]);
    restyle_here(buf, |style| letters_core::ParaStyle { line_spacing: next, ..style.clone() }).then_some(next)
}

/// Set the caret's or selection's paragraphs' tab stops (points from the
/// paragraph's left edge) as a model op. The ruler used to put them on a
/// buffer tag the model never read, so they were not saved.
pub(crate) fn set_tab_stops_in(buf: &gtk::TextBuffer, tabs: &[f64]) -> bool {
    restyle_here(buf, |style| letters_core::ParaStyle { tab_stops_pt: tabs.to_vec(), ..style.clone() })
}

/// The model mark a toggle name sets, if it is one of the toggles.
fn mark_key(name: &str) -> Option<letters_core::edit::MarkKey> {
    use letters_core::edit::MarkKey;
    Some(match name {
        "bold" => MarkKey::Bold,
        "italic" => MarkKey::Italic,
        "underline" => MarkKey::Underline,
        "strikethrough" => MarkKey::Strikethrough,
        "highlight" => MarkKey::Highlight,
        _ => return None,
    })
}

/// Character formatting chosen with nothing selected, waiting for the next
/// text typed at buffer offset `at` (`take_pending`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PendingFormat {
    pub at: i32,
    /// Formatting name (as `toggle_tag` takes it) and whether it turns on.
    pub marks: Vec<(String, bool)>,
}

const PENDING_KEY: &str = "letters-pending-format";

/// The run-style field a toggle name sets, if it is one of the toggles.
pub(crate) fn style_flag<'a>(style: &'a mut letters_core::RunStyle, name: &str) -> Option<&'a mut bool> {
    Some(match name {
        "bold" => &mut style.bold,
        "italic" => &mut style.italic,
        "underline" => &mut style.underline,
        "strikethrough" => &mut style.strikethrough,
        "highlight" => &mut style.highlight,
        _ => return None,
    })
}

fn toggle_pending(buf: &gtk::TextBuffer, name: &str) {
    let at = buf.iter_at_mark(&buf.get_insert()).offset();
    // Pending formatting belongs to one caret position: moved, it is gone.
    let mut pending = take_pending(buf).filter(|p| p.at == at).unwrap_or(PendingFormat { at, marks: Vec::new() });
    if let Some(i) = pending.marks.iter().position(|(n, _)| n == name) {
        // A second press takes the first back.
        pending.marks.remove(i);
    } else {
        // The opposite of what typing here would get: what the model's
        // typing rule gives at the caret.
        let mut typed = match crate::live::of(buf) {
            Some(m) => {
                let mut m = m.borrow_mut();
                let seq = m.sequence_offset(buf, at.max(0) as usize);
                letters_core::edit::typing_style(m.document(buf), seq)
            }
            None => letters_core::RunStyle::default(),
        };
        let Some(current) = style_flag(&mut typed, name) else { return };
        pending.marks.push((name.to_string(), !*current));
    }
    if !pending.marks.is_empty() {
        unsafe { buf.set_data(PENDING_KEY, pending) };
    }
}

/// Take the formatting waiting for typing at buffer offset `at`; pending
/// formatting left at another offset is dropped.
pub(crate) fn take_pending(buf: &gtk::TextBuffer) -> Option<PendingFormat> {
    unsafe { buf.steal_data::<PendingFormat>(PENDING_KEY) }
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
                align_in(&buf, name);
            }
        });
        app.add_action(&a);
    }

    // Paragraph styles: the style picker's model ops (style_picker.rs).
    let styles: &[(&str, &str)] = &[
        ("style-p", "Normal"),
        ("style-h1", "Heading 1"), ("style-h2", "Heading 2"), ("style-h3", "Heading 3"),
        ("style-h4", "Heading 4"), ("style-h5", "Heading 5"), ("style-h6", "Heading 6"),
        ("style-code", "Code"), ("style-quote", "Quote"),
    ];
    for (action_name, style) in styles {
        let tv = tv.clone();
        let a = gtk::gio::SimpleAction::new(action_name, None);
        let style = *style;
        a.connect_activate(move |_, _| {
            if let Some(buf) = active_buffer(&tv) {
                crate::style_picker::apply(&buf, style);
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

/// Connect Markdown inline macro expansion on space / punctuation.
pub fn connect_markdown_macros(buf: &gtk::TextBuffer) {
    let buf_c = buf.clone();
    buf.connect_insert_text(move |b, pos, text| {
        if text != " " && text != "\n" { return; }
        // Not while the live model is writing into the buffer (Print
        // Layout's edits, undo): that text is the model's, not typed here.
        if crate::live::is_busy(b) { return; }
        // After the insertion completes: a handler that edits the buffer
        // while GTK is still inserting invalidates the insert position.
        let (b, at) = (b.clone(), pos.offset());
        glib::idle_add_local_once(move || {
            let end = b.iter_at_offset(at);
            markdown_macro_at(&b, &end);
        });
    });
    let _ = buf_c;
}

/// Expand a Markdown inline macro ("**bold**", "_it_", …) that ends at
/// `pos`: typing into the buffer runs it as a space or newline is typed,
/// the page view after typing one.
pub fn markdown_macro_at(b: &gtk::TextBuffer, pos: &gtk::TextIter) {
    {
        let offset = pos.offset();
        if offset < 2 { return; }

        let mut line_start = *pos;
        line_start.set_line_offset(0);
        let before = b.text(&line_start, pos, false);

        if let Some(inner) = extract_md_pattern(&before, "**", "**") {
            apply_md_pattern(b, pos, "**", inner, "bold");
        } else if let Some(inner) = extract_md_pattern(&before, "_", "_") {
            apply_md_pattern(b, pos, "_", inner, "italic");
        } else if let Some(inner) = extract_md_pattern(&before, "~~", "~~") {
            apply_md_pattern(b, pos, "~~", inner, "strikethrough");
        } else if let Some(inner) = extract_md_pattern(&before, "==", "==") {
            apply_md_pattern(b, pos, "==", inner, "highlight");
        } else if let Some(inner) = extract_md_pattern(&before, "`", "`") {
            apply_md_pattern(b, pos, "`", inner, "code");
        }
    }
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

/// Replace the "**inner**" ending at `at` with `inner`, formatted. (This
/// used to take the pattern's end from the selection, which with no
/// selection is the buffer start: the shortcuts never fired.)
///
/// On a tab's document it is one model edit (#1202 stage 3): the text
/// keeps the formatting it was typed with and gains `tag_name`'s, and the
/// caret stays where typing left it rather than where the edit ended.
fn apply_md_pattern(buf: &gtk::TextBuffer, at: &gtk::TextIter, delimiter: &str, inner: &str, tag_name: &str) {
    let del_len = delimiter.chars().count() * 2 + inner.chars().count();
    let mut end = *at;
    let mut start = end;
    start.backward_chars(del_len as i32);
    if start >= end {
        return;
    }
    if let Some(live) = crate::live::of(buf) {
        use letters_core::edit::Op;
        let (from, to) = (start.offset().max(0) as usize, end.offset().max(0) as usize);
        let caret = buf.iter_at_mark(&buf.get_insert()).offset().max(0) as usize;
        let mut m = live.borrow_mut();
        let (s, e) = (m.sequence_offset(buf, from), m.sequence_offset(buf, to));
        let mut style = letters_core::edit::slice(m.document(buf), s, e)
            .and_then(|paras| paras.into_iter().flat_map(|p| p.runs).find(|r| !r.text.is_empty()))
            .map(|r| r.style)
            .unwrap_or_default();
        match tag_name {
            "code" => style.code = true,
            name => {
                if let Some(flag) = style_flag(&mut style, name) {
                    *flag = true;
                }
            }
        }
        let ops = [
            Op::Delete { at: s, len: e - s },
            Op::Insert {
                at: s,
                content: vec![letters_core::Paragraph {
                    style: Default::default(),
                    runs: vec![letters_core::model::Run { text: inner.to_string(), style }],
                }],
            },
        ];
        let applied = m.apply_user_ops(buf, &ops, false);
        drop(m);
        if applied {
            let removed = del_len - inner.chars().count();
            let caret = if caret >= to { caret - removed } else { caret.min(from) };
            buf.place_cursor(&buf.iter_at_offset(caret as i32));
        }
        return;
    }
    buf.begin_user_action();
    buf.delete(&mut start, &mut end);
    let from = start.offset();
    // `insert` moves the iter to the end of what it inserted.
    buf.insert(&mut start, inner);
    if let Some(tag) = buf.tag_table().lookup(tag_name) {
        buf.apply_tag(&tag, &buf.iter_at_offset(from), &start);
    }
    buf.end_user_action();
}
