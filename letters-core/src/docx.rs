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
        // Decorative rules (LibreOffice's HorizontalLine style) carry no text.
        if p.style_id() == Some("HorizontalLine") && p.text().is_empty() {
            continue;
        }
        paragraphs.push(map_paragraph(&doc, &p));
    }

    // Tables are not modeled yet; flatten their cell paragraphs (styles and
    // all) so no content is lost. Limitation: rdocx exposes tables separately
    // from the paragraph stream, so flattened cells append after body
    // paragraphs rather than interleaving at their true position.
    let tables = doc.tables();
    if !tables.is_empty() {
        // OOXML mandates an (empty) paragraph after each table; with the
        // flattened cells appended at the end it is pure noise — drop it.
        while paragraphs.last().map(|p: &Paragraph| p.runs.is_empty()).unwrap_or(false) {
            paragraphs.pop();
        }
    }
    for table in tables {
        for ri in 0..table.row_count() {
            let Some(row) = table.row(ri) else { continue };
            for ci in 0..row.cell_count() {
                let Some(cell) = row.cell(ci) else { continue };
                for cp in cell.paragraphs() {
                    if cp.text().is_empty() { continue; }
                    paragraphs.push(map_paragraph(&doc, &cp));
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
        drop(p);
        for run in &para.runs {
            if let Some(url) = &run.style.link {
                // Hyperlinks need a document-level relationship; styles on
                // link text are not yet carried through append_hyperlink.
                out.append_hyperlink(&run.text, url);
                continue;
            }
            let mut p = out.last_paragraph_mut().expect("paragraph just added");
            let mut r = p.add_run(&run.text);
            if run.style.bold { r = r.bold(true); }
            if run.style.italic { r = r.italic(true); }
            if run.style.underline { r = r.underline(true); }
            if run.style.strikethrough { r = r.strike(true); }
            if run.style.highlight { r = r.highlight("yellow"); }
            if run.style.code { r = r.style("SourceText"); }
        }
    }
    out.save(path).map_err(|e| format!("Cannot save {}: {}", path, e))
}

/// Map one rdocx paragraph (body or table cell) into a model paragraph.
fn map_paragraph(doc: &rdocx::Document, p: &rdocx::ParagraphRef<'_>) -> Paragraph {
    let heading = p.style_id().and_then(style_id_to_heading);
    // LibreOffice emits PreformattedText for <pre>/code blocks.
    let code_block = matches!(p.style_id(), Some("PreformattedText") | Some("HTMLPreformatted"))
        .then(String::new);
    let alignment = match p.alignment() {
        Some(rdocx::Alignment::Center) => Alignment::Center,
        Some(rdocx::Alignment::Right) => Alignment::Right,
        Some(rdocx::Alignment::Justify) => Alignment::Justify,
        _ => Alignment::Left,
    };
    let list = match p.numbering() {
        Some((num_id, _level)) => match doc.numbering_is_bullet(num_id) {
            Some(false) => ListKind::Numbered,
            // Unknown num_id defaults to bullet — the safer visual guess.
            _ => ListKind::Bullet,
        },
        None => ListKind::None,
    };

    // Per-run link URLs from hyperlink spans (indexes into the runs vec).
    let spans = p.hyperlink_spans();
    let link_for = |idx: usize| -> Option<String> {
        spans.iter()
            .find(|(start, end, _)| idx >= *start && idx < *end)
            .and_then(|(_, _, rel_id)| rel_id.as_deref().and_then(|id| doc.hyperlink_url(id)))
    };

    let mut runs = Vec::new();
    for (idx, r) in p.runs().enumerate() {
        let text = r.text();
        if text.is_empty() { continue; }
        runs.push(Run {
            text,
            style: RunStyle {
                bold: r.is_bold(),
                italic: r.is_italic(),
                underline: r.is_underline(),
                strikethrough: r.is_strike(),
                highlight: r.highlight().is_some(),
                code: r.style_id() == Some("SourceText"),
                link: link_for(idx),
            },
        });
    }
    let mut para = Paragraph {
        style: ParaStyle { heading, alignment, list, code_block, ..Default::default() },
        runs,
    };
    normalize(&mut para);
    para
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
