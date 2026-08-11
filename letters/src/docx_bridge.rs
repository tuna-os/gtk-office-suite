// SPDX-License-Identifier: GPL-3.0-or-later
//
// docx_bridge — Bridge between GtkTextBuffer TextTags and rdocx document model.
// Converts rdocx ↔ GtkTextBuffer with formatting and style preservation.

use gtk4::{self as gtk, prelude::*};

/// Write a GtkTextBuffer to a .docx file with layout-aware page breaks.
///
/// Reading .docx now goes through `letters_core::docx::read` (see
/// `bridge::load_file_to_buffer`); the old buffer-populating reader that
/// used to live here was superseded and has been removed.
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
// ── Style mapping ─────────────────────────────────────────────────────

fn tag_to_style_id(tag: &str) -> &str {
    match tag {
        "h1" => "Heading1", "h2" => "Heading2", "h3" => "Heading3",
        "h4" => "Heading4", "h5" => "Heading5", "h6" => "Heading6",
        "h-title" => "Title", "h-subtitle" => "Subtitle",
        "code" => "Code", "blockquote" => "Blockquote",
        "normal" => "Normal",
        _ => "",
    }
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
    let end = buf.end_iter();
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


#[cfg(test)]
mod tests {
    use super::{split_paragraphs, tag_to_style_id, write_buffer_to_docx_with_layout};
    use gtk4::{self as gtk, prelude::*};

    #[test]
    fn tag_to_style_id_maps_all_heading_and_special_tags() {
        assert_eq!(tag_to_style_id("h1"), "Heading1");
        assert_eq!(tag_to_style_id("h2"), "Heading2");
        assert_eq!(tag_to_style_id("h3"), "Heading3");
        assert_eq!(tag_to_style_id("h4"), "Heading4");
        assert_eq!(tag_to_style_id("h5"), "Heading5");
        assert_eq!(tag_to_style_id("h6"), "Heading6");
        assert_eq!(tag_to_style_id("h-title"), "Title");
        assert_eq!(tag_to_style_id("h-subtitle"), "Subtitle");
        assert_eq!(tag_to_style_id("code"), "Code");
        assert_eq!(tag_to_style_id("blockquote"), "Blockquote");
        assert_eq!(tag_to_style_id("normal"), "Normal");
    }

    #[test]
    fn tag_to_style_id_unknown_returns_empty() {
        assert_eq!(tag_to_style_id("unknown"), "");
        assert_eq!(tag_to_style_id(""), "");
    }

    #[gtk::test]
    fn split_paragraphs_tracks_offsets_and_detects_style_tags() {
        let buf = gtk::TextBuffer::new(None);
        buf.set_text("line1\nline2");

        // Tag the first line as h1.
        let table = buf.tag_table();
        let h1 = gtk::TextTag::builder().name("h1").build();
        table.add(&h1);
        let start = buf.start_iter();
        let end = buf.iter_at_offset(5);
        buf.apply_tag(&h1, &start, &end);
        
        let paras = split_paragraphs(&buf, "line1\nline2");
        assert_eq!(paras.len(), 2);
        assert_eq!(paras[0].text, "line1");
        assert_eq!(paras[0].offset, 0);
        assert_eq!(paras[0].style_id, "Heading1");
        assert_eq!(paras[1].text, "line2");
        assert_eq!(paras[1].offset, 6); // 5 bytes + newline
        assert_eq!(paras[1].style_id, "");
    }

    #[gtk::test]
    fn split_paragraphs_detects_custom_style_prefix() {
        let buf = gtk::TextBuffer::new(None);
        buf.set_text("custom-styled");
        let table = buf.tag_table();
        let tag = gtk::TextTag::builder().name("custom-foo").build();
        table.add(&tag);
        let start = buf.start_iter();
        let end = buf.end_iter();
        buf.apply_tag(&tag, &start, &end);

        let paras = split_paragraphs(&buf, "custom-styled");
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0].style_id, "foo"); // "custom-" prefix stripped
    }

    #[gtk::test]
    fn buffer_round_trips_to_docx_and_back() {
        let buf = gtk::TextBuffer::new(None);
        buf.set_text("Hello docx bridge\nSecond paragraph");

        let path = std::env::temp_dir()
            .join(format!("letters-docx-bridge-{}.docx", std::process::id()));
        let path_str = path.to_string_lossy().to_string();

        let res = write_buffer_to_docx_with_layout(&path_str, &buf, None, &[]);
        assert!(res.is_ok(), "write_buffer_to_docx_with_layout failed: {:?}", res.err());

        let read_doc = letters_core::docx::read(&path_str);
        assert!(read_doc.is_ok(), "read back failed: {:?}", read_doc.err());
        let plain = read_doc.unwrap().to_plain_text();
        assert!(plain.contains("Hello docx bridge"), "missing first paragraph in {plain:?}");
        assert!(plain.contains("Second paragraph"), "missing second paragraph in {plain:?}");

        let _ = std::fs::remove_file(&path_str);
    }
}
