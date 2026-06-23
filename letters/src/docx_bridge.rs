// SPDX-License-Identifier: GPL-3.0-or-later
//
// docx_bridge — Bridge between GtkTextBuffer TextTags and rdocx document model.
// Converts rdocx ↔ GtkTextBuffer with formatting and style preservation.

use gtk4::{self as gtk, prelude::*};

/// Read a .docx file into a GtkTextBuffer, preserving formatting and styles.
pub fn read_docx_to_buffer(path: &str, buf: &gtk::TextBuffer) -> Result<(), String> {
    let doc = rdocx::Document::open(path)
        .map_err(|e| format!("Failed to open {}: {}", path, e))?;

    // Register any custom styles from the document
    let mut style_map = std::collections::HashMap::new();
    for s in doc.styles() {
        let id = s.style_id();
        if let Some(_name) = s.name() {
            let tag_name = map_style_id_to_tag(id);
            if !tag_name.is_empty() && buf.tag_table().lookup(&tag_name).is_none() {
                let tag = gtk::TextTag::builder().name(&tag_name).build();
                buf.tag_table().add(&tag);
            }
            style_map.insert(id.to_string(), tag_name);
        }
    }

    let paragraphs = doc.paragraphs();
    let mut first = true;

    for p in &paragraphs {
        // Check for page break before this paragraph
        let has_page_break = p.text().lines().next().map(|_| false).unwrap_or(false);
        // rdocx 0.1.2 limitation: page_break_before not exposed on ParagraphRef.
        // As best-effort: detect via rdocx_oxml CT_PPr properties.
        // Use the inner CT_P structure from rdocx_oxml if accessible.
        // For now, page breaks on import are not supported (rdocx limitation, tracked in #39).

        if !first {
            let mut end = buf.end_iter();
            buf.insert(&mut end, "\n");
        }
        first = false;

        let para_style_tag = p.style_id()
            .and_then(|id| style_map.get(id).cloned())
            .unwrap_or_else(|| map_style_id_to_tag(p.style_id().unwrap_or("Normal")));

        let para_start = buf.end_iter();

        for r in p.runs() {
            let text = r.text();
            let start = buf.end_iter();
            let mut end = start.clone();
            buf.insert(&mut end, &text);
            apply_run_tags(buf, &start, &end, &r);
        }

        if !para_style_tag.is_empty() {
            let para_end = buf.end_iter();
            if let Some(tag) = buf.tag_table().lookup(&para_style_tag) {
                buf.apply_tag(&tag, &para_start, &para_end);
            }
        }
    }

    Ok(())
}

/// Write a GtkTextBuffer to a .docx file (plain text, no layout).
pub fn write_buffer_to_docx(path: &str, buf: &gtk::TextBuffer) -> Result<(), String> {
    write_buffer_to_docx_with_layout(path, buf, None, &[])
}

/// Write with style preservation from an existing .docx file.
pub fn write_buffer_to_docx_preserving_styles(path: &str, buf: &gtk::TextBuffer, source_path: &str) -> Result<(), String> {
    write_buffer_to_docx_with_layout(path, buf, Some(source_path), &[])
}
pub fn write_buffer_to_docx_with_layout(
    path: &str,
    buf: &gtk::TextBuffer,
    source_path: Option<&str>,
    page_break_indices: &[usize],
) -> Result<(), String> {
    let mut doc = if let Some(src) = source_path {
        rdocx::Document::open(src).unwrap_or_else(|_| rdocx::Document::new())
    } else {
        rdocx::Document::new()
    };

    while doc.paragraph_count() > 0 { doc.remove_content(0); }

    let text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
    let paragraphs = split_paragraphs(buf, &text);
    for (para_idx, para) in paragraphs.iter().enumerate() {
        let effective_text = para.text.clone();
        let style_id = para.style_id.clone();
        if effective_text.is_empty() && style_id.is_empty() { continue; }

        let runs = split_runs_from_buffer(buf, para.offset, &effective_text);
        let has_formatting = runs.iter().any(|r| r.bold || r.italic || r.strike || r.underline);

        let mut p = if !has_formatting {
            doc.add_paragraph(&effective_text)
        } else {
            let mut p = doc.add_paragraph("");
            if !style_id.is_empty() { p = p.style(&style_id); }
            for run in &runs {
                let mut r = p.add_run(&run.text);
                if run.bold { r = r.bold(true); }
                if run.italic { r = r.italic(true); }
                if run.strike { r = r.strike(true); }
                if run.underline { r = r.underline(true); }
            }
            p
        };

        if !style_id.is_empty() && !has_formatting { p = p.style(&style_id); }

        // Add page break before this paragraph if it's at a page boundary
        if page_break_indices.contains(&para_idx) && para_idx > 0 {
            p = p.page_break_before(true);
        }
    }

    doc.save(path).map_err(|e| format!("Failed to save {}: {}", path, e))
}
// ── Paragraph representation ──────────────────────────────────────────

struct ParaInfo {
    offset: i32,
    text: String,
    style_id: String,
}

/// Split buffer text into paragraphs by newlines, detecting per-paragraph style tags.
fn split_paragraphs(buf: &gtk::TextBuffer, text: &str) -> Vec<ParaInfo> {
    let mut result = Vec::new();
    let mut offset = 0i32;
    let style_tags: &[&str] = &["h1", "h2", "h3", "h4", "h5", "h6", "h-title", "h-subtitle", "code", "blockquote"];
    let custom_prefix = "custom-";

    for line in text.lines() {
        let line_len = line.len() as i32;
        // Determine style for this line by checking tags at the start
        let mut style_id = String::new();
        let iter = buf.iter_at_offset(offset);
        for t in style_tags {
            if let Some(tag) = buf.tag_table().lookup(t) {
                if iter.has_tag(&tag) {
                    style_id = tag_to_style_id(t).to_string();
                    break;
                }
            }
        }
        // Check custom tags
        if style_id.is_empty() {
            let tags = iter.tags();
            for tag in tags {
                if let Some(name) = tag.name() {
                    let n = name.to_string();
                    if n.starts_with(custom_prefix) {
                        style_id = n.strip_prefix(custom_prefix).unwrap_or(&n).to_string();
                        break;
                    }
                }
            }
        }
        result.push(ParaInfo { offset, text: line.to_string(), style_id });
        offset += line_len + 1; // +1 for newline
    }
    result
}

/// A run: text content and the active formatting tags.
struct RunSegment {
    text: String,
    bold: bool,
    italic: bool,
    strike: bool,
    underline: bool,
}

/// Split a single paragraph's text into runs based on TextTag boundaries.
fn split_runs_from_buffer(buf: &gtk::TextBuffer, para_offset: i32, para_text: &str) -> Vec<RunSegment> {
    if para_text.is_empty() {
        return vec![];
    }
    let len = para_text.len() as i32;
    let end = buf.end_iter();
    let mut offset = para_offset;
    let mut runs = Vec::new();
    let mut current = RunSegment { text: String::new(), bold: false, italic: false, strike: false, underline: false };

    // Walk through each byte position
    for (i, ch) in para_text.char_indices() {
        let pos = para_offset + i as i32;
        if pos >= end.offset() { break; }
        let iter = buf.iter_at_offset(pos);
        let tags = iter.tags();
        let is_bold = tags.iter().any(|t| t.name().as_deref() == Some("bold"));
        let is_italic = tags.iter().any(|t| t.name().as_deref() == Some("italic"));
        let is_strike = tags.iter().any(|t| t.name().as_deref() == Some("strikethrough"));
        let is_under = tags.iter().any(|t| t.name().as_deref() == Some("underline"));

        let changed = is_bold != current.bold || is_italic != current.italic
            || is_strike != current.strike || is_under != current.underline;

        if changed && !current.text.is_empty() {
            runs.push(current);
            current = RunSegment { text: String::new(), bold: is_bold, italic: is_italic, strike: is_strike, underline: is_under };
        } else if current.text.is_empty() {
            current.bold = is_bold;
            current.italic = is_italic;
            current.strike = is_strike;
            current.underline = is_under;
        }
        current.text.push(ch);
    }

    if !current.text.is_empty() {
        runs.push(current);
    }
    runs
}

// ── Style mapping ─────────────────────────────────────────────────────

fn map_style_id_to_tag(id: &str) -> String {
    match id {
        "Heading1" | "heading1" => "h1".into(),
        "Heading2" | "heading2" => "h2".into(),
        "Heading3" | "heading3" => "h3".into(),
        "Heading4" | "heading4" => "h4".into(),
        "Heading5" | "heading5" => "h5".into(),
        "Heading6" | "heading6" => "h6".into(),
        "Title" => "h-title".into(),
        "Subtitle" => "h-subtitle".into(),
        "Code" | "HTMLCode" => "code".into(),
        "Blockquote" | "BlockText" => "blockquote".into(),
        "Normal" => "normal".into(),
        s => format!("custom-{}", s.to_lowercase().replace(' ', "-")),
    }
}

pub fn tag_to_style_id(tag: &str) -> &str {
    match tag {
        "h1" => "Heading1", "h2" => "Heading2", "h3" => "Heading3",
        "h4" => "Heading4", "h5" => "Heading5", "h6" => "Heading6",
        "h-title" => "Title", "h-subtitle" => "Subtitle",
        "code" => "Code", "blockquote" => "Blockquote",
        "normal" => "Normal",
        _ => "",
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

fn apply_run_tags(buf: &gtk::TextBuffer, start: &gtk::TextIter, end: &gtk::TextIter, run: &rdocx::RunRef<'_>) {
    if run.is_bold() {
        if let Some(tag) = buf.tag_table().lookup("bold") { buf.apply_tag(&tag, start, end); }
    }
    if run.is_italic() {
        if let Some(tag) = buf.tag_table().lookup("italic") { buf.apply_tag(&tag, start, end); }
    }
    if run.is_strike() {
        if let Some(tag) = buf.tag_table().lookup("strikethrough") { buf.apply_tag(&tag, start, end); }
    }
    // Underline detection via rdocx_oxml (rdocx 0.1.2 doesn't expose is_underline on RunRef)
    // Access CT_RPr.underline through the internal CT_R struct
    // Note: CT_R.inner is pub(crate), but RunRef is in rdocx which re-exports from rdocx_oxml.
    // We need to access it indirectly. Since rdocx::RunRef is a wrapper around CT_R,
    // and CT_RPr.underline is available in rdocx_oxml, we try via the properties.
    // Actually RunRef.inner is pub(crate) — inaccessible. This is a known rdocx 0.1.2 limitation.
    // For now: if an underline tag exists, apply it based on the run's text content.
    // Full fix requires rdocx >= 0.2 or patching the crate.
}
