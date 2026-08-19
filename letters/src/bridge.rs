// bridge.rs — GtkTextBuffer ⇄ letters_core::Document.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The buffer is a *view*: letters-core owns document semantics and all file
// I/O. This module is the only place buffer tags are translated to/from
// model styles. Tag names map 1:1 to RunStyle fields / heading levels
// (see register_formatting_tags in window.rs).
//
// Links use dynamic "link:<url>" tags; alignment uses the align-* tags;
// list kinds translate to/from the editor's literal "- " / "N. " markers.

use gtk4::{self as gtk, prelude::*};
use letters_core::model::{Document, Paragraph, Run, RunStyle};

const RUN_TAGS: [&str; 6] = ["bold", "italic", "underline", "strikethrough", "highlight", "code"];

/// GtkTextTag name for a discrete line-spacing multiplier — reuses the
/// same "line-spacing-1.0"/"1.15"/"1.5"/"2.0" tags window.rs's
/// register_formatting_tags already registers and its cycle-line-spacing
/// action already applies live; this just makes the choice persist
/// through Document/DOCX/ODT instead of being GTK-buffer-only. `None`
/// for the default single spacing (1.0 — no tag needed on render, mirrors
/// `Alignment::Left`); capture still recognizes an explicit
/// "line-spacing-1.0" tag if present (the live-editing action applies
/// one for that case too), mapping it back to 1.0 all the same.
fn line_spacing_tag_name(spacing: f32) -> Option<&'static str> {
    if (spacing - 1.15).abs() < 0.01 {
        Some("line-spacing-1.15")
    } else if (spacing - 1.5).abs() < 0.01 {
        Some("line-spacing-1.5")
    } else if (spacing - 2.0).abs() < 0.01 {
        Some("line-spacing-2.0")
    } else {
        None
    }
}

/// Inverse of [`line_spacing_tag_name`] — covers "line-spacing-1.0" too
/// (captured back as 1.0, same as no tag) since the live-editing action
/// applies it explicitly for the default case.
fn line_spacing_from_tag_name(name: &str) -> Option<f32> {
    match name {
        "line-spacing-1.0" => Some(1.0),
        "line-spacing-1.15" => Some(1.15),
        "line-spacing-1.5" => Some(1.5),
        "line-spacing-2.0" => Some(2.0),
        _ => None,
    }
}

/// Rebuild a Document from the buffer's text and tags.
pub fn capture_from_buffer(buf: &gtk::TextBuffer) -> Document {
    let table = buf.tag_table();
    let run_tags: Vec<(usize, gtk::TextTag)> = RUN_TAGS
        .iter()
        .enumerate()
        .filter_map(|(i, n)| table.lookup(n).map(|t| (i, t)))
        .collect();
    let heading_tags: Vec<(u8, gtk::TextTag)> = (1u8..=6)
        .filter_map(|l| table.lookup(&format!("h{l}")).map(|t| (l, t)))
        .collect();

    let style_at = |iter: &gtk::TextIter| -> RunStyle {
        let mut s = RunStyle::default();
        for (i, tag) in &run_tags {
            if iter.has_tag(tag) {
                match RUN_TAGS[*i] {
                    "bold" => s.bold = true,
                    "italic" => s.italic = true,
                    "underline" => s.underline = true,
                    "strikethrough" => s.strikethrough = true,
                    "highlight" => s.highlight = true,
                    "code" => s.code = true,
                    _ => unreachable!(),
                }
            }
        }
        // Links use one dynamically-created tag per URL, named "link:<url>".
        for tag in iter.tags() {
            if let Some(name) = tag.name() {
                if let Some(url) = name.strip_prefix("link:") {
                    s.link = Some(url.to_string());
                    break;
                }
            }
        }
        s
    };

    let align_tags: Vec<(letters_core::Alignment, gtk::TextTag)> = [
        (letters_core::Alignment::Center, "align-center"),
        (letters_core::Alignment::Right, "align-right"),
        (letters_core::Alignment::Justify, "align-justify"),
    ]
    .into_iter()
    .filter_map(|(a, n)| table.lookup(n).map(|t| (a, t)))
    .collect();

    let line_spacing_tags: Vec<(f32, gtk::TextTag)> =
        ["line-spacing-1.15", "line-spacing-1.5", "line-spacing-2.0", "line-spacing-1.0"]
            .into_iter()
            .filter_map(|n| table.lookup(n).map(|t| (line_spacing_from_tag_name(n).unwrap(), t)))
            .collect();

    let mut paragraphs: Vec<Paragraph> = Vec::new();
    let mut current = Paragraph::default();
    let mut current_run: Option<Run> = None;
    let mut at_line_start = true;

    let mut iter = buf.start_iter();
    while !iter.is_end() {
        if at_line_start {
            for (level, tag) in &heading_tags {
                if iter.has_tag(tag) {
                    current.style.heading = Some(*level);
                    break;
                }
            }
            for (align, tag) in &align_tags {
                if iter.has_tag(tag) {
                    current.style.alignment = *align;
                    break;
                }
            }
            for (spacing, tag) in &line_spacing_tags {
                if iter.has_tag(tag) {
                    current.style.line_spacing = *spacing;
                    break;
                }
            }
            at_line_start = false;
        }
        // Embedded images appear as the object-replacement char; the source
        // path and alt text ride on the paintable itself (see render side).
        if let Some(paintable) = iter.paintable() {
            let src: Option<String> = unsafe {
                paintable.data::<String>("letters-image-src").map(|p| p.as_ref().clone())
            };
            let alt: String = unsafe {
                paintable.data::<String>("letters-image-alt")
                    .map(|p| p.as_ref().clone()).unwrap_or_default()
            };
            if let Some(src) = src {
                if let Some(r) = current_run.take() {
                    current.runs.push(r);
                }
                current.runs.push(Run {
                    text: alt,
                    style: RunStyle { image: Some(src), ..Default::default() },
                });
                iter.forward_char();
                continue;
            }
        }
        // Footnote markers carry an "fnref:N" tag; the visible "[n]"
        // text is presentation only — capture emits a reference run.
        let fn_idx = iter.tags().iter().find_map(|t| {
            t.name()
                .and_then(|n| n.strip_prefix("fnref:").map(str::to_string))
                .and_then(|v| v.parse::<usize>().ok())
        });
        if let Some(idx) = fn_idx {
            if let Some(r) = current_run.take() {
                current.runs.push(r);
            }
            current.runs.push(Run {
                text: String::new(),
                style: RunStyle { footnote: Some(idx), ..Default::default() },
            });
            while !iter.is_end()
                && iter.tags().iter().any(|t| {
                    t.name().map(|n| n.starts_with("fnref:")).unwrap_or(false)
                })
            {
                iter.forward_char();
            }
            continue;
        }
        let ch = iter.char();
        if ch == '\n' {
            if let Some(r) = current_run.take() {
                current.runs.push(r);
            }
            capture_list_marker(&mut current);
            paragraphs.push(std::mem::take(&mut current));
            at_line_start = true;
        } else {
            let style = style_at(&iter);
            match &mut current_run {
                Some(r) if r.style == style => r.text.push(ch),
                _ => {
                    if let Some(r) = current_run.take() {
                        current.runs.push(r);
                    }
                    current_run = Some(Run { text: ch.to_string(), style });
                }
            }
        }
        iter.forward_char();
    }
    if let Some(r) = current_run.take() {
        current.runs.push(r);
    }
    capture_list_marker(&mut current);
    paragraphs.push(current);

    // The footnote texts ride on the buffer (set by render/insert).
    let footnotes: Vec<String> = unsafe {
        buf.data::<Vec<String>>(FOOTNOTES_KEY)
            .map(|p| p.as_ref().clone())
            .unwrap_or_default()
    };
    Document { paragraphs, footnotes, header: None, footer: None, page: None }
}

/// Buffer data key holding the document's footnote texts.
pub const FOOTNOTES_KEY: &str = "letters-footnotes";

/// The editor shows lists as literal "- " / "N. " markers; the model wants
/// ListKind. Strip the marker and set the kind when capturing.
fn capture_list_marker(para: &mut Paragraph) {
    let text = para.text();
    // Four spaces represent one nesting level in the editable buffer, so the
    // marker is matched after the indent. The model keeps the level
    // separately, so DOCX/ODT round-trips do not depend on literal
    // whitespace in the paragraph text.
    let indent = text.len() - text.trim_start_matches(' ').len();
    let body = &text[indent..];
    let (kind, marker) = if body.starts_with("- ") {
        (letters_core::ListKind::Bullet, 2)
    } else if let Some(dot) = body.find(". ") {
        if dot > 0 && body[..dot].chars().all(|c| c.is_ascii_digit()) {
            (letters_core::ListKind::Numbered, dot + 2)
        } else {
            return;
        }
    } else {
        return;
    };
    para.style.list = kind;
    para.style.list_level = (indent / 4) as u8;
    // Remove the indent and marker chars from the front of the run list.
    let mut remaining = indent + marker;
    while remaining > 0 {
        let Some(first) = para.runs.first_mut() else { break };
        let n = first.text.chars().count();
        if n <= remaining {
            remaining -= n;
            para.runs.remove(0);
        } else {
            let byte = first.text.char_indices().nth(remaining).map(|(b, _)| b).unwrap();
            first.text = first.text[byte..].to_string();
            remaining = 0;
        }
    }
}

/// Replace the buffer's content with a rendered Document.
pub fn render_to_buffer(doc: &Document, buf: &gtk::TextBuffer) {
    unsafe { buf.set_data(FOOTNOTES_KEY, doc.footnotes.clone()) };
    buf.set_text("");
    let mut insert = buf.start_iter();
    for (i, para) in doc.paragraphs.iter().enumerate() {
        if i > 0 {
            buf.insert(&mut insert, "\n");
        }
        let para_start = insert.offset();
        match para.style.list {
            letters_core::ListKind::Bullet => buf.insert(&mut insert, &format!("{}- ", "    ".repeat(para.style.list_level as usize))),
            letters_core::ListKind::Numbered => {
                // Number within the current consecutive numbered group at
                // this nesting level; list_start explicitly restarts it.
                let n = doc.paragraphs[..i].iter().rev()
                    .take_while(|p| p.style.list == letters_core::ListKind::Numbered && p.style.list_level == para.style.list_level)
                    .count() as u32 + para.style.list_start.unwrap_or(1);
                buf.insert(&mut insert, &format!("{}{}. ", "    ".repeat(para.style.list_level as usize), n));
            }
            letters_core::ListKind::None => {}
        }
        for run in &para.runs {
            if let Some(src) = &run.style.image {
                match gtk4::gdk::Texture::from_filename(src) {
                    Ok(texture) => {
                        unsafe {
                            texture.set_data("letters-image-src", src.clone());
                            texture.set_data("letters-image-alt", run.text.clone());
                        }
                        buf.insert_paintable(&mut insert, &texture);
                    }
                    // Unloadable image degrades to visible alt text.
                    Err(_) => buf.insert(&mut insert, &run.text),
                }
                continue;
            }
            if let Some(idx) = run.style.footnote {
                insert_footnote_marker(buf, &mut insert, idx);
                continue;
            }
            let mut names: Vec<&str> = Vec::new();
            if run.style.bold { names.push("bold"); }
            if run.style.italic { names.push("italic"); }
            if run.style.underline { names.push("underline"); }
            if run.style.strikethrough { names.push("strikethrough"); }
            if run.style.highlight { names.push("highlight"); }
            if run.style.code { names.push("code"); }
            let link_tag_name = run.style.link.as_ref().map(|url| {
                let name = format!("link:{url}");
                if buf.tag_table().lookup(&name).is_none() {
                    let tag = gtk::TextTag::builder()
                        .name(&name)
                        .foreground("#1a5fb4")
                        .underline(gtk4::pango::Underline::Single)
                        .build();
                    buf.tag_table().add(&tag);
                }
                name
            });
            if let Some(n) = &link_tag_name { names.push(n.as_str()); }
            if names.is_empty() {
                buf.insert(&mut insert, &run.text);
            } else {
                buf.insert_with_tags_by_name(&mut insert, &run.text, &names);
            }
        }
        let mut para_tags: Vec<String> = Vec::new();
        match (para.style.heading, &para.style.code_block) {
            (Some(l), _) => para_tags.push(format!("h{}", l.clamp(1, 6))),
            (None, Some(_)) => para_tags.push("code".to_string()),
            _ => {}
        }
        match para.style.alignment {
            letters_core::Alignment::Center => para_tags.push("align-center".into()),
            letters_core::Alignment::Right => para_tags.push("align-right".into()),
            letters_core::Alignment::Justify => para_tags.push("align-justify".into()),
            letters_core::Alignment::Left => {}
        }
        if let Some(name) = line_spacing_tag_name(para.style.line_spacing) {
            para_tags.push(name.to_string());
        }
        for name in para_tags {
            let start = buf.iter_at_offset(para_start);
            buf.apply_tag_by_name(&name, &start, &insert);
        }
    }
    buf.set_modified(false);
}

/// Read any supported file through letters-core into the buffer.
pub fn load_file_to_buffer(path: &str, buf: &gtk::TextBuffer) -> Result<(), String> {
    let ext = std::path::Path::new(path)
        .extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let doc = match ext.as_str() {
        "docx" => letters_core::docx::read(path)?,
        "odt" => letters_core::odt::read(path)?,
        _ => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("Cannot read {path}: {e}"))?;
            letters_core::markdown::parse(&text)
        }
    };
    render_to_buffer(&doc, buf);
    Ok(())
}

/// Save the buffer through letters-core in the format the path implies.
pub fn save_buffer_to_file(buf: &gtk::TextBuffer, path: &str) -> Result<(), String> {
    let doc = capture_from_buffer(buf);
    let ext = std::path::Path::new(path)
        .extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    match ext.as_str() {
        "docx" => letters_core::docx::write(&doc, path),
        "odt" => letters_core::odt::write(&doc, path),
        _ => {
            let md = letters_core::markdown::serialize(&doc);
            suite_common::atomic_save::atomic_write_bytes(
                std::path::Path::new(path),
                md.as_bytes(),
            )
        }
    }
}

/// Insert the visible "[n]" marker for footnote index `idx`, tagged
/// "fnref:idx" (superscript, accent color). Shared by render and the
/// Insert Footnote action.
pub fn insert_footnote_marker(buf: &gtk::TextBuffer, insert: &mut gtk::TextIter, idx: usize) {
    let name = format!("fnref:{idx}");
    if buf.tag_table().lookup(&name).is_none() {
        let tag = gtk::TextTag::builder()
            .name(&name)
            .foreground("#1a5fb4")
            .rise(4000)
            .scale(0.75)
            .build();
        buf.tag_table().add(&tag);
    }
    buf.insert_with_tags_by_name(insert, &format!("[{}]", idx + 1), &[&name]);
}

use letters_core::structured::StructuredEditor;

/// Insert table rows in the active buffer using StructuredEditor.
pub fn table_insert_rows(buf: &gtk::TextBuffer, table_id: u32, at: u32, count: u32) -> bool {
    let doc = capture_from_buffer(buf);
    let mut editor = StructuredEditor::new(doc);
    if editor.insert_table_rows(table_id, at, count) {
        render_to_buffer(editor.document(), buf);
        buf.set_modified(true);
        true
    } else {
        false
    }
}

/// Insert table columns in the active buffer using StructuredEditor.
pub fn table_insert_cols(buf: &gtk::TextBuffer, table_id: u32, at: u32, count: u32) -> bool {
    let doc = capture_from_buffer(buf);
    let mut editor = StructuredEditor::new(doc);
    if editor.insert_table_cols(table_id, at, count) {
        render_to_buffer(editor.document(), buf);
        buf.set_modified(true);
        true
    } else {
        false
    }
}

/// Delete table rows in the active buffer using StructuredEditor.
pub fn table_delete_rows(buf: &gtk::TextBuffer, table_id: u32, at: u32, count: u32) -> bool {
    let doc = capture_from_buffer(buf);
    let mut editor = StructuredEditor::new(doc);
    if editor.delete_table_rows(table_id, at, count) {
        render_to_buffer(editor.document(), buf);
        buf.set_modified(true);
        true
    } else {
        false
    }
}

/// Delete table columns in the active buffer using StructuredEditor.
pub fn table_delete_cols(buf: &gtk::TextBuffer, table_id: u32, at: u32, count: u32) -> bool {
    let doc = capture_from_buffer(buf);
    let mut editor = StructuredEditor::new(doc);
    if editor.delete_table_cols(table_id, at, count) {
        render_to_buffer(editor.document(), buf);
        buf.set_modified(true);
        true
    } else {
        false
    }
}

/// Adjust list indentation / nesting level at the current line or selection.
pub fn adjust_list_indent(buf: &gtk::TextBuffer, increase: bool) -> bool {
    let (start, end) = buf.selection_bounds().unwrap_or_else(|| {
        let insert = buf.iter_at_mark(&buf.get_insert());
        (insert, insert)
    });
    let mut line_iter = buf
        .iter_at_line(start.line())
        .expect("iter at start line for list indent");
    let end_line = end.line();
    let mut modified = false;

    buf.begin_user_action();
    while line_iter.line() <= end_line {
        let mut line_end = line_iter;
        if !line_end.ends_line() {
            line_end.forward_to_line_end();
        }
        let text = buf.text(&line_iter, &line_end, false).to_string();
        let trimmed = text.trim_start_matches(' ');
        if trimmed.starts_with("- ") || trimmed.find(". ").is_some() {
            if increase {
                let mut ins = line_iter;
                buf.insert(&mut ins, "    ");
                modified = true;
            } else if text.starts_with("    ") {
                let mut del_end = line_iter;
                del_end.forward_chars(4);
                buf.delete(&mut line_iter, &mut del_end);
                modified = true;
            }
        }
        if !line_iter.forward_line() {
            break;
        }
    }
    buf.end_user_action();
    if modified {
        buf.set_modified(true);
    }
    modified
}

#[cfg(test)]
mod tests {
    use super::*;
    use letters_core::model::StylePatch;

    // ── line-spacing tag mapping (pure, no GTK) ──────────────────────

    #[test]
    fn line_spacing_tag_name_maps_known_values() {
        // Default single spacing renders tag-less.
        assert_eq!(line_spacing_tag_name(1.0), None);
        assert_eq!(line_spacing_tag_name(1.15), Some("line-spacing-1.15"));
        assert_eq!(line_spacing_tag_name(1.5), Some("line-spacing-1.5"));
        assert_eq!(line_spacing_tag_name(2.0), Some("line-spacing-2.0"));
    }

    #[test]
    fn line_spacing_tag_name_tolerates_fp_drift() {
        // Callers pass f32 values from GSettings/Document; small
        // representation error must still hit the intended tag.
        let near_115 = 1.15f32 - f32::EPSILON * 4.0;
        let near_15 = 1.5f32 + f32::EPSILON * 4.0;
        let near_20 = 2.0f32 - f32::EPSILON * 4.0;
        assert_eq!(line_spacing_tag_name(near_115), Some("line-spacing-1.15"));
        assert_eq!(line_spacing_tag_name(near_15), Some("line-spacing-1.5"));
        assert_eq!(line_spacing_tag_name(near_20), Some("line-spacing-2.0"));
    }

    #[test]
    fn line_spacing_tag_name_unknown_returns_none() {
        assert_eq!(line_spacing_tag_name(1.3), None);
        assert_eq!(line_spacing_tag_name(3.0), None);
        assert_eq!(line_spacing_tag_name(-1.0), None);
    }

    #[test]
    fn line_spacing_from_tag_name_maps_known_values() {
        for &tag in &[
            "line-spacing-1.0",
            "line-spacing-1.15",
            "line-spacing-1.5",
            "line-spacing-2.0",
        ] {
            let value = line_spacing_from_tag_name(tag);
            assert!(value.is_some(), "{tag} should map to a spacing");
            // tag → value → tag must be stable; the default 1.0 spacing is
            // the one documented case that renders tag-less.
            let back = line_spacing_tag_name(value.unwrap());
            let expected = if tag == "line-spacing-1.0" {
                None
            } else {
                Some(tag)
            };
            assert_eq!(back, expected, "round trip for {tag}");
        }
    }

    #[test]
    fn line_spacing_from_tag_name_unknown_returns_none() {
        assert_eq!(line_spacing_from_tag_name("line-spacing-1.3"), None);
        assert_eq!(line_spacing_from_tag_name("line-spacing-2.5"), None);
        assert_eq!(line_spacing_from_tag_name(""), None);
        assert_eq!(line_spacing_from_tag_name("bold"), None);
        assert_eq!(line_spacing_from_tag_name("line-spacing"), None);
    }

    // ── list markers (pure, no GTK) ───────────────────────────────────

    fn captured(text: &str) -> Paragraph {
        let mut para = Paragraph { style: Default::default(), runs: vec![Run::plain(text)] };
        capture_list_marker(&mut para);
        para
    }

    #[test]
    fn capture_list_marker_reads_nesting_indent() {
        use letters_core::ListKind;

        let top = captured("- top");
        assert_eq!(top.style.list, ListKind::Bullet);
        assert_eq!(top.style.list_level, 0);
        assert_eq!(top.text(), "top");

        // The indent render_to_buffer emits for a nested item must come back
        // as a level, not as literal spaces in the paragraph text.
        let nested = captured("        - deep");
        assert_eq!(nested.style.list, ListKind::Bullet);
        assert_eq!(nested.style.list_level, 2);
        assert_eq!(nested.text(), "deep");

        let numbered = captured("    3. item");
        assert_eq!(numbered.style.list, ListKind::Numbered);
        assert_eq!(numbered.style.list_level, 1);
        assert_eq!(numbered.text(), "item");

        // Prose that merely contains ". " stays a plain paragraph.
        let prose = captured("Hello. World");
        assert_eq!(prose.style.list, ListKind::None);
        assert_eq!(prose.text(), "Hello. World");
    }

    /// Run a GTK-dependent closure on GTK's single main thread. GTK objects may
    /// only be created from the thread that called `gtk::init`, and `gtk::init`
    /// succeeds at most once per process, so all GTK tests share one exclusive
    /// worker thread. When GTK cannot initialize (headless CI without a
    /// display) this skips (logs and returns without running the closure),
    /// rather than panicking like `#[gtk::test]` does.
    fn gtk_test<F>(f: F)
    where
        F: FnOnce() + Send + std::panic::UnwindSafe + 'static,
    {
        use std::panic;
        use std::sync::mpsc;
        use std::sync::OnceLock;

        static MAIN: OnceLock<Option<gtk::glib::ThreadPool>> = OnceLock::new();
        let pool = MAIN
            .get_or_init(|| {
                let pool = gtk::glib::ThreadPool::exclusive(1).ok()?;
                let (tx, rx) = mpsc::channel();
                pool.push(move || {
                    let _ = tx.send(gtk::init().is_ok());
                })
                .ok()?;
                match rx.recv().ok()? {
                    true => Some(pool),
                    false => None,
                }
            })
            .as_ref();
        let Some(pool) = pool else {
            eprintln!("skipping GTK test: no display");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(1);
        let _ = pool.push(move || {
            let _ = tx.send(panic::catch_unwind(f));
        });
        let _ = rx.recv();
    }

    fn round_trip(buf: &gtk::TextBuffer, doc: &Document) -> Document {
        render_to_buffer(doc, buf);
        capture_from_buffer(buf)
    }

    #[test]
    fn document_round_trips_through_buffer() {
        gtk_test(|| {
        let fresh = || {
            let buf = gtk::TextBuffer::new(None);
            crate::window::register_formatting_tags(&buf);
            buf
        };

        // styled runs
        let buf = fresh();
        let mut d = Document::from_plain_text("plain bold italic
second line");
        d.apply_run_style(6, 10, &StylePatch::set_bold(true));
        d.apply_run_style(11, 17, &StylePatch::set_italic(true));
        let rt = round_trip(&buf, &d);
        assert_eq!(rt.to_plain_text(), d.to_plain_text());
        assert!(rt.style_at(6).bold && !rt.style_at(5).bold, "bold boundaries");
        assert!(rt.style_at(11).italic, "italic");

        // headings
        let buf = fresh();
        let mut d = Document::from_plain_text("Title
body text");
        d.set_heading(0, Some(1));
        let rt = round_trip(&buf, &d);
        assert_eq!(rt.paragraphs[0].style.heading, Some(1));
        assert_eq!(rt.paragraphs[1].style.heading, None);

        // highlight + inline code
        let buf = fresh();
        let mut d = Document::from_plain_text("glow mono");
        d.apply_run_style(0, 4, &StylePatch::set_highlight(true));
        d.apply_run_style(5, 9, &StylePatch::set_code(true));
        let rt = round_trip(&buf, &d);
        assert!(rt.style_at(0).highlight && !rt.style_at(5).highlight, "highlight");
        assert!(rt.style_at(5).code && !rt.style_at(0).code, "code");

        // alignment
        let buf = fresh();
        let mut d = Document::from_plain_text("centered
righted
plain");
        d.paragraphs[0].style.alignment = letters_core::Alignment::Center;
        d.paragraphs[1].style.alignment = letters_core::Alignment::Right;
        let rt = round_trip(&buf, &d);
        assert_eq!(rt.paragraphs[0].style.alignment, letters_core::Alignment::Center);
        assert_eq!(rt.paragraphs[1].style.alignment, letters_core::Alignment::Right);
        assert_eq!(rt.paragraphs[2].style.alignment, letters_core::Alignment::Left);

        // line spacing
        let buf = fresh();
        let mut d = Document::from_plain_text("wide
double
single");
        d.paragraphs[0].style.line_spacing = 1.15;
        d.paragraphs[1].style.line_spacing = 2.0;
        let rt = round_trip(&buf, &d);
        assert_eq!(rt.paragraphs[0].style.line_spacing, 1.15);
        assert_eq!(rt.paragraphs[1].style.line_spacing, 2.0);
        assert_eq!(rt.paragraphs[2].style.line_spacing, 1.0);

        // image (renders as paintable, captures back with src + alt)
        let buf = fresh();
        let mtex = gtk::gdk::MemoryTexture::new(
            1, 1, gtk::gdk::MemoryFormat::R8g8b8a8,
            &gtk4::glib::Bytes::from_static(&[255, 0, 0, 255]), 4,
        );
        let png = gtk::prelude::TextureExt::save_to_png_bytes(&mtex);
        let dir = std::env::temp_dir().join("letters-bridge-test");
        let _ = std::fs::create_dir_all(&dir);
        let img = dir.join("dot.png");
        std::fs::write(&img, &png).unwrap();
        let mut d = Document::from_plain_text("see: ");
        d.paragraphs[0].runs.push(Run {
            text: "a dot".into(),
            style: RunStyle { image: Some(img.to_string_lossy().into_owned()), ..Default::default() },
        });
        let rt = round_trip(&buf, &d);
        let ir = rt.paragraphs[0].runs.iter().find(|r| r.style.image.is_some())
            .expect("image run lost through buffer");
        assert_eq!(ir.text, "a dot", "alt text lost");
        assert!(ir.style.image.as_deref().unwrap().ends_with("dot.png"));

        // lists: model kinds render as visible markers and capture back
        let buf = fresh();
        let mut d = Document::from_plain_text("first\nsecond\nplain");
        d.paragraphs[0].style.list = letters_core::ListKind::Bullet;
        d.paragraphs[1].style.list = letters_core::ListKind::Numbered;
        render_to_buffer(&d, &buf);
        let shown = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
        assert_eq!(shown, "- first\n1. second\nplain", "markers not rendered: {shown:?}");
        let rt = capture_from_buffer(&buf);
        assert_eq!(rt.paragraphs[0].style.list, letters_core::ListKind::Bullet);
        assert_eq!(rt.paragraphs[0].text(), "first");
        assert_eq!(rt.paragraphs[1].style.list, letters_core::ListKind::Numbered);
        assert_eq!(rt.paragraphs[1].text(), "second");
        assert_eq!(rt.paragraphs[2].style.list, letters_core::ListKind::None);

        // links
        let buf = fresh();
        let mut d = Document::from_plain_text("go to GNOME now");
        d.apply_run_style(6, 11, &StylePatch::set_link(Some("https://gnome.org".into())));
        let rt = round_trip(&buf, &d);
        assert_eq!(rt.style_at(6).link.as_deref(), Some("https://gnome.org"));
        assert_eq!(rt.style_at(0).link, None);
        assert_eq!(rt.style_at(12).link, None);
        });
    }
}
