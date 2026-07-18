// docx.rs — Document ⇄ DOCX via rdocx.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Replaces the ad-hoc GtkTextBuffer↔docx logic in letters/src/docx_bridge.rs
// with model-level I/O. Current coverage: paragraphs, bold/italic/underline/
// strikethrough runs, heading styles. Not yet mapped: highlight, links,
// lists, alignment — tracked by red tests as they are added.

use crate::model::{Alignment, Document, ListKind, Paragraph, ParaStyle, Run, RunStyle};

/// Read a .docx file into a Document.
pub fn read(path: &str) -> Result<Document, String> {
    let doc = rdocx::Document::open(path)
        .map_err(|e| format!("Cannot open .docx {}: {}", path, e))?;

    let mut paragraphs = Vec::new();
    for p in doc.paragraphs() {
        let heading = p.style_id().and_then(style_id_to_heading);
        let alignment = match p.alignment() {
            Some(rdocx::Alignment::Center) => Alignment::Center,
            Some(rdocx::Alignment::Right) => Alignment::Right,
            Some(rdocx::Alignment::Justify) => Alignment::Justify,
            _ => Alignment::Left,
        };
        // NOTE: list kind and highlight cannot be read back yet — rdocx has
        // no numbering/highlight getters on ParagraphRef/RunRef. Red tests
        // in tests/docx.rs track this; fix lands upstream in hanthor/rdocx.
        let mut runs = Vec::new();
        for r in p.runs() {
            let text = r.text();
            if text.is_empty() { continue; }
            runs.push(Run {
                text,
                style: RunStyle {
                    bold: r.is_bold(),
                    italic: r.is_italic(),
                    underline: r.is_underline(),
                    strikethrough: r.is_strike(),
                    ..Default::default()
                },
            });
        }
        let mut para = Paragraph {
            style: ParaStyle { heading, alignment, ..Default::default() },
            runs,
        };
        normalize(&mut para);
        paragraphs.push(para);
    }

    // Tables are not modeled yet; flatten their cell text into paragraphs so
    // no content is lost. Limitation: rdocx exposes tables separately from
    // the paragraph stream, so flattened cells append after body paragraphs
    // rather than interleaving at their true position.
    for table in doc.tables() {
        for ri in 0..table.row_count() {
            let Some(row) = table.row(ri) else { continue };
            for ci in 0..row.cell_count() {
                let Some(cell) = row.cell(ci) else { continue };
                for cp in cell.paragraphs() {
                    let text = cp.text();
                    if text.is_empty() { continue; }
                    paragraphs.push(Paragraph {
                        style: ParaStyle::default(),
                        runs: vec![Run { text, style: RunStyle::default() }],
                    });
                }
            }
        }
    }

    if paragraphs.is_empty() {
        return Ok(Document::new());
    }
    Ok(Document { paragraphs })
}

/// Write a Document to a .docx file.
pub fn write(doc: &Document, path: &str) -> Result<(), String> {
    let mut out = rdocx::Document::new();
    for para in &doc.paragraphs {
        let mut p = match para.style.list {
            ListKind::Bullet => out.add_bullet_list_item("", 0),
            ListKind::Numbered => out.add_numbered_list_item("", 0),
            ListKind::None => out.add_paragraph(""),
        };
        if let Some(level) = para.style.heading {
            p = p.style(&format!("Heading{}", level.clamp(1, 6)));
        }
        p = match para.style.alignment {
            Alignment::Left => p,
            Alignment::Center => p.alignment(rdocx::Alignment::Center),
            Alignment::Right => p.alignment(rdocx::Alignment::Right),
            Alignment::Justify => p.alignment(rdocx::Alignment::Justify),
        };
        for run in &para.runs {
            let mut r = p.add_run(&run.text);
            if run.style.bold { r = r.bold(true); }
            if run.style.italic { r = r.italic(true); }
            if run.style.underline { r = r.underline(true); }
            if run.style.strikethrough { r = r.strike(true); }
            if run.style.highlight { r = r.highlight("yellow"); }
        }
    }
    out.save(path).map_err(|e| format!("Cannot save {}: {}", path, e))
}

fn style_id_to_heading(id: &str) -> Option<u8> {
    let level = id.strip_prefix("Heading")?.parse::<u8>().ok()?;
    (1..=6).contains(&level).then_some(level)
}

fn normalize(p: &mut Paragraph) {
    p.runs.retain(|r| !r.text.is_empty());
    let mut i = 0;
    while i + 1 < p.runs.len() {
        if p.runs[i].style == p.runs[i + 1].style {
            let next = p.runs.remove(i + 1);
            p.runs[i].text.push_str(&next.text);
        } else {
            i += 1;
        }
    }
}
