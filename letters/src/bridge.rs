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

    Document { paragraphs, header: None, footer: None }
}

/// The editor shows lists as literal "- " / "N. " markers; the model wants
/// ListKind. Strip the marker and set the kind when capturing.
fn capture_list_marker(para: &mut Paragraph) {
    let text = para.text();
    let (kind, strip) = if text.starts_with("- ") {
        (letters_core::ListKind::Bullet, 2)
    } else if let Some(dot) = text.find(". ") {
        if dot > 0 && text[..dot].chars().all(|c| c.is_ascii_digit()) {
            (letters_core::ListKind::Numbered, dot + 2)
        } else {
            return;
        }
    } else {
        return;
    };
    para.style.list = kind;
    // Remove `strip` chars from the front of the run list.
    let mut remaining = strip;
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
    buf.set_text("");
    let mut insert = buf.start_iter();
    for (i, para) in doc.paragraphs.iter().enumerate() {
        if i > 0 {
            buf.insert(&mut insert, "\n");
        }
        let para_start = insert.offset();
        match para.style.list {
            letters_core::ListKind::Bullet => buf.insert(&mut insert, "- "),
            letters_core::ListKind::Numbered => {
                // Number within the current consecutive numbered group.
                let n = doc.paragraphs[..i].iter().rev()
                    .take_while(|p| p.style.list == letters_core::ListKind::Numbered)
                    .count() + 1;
                buf.insert(&mut insert, &format!("{n}. "));
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

    fn round_trip(buf: &gtk::TextBuffer, doc: &Document) -> Document {
        render_to_buffer(doc, buf);
        capture_from_buffer(buf)
    }

    // GTK insists on being initialized and used from a single thread, and
    // cargo gives every #[test] its own thread — so all buffer round-trip
    // cases live in this one test. Skips cleanly with no display; the
    // gui-tests smoke job runs it under Xvfb where it must pass.
    #[test]
    fn document_round_trips_through_buffer() {
        if gtk::init().is_err() {
            eprintln!("skipping: no display for GTK");
            return;
        }
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
    }
}
