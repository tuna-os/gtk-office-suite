// bridge.rs — GtkTextBuffer ⇄ letters_core::Document.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The buffer is a *view*: letters-core owns document semantics and all file
// I/O. This module is the only place buffer tags are translated to/from
// model styles. Tag names map 1:1 to RunStyle fields / heading levels
// (see register_formatting_tags in window.rs).
//
// Not yet bridged: links (no link tag in the buffer yet), alignment and
// list kind (kept as literal text / view state today).

use gtk4::{self as gtk, prelude::*};
use letters_core::model::{Document, Paragraph, ParaStyle, Run, RunStyle};

const RUN_TAGS: [&str; 6] = ["bold", "italic", "underline", "strikethrough", "highlight", "code"];

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
        s
    };

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
            at_line_start = false;
        }
        let ch = iter.char();
        if ch == '\n' {
            if let Some(r) = current_run.take() {
                current.runs.push(r);
            }
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
    paragraphs.push(current);

    Document { paragraphs }
}

/// Replace the buffer's content with a rendered Document.
pub fn render_to_buffer(doc: &Document, buf: &gtk::TextBuffer) {
    buf.set_text("");
    let mut insert = buf.start_iter();
    for (i, para) in doc.paragraphs.iter().enumerate() {
        if i > 0 {
            buf.insert(&mut insert, "\n");
        }
        let para_start = insert.offset();
        for run in &para.runs {
            let mut names: Vec<&str> = Vec::new();
            if run.style.bold { names.push("bold"); }
            if run.style.italic { names.push("italic"); }
            if run.style.underline { names.push("underline"); }
            if run.style.strikethrough { names.push("strikethrough"); }
            if run.style.highlight { names.push("highlight"); }
            if run.style.code { names.push("code"); }
            if names.is_empty() {
                buf.insert(&mut insert, &run.text);
            } else {
                buf.insert_with_tags_by_name(&mut insert, &run.text, &names);
            }
        }
        let tag_name = match (para.style.heading, &para.style.code_block) {
            (Some(l), _) => Some(format!("h{}", l.clamp(1, 6))),
            (None, Some(_)) => Some("code".to_string()),
            _ => None,
        };
        if let Some(name) = tag_name {
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
        _ => {
            let md = letters_core::markdown::serialize(&doc);
            std::fs::write(path, md).map_err(|e| format!("Cannot write {path}: {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use letters_core::model::StylePatch;

    // Buffer round-trips need a display; skip cleanly when GTK can't init
    // (plain `cargo test` on headless boxes). The gui-tests smoke job runs
    // these under Xvfb where they must pass.
    fn buffer_or_skip() -> Option<gtk::TextBuffer> {
        if gtk::init().is_err() {
            eprintln!("skipping: no display for GTK");
            return None;
        }
        let buf = gtk::TextBuffer::new(None);
        crate::window::register_formatting_tags(&buf);
        Some(buf)
    }

    fn round_trip(buf: &gtk::TextBuffer, doc: &Document) -> Document {
        render_to_buffer(doc, buf);
        capture_from_buffer(buf)
    }

    #[test]
    fn styled_document_round_trips_through_buffer() {
        let Some(buf) = buffer_or_skip() else { return };
        let mut d = Document::from_plain_text("plain bold italic\nsecond line");
        d.apply_run_style(6, 10, &StylePatch::set_bold(true));
        d.apply_run_style(11, 17, &StylePatch::set_italic(true));
        let rt = round_trip(&buf, &d);
        assert_eq!(rt.to_plain_text(), d.to_plain_text());
        assert!(rt.style_at(6).bold && !rt.style_at(5).bold);
        assert!(rt.style_at(11).italic);
    }

    #[test]
    fn headings_round_trip_through_buffer() {
        let Some(buf) = buffer_or_skip() else { return };
        let mut d = Document::from_plain_text("Title\nbody text");
        d.set_heading(0, Some(1));
        let rt = round_trip(&buf, &d);
        assert_eq!(rt.paragraphs[0].style.heading, Some(1));
        assert_eq!(rt.paragraphs[1].style.heading, None);
    }

    #[test]
    fn highlight_and_code_round_trip_through_buffer() {
        let Some(buf) = buffer_or_skip() else { return };
        let mut d = Document::from_plain_text("glow mono");
        d.apply_run_style(0, 4, &StylePatch::set_highlight(true));
        d.apply_run_style(5, 9, &StylePatch::set_code(true));
        let rt = round_trip(&buf, &d);
        assert!(rt.style_at(0).highlight && !rt.style_at(5).highlight);
        assert!(rt.style_at(5).code && !rt.style_at(0).code);
    }
}
