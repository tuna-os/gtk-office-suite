// bridge.rs — GtkTextBuffer ⇄ letters_core::Document.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The buffer is a *view*: letters-core owns document semantics and all file
// I/O. This module is the only place buffer tags are translated to/from
// model styles. Tag names map 1:1 to RunStyle fields / heading levels
// (see register_formatting_tags in window.rs).
//
// Links use dynamic "link:<url>" tags; alignment uses the align-* tags;
// list kinds translate to/from the editor's "•" / "N." markers (letters_core::lists).

use gtk4::{self as gtk, prelude::*};
use letters_core::model::{Document, PageGeometry, Paragraph, Run, RunStyle};

const RUN_TAGS: [&str; 8] = ["bold", "italic", "underline", "strikethrough", "highlight", "code", "superscript", "subscript"];

/// The formatting tags a run with `style` is drawn with. The one mapping
/// from model to buffer: render and paste both use it, so a style can't be
/// drawn by one and dropped by the other (super/subscript were in the model
/// and in every reader and writer, and drawn by neither).
pub(crate) fn run_tag_names(style: &RunStyle) -> Vec<&'static str> {
    let mut names = Vec::new();
    if style.bold { names.push("bold"); }
    if style.italic { names.push("italic"); }
    if style.underline { names.push("underline"); }
    if style.strikethrough { names.push("strikethrough"); }
    if style.highlight { names.push("highlight"); }
    if style.code { names.push("code"); }
    match style.vert_align {
        Some(letters_core::model::VertAlign::Superscript) => names.push("superscript"),
        Some(letters_core::model::VertAlign::Subscript) => names.push("subscript"),
        None => {}
    }
    names
}

/// Every tag a run with `style` is drawn with: the fixed formatting tags,
/// plus the per-value ones for a link, a font family, a size and a colour,
/// created on first use. Render and paste both use it.
///
/// Family, size and colour used to have no tags at all, so every one of
/// them was dropped on the way into the editor and was gone on the next
/// save: a docx with red 18pt headings came back black and default-sized.
pub(crate) fn run_tags(buf: &gtk::TextBuffer, style: &RunStyle) -> Vec<String> {
    let mut names: Vec<String> = run_tag_names(style).into_iter().map(str::to_string).collect();
    let table = buf.tag_table();
    let mut dynamic = |name: String, build: &dyn Fn(gtk::TextTag)| {
        if table.lookup(&name).is_none() {
            let tag = gtk::TextTag::builder().name(&name).build();
            build(tag.clone());
            table.add(&tag);
        }
        names.push(name);
    };
    if let Some(url) = &style.link {
        dynamic(format!("{LINK_TAG_PREFIX}{url}"), &|t| {
            t.set_foreground(Some("#1a5fb4"));
            t.set_underline(gtk4::pango::Underline::Single);
        });
    }
    if let Some(family) = &style.font_family {
        let family = family.clone();
        dynamic(format!("{FONT_TAG_PREFIX}{family}"), &move |t| t.set_family(Some(&family)));
    }
    if let Some(hp) = style.font_size_hp {
        dynamic(format!("{SIZE_TAG_PREFIX}{hp}"), &move |t| t.set_size_points(f64::from(hp) / 2.0));
    }
    if let Some(color) = &style.color {
        let hex = format!("#{}", color.trim_start_matches('#'));
        dynamic(format!("{COLOR_TAG_PREFIX}{}", color.trim_start_matches('#')), &move |t| t.set_foreground(Some(&hex)));
    }
    names
}

const LINK_TAG_PREFIX: &str = "link:";
const FONT_TAG_PREFIX: &str = "font:";
const SIZE_TAG_PREFIX: &str = "size-hp:";
const COLOR_TAG_PREFIX: &str = "color:";

/// Read a per-value tag back into `style`. Inverse of `run_tags`.
fn apply_dynamic_tag(name: &str, style: &mut RunStyle) {
    if let Some(url) = name.strip_prefix(LINK_TAG_PREFIX) {
        style.link = Some(url.to_string());
    } else if let Some(family) = name.strip_prefix(FONT_TAG_PREFIX) {
        style.font_family = Some(family.to_string());
    } else if let Some(hp) = name.strip_prefix(SIZE_TAG_PREFIX).and_then(|v| v.parse().ok()) {
        style.font_size_hp = Some(hp);
    } else if let Some(color) = name.strip_prefix(COLOR_TAG_PREFIX) {
        style.color = Some(color.to_string());
    }
}

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
    capture_with_starts(buf).0
}

/// `capture_from_buffer`, plus where each captured paragraph's text starts
/// in the buffer (a char offset, after any list marker or table pipe).
/// With `buffer_offset`/`paragraph_offset` it maps a place in the document
/// to a buffer offset and back: how the page view edits the buffer.
pub fn capture_with_starts(buf: &gtk::TextBuffer) -> (Document, Vec<usize>) {
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
                    "superscript" => s.vert_align = Some(letters_core::model::VertAlign::Superscript),
                    "subscript" => s.vert_align = Some(letters_core::model::VertAlign::Subscript),
                    _ => unreachable!(),
                }
            }
        }
        // Links, fonts, sizes and colours use one tag per value.
        for tag in iter.tags() {
            if let Some(name) = tag.name() {
                apply_dynamic_tag(&name, &mut s);
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

    let page_break_tag = table.lookup(PAGE_BREAK_TAG);

    let line_spacing_tags: Vec<(f32, gtk::TextTag)> =
        ["line-spacing-1.15", "line-spacing-1.5", "line-spacing-2.0", "line-spacing-1.0"]
            .into_iter()
            .filter_map(|n| table.lookup(n).map(|t| (line_spacing_from_tag_name(n).unwrap(), t)))
            .collect();

    let mut paragraphs: Vec<Paragraph> = Vec::new();
    let mut current = Paragraph::default();
    let mut current_run: Option<Run> = None;
    let mut at_line_start = true;
    let mut line_list_level: Option<u8> = None;
    let mut starts: Vec<usize> = Vec::new();
    let mut line_start = 0usize;

    let mut iter = buf.start_iter();
    while !iter.is_end() {
        if at_line_start {
            line_start = iter.offset().max(0) as usize;
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
            // A page break is paragraph state with no text of its own. It
            // rides on a tag for the same reason headings and alignment
            // do: anything written into the text would be indistinguishable
            // from a user typing the same characters, and capture would
            // have to guess. Without this the break was lost on the next
            // capture, so it never reached a saved DOCX or ODT.
            if let Some(tag) = &page_break_tag {
                if iter.has_tag(tag) {
                    current.style.page_break_before = true;
                }
            }
            line_list_level = iter.tags().iter().find_map(|t| t.name().and_then(|n| list_level_from_tag_name(&n)));
            for tag in iter.tags() {
                if let Some(name) = tag.name() {
                    apply_para_tag_name(&name, &mut current.style);
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
            let extent: Option<(u64, u64)> = unsafe {
                paintable.data::<Option<(u64, u64)>>("letters-image-extent").and_then(|p| *p.as_ref())
            };
            if let Some(src) = src {
                if let Some(r) = current_run.take() {
                    current.runs.push(r);
                }
                current.runs.push(Run {
                    text: alt,
                    style: RunStyle { image: Some(src), image_extent_emu: extent, ..Default::default() },
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
            let marker = capture_list_marker(&mut current, line_list_level);
            starts.push(line_start + marker);
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
    if at_line_start {
        line_start = buf.char_count().max(0) as usize;
    }
    let marker = capture_list_marker(&mut current, line_list_level);
    starts.push(line_start + marker);
    paragraphs.push(current);

    // Footnotes, header, footer and page geometry are document state that has
    // no representation in the text buffer, so they ride on the buffer itself
    // (set by render/insert) rather than being reconstructed from the text.
    // Returning `None` for them here — as this function used to — discarded
    // them on every save, because `save_buffer_to_path` writes exactly the
    // Document this returns and both the ODT and DOCX writers emit all three
    // (#438).
    let footnotes: Vec<String> = unsafe {
        buf.data::<Vec<String>>(FOOTNOTES_KEY)
            .map(|p| p.as_ref().clone())
            .unwrap_or_default()
    };
    let header = header_sidecar(buf);
    let footer = footer_sidecar(buf);
    let page = page_sidecar(buf);
    capture_tables(&mut paragraphs, &mut starts);
    (Document { paragraphs, footnotes, header, footer, page, base_font: base_font_sidecar(buf) }, starts)
}

/// Chars a run takes in the layout text (an image or footnote reference is
/// one object char) and in the buffer (a footnote reference is its visible
/// "[n]" marker).
fn run_lengths(run: &Run) -> (usize, usize) {
    if run.style.image.is_some() {
        (1, 1)
    } else if let Some(idx) = run.style.footnote {
        (1, format!("[{}]", idx + 1).chars().count())
    } else {
        let n = run.text.chars().count();
        (n, n)
    }
}

/// The document between buffer offsets `start` and `end`, as a clipboard
/// fragment.
///
/// Buffer offsets are not document offsets: a list item's marker, a
/// table's pipes and a footnote's "[n]" are in the buffer only. Copying
/// used to pass buffer offsets as document offsets, so a selection after a
/// list or a table came out shifted by those characters.
pub fn selection_fragment(buf: &gtk::TextBuffer, start: usize, end: usize) -> letters_core::fragment::Fragment {
    let (doc, starts) = capture_with_starts(buf);
    let seq = |off: usize| {
        let (para, offset) = paragraph_offset(&doc, &starts, off);
        letters_core::edit::paragraph_start(&doc, para) + offset
    };
    letters_core::fragment::from_sequence(&doc, seq(start), seq(end))
}

/// Buffer offset of char `offset` of paragraph `para`'s layout text, the
/// paragraph's text starting at buffer offset `start`.
pub fn buffer_offset(para: &Paragraph, start: usize, offset: usize) -> usize {
    let (mut layout, mut buffer) = (0usize, 0usize);
    for run in &para.runs {
        let (l, b) = run_lengths(run);
        if offset < layout + l {
            // Inside this run; text maps char for char.
            return start + buffer + (offset - layout).min(b);
        }
        layout += l;
        buffer += b;
    }
    start + buffer
}

/// The paragraph and layout offset of buffer offset `off`, given the
/// captured paragraphs and their buffer starts. An offset inside a list
/// marker or a table's pipes belongs to the start of the paragraph after it.
pub fn paragraph_offset(doc: &Document, starts: &[usize], off: usize) -> (usize, usize) {
    let i = starts.partition_point(|&s| s <= off).saturating_sub(1);
    let Some(para) = doc.paragraphs.get(i) else { return (0, 0) };
    let start = starts[i];
    if off < start {
        return (i, 0);
    }
    let buffer_len: usize = para.runs.iter().map(|r| run_lengths(r).1).sum();
    if off > start + buffer_len && i + 1 < doc.paragraphs.len() {
        return (i + 1, 0);
    }
    let (mut layout, mut buffer) = (0usize, 0usize);
    for run in &para.runs {
        let (l, b) = run_lengths(run);
        if off - start < buffer + b {
            return (i, layout + (off - start - buffer).min(l));
        }
        layout += l;
        buffer += b;
    }
    (i, layout)
}

/// Fold rendered pipe grids back into table-cell paragraphs.
///
/// A separate pass rather than part of the character walk above: a table
/// is only recognizable once a whole line is known (a row line means
/// nothing until the *next* line proves to be a delimiter), and the walk
/// deliberately knows nothing beyond the character in hand.
///
/// Without this, `render_to_buffer` → `capture_from_buffer` turned every
/// table into literal "| a | b |" prose — which is how a table inserted
/// into the editor used to reach DOCX as text and vanish as a table.
fn capture_tables(paragraphs: &mut Vec<Paragraph>, starts: &mut Vec<usize>) {
    use letters_core::table_text;

    let mut out: Vec<Paragraph> = Vec::with_capacity(paragraphs.len());
    let mut out_starts: Vec<usize> = Vec::with_capacity(starts.len());
    let mut table_id = 0u32;
    let mut i = 0;
    while i < paragraphs.len() {
        let header_cells = table_text::parse_row(&paragraphs[i].text());
        let delimiter_ok = paragraphs
            .get(i + 1)
            .is_some_and(|p| table_text::is_delimiter_line(&p.text()));
        let Some(header_cells) = header_cells.filter(|_| delimiter_ok) else {
            out.push(std::mem::take(&mut paragraphs[i]));
            out_starts.push(starts[i]);
            i += 1;
            continue;
        };
        // A table's column count is its header's. A body row with a
        // different count is not part of this table — stopping there keeps
        // the grid rectangular, which every reader of (row, col) assumes.
        let cols = header_cells.len();
        table_id += 1;
        let mut row = 0u32;
        let push_row = |out: &mut Vec<Paragraph>, out_starts: &mut Vec<usize>, para: &Paragraph, start: usize, ranges: &[std::ops::Range<usize>], row: u32| {
            for (col, range) in ranges.iter().enumerate() {
                out_starts.push(start + range.start);
                out.push(Paragraph {
                    style: letters_core::ParaStyle {
                        table_cell: Some(letters_core::TableCell { table: table_id, row, col: col as u32 }),
                        ..Default::default()
                    },
                    runs: table_text::slice_runs(&para.runs, range),
                });
            }
        };
        push_row(&mut out, &mut out_starts, &paragraphs[i], starts[i], &header_cells, row);
        i += 2; // header + delimiter
        while i < paragraphs.len() {
            let Some(ranges) = table_text::parse_row(&paragraphs[i].text()) else { break };
            if ranges.len() != cols || table_text::is_delimiter_line(&paragraphs[i].text()) {
                break;
            }
            row += 1;
            push_row(&mut out, &mut out_starts, &paragraphs[i], starts[i], &ranges, row);
            i += 1;
        }
    }
    *paragraphs = out;
    *starts = out_starts;
}

/// GtkTextTag marking a paragraph that starts on a new page. Registered
/// by `register_formatting_tags`, which also gives it the space and rule
/// that make the break visible in the editor.
pub const PAGE_BREAK_TAG: &str = "page-break";

/// Buffer data key holding the document's footnote texts.
pub const FOOTNOTES_KEY: &str = "letters-footnotes";
/// Buffer data key holding the document's header text, if it has one.
pub const HEADER_KEY: &str = "letters-header";
/// Buffer data key holding the document's footer text, if it has one.
pub const FOOTER_KEY: &str = "letters-footer";
/// Buffer data key holding the document's page geometry, if it has one.
pub const PAGE_KEY: &str = "letters-page";
/// Buffer data key holding the document's base (body) font.
pub const BASE_FONT_KEY: &str = "letters-base-font";

// GObject data is an untyped pointer: reading a key back at a type other than
// the one it was written with is undefined behaviour, not a panic, and no test
// will catch it. So each key gets one reader and the type appears exactly
// once per key — rather than a generic helper any future caller could
// instantiate at the wrong type.
//
// The outer `Option` distinguishes "key never set" (a buffer that was never
// rendered from a Document) from a document that genuinely has no header;
// both flatten to `None`, but only the former is worth keeping separate for
// anyone extending this.

fn header_sidecar(buf: &gtk::TextBuffer) -> Option<String> {
    unsafe { buf.data::<Option<String>>(HEADER_KEY).and_then(|p| p.as_ref().clone()) }
}

fn footer_sidecar(buf: &gtk::TextBuffer) -> Option<String> {
    unsafe { buf.data::<Option<String>>(FOOTER_KEY).and_then(|p| p.as_ref().clone()) }
}

fn base_font_sidecar(buf: &gtk::TextBuffer) -> letters_core::model::BaseFont {
    unsafe { buf.data::<letters_core::model::BaseFont>(BASE_FONT_KEY).map(|p| p.as_ref().clone()).unwrap_or_default() }
}

fn page_sidecar(buf: &gtk::TextBuffer) -> Option<PageGeometry> {
    unsafe { buf.data::<Option<PageGeometry>>(PAGE_KEY).and_then(|p| *p.as_ref()) }
}

/// Read the page geometry currently attached to `buf`, if it has one.
pub fn buffer_page_geometry(buf: &gtk::TextBuffer) -> Option<PageGeometry> {
    page_sidecar(buf)
}

/// Read the header/footer currently attached to `buf`.
pub fn buffer_header_footer(buf: &gtk::TextBuffer) -> (Option<String>, Option<String>) {
    (header_sidecar(buf), footer_sidecar(buf))
}

/// Update just the header/footer, leaving the rest of the buffer's document
/// state alone.
///
/// Empty text means "no header", not an empty one: the ODT and DOCX writers
/// both emit a header block for `Some("")`, so treating a cleared entry as
/// `Some("")` would put an empty header into every saved file.
pub fn set_buffer_header_footer(buf: &gtk::TextBuffer, header: &str, footer: &str) {
    let present = |text: &str| (!text.is_empty()).then(|| text.to_string());
    unsafe {
        buf.set_data(HEADER_KEY, present(header));
        buf.set_data(FOOTER_KEY, present(footer));
    }
}

/// Attach the document state that the text buffer cannot represent.
///
/// Called by `render_to_buffer`, and by anything else that replaces a
/// buffer's document wholesale, so a later `capture_from_buffer` can put the
/// Document back together.
pub fn set_buffer_sidecars(doc: &Document, buf: &gtk::TextBuffer) {
    unsafe {
        buf.set_data(FOOTNOTES_KEY, doc.footnotes.clone());
        buf.set_data(HEADER_KEY, doc.header.clone());
        buf.set_data(FOOTER_KEY, doc.footer.clone());
        buf.set_data(PAGE_KEY, doc.page);
        buf.set_data(BASE_FONT_KEY, doc.base_font.clone());
    }
}

const PARA_TAG_PREFIX: &str = "para:";

/// The paragraph properties the fixed tags do not carry, as one tag name:
/// spacing, indents, tab stops, a line spacing no `line-spacing-*` tag
/// names, block quote, named style and a list restart. `None` when the
/// paragraph has none of them.
///
/// Without it all of these were dropped on the way into the editor, so a
/// save wrote the document back without its paragraph spacing and indents,
/// and Print Layout (which reads the document back from the editor) drew
/// every paragraph tight.
fn para_tag_name(style: &letters_core::ParaStyle) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut num = |key: &str, v: f64| {
        if v != 0.0 {
            parts.push(format!("{key}={v}"));
        }
    };
    num("b", style.space_before_pt);
    num("a", style.space_after_pt);
    num("l", style.left_indent_pt);
    num("r", style.right_indent_pt);
    num("f", style.first_line_indent_pt);
    if !style.tab_stops_pt.is_empty() {
        let tabs: Vec<String> = style.tab_stops_pt.iter().map(f64::to_string).collect();
        parts.push(format!("t={}", tabs.join(",")));
    }
    if line_spacing_tag_name(style.line_spacing).is_none() && (style.line_spacing - 1.0).abs() > 0.001 {
        parts.push(format!("ls={}", style.line_spacing));
    }
    if style.block_quote {
        parts.push("q".into());
    }
    if let Some(start) = style.list_start {
        parts.push(format!("n={start}"));
    }
    if let Some(name) = &style.named_style {
        // Last, and the only free text: everything after "s=" is the name.
        parts.push(format!("s={name}"));
    }
    (!parts.is_empty()).then(|| format!("{PARA_TAG_PREFIX}{}", parts.join(";")))
}

/// Read a `para:` tag back into `style`. Inverse of `para_tag_name`.
fn apply_para_tag_name(name: &str, style: &mut letters_core::ParaStyle) {
    let Some(mut rest) = name.strip_prefix(PARA_TAG_PREFIX) else { return };
    while !rest.is_empty() {
        if let Some(named) = rest.strip_prefix("s=") {
            style.named_style = Some(named.to_string());
            return;
        }
        let (part, tail) = rest.split_once(';').unwrap_or((rest, ""));
        rest = tail;
        let (key, value) = part.split_once('=').unwrap_or((part, ""));
        let f = || value.parse::<f64>().ok();
        match key {
            "b" => style.space_before_pt = f().unwrap_or(0.0),
            "a" => style.space_after_pt = f().unwrap_or(0.0),
            "l" => style.left_indent_pt = f().unwrap_or(0.0),
            "r" => style.right_indent_pt = f().unwrap_or(0.0),
            "f" => style.first_line_indent_pt = f().unwrap_or(0.0),
            "t" => style.tab_stops_pt = value.split(',').filter_map(|t| t.parse().ok()).collect(),
            "ls" => style.line_spacing = value.parse().unwrap_or(1.0),
            "q" => style.block_quote = true,
            "n" => style.list_start = value.parse().ok(),
            _ => {}
        }
    }
}

/// Create (on first use) the `para:` tag for `style` and return its name.
/// It also draws what it can in the Draft editor: spacing above and below,
/// and — outside lists, whose own tag owns the margin — the indents.
fn para_tag(buf: &gtk::TextBuffer, style: &letters_core::ParaStyle) -> Option<String> {
    let name = para_tag_name(style)?;
    if buf.tag_table().lookup(&name).is_none() {
        let px = |pt: f64| (pt * 96.0 / 72.0).round() as i32;
        let tag = gtk::TextTag::builder().name(&name).build();
        if style.space_before_pt > 0.0 {
            tag.set_pixels_above_lines(px(style.space_before_pt));
        }
        if style.space_after_pt > 0.0 {
            tag.set_pixels_below_lines(px(style.space_after_pt));
        }
        if style.list == letters_core::ListKind::None {
            if style.left_indent_pt != 0.0 {
                tag.set_left_margin(EDITOR_LEFT_MARGIN_PX + px(style.left_indent_pt));
            }
            if style.first_line_indent_pt != 0.0 {
                tag.set_indent(px(style.first_line_indent_pt));
            }
        }
        if style.right_indent_pt > 0.0 {
            tag.set_right_margin(EDITOR_LEFT_MARGIN_PX + px(style.right_indent_pt));
        }
        buf.tag_table().add(&tag);
    }
    Some(name)
}

/// GtkTextTag name for the list geometry of nesting level `level`.
fn list_level_tag_name(level: u8) -> String {
    format!("list-level-{}", level.min(letters_core::lists::MAX_LEVEL))
}

/// Inverse of [`list_level_tag_name`].
fn list_level_from_tag_name(name: &str) -> Option<u8> {
    name.strip_prefix("list-level-")?.parse().ok()
}

/// The editor's left margin in pixels (`TextView::set_left_margin`). A tag
/// with its own left margin *replaces* the view's, so list indents are
/// measured from this.
pub const EDITOR_LEFT_MARGIN_PX: i32 = 24;

/// Tag giving a list item at `level` its hanging indent: the marker sits at
/// the level's indent, a tab carries the text to the text indent, and
/// wrapped lines line up with the text rather than under the marker.
/// Created on first use, like the `link:` tags. Returns its name.
pub(crate) fn list_level_tag(buf: &gtk::TextBuffer, level: u8) -> String {
    use letters_core::lists;
    let name = list_level_tag_name(level);
    if buf.tag_table().lookup(&name).is_none() {
        // Points to the editor's pixels (GTK's 96 dpi), as fonts are drawn.
        let px = |pt: f64| (pt * 96.0 / 72.0).round() as i32;
        let hang = px(lists::HANGING_PT);
        let marker_x = px(lists::text_indent_pt(level) - lists::HANGING_PT);
        let mut tabs = gtk4::pango::TabArray::new(1, true);
        tabs.set_tab(0, gtk4::pango::TabAlign::Left, hang);
        let tag = gtk::TextTag::builder()
            .name(&name)
            .left_margin(EDITOR_LEFT_MARGIN_PX + marker_x)
            .indent(-hang)
            .tabs(&tabs)
            .build();
        buf.tag_table().add(&tag);
    }
    name
}

/// The editor shows a list item as its marker ("•", "3.") and a tab,
/// followed by the item's text; the model wants `ListKind` and the text
/// alone. Strip the marker and set the kind when capturing.
///
/// `tag_level` is the level carried by the paragraph's `list-level-N` tag.
/// A line without one (typed by hand, or pasted) still counts as a list
/// item when it starts with a marker; then four leading spaces make one
/// nesting level. Markdown's "- " and "N. " are accepted as typed markers.
/// A list marker at the start of an editor line: its kind, the number of
/// leading spaces, the marker's own length in chars (marker and separator),
/// and a numbered item's number.
fn list_marker_prefix(text: &str) -> Option<(letters_core::ListKind, usize, usize, u32)> {
    let indent = text.len() - text.trim_start_matches(' ').len();
    let body = &text[indent..];
    // A marker is followed by a tab (as rendered) or a space (as typed).
    let separated = |rest: &str| rest.starts_with(['\t', ' ']);
    if let Some(rest) = body.strip_prefix(letters_core::lists::BULLET) {
        return separated(rest).then_some((letters_core::ListKind::Bullet, indent, 2, 0));
    }
    if body.starts_with("- ") {
        return Some((letters_core::ListKind::Bullet, indent, 2, 0));
    }
    let digits = body.chars().take_while(|c| c.is_ascii_digit()).count();
    match body[digits..].strip_prefix('.') {
        Some(rest) if digits > 0 && separated(rest) => {
            Some((letters_core::ListKind::Numbered, indent, digits + 2, body[..digits].parse().unwrap_or(0)))
        }
        _ => None,
    }
}

/// Enter at the caret inside a list item: continue the list with the next
/// marker (at the same level), or, on an empty item, end the list by
/// removing that item's marker. `false` when the caret is not in a list
/// item, so Enter does what it always does. Shared by both views.
pub(crate) fn enter_in_list(buf: &gtk::TextBuffer) -> bool {
    let cursor = buf.iter_at_mark(&buf.get_insert());
    let mut line_start = cursor;
    line_start.set_line_offset(0);
    let mut line_end = cursor;
    if !line_end.ends_line() {
        line_end.forward_to_line_end();
    }
    let line = buf.text(&line_start, &line_end, false).to_string();
    let Some((kind, indent, marker, number)) = list_marker_prefix(&line) else { return false };
    // The caret inside the marker itself is not "in the item".
    if (cursor.line_offset() as usize) < indent + marker {
        return false;
    }
    let level_tag = line_start
        .tags()
        .into_iter()
        .find(|t| t.name().is_some_and(|n| list_level_from_tag_name(&n).is_some()));
    let body_empty = line.chars().skip(indent + marker).all(char::is_whitespace);
    buf.begin_user_action();
    if body_empty {
        let mut s = line_start;
        let mut e = line_start;
        e.forward_chars((indent + marker) as i32);
        buf.delete(&mut s, &mut e);
        if let Some(tag) = &level_tag {
            let mut end = buf.iter_at_mark(&buf.get_insert());
            if !end.ends_line() {
                end.forward_to_line_end();
            }
            let mut start = end;
            start.set_line_offset(0);
            buf.remove_tag(tag, &start, &end);
        }
    } else {
        buf.delete_selection(true, true);
        let next = match kind {
            letters_core::ListKind::Numbered => format!("{}.\t", number + 1),
            _ => format!("{}\t", letters_core::lists::BULLET),
        };
        // An untagged (typed) item keeps its indent as spaces.
        let prefix = if level_tag.is_some() { next } else { format!("{}{next}", " ".repeat(indent)) };
        let mut at = buf.iter_at_mark(&buf.get_insert());
        buf.insert(&mut at, &format!("\n{prefix}"));
        if let Some(tag) = &level_tag {
            let mut start = buf.iter_at_mark(&buf.get_insert());
            start.set_line_offset(0);
            let mut end = start;
            if !end.ends_line() {
                end.forward_to_line_end();
            }
            buf.apply_tag(tag, &start, &end);
        }
    }
    buf.end_user_action();
    true
}

fn capture_list_marker(para: &mut Paragraph, tag_level: Option<u8>) -> usize {
    let text = para.text();
    let Some((kind, indent, marker_chars, _)) = list_marker_prefix(&text) else { return 0 };
    para.style.list = kind;
    para.style.list_level = tag_level.unwrap_or((indent / 4) as u8);
    // Remove the indent and marker from the front of the run list. Counts
    // are in chars (the bullet is one char, three bytes), as runs slice.
    let stripped = indent + marker_chars;
    let mut remaining = stripped;
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
    stripped
}

/// Flatten a document's paragraphs into the lines the editor shows.
///
/// Everything is one line per paragraph, except a table: its cells are
/// paragraphs in the model but a *grid* on screen, so each row's cells
/// collapse into one pipe line and a delimiter line follows the header.
/// `capture_tables` reverses exactly this.
fn render_lines(doc: &Document) -> Vec<std::borrow::Cow<'_, Paragraph>> {
    use letters_core::table_text;
    use std::borrow::Cow;

    let mut lines: Vec<Cow<Paragraph>> = Vec::with_capacity(doc.paragraphs.len());
    let mut i = 0;
    while i < doc.paragraphs.len() {
        let Some(cell) = doc.paragraphs[i].style.table_cell else {
            lines.push(Cow::Borrowed(&doc.paragraphs[i]));
            i += 1;
            continue;
        };
        let table = cell.table;
        let end = doc.paragraphs[i..]
            .iter()
            .position(|p| p.style.table_cell.is_none_or(|c| c.table != table))
            .map_or(doc.paragraphs.len(), |n| i + n);
        let cells = &doc.paragraphs[i..end];
        let cols = cells.iter().filter_map(|p| p.style.table_cell).map(|c| c.col).max().unwrap_or(0) + 1;

        // Cells arrive in row-major order (the model keeps them that way);
        // chunking by the column count is what turns them back into rows.
        for (row, chunk) in cells.chunks(cols as usize).enumerate() {
            let row_cells: Vec<Vec<letters_core::Run>> = chunk.iter().map(|p| p.runs.clone()).collect();
            lines.push(Cow::Owned(Paragraph {
                style: letters_core::ParaStyle::default(),
                runs: table_text::layout_row_runs(&row_cells),
            }));
            if row == 0 {
                lines.push(Cow::Owned(Paragraph {
                    style: letters_core::ParaStyle::default(),
                    runs: vec![letters_core::Run::plain(table_text::delimiter_line(cols as usize))],
                }));
            }
        }
        i = end;
    }
    lines
}

/// Replace the buffer's content with a rendered Document.
pub fn render_to_buffer(doc: &Document, buf: &gtk::TextBuffer) {
    set_buffer_sidecars(doc, buf);
    buf.set_text("");
    let mut insert = buf.start_iter();
    let lines = render_lines(doc);
    let ordinals = letters_core::lists::ordinals(lines.iter().map(|p| &p.style));
    for (i, para) in lines.iter().map(|p| p.as_ref()).enumerate() {
        if i > 0 {
            buf.insert(&mut insert, "\n");
        }
        let para_start = insert.offset();
        if let Some(marker) = letters_core::lists::marker(para.style.list, ordinals[i]) {
            buf.insert(&mut insert, &format!("{marker}\t"));
        }
        for run in &para.runs {
            if let Some(src) = &run.style.image {
                match gtk4::gdk::Texture::from_filename(src) {
                    Ok(texture) => {
                        unsafe {
                            texture.set_data("letters-image-src", src.clone());
                            texture.set_data("letters-image-alt", run.text.clone());
                            texture.set_data("letters-image-extent", run.style.image_extent_emu);
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
            let tags = run_tags(buf, &run.style);
            let names: Vec<&str> = tags.iter().map(String::as_str).collect();
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
        if para.style.page_break_before {
            para_tags.push(PAGE_BREAK_TAG.to_string());
        }
        if para.style.list != letters_core::ListKind::None {
            para_tags.push(list_level_tag(buf, para.style.list_level));
        }
        if let Some(name) = para_tag(buf, &para.style) {
            para_tags.push(name);
        }
        for name in para_tags {
            let start = buf.iter_at_offset(para_start);
            buf.apply_tag_by_name(&name, &start, &insert);
        }
    }
    buf.set_modified(false);
}

/// Read any supported file through letters-core into the buffer.
///
/// The dispatch lives beside the writer's in `letters_core::save`, which
/// is permissive here on purpose — an unfamiliar extension still opens —
/// but no longer runs a `.txt` file through the Markdown parser (#436).
pub fn load_file_to_buffer(path: &str, buf: &gtk::TextBuffer) -> Result<(), String> {
    let doc = letters_core::save::read(std::path::Path::new(path))?;
    render_to_buffer(&doc, buf);
    Ok(())
}

/// Save the buffer through letters-core in the format the path implies.
/// Capture the buffer as a document and write it in the format `path`
/// names, reporting what that format could not carry.
///
/// The format decision itself lives in `letters_core::save` — this used to
/// be a two-arm match with a Markdown catch-all, so every extension it did
/// not name received Markdown bytes (#436).
pub fn save_buffer_to_file(
    buf: &gtk::TextBuffer,
    path: &std::path::Path,
) -> Result<suite_common::interop::CompatibilityReport, String> {
    letters_core::save::write(&capture_from_buffer(buf), path)
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

/// Apply a structured editing operation directly to the GtkTextBuffer through
/// letters_core::StructuredEditor, preserving document structure and cursor position.
pub fn apply_structured_edit<F>(buf: &gtk::TextBuffer, edit: F)
where
    F: FnOnce(&mut letters_core::structured::StructuredEditor),
{
    let doc = capture_from_buffer(buf);
    let mut editor = letters_core::structured::StructuredEditor::new(doc);
    // The caret, not the selection start: `unwrap_or(0)` for an unselected
    // buffer — which is what this used to do — told every structured
    // command that the cursor was at the very start of the document, so
    // "Insert Table" put its table before the first character no matter
    // where the user was typing.
    let cursor_offset = buf.iter_at_mark(&buf.get_insert()).offset().max(0) as usize;
    editor.set_cursor(cursor_offset);
    if let Some((start, end)) = buf.selection_bounds() {
        editor.select(start.offset().max(0) as usize, end.offset().max(0) as usize);
    }
    edit(&mut editor);
    render_to_buffer(editor.document(), buf);

    // Re-rendering replaces the buffer's contents, which drops the caret at
    // the start. Put it back where the edit left it so typing continues in
    // the new table's first cell rather than at the top of the document.
    let restored = (editor.cursor() as i32).min(buf.char_count());
    buf.place_cursor(&buf.iter_at_offset(restored));
}

#[cfg(test)]
mod tests {
    use super::*;
    use suite_common::gtk_test::run as gtk_test;
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
        capture_list_marker(&mut para, None);
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

    #[test]
    fn capture_list_marker_reads_rendered_and_typed_glyph_markers() {
        use letters_core::ListKind;

        // As render_to_buffer draws them: glyph, then a tab.
        let bullet = captured("\u{2022}\tApples");
        assert_eq!((bullet.style.list, bullet.text().as_str()), (ListKind::Bullet, "Apples"));
        let numbered = captured("12.\tTwelfth");
        assert_eq!((numbered.style.list, numbered.text().as_str()), (ListKind::Numbered, "Twelfth"));

        // As list continuation types them: glyph, then a space.
        let typed = captured("\u{2022} Oranges");
        assert_eq!((typed.style.list, typed.text().as_str()), (ListKind::Bullet, "Oranges"));

        // A bullet glued to a word is text, not a marker.
        let glued = captured("\u{2022}Pears");
        assert_eq!((glued.style.list, glued.text().as_str()), (ListKind::None, "\u{2022}Pears"));
        let decimal = captured("3.5 apples");
        assert_eq!(decimal.style.list, ListKind::None);

        // The level tag wins over leading spaces.
        let mut para = Paragraph { style: Default::default(), runs: vec![Run::plain("\u{2022}\tdeep")] };
        capture_list_marker(&mut para, Some(2));
        assert_eq!((para.style.list_level, para.text().as_str()), (2, "deep"));
    }

    /// Font family, size and colour had no buffer tags, so opening a
    /// document dropped them from the editor and the next save dropped them
    /// from the file.
    #[test]
    fn font_family_size_and_colour_survive_the_buffer() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            let mut d = Document::from_plain_text("plain big red");
            let styled = RunStyle {
                font_family: Some("Liberation Mono".into()),
                font_size_hp: Some(36),
                color: Some("C80000".into()),
                bold: true,
                ..Default::default()
            };
            d.paragraphs[0].runs = vec![Run::plain("plain "), Run { text: "big red".into(), style: styled.clone() }];
            let rt = round_trip(&buf, &d);
            assert_eq!(rt.paragraphs[0].runs, d.paragraphs[0].runs);
            // And they are drawn: the tag carries the size and colour.
            let tag = buf.tag_table().lookup("size-hp:36").expect("size tag");
            assert_eq!(tag.size_points(), 18.0);
            assert!(buf.tag_table().lookup("color:C80000").is_some());
            assert!(buf.tag_table().lookup("font:Liberation Mono").is_some());
        });
    }

    /// Every character of the captured document maps to the buffer
    /// character it came from, and back — through list markers, table pipes
    /// and footnote markers, which exist in the buffer only.
    #[test]
    fn document_positions_map_to_buffer_offsets_and_back() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            let mut d = doc_with_table(&[&["Name", "Qty"], &["Bolts", "12"]]);
            d.paragraphs.insert(0, Paragraph {
                style: letters_core::ParaStyle { list: letters_core::ListKind::Numbered, ..Default::default() },
                runs: vec![Run::plain("first item")],
            });
            d.paragraphs.insert(1, Paragraph::default());
            d.footnotes = vec!["a note".into()];
            d.paragraphs.push(Paragraph {
                style: Default::default(),
                runs: vec![
                    Run::plain("see"),
                    Run { text: String::new(), style: RunStyle { footnote: Some(0), ..Default::default() } },
                    Run::plain(" here"),
                ],
            });
            render_to_buffer(&d, &buf);
            let (doc, starts) = capture_with_starts(&buf);
            assert_eq!(starts.len(), doc.paragraphs.len());
            let text: Vec<char> = buf.text(&buf.start_iter(), &buf.end_iter(), false).chars().collect();
            for (i, p) in doc.paragraphs.iter().enumerate() {
                let layout: Vec<char> = letters_core::layout::layout_text(&p.runs).chars().collect();
                for (k, ch) in layout.iter().enumerate() {
                    let off = buffer_offset(p, starts[i], k);
                    // A footnote reference's object char is its "[n]" in the buffer.
                    let want = if *ch == letters_core::layout::OBJECT && text.get(off) == Some(&'[') { '[' } else { *ch };
                    assert_eq!(text.get(off), Some(&want), "paragraph {i} char {k} ({:?})", p.text());
                    assert_eq!(paragraph_offset(&doc, &starts, off), (i, k), "back from buffer offset {off}");
                }
                // The end of a paragraph maps to where its text ends.
                let end = buffer_offset(p, starts[i], layout.len());
                assert_eq!(paragraph_offset(&doc, &starts, end), (i, layout.len()));
            }
            // A click in a list marker lands at the start of that item.
            assert_eq!(paragraph_offset(&doc, &starts, 0), (0, 0));
        });
    }

    /// Copying after a list or a table copies what was selected, not text
    /// shifted by the markers and pipes that exist only in the buffer.
    #[test]
    fn a_copied_selection_is_the_selected_text() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            let mut d = doc_with_table(&[&["Name", "Qty"], &["Bolts", "12"]]);
            d.paragraphs.insert(0, Paragraph {
                style: letters_core::ParaStyle { list: letters_core::ListKind::Bullet, ..Default::default() },
                runs: vec![Run::plain("apples")],
            });
            render_to_buffer(&d, &buf);
            let text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
            let find = |needle: &str| {
                let b = text.find(needle).unwrap();
                let s = text[..b].chars().count();
                (s, s + needle.chars().count())
            };
            for want in ["apples", "Bolts", "after the table", "the tab"] {
                let (s, e) = find(want);
                let frag = selection_fragment(&buf, s, e);
                assert_eq!(frag.to_plain().trim_end(), want, "copying {want:?}");
            }
        });
    }

    #[test]
    fn paragraph_properties_round_trip_through_their_tag_name() {
        let style = letters_core::ParaStyle {
            space_before_pt: 12.0,
            space_after_pt: 10.0,
            left_indent_pt: 36.0,
            right_indent_pt: 4.5,
            first_line_indent_pt: -18.0,
            tab_stops_pt: vec![36.0, 72.5],
            line_spacing: 1.08,
            block_quote: true,
            list_start: Some(5),
            named_style: Some("My; odd=style".into()),
            ..Default::default()
        };
        let name = para_tag_name(&style).expect("a tag for a styled paragraph");
        let mut back = letters_core::ParaStyle::default();
        apply_para_tag_name(&name, &mut back);
        assert_eq!(back, style);
        assert_eq!(para_tag_name(&letters_core::ParaStyle::default()), None, "a plain paragraph needs no tag");
        // Line spacings the line-spacing-* tags carry stay with them.
        let tagged = letters_core::ParaStyle { line_spacing: 1.5, ..Default::default() };
        assert_eq!(para_tag_name(&tagged), None);
    }

    /// Spacing and indents used to be dropped by the buffer: lost on save,
    /// and missing from Print Layout.
    #[test]
    fn paragraph_spacing_and_indents_survive_the_buffer() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            let mut d = Document::from_plain_text("spaced\nindented\nplain");
            d.paragraphs[0].style.space_after_pt = 10.0;
            d.paragraphs[0].style.line_spacing = 1.15;
            d.paragraphs[1].style.left_indent_pt = 72.0;
            d.paragraphs[1].style.first_line_indent_pt = 36.0;
            let rt = round_trip(&buf, &d);
            let styles: Vec<&letters_core::ParaStyle> = rt.paragraphs.iter().map(|p| &p.style).collect();
            assert_eq!(styles[0], &d.paragraphs[0].style);
            assert_eq!(styles[1], &d.paragraphs[1].style);
            assert_eq!(styles[2], &letters_core::ParaStyle::default());
        });
    }

    /// Enter in a list item continues the list at the same level with the
    /// next number; Enter on an empty item ends the list.
    #[test]
    fn enter_continues_and_ends_a_list() {
        use letters_core::ListKind;
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            let mut d = Document::from_plain_text("one\ntwo");
            for p in &mut d.paragraphs {
                p.style.list = ListKind::Numbered;
                p.style.list_level = 1;
            }
            render_to_buffer(&d, &buf);
            buf.place_cursor(&buf.end_iter());
            assert!(enter_in_list(&buf));
            buf.insert_at_cursor("three");
            let doc = capture_from_buffer(&buf);
            let items: Vec<(ListKind, u8, String)> =
                doc.paragraphs.iter().map(|p| (p.style.list, p.style.list_level, p.text())).collect();
            assert_eq!(items[2], (ListKind::Numbered, 1, "three".to_string()));
            assert_eq!(buf.text(&buf.start_iter(), &buf.end_iter(), false), "1.\tone\n2.\ttwo\n3.\tthree");

            // Enter twice: the second, on the new empty item, ends the list.
            assert!(enter_in_list(&buf));
            assert!(enter_in_list(&buf));
            let doc = capture_from_buffer(&buf);
            let last = doc.paragraphs.last().unwrap();
            assert_eq!((last.style.list, last.text().as_str()), (ListKind::None, ""));

            // Outside a list, Enter is not ours.
            buf.place_cursor(&buf.end_iter());
            buf.insert_at_cursor("plain");
            assert!(!enter_in_list(&buf));
        });
    }

    #[test]
    fn list_level_tag_names_round_trip() {
        for level in 0..=letters_core::lists::MAX_LEVEL {
            assert_eq!(list_level_from_tag_name(&list_level_tag_name(level)), Some(level));
        }
        assert_eq!(list_level_tag_name(40), list_level_tag_name(letters_core::lists::MAX_LEVEL));
        assert_eq!(list_level_from_tag_name("line-spacing-1.5"), None);
    }

    /// Lists are drawn as lists: a bullet glyph or number, a tab, and a
    /// hanging indent that grows with the level; never Markdown's "- ".
    #[test]
    fn lists_render_as_glyphs_with_a_hanging_indent_per_level() {
        use letters_core::ListKind;
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            let mut d = Document::from_plain_text("one\ntwo\nthree\nfirst\nsecond\nnested\nthird");
            let levels = [0u8, 1, 2, 0, 0, 1, 0];
            let kinds = [ListKind::Bullet, ListKind::Bullet, ListKind::Bullet,
                ListKind::Numbered, ListKind::Numbered, ListKind::Bullet, ListKind::Numbered];
            for (p, (level, kind)) in d.paragraphs.iter_mut().zip(levels.iter().zip(kinds)) {
                p.style.list = kind;
                p.style.list_level = *level;
            }
            render_to_buffer(&d, &buf);
            let shown = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
            assert_eq!(
                shown,
                "\u{2022}\tone\n\u{2022}\ttwo\n\u{2022}\tthree\n1.\tfirst\n2.\tsecond\n\u{2022}\tnested\n3.\tthird",
                "a nested item does not restart the outer numbering"
            );

            let margin = |line: i32| -> (i32, i32) {
                let it = buf.iter_at_line(line).unwrap();
                let tag = it.tags().into_iter()
                    .find(|t| t.name().is_some_and(|n| n.starts_with("list-level-")))
                    .expect("list item carries its level tag");
                (tag.left_margin(), tag.indent())
            };
            let (m0, i0) = margin(0);
            let (m1, i1) = margin(1);
            let (m2, _) = margin(2);
            assert_eq!(m0, EDITOR_LEFT_MARGIN_PX, "level 0 marker sits at the text margin");
            assert!(i0 < 0 && i0 == i1, "hanging indent: wrapped lines align with the text");
            assert_eq!(m1 - m0, m2 - m1, "each level indents one equal step");
            assert!(m1 > m0);

            let rt = capture_from_buffer(&buf);
            let back: Vec<(ListKind, u8, String)> =
                rt.paragraphs.iter().map(|p| (p.style.list, p.style.list_level, p.text())).collect();
            let want: Vec<(ListKind, u8, String)> =
                d.paragraphs.iter().map(|p| (p.style.list, p.style.list_level, p.text())).collect();
            assert_eq!(back, want, "kinds, levels and text survive the buffer");
        });
    }

    fn round_trip(buf: &gtk::TextBuffer, doc: &Document) -> Document {
        render_to_buffer(doc, buf);
        capture_from_buffer(buf)
    }

    /// A document holding one table with the given cell texts, plus a
    /// paragraph of prose after it.
    fn doc_with_table(rows: &[&[&str]]) -> Document {
        let mut doc = Document::from_plain_text("after the table");
        let table = doc.insert_table_at(0, rows.len() as u32, rows[0].len() as u32);
        for (r, row) in rows.iter().enumerate() {
            for (c, text) in row.iter().enumerate() {
                let idx = doc.paragraphs.iter().position(|p| {
                    p.style.table_cell
                        == Some(letters_core::TableCell { table, row: r as u32, col: c as u32 })
                }).expect("cell exists");
                doc.paragraphs[idx].runs = vec![Run::plain(*text)];
            }
        }
        doc
    }

    #[test]
    fn tables_survive_the_buffer_round_trip() {
        // Before tables had a buffer mapping, a document's cells came back
        // as literal "| a | b |" prose — so a table inserted in the editor
        // reached DOCX as text and stopped being a table at all (#438).
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            let doc = doc_with_table(&[&["Name", "Qty"], &["Bolts", "12"]]);
            let rt = round_trip(&buf, &doc);

            assert_eq!(rt.paragraphs.len(), doc.paragraphs.len());
            let table = rt.paragraphs[0].style.table_cell.expect("first paragraph is a cell").table;
            assert_eq!(rt.table_dimensions(table), Some((2, 2)));
            let texts: Vec<String> = rt.paragraphs.iter()
                .filter(|p| p.style.table_cell.is_some()).map(|p| p.text()).collect();
            assert_eq!(texts, vec!["Name", "Qty", "Bolts", "12"]);
            assert_eq!(rt.paragraphs.last().unwrap().text(), "after the table");
            assert!(rt.paragraphs.last().unwrap().style.table_cell.is_none());
        });
    }

    #[test]
    fn the_editor_shows_a_table_as_a_pipe_grid() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            render_to_buffer(&doc_with_table(&[&["Name", "Qty"], &["Bolts", "12"]]), &buf);
            let text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
            assert_eq!(
                text,
                "| Name | Qty |\n| --- | --- |\n| Bolts | 12 |\nafter the table"
            );
        });
    }

    #[test]
    fn cell_styles_survive_the_round_trip() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            let mut doc = doc_with_table(&[&["Name", "Qty"], &["Bolts", "12"]]);
            doc.paragraphs[0].runs = vec![Run {
                text: "Name".into(),
                style: RunStyle { bold: true, ..Default::default() },
            }];
            let rt = round_trip(&buf, &doc);
            assert_eq!(rt.paragraphs[0].text(), "Name");
            assert!(rt.paragraphs[0].runs[0].style.bold, "bold inside a cell must survive");
            assert!(!rt.paragraphs[1].runs.first().is_some_and(|r| r.style.bold),
                    "the separator must not carry the cell's style into the next cell");
        });
    }

    #[test]
    fn unicode_cells_round_trip() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            let doc = doc_with_table(&[&["日本語", "e👍"], &["combining é", "مرحبا"]]);
            let rt = round_trip(&buf, &doc);
            let texts: Vec<String> = rt.paragraphs.iter()
                .filter(|p| p.style.table_cell.is_some()).map(|p| p.text()).collect();
            assert_eq!(texts, vec!["日本語", "e👍", "combining é", "مرحبا"]);
        });
    }

    #[test]
    fn prose_containing_pipes_is_not_captured_as_a_table() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            // No delimiter line: two ordinary paragraphs that happen to
            // look table-ish. Swallowing them would lose the user's text.
            buf.set_text("| a | b |\n| c | d |");
            let doc = capture_from_buffer(&buf);
            assert!(doc.paragraphs.iter().all(|p| p.style.table_cell.is_none()));
            assert_eq!(doc.paragraphs.len(), 2);
            assert_eq!(doc.paragraphs[0].text(), "| a | b |");
        });
    }

    #[test]
    fn inserting_a_table_through_the_editor_produces_exactly_one_table() {
        // The journey the recorded GUI test covers, at the bridge level:
        // one Insert Table gives one table, at the cursor, and no literal
        // grid text left behind as prose.
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            buf.set_text("intro paragraph");
            buf.place_cursor(&buf.end_iter());

            apply_structured_edit(&buf, |editor| {
                editor.insert_table(2, 2);
            });

            let doc = capture_from_buffer(&buf);
            let tables: std::collections::BTreeSet<u32> = doc.paragraphs.iter()
                .filter_map(|p| p.style.table_cell.map(|c| c.table)).collect();
            assert_eq!(tables.len(), 1, "exactly one table: {doc:?}");
            let table = *tables.iter().next().unwrap();
            assert_eq!(doc.table_dimensions(table), Some((2, 2)));
            assert_eq!(doc.paragraphs[0].text(), "intro paragraph",
                       "the table goes after the cursor's paragraph, not before it");
            assert!(doc.paragraphs.iter().filter(|p| p.style.table_cell.is_none())
                        .all(|p| !p.text().contains('|')),
                    "no literal grid text should survive as prose: {doc:?}");
        });
    }

    #[test]
    fn page_breaks_survive_the_buffer_round_trip() {
        // Before the tag existed, render dropped page_break_before and
        // capture could not recover it, so a break inserted in the editor
        // never reached a saved DOCX or ODT — both of which write it.
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            let mut doc = Document::from_plain_text("first page\nsecond page");
            doc.paragraphs[1].style.page_break_before = true;
            let rt = round_trip(&buf, &doc);
            assert!(!rt.paragraphs[0].style.page_break_before);
            assert!(rt.paragraphs[1].style.page_break_before, "page break lost");
            assert_eq!(rt.to_plain_text(), "first page\nsecond page",
                       "a page break adds no text of its own");
        });
    }

    #[test]
    fn toggling_a_list_adds_one_marker_and_leaves_other_paragraphs_alone() {
        // The editor renders a list item's marker; the action must not
        // write one too. "- • item" on screen (and "• item" in the saved
        // document) is what two writers looked like.
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            render_to_buffer(&Document::from_plain_text("first\nsecond"), &buf);
            let second = buf.iter_at_line(1).expect("second line");
            buf.place_cursor(&second);

            apply_structured_edit(&buf, |editor| {
                editor.toggle_list_at_cursor(letters_core::ListKind::Bullet);
            });

            let shown = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
            assert_eq!(shown, "first\n\u{2022}\tsecond", "one rendered marker, on the caret's line");
            let doc = capture_from_buffer(&buf);
            assert_eq!(doc.paragraphs[0].style.list, letters_core::ListKind::None);
            assert_eq!(doc.paragraphs[1].style.list, letters_core::ListKind::Bullet);
            assert_eq!(doc.paragraphs[1].text(), "second", "the marker is not document text");
        });
    }

    #[test]
    fn inserting_a_page_break_marks_the_cursors_paragraph() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            render_to_buffer(&Document::from_plain_text("intro\nchapter two"), &buf);
            buf.place_cursor(&buf.iter_at_line(1).expect("second line"));

            apply_structured_edit(&buf, |editor| {
                editor.toggle_page_break_at_cursor();
            });

            let doc = capture_from_buffer(&buf);
            assert!(doc.paragraphs[1].style.page_break_before);
            assert_eq!(doc.to_plain_text(), "intro\nchapter two",
                       "no literal '---' paragraph is inserted");
        });
    }

    #[test]
    fn document_round_trips_through_buffer() {
        gtk_test(|| {
        let fresh = || {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
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

        // super/subscript: E = mc² and H₂O
        let buf = fresh();
        let mut d = Document::from_plain_text("E = mc2 and H2O");
        let sup = RunStyle { vert_align: Some(letters_core::model::VertAlign::Superscript), ..Default::default() };
        let sub = RunStyle { vert_align: Some(letters_core::model::VertAlign::Subscript), ..Default::default() };
        d.paragraphs[0].runs = vec![
            letters_core::Run { text: "E = mc".into(), style: RunStyle::default() },
            letters_core::Run { text: "2".into(), style: sup },
            letters_core::Run { text: " and H".into(), style: RunStyle::default() },
            letters_core::Run { text: "2".into(), style: sub },
            letters_core::Run { text: "O".into(), style: RunStyle::default() },
        ];
        let rt = round_trip(&buf, &d);
        assert_eq!(rt.to_plain_text(), "E = mc2 and H2O");
        assert_eq!(rt.style_at(6).vert_align, Some(letters_core::model::VertAlign::Superscript));
        assert_eq!(rt.style_at(13).vert_align, Some(letters_core::model::VertAlign::Subscript));
        assert_eq!(rt.style_at(5).vert_align, None, "only the digit is raised");

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
        assert_eq!(shown, "\u{2022}\tfirst\n1.\tsecond\nplain", "markers not rendered: {shown:?}");
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

    // ── non-buffer document state (#438) ─────────────────────────────────
    // Header, footer and page geometry have no representation in the text
    // buffer. `capture_from_buffer` used to hardcode all three to `None`,
    // and `save_buffer_to_path` saves exactly what it returns — so every
    // save silently dropped them, even though both the ODT and DOCX writers
    // emit all three.

    fn geometry() -> letters_core::model::PageGeometry {
        letters_core::model::PageGeometry {
            width_pt: 595.0,
            height_pt: 842.0,
            margin_top_pt: 72.0,
            margin_bottom_pt: 72.0,
            margin_left_pt: 54.0,
            margin_right_pt: 54.0,
            columns: 2,
            column_gap_pt: 18.0,
        }
    }

    #[test]
    fn header_and_footer_survive_a_buffer_round_trip() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            let mut doc = Document::from_plain_text("body text");
            doc.header = Some("Quarterly Report".into());
            doc.footer = Some("Page {page}".into());

            let captured = round_trip(&buf, &doc);
            assert_eq!(captured.header.as_deref(), Some("Quarterly Report"));
            assert_eq!(captured.footer.as_deref(), Some("Page {page}"));
            assert_eq!(captured.to_plain_text(), "body text");
        });
    }

    #[test]
    fn page_geometry_survives_a_buffer_round_trip() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            let mut doc = Document::from_plain_text("body");
            doc.page = Some(geometry());

            let captured = round_trip(&buf, &doc);
            let page = captured.page.expect("page geometry must survive the round trip");
            assert_eq!(page, geometry());
        });
    }

    /// The state must survive *editing*, not merely an untouched round trip —
    /// that is the case a user actually hits: open a document with a header,
    /// type a word, save.
    #[test]
    fn non_buffer_state_survives_an_edit() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            let mut doc = Document::from_plain_text("before");
            doc.header = Some("Kept".into());
            doc.footer = Some("Also kept".into());
            doc.page = Some(geometry());
            render_to_buffer(&doc, &buf);

            let mut end = buf.end_iter();
            buf.insert(&mut end, " and after");

            let captured = capture_from_buffer(&buf);
            assert_eq!(captured.to_plain_text(), "before and after");
            assert_eq!(captured.header.as_deref(), Some("Kept"));
            assert_eq!(captured.footer.as_deref(), Some("Also kept"));
            assert_eq!(captured.page, Some(geometry()));
        });
    }

    /// A document with no header must capture as `None`, not as an empty
    /// string — the writers treat the two differently, and `Some("")` would
    /// emit an empty header block into every saved file.
    #[test]
    fn absent_header_stays_absent() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            let doc = Document::from_plain_text("no header here");
            let captured = round_trip(&buf, &doc);
            assert_eq!(captured.header, None);
            assert_eq!(captured.footer, None);
            assert_eq!(captured.page, None);
        });
    }

    /// A buffer that was never rendered from a Document has no sidecars at
    /// all. Capturing it must not panic and must report absence.
    #[test]
    fn buffer_without_sidecars_captures_cleanly() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            buf.set_text("typed straight into an empty buffer");
            let captured = capture_from_buffer(&buf);
            assert_eq!(captured.to_plain_text(), "typed straight into an empty buffer");
            assert_eq!(captured.header, None);
            assert_eq!(captured.footer, None);
            assert_eq!(captured.page, None);
            assert!(captured.footnotes.is_empty());
        });
    }

    /// Re-rendering a different document must replace the previous
    /// document's state, not leave the old header attached to the new one.
    #[test]
    fn rendering_a_new_document_replaces_the_previous_state() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            let mut first = Document::from_plain_text("first");
            first.header = Some("Old header".into());
            first.page = Some(geometry());
            render_to_buffer(&first, &buf);

            let second = Document::from_plain_text("second");
            let captured = round_trip(&buf, &second);
            assert_eq!(captured.to_plain_text(), "second");
            assert_eq!(captured.header, None, "the previous document's header must not leak");
            assert_eq!(captured.page, None, "nor its page geometry");
        });
    }

    /// The header/footer dialog writes through this helper. An empty entry
    /// means "no header": `Some("")` would make both writers emit an empty
    /// header block into every saved file.
    #[test]
    fn setting_an_empty_header_clears_it_rather_than_storing_a_blank() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            let mut doc = Document::from_plain_text("body");
            doc.header = Some("Existing".into());
            render_to_buffer(&doc, &buf);

            set_buffer_header_footer(&buf, "", "");
            let captured = capture_from_buffer(&buf);
            assert_eq!(captured.header, None, "a cleared entry must remove the header");
            assert_eq!(captured.footer, None);
        });
    }

    /// Editing the header without touching the text must persist, and must
    /// leave the page geometry alone.
    #[test]
    fn dialog_edits_reach_the_captured_document() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            let mut doc = Document::from_plain_text("body");
            doc.page = Some(geometry());
            render_to_buffer(&doc, &buf);

            set_buffer_header_footer(&buf, "New header", "New footer");
            let captured = capture_from_buffer(&buf);
            assert_eq!(captured.header.as_deref(), Some("New header"));
            assert_eq!(captured.footer.as_deref(), Some("New footer"));
            assert_eq!(captured.page, Some(geometry()), "geometry must be untouched");
            assert_eq!(
                buffer_header_footer(&buf),
                (Some("New header".into()), Some("New footer".into())),
                "the dialog reads back what it wrote"
            );
        });
    }
}
