// docx.rs — Document ⇄ DOCX via rdocx.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Model-level DOCX I/O: paragraphs, styled runs (incl. highlight and inline
// code via SourceText), headings, alignment, lists, hyperlinks. Tables are
// flattened (see read()). Fidelity is measured by tests/docx.rs and the
// LO-authored corpus in tests/lo_parity.rs.

use crate::model::{Alignment, Document, ListKind, PageGeometry, Paragraph, ParaStyle, Run, RunStyle};
use rdocx_oxml::shared::ST_Jc;

/// The body-paragraph properties rdocx does not hand back, in twips.
///
/// Two unrelated gaps share one scan of `word/document.xml`: the
/// strict-OOXML indents rdocx never parses, and the tab stops it parses
/// but exposes only as a count (`tab_stop_count`), with no positions.
#[derive(Clone, Default)]
struct RawPara {
    start_twips: Option<f64>,
    end_twips: Option<f64>,
    tab_twips: Vec<f64>,
    /// Direct `w:contextualSpacing` (rdocx does not model it).
    contextual: bool,
}

/// Per body paragraph: strict-spelled indents and tab-stop positions.
///
/// ISO/IEC 29500 strict names the horizontal indents `w:start`/`w:end`
/// rather than the transitional `w:left`/`w:right`, and LibreOffice's
/// "Office Open XML Text" export filter writes the strict spelling — as
/// does anything else targeting that conformance class. rdocx reads only
/// the transitional attributes, so those indents arrive as "absent" and
/// the paragraph reads as unindented. Everything else in `w:ind` is
/// already shared between the two spellings (`w:firstLine`, `w:hanging`),
/// as is `w:jc`'s `start`/`end`, which rdocx does map.
///
/// The vector is positional: one entry per body-level `w:p`, in document
/// order, so the caller can pair it with `doc.paragraphs()`. Paragraphs
/// inside `w:tbl` are skipped because rdocx exposes those separately;
/// a strict indent inside a table cell stays unread.
///
/// Returns the pairs in twips. Any failure to open or scan the part
/// yields an empty vector, which the caller treats as "nothing to add".
fn raw_paragraph_props(path: &str) -> Vec<RawPara> {
    fn scan(path: &str) -> Result<Vec<RawPara>, Box<dyn std::error::Error>> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml")?, &mut xml)?;

        let mut reader = quick_xml::Reader::from_str(&xml);
        reader.config_mut().trim_text(true);
        let mut out: Vec<RawPara> = Vec::new();
        let mut table_depth = 0usize;
        // `w:tab` means two different things: a stop inside `w:tabs`, and
        // a tab character inside a run. Only the former has a position,
        // so the flag is what keeps a tabbed line from inventing stops.
        let mut in_tabs = false;
        loop {
            match reader.read_event()? {
                quick_xml::events::Event::Eof => break,
                quick_xml::events::Event::Start(e) => match e.name().as_ref() {
                    "w:tbl" => table_depth += 1,
                    "w:tabs" => in_tabs = true,
                    "w:p" if table_depth == 0 => out.push(RawPara::default()),
                    _ => {}
                },
                quick_xml::events::Event::End(e) => match e.name().as_ref() {
                    "w:tbl" => table_depth = table_depth.saturating_sub(1),
                    "w:tabs" => in_tabs = false,
                    _ => {}
                },
                quick_xml::events::Event::Empty(e) => match e.name().as_ref() {
                    // A `w:p` with nothing in it is still a paragraph.
                    "w:p" if table_depth == 0 => out.push(RawPara::default()),
                    "w:tab" if in_tabs && table_depth == 0 => {
                        let Some(last) = out.last_mut() else { continue };
                        let mut pos = None;
                        let mut val = None;
                        for a in e.attributes().with_checks(false).flatten() {
                            match a.key.as_ref() {
                                "w:pos" => pos = a.value.trim().parse::<f64>().ok(),
                                "w:val" => val = Some(a.value.trim().to_ascii_lowercase()),
                                _ => {}
                            }
                        }
                        // `w:val` decides whether this is a stop at all.
                        // `clear` *removes* an inherited stop at that
                        // position — LibreOffice writes one to drop the
                        // 2cm default — and `bar`/`num` are a rule and a
                        // list's numbering gap, not user tab stops. An
                        // allowlist keeps each of those from arriving as
                        // a phantom stop the document never had.
                        let keep = matches!(
                            val.as_deref().unwrap_or("left"),
                            "left" | "start" | "center" | "centre" | "right" | "end" | "decimal"
                        );
                        if let (Some(v), true) = (pos, keep) {
                            last.tab_twips.push(v);
                        }
                    }
                    "w:contextualSpacing" if table_depth == 0 => {
                        if let Some(last) = out.last_mut() {
                            last.contextual = on_off(&e);
                        }
                    }
                    "w:ind" if table_depth == 0 => {
                        let Some(last) = out.last_mut() else { continue };
                        for a in e.attributes().with_checks(false).flatten() {
                            let v = || {
                                a.value.trim().parse::<f64>().ok()
                            };
                            match a.key.as_ref() {
                                "w:start" => last.start_twips = v(),
                                "w:end" => last.end_twips = v(),
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        Ok(out)
    }
    scan(path).unwrap_or_default()
}

/// An OOXML on/off element's value: present means on, unless `w:val`
/// says "0", "false" or "off".
fn on_off(e: &quick_xml::events::BytesStart<'_>) -> bool {
    !e.attributes().with_checks(false).flatten().any(|a| {
        a.key.as_ref() == "w:val" && matches!(a.value.trim(), "0" | "false" | "off")
    })
}

/// Paragraph styles whose paragraphs have `w:contextualSpacing` ("don't add
/// space between paragraphs of the same style"), through `w:basedOn`.
/// Word's list styles set it: without it every list item of a python-docx
/// or Word list would be 10pt apart.
fn contextual_styles(path: &str) -> std::collections::HashSet<String> {
    /// (style id, basedOn, contextualSpacing if the style itself says)
    type StyleSpacing = (String, Option<String>, Option<bool>);
    fn scan(path: &str) -> Result<Vec<StyleSpacing>, Box<dyn std::error::Error>> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/styles.xml")?, &mut xml)?;
        let mut reader = quick_xml::Reader::from_str(&xml);
        reader.config_mut().trim_text(true);
        let mut out: Vec<StyleSpacing> = Vec::new();
        let attr = |e: &quick_xml::events::BytesStart<'_>, key: &str| {
            e.attributes().with_checks(false).flatten().find(|a| a.key.as_ref() == key).map(|a| a.value.to_string())
        };
        loop {
            match reader.read_event()? {
                quick_xml::events::Event::Eof => break,
                quick_xml::events::Event::Start(e) if e.name().as_ref() == "w:style" => {
                    out.push((attr(&e, "w:styleId").unwrap_or_default(), None, None));
                }
                quick_xml::events::Event::Empty(e) => match e.name().as_ref() {
                    "w:basedOn" => {
                        if let Some(last) = out.last_mut() {
                            last.1 = attr(&e, "w:val");
                        }
                    }
                    "w:contextualSpacing" => {
                        if let Some(last) = out.last_mut() {
                            last.2 = Some(on_off(&e));
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        Ok(out)
    }
    let styles = scan(path).unwrap_or_default();
    let by_id: std::collections::HashMap<&str, &StyleSpacing> =
        styles.iter().map(|s| (s.0.as_str(), s)).collect();
    styles
        .iter()
        .filter(|s| {
            // Walk basedOn to the first style that decides; bounded, as a
            // hostile file may make the chain a cycle.
            let mut cur = Some(*s);
            for _ in 0..32 {
                let Some(style) = cur else { return false };
                if let Some(on) = style.2 {
                    return on;
                }
                cur = style.1.as_deref().and_then(|b| by_id.get(b).copied());
            }
            false
        })
        .map(|s| s.0.clone())
        .collect()
}

/// Read a .docx file into a Document.
pub fn read(path: &str) -> Result<Document, String> {
    let doc = rdocx::Document::open(path)
        .map_err(|e| format!("Cannot open .docx {}: {}", path, e))?;
    THEME.with(|t| *t.borrow_mut() = theme_fonts(path));
    STYLE_FONTS.with(|s| *s.borrow_mut() = style_fonts(path));

    // Paragraph properties rdocx cannot hand back: strict-spelled indents
    // and tab-stop positions. The scan is positional, so it is only
    // trusted when it found exactly as many body paragraphs as rdocx did;
    // otherwise the two disagree about what a paragraph is and pairing
    // them would misattribute a property to its neighbour.
    let body = doc.paragraphs();
    let raw = raw_paragraph_props(path);
    let raw = (raw.len() == body.len()).then_some(raw);

    let mut paragraphs = Vec::new();
    // Per kept body paragraph: its style id and whether it has contextual
    // spacing, for the pass after this loop.
    let mut contextual: Vec<(Option<String>, bool)> = Vec::new();
    let contextual_ids = contextual_styles(path);
    // Set when a paragraph ends with a run-level page break, and consumed
    // by the next paragraph this loop keeps.
    let mut carried_break = false;
    for (i, p) in body.iter().enumerate() {
        let mut pending_break = false;
        // Decorative rules (LibreOffice's HorizontalLine style) carry no text.
        if p.style_id() == Some("HorizontalLine") && p.text().is_empty() {
            continue;
        }
        let mut para = map_paragraph(&doc, p);
        // A run-level break after this paragraph's text belongs to the
        // paragraph that follows, which is where LibreOffice puts the
        // break it converts from an ODF `fo:break-before`. The trailing
        // newline it leaves behind is the break itself, not content: this
        // model's paragraphs never contain one.
        let breaks = run_page_break(p);
        if breaks.trailing {
            if let Some(last) = para.runs.last_mut() {
                if last.text.ends_with('\n') {
                    last.text.pop();
                }
            }
            para.runs.retain(|r| {
                !r.text.is_empty() || r.style.image.is_some() || r.style.footnote.is_some()
            });
            pending_break = true;
        }
        if std::mem::take(&mut carried_break) {
            para.style.page_break_before = true;
        }
        // A paragraph holding nothing but a page break (python-docx's
        // `add_run().add_break(WD_BREAK.PAGE)`, Word's Ctrl+Enter on an
        // empty line) is the break, not a line of its own: LibreOffice
        // starts the next paragraph at the top of the new page. Kept, it
        // put an empty line there instead.
        // (rdocx gives the break's run the text "\n".)
        let only_break = para.runs.iter().all(|r| {
            r.style.image.is_none() && r.style.footnote.is_none() && r.text.chars().all(|c| c == '\n')
        });
        if only_break && breaks.leading && !pending_break && i + 1 < body.len() {
            carried_break = true;
            continue;
        }
        let style_id = p.style_id().map(str::to_string);
        let direct_contextual = raw.as_ref().and_then(|s| s.get(i)).is_some_and(|r| r.contextual);
        let is_contextual = direct_contextual || style_id.as_deref().is_some_and(|id| contextual_ids.contains(id));
        contextual.push((style_id, is_contextual));
        if let Some(RawPara { start_twips, end_twips, tab_twips, .. }) =
            raw.as_ref().and_then(|s| s.get(i))
        {
            // Transitional wins where both are present: it is what rdocx
            // read, and a file carrying both is already self-contradictory.
            if para.style.left_indent_pt == 0.0 {
                if let Some(tw) = start_twips { para.style.left_indent_pt = tw / 20.0; }
            }
            if para.style.right_indent_pt == 0.0 {
                if let Some(tw) = end_twips { para.style.right_indent_pt = tw / 20.0; }
            }
            para.style.tab_stops_pt = tab_twips.iter().map(|tw| tw / 20.0).collect();
        }
        paragraphs.push(para);
        carried_break = pending_break;
    }

    // Contextual spacing: no space between two paragraphs of one style
    // when the paragraph asks for it (Word's and LibreOffice's rule).
    for k in 1..contextual.len().min(paragraphs.len()) {
        let same = contextual[k - 1].0 == contextual[k].0;
        if same && contextual[k - 1].1 {
            paragraphs[k - 1].style.space_after_pt = 0.0;
        }
        if same && contextual[k].1 {
            paragraphs[k].style.space_before_pt = 0.0;
        }
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
        let mut d = Document::new();
        d.header = doc.header_text();
        d.footer = doc.footer_text();
        return Ok(d);
    }
    // Footnote texts land in the document list; docx ids remap to
    // zero-based indexes on the referencing runs (see map_paragraph).
    let footnotes: Vec<String> = doc.footnotes().into_iter().map(|(_, t)| t).collect();

    Ok(Document {
        paragraphs,
        footnotes,
        header: doc.header_text(),
        footer: doc.footer_text(),
        page: read_page_geometry(&doc),
        base_font: read_base_font(&doc),
        heading_styles: read_heading_styles(&doc),
    })
}

/// Read a DOCX and retain package members this reader does not interpret.
/// The report is suitable for a structured save-warning UI; the opaque
/// package can be passed to [`write_with_opaque`] after an unrelated edit.
pub fn read_with_report(path: &str) -> Result<(Document, suite_common_core::interop::CompatibilityReport, suite_common_core::interop::OpaquePackage), String> {
    let document = read(path)?;
    let opaque = suite_common_core::interop::OpaquePackage::capture(
        path,
        &["[Content_Types].xml", "_rels/.rels", "word/document.xml", "word/_rels/document.xml.rels"],
    )?;
    let mut report = suite_common_core::interop::CompatibilityReport::new("docx");
    for name in opaque.part_names() {
        report.record(suite_common_core::interop::UnsupportedFeature::new(
            "uninterpreted-package-part", "Uninterpreted package part", name,
            suite_common_core::interop::FeatureDisposition::OpaquePassThrough,
            "will be copied through on an opaque save",
        ));
    }
    Ok((document, report, opaque))
}

/// Page geometry from the docx section properties, when present.
fn read_page_geometry(doc: &rdocx::Document) -> Option<PageGeometry> {
    let sect = doc.section_properties()?;
    // Twips (1/20 pt) → points; the Twips newtype is not re-exported, so
    // work through the tuple field.
    let w = sect.page_width.map(|v| v.0 as f64 / 20.0)?;
    let h = sect.page_height.map(|v| v.0 as f64 / 20.0)?;
    let default = PageGeometry::default();
    Some(PageGeometry {
        width_pt: w,
        height_pt: h,
        margin_top_pt: sect.margin_top.map(|v| v.0 as f64 / 20.0).unwrap_or(default.margin_top_pt),
        margin_bottom_pt: sect.margin_bottom.map(|v| v.0 as f64 / 20.0).unwrap_or(default.margin_bottom_pt),
        margin_left_pt: sect.margin_left.map(|v| v.0 as f64 / 20.0).unwrap_or(default.margin_left_pt),
        margin_right_pt: sect.margin_right.map(|v| v.0 as f64 / 20.0).unwrap_or(default.margin_right_pt),
        // `w:cols` is absent for a single-column section, which is the
        // default this falls back to.
        columns: sect
            .columns
            .as_ref()
            .and_then(|c| c.num)
            .and_then(|n| u8::try_from(n).ok())
            .filter(|n| *n > 0)
            .unwrap_or(default.columns),
        column_gap_pt: sect
            .columns
            .as_ref()
            .and_then(|c| c.space)
            .map(|v| v.0 as f64 / 20.0)
            .unwrap_or(default.column_gap_pt),
    })
}

/// Write a Document to a .docx file.
pub fn write(doc: &Document, path: impl AsRef<std::path::Path>) -> Result<(), String> {
    let mut out = rdocx::Document::new();
    // Footnote texts first: model index → docx id.
    let footnote_ids: Vec<i32> = doc.footnotes.iter().map(|t| out.add_footnote(t)).collect();
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
            ListKind::Bullet => out.add_bullet_list_item("", u32::from(para.style.list_level)),
            ListKind::Numbered => out.add_numbered_list_item("", u32::from(para.style.list_level)),
            ListKind::None => out.add_paragraph(""),
        };
        if let Some(level) = para.style.heading {
            p = p.style(&format!("Heading{}", level.clamp(1, 6)));
        }
        if para.style.block_quote {
            p = p.style("Quote");
        }
        if let Some(name) = &para.style.named_style {
            p = p.style(name);
        }
        if para.style.page_break_before {
            p = p.page_break_before(true);
        }
        if (para.style.line_spacing - 1.0).abs() > 0.01 {
            p = p.line_spacing_multiple(para.style.line_spacing as f64);
        }
        // Paragraph indents and spacing. The odt writer has carried these
        // since it was written; this one never emitted them at all, so a
        // document with an indented or spaced paragraph lost that on every
        // .docx save while keeping it on .odt. rdocx has had the builders
        // the whole time — they were simply never called.
        //
        // Only non-zero values are written: OOXML treats an absent `w:ind`
        // or `w:spacing` as "inherit from the style", and writing an
        // explicit zero is a different claim, one that overrides a style's
        // own indent with nothing.
        if para.style.left_indent_pt != 0.0 {
            p = p.indent_left(rdocx::Length::pt(para.style.left_indent_pt));
        }
        if para.style.right_indent_pt != 0.0 {
            p = p.indent_right(rdocx::Length::pt(para.style.right_indent_pt));
        }
        if para.style.first_line_indent_pt != 0.0 {
            p = p.first_line_indent(rdocx::Length::pt(para.style.first_line_indent_pt));
        }
        if para.style.space_before_pt != 0.0 {
            p = p.space_before(rdocx::Length::pt(para.style.space_before_pt));
        }
        if para.style.space_after_pt != 0.0 {
            p = p.space_after(rdocx::Length::pt(para.style.space_after_pt));
        }
        // Tab stops, which neither writer persisted: a paragraph's stops
        // were lost on every save in both formats, self round trip
        // included. The model has positions only, so every stop is a
        // left-aligned one.
        for pos in &para.style.tab_stops_pt {
            p = p.add_tab_stop(rdocx::TabAlignment::Left, rdocx::Length::pt(*pos));
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
                        // The size it was shown at; every picture used to
                        // be saved 4in x 3in whatever its size.
                        let (w, h) = match run.style.image_extent_emu {
                            Some((w, h)) => (rdocx::Length::emu(w as i64), rdocx::Length::emu(h as i64)),
                            None => (rdocx::Length::inches(4.0), rdocx::Length::inches(3.0)),
                        };
                        let mut pic = out.add_picture(&bytes, &name, w, h);
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
            if let Some(idx) = run.style.footnote {
                if let Some(&fid) = footnote_ids.get(idx) {
                    let mut p = out.last_paragraph_mut().expect("paragraph");
                    p.add_footnote_ref(fid);
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
            if let Some(f) = &run.style.font_family { r = r.font(f); }
            if let Some(hp) = run.style.font_size_hp { r = r.size(hp as f64 / 2.0); }
            if let Some(c) = &run.style.color { r = r.color(c); }
            match run.style.vert_align {
                Some(crate::model::VertAlign::Superscript) => { r = r.superscript(); }
                Some(crate::model::VertAlign::Subscript) => { r = r.subscript(); }
                None => {}
            }
        }
    }
    if let Some(h) = &doc.header {
        out.set_header(h);
    }
    if let Some(f) = &doc.footer {
        out.set_footer(f);
    }
    if let Some(pg) = &doc.page {
        out.set_page_size(rdocx::Length::pt(pg.width_pt), rdocx::Length::pt(pg.height_pt));
        out.set_margins(
            rdocx::Length::pt(pg.margin_top_pt),
            rdocx::Length::pt(pg.margin_right_pt),
            rdocx::Length::pt(pg.margin_bottom_pt),
            rdocx::Length::pt(pg.margin_left_pt),
        );
        // A single column is the default section layout, and `w:cols`
        // with `w:num="1"` is noise; more than one has to be written or a
        // two-column document saves as one.
        if pg.columns > 1 {
            out.set_columns(pg.columns as u32, rdocx::Length::pt(pg.column_gap_pt));
        }
    }
    let bytes = out
        .to_bytes()
        .map_err(|e| format!("Cannot save {}: {}", path.as_ref().display(), e))?;
    let bytes = with_letters_styles(&bytes, &doc.base_font, &doc.heading_styles)
        .map_err(|e| format!("Cannot save {}: {}", path.as_ref().display(), e))?;
    suite_common_core::atomic_save::atomic_write_bytes(path.as_ref(), &bytes)
}

/// The styles a document written by Letters must carry so that Word and
/// LibreOffice show it as Letters does.
///
/// rdocx's template says body text is Calibri 11pt with 8pt after each
/// paragraph at 1.08 line spacing, and defines only Heading 1 (16pt blue)
/// — Heading 2 to 6 fell back to body text. Letters draws none of that,
/// so every saved document looked different elsewhere, and, now that the
/// reader resolves styles, would look different in Letters after a reload.
/// This writes the document's base font with no paragraph spacing, and
/// Heading 1–6 as Letters draws them: bold, `layout::heading_scale` times
/// the body size.
fn with_letters_styles(package: &[u8], base: &crate::model::BaseFont, heading_styles: &[RunStyle]) -> Result<Vec<u8>, String> {
    let family = base.family.clone().unwrap_or_else(|| crate::layout::LayoutOptions::default().font_family);
    let size_hp = base.size_hp.unwrap_or((crate::layout::LayoutOptions::default().font_size_pt * 2.0) as u16);
    let family = xml_escape(&family);
    let defaults = format!(
        "<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"{family}\" w:hAnsi=\"{family}\" \
         w:eastAsia=\"{family}\" w:cs=\"{family}\"/><w:sz w:val=\"{size_hp}\"/><w:szCs w:val=\"{size_hp}\"/>\
         </w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"240\" \
         w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>"
    );
    let headings: String = (1u8..=6)
        .map(|n| {
            // The document's own heading look if it has one, else Letters'.
            let rpr = match heading_styles.get(usize::from(n) - 1) {
                Some(h) => {
                    let mut r = String::new();
                    if let Some(f) = &h.font_family {
                        let f = xml_escape(f);
                        r.push_str(&format!("<w:rFonts w:ascii=\"{f}\" w:hAnsi=\"{f}\" w:cs=\"{f}\"/>"));
                    }
                    if h.bold {
                        r.push_str("<w:b/><w:bCs/>");
                    }
                    if h.italic {
                        r.push_str("<w:i/><w:iCs/>");
                    }
                    if let Some(c) = &h.color {
                        r.push_str(&format!("<w:color w:val=\"{}\"/>", xml_escape(c)));
                    }
                    let hp = h.font_size_hp.map_or(u32::from(size_hp), u32::from);
                    r.push_str(&format!("<w:sz w:val=\"{hp}\"/><w:szCs w:val=\"{hp}\"/>"));
                    r
                }
                None => {
                    let hp = (f64::from(size_hp) * crate::layout::heading_scale(n)).round() as u32;
                    format!("<w:b/><w:bCs/><w:sz w:val=\"{hp}\"/><w:szCs w:val=\"{hp}\"/>")
                }
            };
            format!(
                "<w:style w:type=\"paragraph\" w:styleId=\"Heading{n}\"><w:name w:val=\"heading {n}\"/>\
                 <w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/>\
                 <w:outlineLvl w:val=\"{lvl}\"/></w:pPr><w:rPr>{rpr}</w:rPr></w:style>",
                lvl = n - 1
            )
        })
        .collect();

    let mut zin = zip::ZipArchive::new(std::io::Cursor::new(package)).map_err(|e| e.to_string())?;
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for i in 0..zin.len() {
        let mut f = zin.by_index(i).map_err(|e| e.to_string())?;
        let name = f.name().to_string();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut data).map_err(|e| e.to_string())?;
        if name == "word/styles.xml" {
            let xml = String::from_utf8(data).map_err(|e| e.to_string())?;
            data = patch_styles_xml(&xml, &defaults, &headings).into_bytes();
        }
        out.start_file(name, options).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut out, &data).map_err(|e| e.to_string())?;
    }
    Ok(out.finish().map_err(|e| e.to_string())?.into_inner())
}

/// Replace `styles.xml`'s docDefaults and its Heading1–6 styles.
fn patch_styles_xml(xml: &str, defaults: &str, headings: &str) -> String {
    let mut xml = xml.to_string();
    if let (Some(a), Some(b)) = (xml.find("<w:docDefaults"), xml.find("</w:docDefaults>")) {
        xml.replace_range(a..b + "</w:docDefaults>".len(), defaults);
    } else if let Some(at) = xml.find("<w:style ") {
        xml.insert_str(at, defaults);
    }
    for n in 1..=6 {
        let id = format!("w:styleId=\"Heading{n}\"");
        while let Some(at) = xml.find(&id) {
            let Some(start) = xml[..at].rfind("<w:style ") else { break };
            let Some(len) = xml[start..].find("</w:style>") else { break };
            xml.replace_range(start..start + len + "</w:style>".len(), "");
        }
    }
    if let Some(end) = xml.rfind("</w:styles>") {
        xml.insert_str(end, headings);
    }
    xml
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Write a DOCX and append previously captured, non-conflicting package parts.
pub fn write_with_opaque(
    doc: &Document,
    path: impl AsRef<std::path::Path>,
    opaque: &suite_common_core::interop::OpaquePackage,
) -> Result<(), String> {
    let path = path.as_ref();
    write(doc, path)?;
    opaque.append_to(path)
}

/// Map one rdocx paragraph (body or table cell) into a model paragraph.
/// Where a run-level page break sits in a paragraph, if there is one.
///
/// OOXML expresses a page break two ways: `w:pageBreakBefore` in the
/// paragraph properties, and a `<w:br w:type="page"/>` inside a run.
/// LibreOffice writes the second when it converts an ODF
/// `fo:break-before="page"` — as the *last* run of the paragraph
/// *before* the break, which is the same thing as this model's
/// `page_break_before` on the paragraph that follows.
///
/// A break before any text in its own paragraph means the same flag on
/// that paragraph, which is how Word writes a break the user inserted at
/// the start of a line. A break with text on both sides of it splits one
/// paragraph across pages, which this model cannot express and which
/// this therefore ignores rather than misreport as a paragraph break.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
struct RunPageBreak {
    /// Before any text in this paragraph.
    leading: bool,
    /// After all of this paragraph's text, so the break belongs to the
    /// next paragraph.
    trailing: bool,
}

fn run_page_break(p: &rdocx::ParagraphRef<'_>) -> RunPageBreak {
    let mut seen_text = false;
    let mut break_after_text = false;
    let mut leading = false;
    for r in p.runs() {
        for item in r.items() {
            match item {
                rdocx::RunItemRef::Break(rdocx::BreakKind::Page) => {
                    if seen_text {
                        break_after_text = true;
                    } else {
                        leading = true;
                    }
                }
                rdocx::RunItemRef::Text(t) if !t.is_empty() => {
                    seen_text = true;
                    // Text after a break means it was not trailing after all.
                    break_after_text = false;
                }
                _ => {}
            }
        }
    }
    RunPageBreak { leading, trailing: break_after_text }
}

/// A paragraph's list numbering as `(num_id, level)`, wherever it is set.
///
/// Word's built-in list styles ("List Bullet", "List Number 2", …) carry
/// their `w:numPr` on the *style*, not the paragraph: python-docx, Word's
/// own style gallery and many templates write list items that way. Reading
/// only the paragraph's direct `w:numPr` turned every such list into plain
/// paragraphs with no marker at all.
///
/// The level is the paragraph's own `w:ilvl` when it has one. Otherwise a
/// numbered built-in style names its level ("List Bullet 3" is the third
/// level: Word gives each its own `numId` at `ilvl` 0, indented one step
/// further), and failing that the style's inherited `w:ilvl`. `numId` 0
/// means "no numbering" and switches an inherited list off.
fn paragraph_numbering(doc: &rdocx::Document, p: &rdocx::ParagraphRef<'_>) -> Option<(u32, u32)> {
    if let Some((num_id, level)) = p.numbering() {
        return (num_id != 0).then_some((num_id, level));
    }
    let style_id = p.style_id()?;
    let resolved = doc.resolve_paragraph_properties(Some(style_id));
    let num_id = resolved.num_id.filter(|&id| id != 0)?;
    let level = doc
        .style(style_id)
        .and_then(|s| s.name().and_then(builtin_list_style_level))
        .or(resolved.num_ilvl)
        .unwrap_or(0);
    Some((num_id, level))
}

/// The level a Word built-in list style name implies: "List Bullet" and
/// "List Number" are level 0, "List Bullet 2" … "List Bullet 5" levels 1–4.
fn builtin_list_style_level(name: &str) -> Option<u32> {
    let rest = name
        .strip_prefix("List Bullet")
        .or_else(|| name.strip_prefix("List Number"))?
        .trim();
    if rest.is_empty() {
        return Some(0);
    }
    rest.parse::<u32>().ok().filter(|n| (2..=9).contains(n)).map(|n| n - 1)
}

fn map_paragraph(doc: &rdocx::Document, p: &rdocx::ParagraphRef<'_>) -> Paragraph {
    let heading = p.style_id().and_then(style_id_to_heading);
    // LO uses "Quotations"; Word uses "Quote"/"IntenseQuote".
    let block_quote = matches!(p.style_id(), Some("Quote") | Some("Quotations") | Some("IntenseQuote") | Some("BlockQuote") | Some("BlockQuotation"));
    // LibreOffice emits PreformattedText for <pre>/code blocks.
    let code_block = matches!(p.style_id(), Some("PreformattedText") | Some("HTMLPreformatted"))
        .then(String::new);
    // What the paragraph's style chain (docDefaults, basedOn, its style)
    // says, for everything the paragraph does not set itself. Numbering
    // indents are left out: the model's list level carries them.
    let styled = doc.resolve_paragraph_properties(p.style_id());
    let alignment = match p.alignment() {
        Some(rdocx::Alignment::Center) => Alignment::Center,
        Some(rdocx::Alignment::Right) => Alignment::Right,
        Some(rdocx::Alignment::Justify) => Alignment::Justify,
        Some(_) => Alignment::Left,
        None => match styled.jc {
            Some(ST_Jc::Center) => Alignment::Center,
            Some(ST_Jc::Right | ST_Jc::End) => Alignment::Right,
            Some(ST_Jc::Both | ST_Jc::Distribute) => Alignment::Justify,
            _ => Alignment::Left,
        },
    };
    let named_style = match p.style_id() {
        Some(id @ ("Title" | "Subtitle")) => Some(id.to_string()),
        _ => None,
    };
    // Either spelling counts: the paragraph property, or a run-level
    // break before this paragraph's own text.
    let page_break_before = p.is_page_break_before() || run_page_break(p).leading;
    let (list, list_level) = match paragraph_numbering(doc, p) {
        Some((num_id, level)) => (match doc.numbering_is_bullet(num_id) {
            Some(false) => ListKind::Numbered,
            // Unknown num_id defaults to bullet — the safer visual guess.
            _ => ListKind::Bullet,
        }, level.min(8) as u8),
        None => (ListKind::None, 0),
    };

    // Per-run link URLs from hyperlink spans (indexes into the runs vec).
    let spans = p.hyperlink_spans();
    let link_for = |idx: usize| -> Option<String> {
        spans.iter()
            .find(|(start, end, _)| idx >= *start && idx < *end)
            .and_then(|(_, _, rel_id)| rel_id.as_deref().and_then(|id| doc.hyperlink_url(id)))
    };

    let base = read_base_font(doc);
    let mut runs = Vec::new();
    for (idx, r) in p.runs().enumerate() {
        // Inline images: extract bytes to a cache file so the model's
        // image path is always locally readable.
        if let Some((rel_id, alt)) = r.inline_image() {
            if let Some(bytes) = doc.image_data(rel_id) {
                // gh-268: the previous code wrote to a predictable
                // /tmp/letters-images/<content-hash>.png via fs::write, which
                // follows a pre-created symlink — a local user could point the
                // write anywhere. NamedTempFile gives O_EXCL + O_NOFOLLOW + an
                // unpredictable name; keep the file alive and hand the model
                // its path (deleted on drop once the model is done).
                let mut tmp = match tempfile::NamedTempFile::new() {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                if std::io::Write::write_all(&mut tmp, &bytes).is_err() {
                    continue;
                }
                let (_, path) = match tmp.keep() {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                // The displayed size is the drawing's extent, not the
                // image's pixels: a 200px picture placed 2in wide is 2in.
                let extent = r.items().into_iter().find_map(|item| match item {
                    rdocx::RunItemRef::Drawing(d) => d.width().zip(d.height()),
                    _ => None,
                });
                runs.push(Run {
                    text: alt.unwrap_or("").to_string(),
                    style: RunStyle {
                        image: Some(path.to_string_lossy().into_owned()),
                        image_extent_emu: extent
                            .map(|(w, h)| (w.to_emu().max(0) as u64, h.to_emu().max(0) as u64))
                            .filter(|(w, h)| *w > 0 && *h > 0),
                        ..Default::default()
                    },
                });
                continue;
            }
        }
        if let Some(fid) = r.footnote_id() {
            if let Some(idx) = doc.footnotes().iter().position(|(id, _)| *id == fid) {
                runs.push(Run {
                    text: String::new(),
                    style: RunStyle { footnote: Some(idx), ..Default::default() },
                });
            }
            continue;
        }
        let text = r.text();
        if text.is_empty() { continue; }
        // A style's font, size and colour (a heading style's 16pt blue)
        // are the run's too; only what differs from the body font is
        // recorded, so an ordinary run stays unstyled.
        let eff = if heading.is_some() {
            // A heading's look is its level (layout::heading_scale); the
            // file's heading style is not copied onto every run.
            rdocx_oxml::properties::CT_RPr::default()
        } else {
            doc.effective_run_properties(p, &r)
        };
        let family = r.font_name().map(|f| f.to_string()).or_else(|| {
            heading.is_none().then(|| style_family(p.style_id())).flatten().filter(|f| Some(f) != base.family.as_ref())
        });
        let size_hp = r
            .size()
            .map(|pt| (pt * 2.0).round() as u16)
            .or_else(|| eff.sz.map(|s| s.0.min(u32::from(u16::MAX)) as u16).filter(|hp| Some(*hp) != base.size_hp));
        let color = r
            .color()
            .map(|c| c.trim_start_matches('#').to_uppercase())
            .or_else(|| eff.color.as_deref().filter(|c| !c.eq_ignore_ascii_case("auto")).map(|c| c.to_uppercase()));
        runs.push(Run {
            text,
            style: RunStyle {
                bold: r.is_bold() || eff.bold == Some(true),
                italic: r.is_italic() || eff.italic == Some(true),
                underline: r.is_underline(),
                strikethrough: r.is_strike(),
                highlight: r.highlight().is_some(),
                code: r.style_id() == Some("SourceText"),
                link: link_for(idx),
                image: None,
                image_extent_emu: None,
                footnote: None,
                html: false,
                chip: None,
                font_family: family,
                font_size_hp: size_hp,
                color,
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
        style: ParaStyle {
            heading, alignment, list, code_block, block_quote,
            list_level,
            named_style, page_break_before,
            // Absent means "inherit": the paragraph looks the way its style
            // chain says, which the model has no styles to express, so the
            // inherited value is read into the paragraph. Reading only
            // direct values drew every paragraph of a python-docx or Word
            // document without its style's 10pt spacing and 1.15 lines.
            line_spacing: p
                .line_spacing_multiple()
                .or_else(|| styled_line_multiple(&styled))
                .map(|m| m as f32)
                .unwrap_or(1.0),
            left_indent_pt: p
                .indent_left()
                .map(|l| l.to_pt())
                .or_else(|| styled.ind_left.or(styled.ind_start).map(twips_pt))
                .unwrap_or(0.0),
            right_indent_pt: p
                .indent_right()
                .map(|l| l.to_pt())
                .or_else(|| styled.ind_right.or(styled.ind_end).map(twips_pt))
                .unwrap_or(0.0),
            first_line_indent_pt: p
                .first_line_indent()
                .map(|l| l.to_pt())
                .or_else(|| {
                    styled.ind_first_line.map(twips_pt).or_else(|| styled.ind_hanging.map(|h| -twips_pt(h)))
                })
                .unwrap_or(0.0),
            space_before_pt: p.space_before().map(|l| l.to_pt()).or_else(|| styled.space_before.map(twips_pt)).unwrap_or(0.0),
            space_after_pt: p.space_after().map(|l| l.to_pt()).or_else(|| styled.space_after.map(twips_pt)).unwrap_or(0.0),
            ..Default::default()
        },
        runs,
    };
    normalize(&mut para);
    para
}

fn twips_pt(t: rdocx_oxml::Twips) -> f64 {
    f64::from(t.0) / 20.0
}

/// A style's "auto" line spacing as a multiple of single (240 = 1.0).
/// Exact and at-least spacing have no multiple and read as unset.
fn styled_line_multiple(ppr: &rdocx_oxml::properties::CT_PPr) -> Option<f64> {
    let line = ppr.line_spacing?;
    match ppr.line_rule.as_deref() {
        None | Some("auto") if line.0 > 0 => Some(f64::from(line.0) / 240.0),
        _ => None,
    }
}

/// The theme's major (headings) and minor (body) Latin fonts, from
/// `word/theme/theme1.xml`; Office's current defaults when the file has no
/// theme or names none.
#[derive(Clone, Debug)]
struct ThemeFonts {
    major: String,
    minor: String,
}

fn theme_fonts(path: &str) -> ThemeFonts {
    let read = || -> Option<String> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/theme/theme1.xml").ok()?, &mut xml).ok()?;
        Some(xml)
    };
    let xml = read().unwrap_or_default();
    let face = |which: &str| {
        let at = xml.find(&format!("<a:{which}>"))?;
        let latin = xml[at..].find("<a:latin ")? + at;
        let tf = xml[latin..].find("typeface=\"")? + latin + "typeface=\"".len();
        let end = xml[tf..].find('"')? + tf;
        Some(xml[tf..end].to_string()).filter(|f| !f.is_empty())
    };
    ThemeFonts {
        major: face("majorFont").unwrap_or_else(|| "Calibri Light".into()),
        minor: face("minorFont").unwrap_or_else(|| "Calibri".into()),
    }
}

thread_local! {
    /// The theme of the document being read (set by `read`).
    static THEME: std::cell::RefCell<ThemeFonts> =
        std::cell::RefCell::new(ThemeFonts { major: "Calibri Light".into(), minor: "Calibri".into() });
}

/// One `w:rFonts`: a theme font (which wins over an explicit face on the
/// same element, ISO/IEC 29500 §17.3.2.26) or a named face.
#[derive(Clone, Debug, PartialEq)]
enum FontRef {
    Major,
    Minor,
    Named(String),
}

/// The fonts styles.xml names, for resolving a style's font family through
/// its `w:basedOn` chain the way Word does: the most derived style that
/// names a font decides, whether by name or by theme. rdocx's merged
/// properties keep both a base style's explicit face and a derived style's
/// theme font, and cannot say which came from where (Heading 1's theme
/// "major" font lost to Normal's "Liberation Serif").
#[derive(Default)]
struct StyleFonts {
    /// style id -> (basedOn, its own run font)
    styles: std::collections::HashMap<String, (Option<String>, Option<FontRef>)>,
    default_para: Option<String>,
    doc_default: Option<FontRef>,
}

fn style_fonts(path: &str) -> StyleFonts {
    fn scan(path: &str) -> Result<StyleFonts, Box<dyn std::error::Error>> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/styles.xml")?, &mut xml)?;
        let mut reader = quick_xml::Reader::from_str(&xml);
        reader.config_mut().trim_text(true);
        let mut out = StyleFonts::default();
        let attr = |e: &quick_xml::events::BytesStart<'_>, key: &str| {
            e.attributes().with_checks(false).flatten().find(|a| a.key.as_ref() == key).map(|a| a.value.to_string())
        };
        // Where we are: inside a style (its id), inside docDefaults'
        // rPrDefault, and inside a pPr (whose rPr is the paragraph mark's).
        let (mut style, mut in_defaults, mut in_ppr) = (None::<String>, false, false);
        loop {
            let ev = reader.read_event()?;
            let (e, empty) = match &ev {
                quick_xml::events::Event::Eof => break,
                quick_xml::events::Event::Start(e) => (e.clone(), false),
                quick_xml::events::Event::Empty(e) => (e.clone(), true),
                quick_xml::events::Event::End(e) => {
                    match e.name().as_ref() {
                        "w:style" => style = None,
                        "w:rPrDefault" => in_defaults = false,
                        "w:pPr" => in_ppr = false,
                        _ => {}
                    }
                    continue;
                }
                _ => continue,
            };
            match e.name().as_ref() {
                "w:style" if !empty => {
                    let id = attr(&e, "w:styleId").unwrap_or_default();
                    let is_para = attr(&e, "w:type").as_deref() == Some("paragraph");
                    if is_para && matches!(attr(&e, "w:default").as_deref(), Some("1" | "true")) {
                        out.default_para = Some(id.clone());
                    }
                    out.styles.entry(id.clone()).or_default();
                    style = Some(id);
                }
                "w:rPrDefault" if !empty => in_defaults = true,
                "w:pPr" if !empty => in_ppr = true,
                "w:basedOn" => {
                    if let Some(id) = &style {
                        out.styles.entry(id.clone()).or_default().0 = attr(&e, "w:val");
                    }
                }
                "w:rFonts" if !in_ppr => {
                    let font = match attr(&e, "w:asciiTheme").or_else(|| attr(&e, "w:hAnsiTheme")) {
                        Some(t) if t.starts_with("major") => Some(FontRef::Major),
                        Some(t) if t.starts_with("minor") => Some(FontRef::Minor),
                        _ => attr(&e, "w:ascii").or_else(|| attr(&e, "w:hAnsi")).map(FontRef::Named),
                    };
                    if let Some(id) = &style {
                        out.styles.entry(id.clone()).or_default().1 = font;
                    } else if in_defaults {
                        out.doc_default = font;
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }
    scan(path).unwrap_or_default()
}

thread_local! {
    /// The styles' fonts of the document being read (set by `read`).
    static STYLE_FONTS: std::cell::RefCell<StyleFonts> = std::cell::RefCell::new(StyleFonts::default());
}

/// The font family paragraph style `id` (or the default paragraph style)
/// gives its text, resolved through `w:basedOn` and docDefaults.
fn style_family(id: Option<&str>) -> Option<String> {
    STYLE_FONTS.with(|sf| {
        let sf = sf.borrow();
        let mut cur = id.map(str::to_string).or_else(|| sf.default_para.clone());
        let mut found = None;
        for _ in 0..32 {
            let Some(c) = cur else { break };
            let Some((based_on, font)) = sf.styles.get(&c) else { break };
            if font.is_some() {
                found = font.clone();
                break;
            }
            cur = based_on.clone();
        }
        let font = found.or_else(|| sf.doc_default.clone())?;
        THEME.with(|t| {
            let t = t.borrow();
            Some(match font {
                FontRef::Major => t.major.clone(),
                FontRef::Minor => t.minor.clone(),
                FontRef::Named(n) => n,
            })
        })
    })
}

/// How the document's Heading 1–6 styles look, as `RunStyle`s; empty when
/// it defines none (then Letters' own heading look applies).
fn read_heading_styles(doc: &rdocx::Document) -> Vec<RunStyle> {
    if !(1..=6).any(|n| doc.style(&format!("Heading{n}")).is_some()) {
        return Vec::new();
    }
    (1..=6)
        .map(|n| {
            let id = format!("Heading{n}");
            let rpr = doc.resolve_run_properties(Some(&id), None);
            RunStyle {
                bold: rpr.bold == Some(true),
                italic: rpr.italic == Some(true),
                font_family: style_family(Some(&id)),
                font_size_hp: rpr.sz.map(|s| s.0.min(u32::from(u16::MAX)) as u16),
                color: rpr.color.as_deref().filter(|c| !c.eq_ignore_ascii_case("auto")).map(|c| c.to_uppercase()),
                ..Default::default()
            }
        })
        .collect()
}

/// The body font: docDefaults run properties plus the default paragraph
/// style's (python-docx and Word put "Liberation Serif 12pt" or "Calibri
/// 11pt" there, never on the runs).
fn read_base_font(doc: &rdocx::Document) -> crate::model::BaseFont {
    let rpr = doc.resolve_run_properties(None, None);
    crate::model::BaseFont { family: style_family(None), size_hp: rpr.sz.map(|s| s.0.min(u32::from(u16::MAX)) as u16) }
}

fn style_id_to_heading(id: &str) -> Option<u8> {
    let level = id.strip_prefix("Heading")?.parse::<u8>().ok()?;
    (1..=6).contains(&level).then_some(level)
}

fn normalize(p: &mut Paragraph) {
    p.runs
        .retain(|r| !r.text.is_empty() || r.style.image.is_some() || r.style.footnote.is_some());
    let mut i = 0;
    while i + 1 < p.runs.len() {
        if p.runs[i].style.footnote.is_some() || p.runs[i + 1].style.footnote.is_some() {
            i += 1;
            continue;
        }
        if p.runs[i].style == p.runs[i + 1].style {
            let next = p.runs.remove(i + 1);
            p.runs[i].text.push_str(&next.text);
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Paragraph, Run, RunStyle};

    // ── style_id_to_heading ───────────────────────────────────────────────────

    #[test]
    fn style_id_heading_1_to_6_parse() {
        assert_eq!(style_id_to_heading("Heading1"), Some(1));
        assert_eq!(style_id_to_heading("Heading3"), Some(3));
        assert_eq!(style_id_to_heading("Heading6"), Some(6));
    }

    #[test]
    fn style_id_heading_out_of_range_is_none() {
        assert_eq!(style_id_to_heading("Heading0"), None);
        assert_eq!(style_id_to_heading("Heading7"), None);
    }

    #[test]
    fn style_id_heading_non_numeric_or_malformed_is_none() {
        assert_eq!(style_id_to_heading("HeadingX"), None);
        assert_eq!(style_id_to_heading("heading1"), None);
        assert_eq!(style_id_to_heading("Subtitle"), None);
    }

    // ── normalize ─────────────────────────────────────────────────────────────

    fn para_with(runs: Vec<Run>) -> Paragraph {
        Paragraph { style: Default::default(), runs }
    }

    fn run(text: &str, style: RunStyle) -> Run {
        Run { text: text.to_string(), style }
    }

    #[test]
    fn normalize_merges_adjacent_runs_with_same_style() {
        let mut p = para_with(vec![
            run("foo", RunStyle::default()),
            run("bar", RunStyle::default()),
        ]);
        normalize(&mut p);
        assert_eq!(p.runs.len(), 1);
        assert_eq!(p.runs[0].text, "foobar");
    }

    #[test]
    fn normalize_keeps_runs_with_different_styles() {
        let mut p = para_with(vec![
            run("foo", RunStyle { bold: true, ..Default::default() }),
            run("bar", RunStyle::default()),
        ]);
        normalize(&mut p);
        assert_eq!(p.runs.len(), 2);
    }

    #[test]
    fn normalize_drops_empty_runs() {
        let mut p = para_with(vec![
            run("", RunStyle::default()),
            run("kept", RunStyle::default()),
        ]);
        normalize(&mut p);
        assert_eq!(p.runs.len(), 1);
        assert_eq!(p.runs[0].text, "kept");
    }

    #[test]
    fn normalize_does_not_merge_footnote_runs() {
        let mut p = para_with(vec![
            run("a", RunStyle { footnote: Some(0), ..Default::default() }),
            run("b", RunStyle::default()),
        ]);
        normalize(&mut p);
        assert_eq!(p.runs.len(), 2);
    }
}
