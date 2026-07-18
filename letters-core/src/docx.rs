// docx.rs — Document ⇄ DOCX via rdocx.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Model-level DOCX I/O: paragraphs, styled runs (incl. highlight and inline
// code via SourceText), headings, alignment, lists, hyperlinks. Tables are
// flattened (see read()). Fidelity is measured by tests/docx.rs and the
// LO-authored corpus in tests/lo_parity.rs.

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

    // Table cells become paragraphs tagged with (table, row, col) — the
    // document stays flat (offset invariants intact) and the grid is fully
    // recoverable. Position limitation: rdocx exposes tables separately
    // from the paragraph stream, so tables append after body paragraphs.
    let tables = doc.tables();
    if !tables.is_empty() {
        // OOXML mandates an (empty) paragraph after each table; with the
        // tables appended at the end it is pure noise — drop it.
        while paragraphs.last().map(|p: &Paragraph| p.runs.is_empty()).unwrap_or(false) {
            paragraphs.pop();
        }
    }
    for (ti, table) in tables.iter().enumerate() {
        for ri in 0..table.row_count() {
            let Some(row) = table.row(ri) else { continue };
            for ci in 0..row.cell_count() {
                let Some(cell) = row.cell(ci) else { continue };
                let mut wrote_any = false;
                for cp in cell.paragraphs() {
                    if cp.text().is_empty() { continue; }
                    let mut para = map_paragraph(&doc, &cp);
                    para.style.table_cell = Some(crate::model::TableCell {
                        table: ti as u32, row: ri as u32, col: ci as u32,
                    });
                    paragraphs.push(para);
                    wrote_any = true;
                }
                // Empty cells still occupy a grid position.
                if !wrote_any {
                    paragraphs.push(Paragraph {
                        style: ParaStyle {
                            table_cell: Some(crate::model::TableCell {
                                table: ti as u32, row: ri as u32, col: ci as u32,
                            }),
                            ..Default::default()
                        },
                        runs: vec![],
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
    let paras = &doc.paragraphs;
    let mut i = 0;
    while i < paras.len() {
        // Consecutive paragraphs sharing a table id become one rdocx table.
        if let Some(tc0) = paras[i].style.table_cell {
            let start = i;
            while i < paras.len()
                && paras[i].style.table_cell.map(|t| t.table) == Some(tc0.table)
            {
                i += 1;
            }
            let group = &paras[start..i];
            let rows = group.iter().filter_map(|p| p.style.table_cell.map(|t| t.row)).max().unwrap_or(0) as usize + 1;
            let cols = group.iter().filter_map(|p| p.style.table_cell.map(|t| t.col)).max().unwrap_or(0) as usize + 1;
            let mut tbl = out.add_table(rows, cols);
            let mut filled = std::collections::HashSet::new();
            for p in group {
                let tc = p.style.table_cell.expect("grouped by table_cell");
                if let Some(mut cell) = tbl.cell(tc.row as usize, tc.col as usize) {
                    filled.insert((tc.row, tc.col));
                    let mut cp = cell.add_paragraph("");
                    for run in &p.runs {
                        let mut r = cp.add_run(&run.text);
                        if run.style.bold { r = r.bold(true); }
                        if run.style.italic { r = r.italic(true); }
                        if run.style.underline { r = r.underline(true); }
                        if run.style.strikethrough { r = r.strike(true); }
                        if run.style.highlight { r = r.highlight("yellow"); }
                        if run.style.code { r = r.style("SourceText"); }
                    }
                }
            }
            // OOXML requires a paragraph in every cell and one after a table.
            for r in 0..rows {
                for c in 0..cols {
                    if !filled.contains(&(r as u32, c as u32)) {
                        if let Some(mut cell) = tbl.cell(r, c) {
                            cell.add_paragraph("");
                        }
                    }
                }
            }
            out.add_paragraph("");
            continue;
        }
        let para = &paras[i];
        i += 1;
        let mut p = match para.style.list {
            ListKind::Bullet => out.add_bullet_list_item("", 0),
            ListKind::Numbered => out.add_numbered_list_item("", 0),
            ListKind::None => out.add_paragraph(""),
        };
        if let Some(level) = para.style.heading {
            p = p.style(&format!("Heading{}", level.clamp(1, 6)));
        }
        if para.style.block_quote {
            p = p.style("Quote");
        }
        p = match para.style.alignment {
            Alignment::Left => p,
            Alignment::Center => p.alignment(rdocx::Alignment::Center),
            Alignment::Right => p.alignment(rdocx::Alignment::Right),
            Alignment::Justify => p.alignment(rdocx::Alignment::Justify),
        };
        let _ = p; // release the builder borrow before append_hyperlink
        for run in &para.runs {
            if let Some(src) = &run.style.image {
                // Images embed via add_picture, which appends its own
                // paragraph — mid-paragraph images therefore split the
                // paragraph (documented v1 limitation). Unreadable sources
                // degrade to the alt text.
                match std::fs::read(src) {
                    Ok(bytes) => {
                        let name = std::path::Path::new(src)
                            .file_name().map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "image.png".into());
                        let mut pic = out.add_picture(
                            &bytes, &name,
                            rdocx::Length::inches(4.0), rdocx::Length::inches(3.0),
                        );
                        pic = pic.style("Figure");
                        let _ = pic;
                        out.add_paragraph("");
                    }
                    Err(_) => {
                        let mut p = out.last_paragraph_mut().expect("paragraph");
                        let _ = p.add_run(&run.text);
                    }
                }
                continue;
            }
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
            if let Some(hp) = run.style.font_size_hp { r = r.size(hp as f64 / 2.0); }
            if let Some(c) = &run.style.color { r = r.color(c); }
            match run.style.vert_align {
                Some(crate::model::VertAlign::Superscript) => { r = r.superscript(); }
                Some(crate::model::VertAlign::Subscript) => { r = r.subscript(); }
                None => {}
            }
        }
    }
    out.save(path).map_err(|e| format!("Cannot save {}: {}", path, e))
}

/// Map one rdocx paragraph (body or table cell) into a model paragraph.
fn map_paragraph(doc: &rdocx::Document, p: &rdocx::ParagraphRef<'_>) -> Paragraph {
    let heading = p.style_id().and_then(style_id_to_heading);
    // LO uses "Quotations"; Word uses "Quote"/"IntenseQuote".
    let block_quote = matches!(p.style_id(), Some("Quote") | Some("Quotations") | Some("IntenseQuote") | Some("BlockQuote") | Some("BlockQuotation"));
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
        // Inline images: extract bytes to a cache file so the model's
        // image path is always locally readable.
        if let Some((rel_id, alt)) = r.inline_image() {
            if let Some(bytes) = doc.image_data(rel_id) {
                let dir = std::env::temp_dir().join("letters-images");
                let _ = std::fs::create_dir_all(&dir);
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                std::hash::Hash::hash(&bytes, &mut hasher);
                let path = dir.join(format!("{:x}.png", std::hash::Hasher::finish(&hasher)));
                let _ = std::fs::write(&path, &bytes);
                runs.push(Run {
                    text: alt.unwrap_or("").to_string(),
                    style: RunStyle {
                        image: Some(path.to_string_lossy().into_owned()),
                        ..Default::default()
                    },
                });
                continue;
            }
        }
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
                image: None,
                font_size_hp: r.size().map(|pt| (pt * 2.0).round() as u16),
                color: r.color().map(|c| c.trim_start_matches('#').to_uppercase()),
                vert_align: match r.vert_align() {
                    Some("superscript") => Some(crate::model::VertAlign::Superscript),
                    Some("subscript") => Some(crate::model::VertAlign::Subscript),
                    // LibreOffice encodes super/subscript as raised/lowered
                    // position instead of vertAlign.
                    _ => match r.position() {
                        Some(p) if p > 0 => Some(crate::model::VertAlign::Superscript),
                        Some(p) if p < 0 => Some(crate::model::VertAlign::Subscript),
                        _ => None,
                    },
                },
            },
        });
    }
    let mut para = Paragraph {
        style: ParaStyle { heading, alignment, list, code_block, block_quote, ..Default::default() },
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
    p.runs.retain(|r| !r.text.is_empty() || r.style.image.is_some());
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
