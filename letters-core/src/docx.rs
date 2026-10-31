// docx.rs — Document ⇄ DOCX via rdocx.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Model-level DOCX I/O: paragraphs, styled runs (incl. highlight and inline
// code via SourceText), headings, alignment, lists, hyperlinks. Tables are
// flattened (see read()). Fidelity is measured by tests/docx.rs and the
// LO-authored corpus in tests/lo_parity.rs.

use crate::model::{Alignment, BorderStyle, Document, LineHeight, ListKind, ListLabel, NumberFormat, PageBorder, PageGeometry, Paragraph, ParaBorder, ParaBorders, ParaStyle, Run, RunStyle, TabAlign, TabLeader, TabStop};
use rdocx_oxml::shared::ST_Jc;

/// The body-paragraph properties rdocx does not hand back, in twips.
///
/// Several unrelated gaps share one scan of `word/document.xml`: the
/// strict-OOXML indents rdocx never parses, the tab stops it parses but
/// exposes only as a count (`tab_stop_count`), with no positions,
/// alignments or leaders, the line rule (`line_spacing` reports the
/// length but not exact vs atLeast), and the per-side paragraph borders
/// (only the bottom edge has a reader).
#[derive(Clone, Default)]
struct RawPara {
    start_twips: Option<f64>,
    end_twips: Option<f64>,
    tabs: Vec<RawTab>,
    /// Direct `w:contextualSpacing` (rdocx does not model it).
    contextual: bool,
    /// Direct `w:lineRule` of the paragraph's `w:spacing`.
    line_rule: Option<String>,
    /// Direct `w:pBdr` edges: top, left, bottom, right. `None` is no
    /// direct edge; `Some(None)` is a direct edge the model drops
    /// (nil, or an unmodelled look), which still suppresses the style's.
    borders: [Option<Option<RawBorder>>; 4],
}

/// A tab stop as the file spells it: its position in twips, its
/// alignment (`w:val`) and its leader (`w:leader`).
#[derive(Clone, Copy)]
struct RawTab {
    twips: f64,
    align: TabAlign,
    leader: TabLeader,
}

/// One `w:pBdr` edge as the file spells it: a modelled look, an RRGGBB
/// colour and a width in points.
#[derive(Clone)]
struct RawBorder {
    style: BorderStyle,
    color: String,
    width_pt: f64,
    space_pt: f64,
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
/// Both vectors are positional, in document order. `body` has one entry
/// per body-level `w:p`, so the caller can pair it with
/// `doc.paragraphs()`. `cells` has one per `w:p` directly inside a
/// top-level table's cells, in the order rdocx's tables → rows → cells →
/// paragraphs walk visits them (#1204). Paragraphs in a table nested inside
/// a cell belong to neither and are skipped.
///
/// Returns the values in twips. Any failure to open or scan the part
/// yields empty vectors, which the caller treats as "nothing to add".
#[derive(Default)]
struct RawParas {
    body: Vec<RawPara>,
    cells: Vec<RawPara>,
    /// Per body-level table, in order: how many body paragraphs precede it.
    tables_at: Vec<usize>,
    /// Every section's properties, in document order: the paragraph-level
    /// ones, then the body's last (`RawSection::para` `None`).
    sections: Vec<RawSection>,
}

/// One `w:sectPr` as the file spells it, in twips: a section's page,
/// columns, break type and default header and footer references.
#[derive(Clone, Default)]
struct RawSection {
    /// The body paragraph whose properties hold it (the section ends with
    /// that paragraph); `None` for the body's last, which ends the
    /// document.
    para: Option<usize>,
    /// `w:pgSz` width and height.
    size: [Option<f64>; 2],
    /// `w:pgMar` top, right, bottom, left.
    margins: [Option<f64>; 4],
    /// `w:pgMar` `w:header` and `w:footer`.
    header_footer: [Option<f64>; 2],
    /// `w:cols` `w:num` and `w:space`. (`w:type`, the break's kind, is not
    /// kept: every break here starts a new page, `loss` names the others.)
    columns: Option<u32>,
    column_gap: Option<f64>,
    /// The `r:id`s of the `default` header and footer references.
    header: Option<String>,
    footer: Option<String>,
}

/// Record a child of a scanned `w:sectPr` (`w:pgSz`, `w:pgMar`, `w:cols`,
/// the header and footer references) into `sect`.
fn raw_section_child(sect: &mut RawSection, e: &quick_xml::events::BytesStart<'_>) {
    let number = |v: &str| v.trim().parse::<f64>().ok();
    let qname = e.name();
    let name: &str = qname.as_ref();
    let mut reference_default = false;
    let mut reference_id = None;
    for a in e.attributes().with_checks(false).flatten() {
        let key: &str = a.key.as_ref();
        let value: &str = &a.value;
        match (name, key) {
            ("w:pgSz", "w:w") => sect.size[0] = number(value),
            ("w:pgSz", "w:h") => sect.size[1] = number(value),
            ("w:pgMar", "w:top") => sect.margins[0] = number(value),
            ("w:pgMar", "w:right") => sect.margins[1] = number(value),
            ("w:pgMar", "w:bottom") => sect.margins[2] = number(value),
            ("w:pgMar", "w:left") => sect.margins[3] = number(value),
            ("w:pgMar", "w:header") => sect.header_footer[0] = number(value),
            ("w:pgMar", "w:footer") => sect.header_footer[1] = number(value),
            ("w:cols", "w:num") => sect.columns = value.trim().parse::<u32>().ok(),
            ("w:cols", "w:space") => sect.column_gap = number(value),
            ("w:headerReference" | "w:footerReference", "w:type") => reference_default = value.trim() == "default",
            ("w:headerReference" | "w:footerReference", "r:id") => reference_id = Some(value.trim().to_string()),
            _ => {}
        }
    }
    if reference_default {
        match name {
            "w:headerReference" => sect.header = reference_id,
            "w:footerReference" => sect.footer = reference_id,
            _ => {}
        }
    }
}

fn raw_paragraph_props(path: &str) -> RawParas {
    fn scan(path: &str) -> Result<RawParas, Box<dyn std::error::Error>> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml")?, &mut xml)?;

        let mut reader = quick_xml::Reader::from_str(&xml);
        reader.config_mut().trim_text(true);
        let mut out = RawParas::default();
        let mut table_depth = 0usize;
        // The paragraph list the current depth feeds, if any.
        fn at(out: &mut RawParas, depth: usize) -> Option<&mut Vec<RawPara>> {
            match depth {
                0 => Some(&mut out.body),
                1 => Some(&mut out.cells),
                _ => None,
            }
        }
        // `w:tab` means two different things: a stop inside `w:tabs`, and
        // a tab character inside a run. Only the former has a position,
        // so the flag is what keeps a tabbed line from inventing stops.
        let mut in_tabs = false;
        // The paragraph whose properties the scan is inside, if the scan
        // pushed one for it: `w:p` inside a text box (`w:txbxContent`)
        // is not a body paragraph, so it pushes none and sets none of
        // these. Without the box guard a text box's paragraphs threw the
        // positional pairing off and the whole scan was discarded.
        let mut txbx_depth = 0usize;
        let mut in_para = false;
        // Inside the paragraph's own `w:pPr` (depth 1; a `w:pPrChange`
        // holds the old value, not the paragraph's), and outside any
        // `w:rPr` (a run's `w:spacing` is letter spacing, and a paragraph
        // mark's sits in the `w:pPr`'s own `w:rPr`).
        let mut ppr_depth = 0u32;
        let mut rpr_depth = 0u32;
        let mut in_pbdr = false;
        // The paragraph list the current paragraph feeds, if any: what
        // the scanned properties below append to.
        let mut list: Option<usize> = None;
        fn last_mut(out: &mut RawParas, list: Option<usize>) -> Option<&mut RawPara> {
            match list {
                Some(0) => out.body.last_mut(),
                Some(1) => out.cells.last_mut(),
                _ => None,
            }
        }
        // Inside a section's properties the scan records (`in_sect`), and
        // inside its `w:sectPrChange` (the old value, not the section's).
        let mut in_sect = false;
        let mut sect_change = 0usize;
        // Where a `w:sectPr` starting now belongs: a body paragraph's own
        // properties (the paragraph's index), the body's last (`Some(None)`)
        // or neither (a table's or text box's paragraph, or a change's).
        fn sect_owner(out: &RawParas, in_para: bool, ppr_depth: u32, table_depth: usize, txbx_depth: usize, list: Option<usize>, sect_change: usize) -> Option<Option<usize>> {
            if table_depth > 0 || txbx_depth > 0 || sect_change > 0 {
                None
            } else if !in_para {
                Some(None)
            } else if ppr_depth == 1 && list == Some(0) {
                Some(Some(out.body.len().checked_sub(1)?))
            } else {
                None
            }
        }
        loop {
            match reader.read_event()? {
                quick_xml::events::Event::Eof => break,
                // A recorded section's children: its page, columns and
                // references. Nothing else the scan reads sits in there.
                quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e)
                    if in_sect && sect_change == 0 && e.name().as_ref() != "w:sectPrChange" =>
                {
                    if let Some(sect) = out.sections.last_mut() {
                        raw_section_child(sect, &e);
                    }
                }
                quick_xml::events::Event::Start(e) => match e.name().as_ref() {
                    "w:sectPr" => {
                        if let Some(para) = sect_owner(&out, in_para, ppr_depth, table_depth, txbx_depth, list, sect_change) {
                            out.sections.push(RawSection { para, ..Default::default() });
                            in_sect = true;
                        }
                    }
                    "w:sectPrChange" if in_sect => sect_change += 1,
                    "w:tbl" => {
                        if table_depth == 0 {
                            out.tables_at.push(out.body.len());
                        }
                        table_depth += 1;
                    }
                    "w:txbxContent" => txbx_depth += 1,
                    "w:tabs" => in_tabs = true,
                    "w:p" => {
                        // A skipped paragraph (in a text box, or nested
                        // past the tables the scan pairs) clears the
                        // attachment: its properties belong to no entry.
                        if txbx_depth == 0 {
                            if let Some(fed) = at(&mut out, table_depth) {
                                fed.push(RawPara::default());
                                list = Some(table_depth.min(1));
                                in_para = true;
                            } else {
                                list = None;
                            }
                        } else {
                            list = None;
                        }
                    }
                    "w:pPr" if in_para => {
                        ppr_depth += 1;
                    }
                    "w:rPr" => rpr_depth += 1,
                    "w:pBdr" if in_para && ppr_depth == 1 => {
                        in_pbdr = true;
                    }
                    _ => {}
                },
                quick_xml::events::Event::End(e) => match e.name().as_ref() {
                    "w:sectPr" => {
                        if sect_change == 0 {
                            in_sect = false;
                        }
                    }
                    "w:sectPrChange" => sect_change = sect_change.saturating_sub(1),
                    "w:tbl" => table_depth = table_depth.saturating_sub(1),
                    "w:txbxContent" => txbx_depth = txbx_depth.saturating_sub(1),
                    "w:tabs" => in_tabs = false,
                    "w:p" => {
                        if txbx_depth == 0 {
                            in_para = false;
                            ppr_depth = 0;
                            in_pbdr = false;
                        } else {
                            // A text box's paragraph ends: attachment
                            // returns to the outer paragraph's list.
                            list = match table_depth {
                                0 => Some(0),
                                1 => Some(1),
                                _ => None,
                            };
                        }
                    }
                    "w:pPr" => {
                        ppr_depth = ppr_depth.saturating_sub(1);
                        if ppr_depth == 0 {
                            in_pbdr = false;
                        }
                    }
                    "w:rPr" => rpr_depth = rpr_depth.saturating_sub(1),
                    "w:pBdr" => in_pbdr = false,
                    _ => {}
                },
                quick_xml::events::Event::Empty(e) => match e.name().as_ref() {
                    "w:sectPr" => {
                        if let Some(para) = sect_owner(&out, in_para, ppr_depth, table_depth, txbx_depth, list, sect_change) {
                            out.sections.push(RawSection { para, ..Default::default() });
                        }
                    }
                    // A `w:p` with nothing in it is still a paragraph.
                    "w:p" => {
                        if txbx_depth == 0 {
                            if let Some(fed) = at(&mut out, table_depth) {
                                fed.push(RawPara::default());
                            }
                        }
                    }
                    "w:tab" if in_tabs => {
                        let Some(last) = last_mut(&mut out, list) else { continue };
                        let mut pos = None;
                        let mut val = None;
                        let mut leader = None;
                        for a in e.attributes().with_checks(false).flatten() {
                            match a.key.as_ref() {
                                "w:pos" => pos = a.value.trim().parse::<f64>().ok(),
                                "w:val" => val = Some(a.value.trim().to_ascii_lowercase()),
                                "w:leader" => leader = Some(a.value.trim().to_ascii_lowercase()),
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
                        let align = match val.as_deref().unwrap_or("left") {
                            "left" | "start" => Some(TabAlign::Left),
                            "center" | "centre" => Some(TabAlign::Center),
                            "right" | "end" => Some(TabAlign::Right),
                            "decimal" => Some(TabAlign::Decimal),
                            _ => None,
                        };
                        // `heavy` is a thick rule the model has no look
                        // for: it draws as an underscore, and the save
                        // dialog says so (`loss::content_a_save_drops`).
                        let leader = match leader.as_deref().unwrap_or("none") {
                            "none" => Some(TabLeader::None),
                            "dot" => Some(TabLeader::Dot),
                            "hyphen" => Some(TabLeader::Hyphen),
                            "underscore" | "heavy" => Some(TabLeader::Underscore),
                            "middledot" => Some(TabLeader::MiddleDot),
                            _ => None,
                        };
                        if let (Some(v), Some(align), Some(leader)) = (pos, align, leader) {
                            last.tabs.push(RawTab { twips: v, align, leader });
                        }
                    }
                    "w:contextualSpacing" => {
                        if let Some(last) = last_mut(&mut out, list) {
                            last.contextual = on_off(&e);
                        }
                    }
                    "w:ind" => {
                        let Some(last) = last_mut(&mut out, list) else { continue };
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
                    "w:spacing" => {
                        if in_para && ppr_depth == 1 && rpr_depth == 0 {
                            let Some(last) = last_mut(&mut out, list) else { continue };
                            for a in e.attributes().with_checks(false).flatten() {
                                if a.key.as_ref() == "w:lineRule" {
                                    last.line_rule = Some(a.value.trim().to_ascii_lowercase());
                                }
                            }
                        }
                    }
                    "w:top" | "w:left" | "w:bottom" | "w:right" if in_pbdr => {
                        let Some(last) = last_mut(&mut out, list) else { continue };
                        let idx = match e.name().as_ref() {
                            "w:top" => 0,
                            "w:left" => 1,
                            "w:bottom" => 2,
                            _ => 3,
                        };
                        last.borders[idx] = Some(raw_border(&e));
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

/// One `w:pBdr` edge as a modelled border, if its look is one the model
/// keeps. Anything else (`w:val` wave, triple, inset and the rest) reads
/// as no border here, and the save dialog names it (`loss`), never a
/// silent restyle.
fn raw_border(e: &quick_xml::events::BytesStart<'_>) -> Option<RawBorder> {
    let mut val = None;
    let mut sz = None;
    let mut color = None;
    let mut space = None;
    for a in e.attributes().with_checks(false).flatten() {
        match a.key.as_ref() {
            "w:val" => val = Some(a.value.trim().to_ascii_lowercase()),
            "w:sz" => sz = a.value.trim().parse::<f64>().ok(),
            // Points (not eighths), 0..=31.
            "w:space" => space = a.value.trim().parse::<f64>().ok().filter(|v| v.is_finite()),
            "w:color" => color = Some(a.value.trim().to_ascii_uppercase()),
            _ => {}
        }
    }
    let style = match val.as_deref().unwrap_or("single") {
        "none" if space.is_some_and(|s| s > 0.0) => BorderStyle::None,
        "nil" | "none" => return None,
        "single" | "thick" => BorderStyle::Single,
        "double" => BorderStyle::Double,
        "dotted" => BorderStyle::Dotted,
        "dashed" | "dashSmallGap" => BorderStyle::Dashed,
        _ => return None,
    };
    // "auto" is Word's current-text colour, which here is black; anything
    // that is not six hex digits cannot go back into a file, so it reads
    // as black too rather than corrupting the save.
    let color = match color.as_deref() {
        Some(c) if c.len() == 6 && c.bytes().all(|b| b.is_ascii_hexdigit()) => c.to_string(),
        _ => "000000".to_string(),
    };
    Some(RawBorder { style, color, width_pt: sz.unwrap_or(4.0) / 8.0, space_pt: space.unwrap_or(0.0).clamp(0.0, 31.0) })
}

/// Lay the scanned properties over what rdocx read. Transitional wins
/// where both are present: it is what rdocx read, and a file carrying
/// both is already self-contradictory.
fn apply_strict_indents(style: &mut ParaStyle, raw: &RawPara) {
    if style.left_indent_pt == 0.0 {
        if let Some(tw) = raw.start_twips { style.left_indent_pt = tw / 20.0; }
    }
    if style.right_indent_pt == 0.0 {
        if let Some(tw) = raw.end_twips { style.right_indent_pt = tw / 20.0; }
    }
    apply_raw_typography(style, raw);
}

/// Lay the scanned direct typography over what rdocx read: tab stops
/// with their alignments and leaders. rdocx exposes stops as a count
/// only, so the scan is the only source.
fn apply_raw_typography(style: &mut ParaStyle, raw: &RawPara) {
    style.tab_stops_pt = raw
        .tabs
        .iter()
        .map(|t| TabStop { pos_pt: t.twips / 20.0, align: t.align, leader: t.leader })
        .collect();
}

/// A paragraph's borders: its style chain's edges, with the scanned
/// direct edges over them. A direct edge the model drops (nil, or an
/// unmodelled look) still suppresses the style's, as Word draws it.
fn para_borders(raw: Option<&RawPara>, styled: &rdocx_oxml::properties::CT_PPr) -> ParaBorders {
    let base = styled_borders(styled);
    let Some(raw) = raw else { return base };
    let [top, left, bottom, right] = &raw.borders;
    let pick = |direct: &Option<Option<RawBorder>>, fallback: Option<ParaBorder>| {
        direct.as_ref().map(|b| b.as_ref().map(|b| ParaBorder { style: b.style, color: b.color.clone(), width_pt: b.width_pt, space_pt: b.space_pt })).unwrap_or(fallback)
    };
    padding_only_above_and_below(ParaBorders { top: pick(top, base.top), left: pick(left, base.left), bottom: pick(bottom, base.bottom), right: pick(right, base.right) })
}

/// An invisible edge's gap pads a paragraph above and below only: Writer
/// sets no text in from an invisible side edge (measured: a `none` edge
/// with `w:space="31"` on every side moves the text down 31pt, not in).
fn padding_only_above_and_below(mut b: ParaBorders) -> ParaBorders {
    for side in [&mut b.left, &mut b.right] {
        if side.as_ref().is_some_and(|e| e.style == BorderStyle::None) {
            *side = None;
        }
    }
    b
}

/// A style chain's `w:pBdr` edges as modelled borders.
fn styled_borders(styled: &rdocx_oxml::properties::CT_PPr) -> ParaBorders {
    let edge = |b: Option<&rdocx_oxml::borders::CT_BorderEdge>| b.and_then(styled_edge_border);
    match &styled.borders {
        Some(b) => padding_only_above_and_below(ParaBorders {
            top: edge(b.top.as_ref()),
            left: edge(b.left.as_ref()),
            bottom: edge(b.bottom.as_ref()),
            right: edge(b.right.as_ref()),
        }),
        None => ParaBorders::default(),
    }
}

/// One style-chain border edge as a modelled border, if its look is one
/// the model keeps (see `raw_border`).
fn styled_edge_border(e: &rdocx_oxml::borders::CT_BorderEdge) -> Option<ParaBorder> {
    use rdocx_oxml::shared::ST_Border;
    let style = match e.val {
        ST_Border::None if e.space.is_some_and(|s| s > 0) => BorderStyle::None,
        ST_Border::None => return None,
        ST_Border::Single | ST_Border::Thick => BorderStyle::Single,
        ST_Border::Double => BorderStyle::Double,
        ST_Border::Dotted => BorderStyle::Dotted,
        ST_Border::Dashed => BorderStyle::Dashed,
        _ => return None,
    };
    let color = match e.color.as_deref() {
        Some(c) if c.len() == 6 && c.bytes().all(|b| b.is_ascii_hexdigit()) => c.to_uppercase(),
        _ => "000000".to_string(),
    };
    Some(ParaBorder {
        style,
        color,
        width_pt: e.sz.map_or(0.5, |s| f64::from(s) / 8.0),
        space_pt: e.space.map_or(0.0, |s| f64::from(s.min(31))),
    })
}

/// A paragraph's fixed or minimum line height: the direct `w:spacing`
/// length when its rule is exact/atLeast, else the style chain's.
/// The auto rule is already a multiple (`line_spacing_multiple`).
fn para_line_height(
    direct_multiple: Option<f64>,
    direct_len: Option<rdocx::Length>,
    raw_rule: Option<&str>,
    styled: &rdocx_oxml::properties::CT_PPr,
) -> Option<LineHeight> {
    if direct_multiple.is_some() {
        return None;
    }
    if let Some(len) = direct_len {
        let pt = len.to_pt();
        return match raw_rule.or(styled.line_rule.as_deref()) {
            Some("atLeast") | Some("atleast") => Some(LineHeight::Minimum(pt)),
            _ => Some(LineHeight::Exact(pt)),
        };
    }
    let line = styled.line_spacing?;
    let pt = f64::from(line.0) / 20.0;
    match styled.line_rule.as_deref() {
        Some("exact") => Some(LineHeight::Exact(pt)),
        Some("atLeast") | Some("atleast") => Some(LineHeight::Minimum(pt)),
        _ => None,
    }
}

/// A paragraph's shading fill: its own `w:shd`, else its style's.
/// "auto" is no fill; the fill is an RRGGBB claim.
fn para_shading(direct: Option<&str>, styled: &rdocx_oxml::properties::CT_PPr) -> Option<String> {
    let fill = direct
        .filter(|f| !f.eq_ignore_ascii_case("auto") && !f.is_empty())
        .map(str::to_string)
        .or_else(|| {
            styled.shading.as_ref().and_then(|s| s.fill.clone()).filter(|f| !f.eq_ignore_ascii_case("auto") && !f.is_empty())
        })?;
    Some(fill.trim_start_matches('#').to_uppercase())
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
/// How deeply a docx's XML may nest before it is refused unread. rdocx
/// parses a table inside a cell by recursing, at about 100 KB of stack a
/// level in a debug build, and an overflow aborts the app rather than
/// failing the read: 25 nested tables were enough on a 2 MiB thread
/// (#1206). Real documents nest a few dozen elements deep.
pub const MAX_XML_DEPTH: usize = 256;

/// The stack a docx is read on: room for [`MAX_XML_DEPTH`] levels of the
/// deepest recursion with a wide margin. Only the pages used are committed.
const READ_STACK_BYTES: usize = 64 << 20;

/// Read a docx: refused when it nests past [`MAX_XML_DEPTH`], otherwise
/// read on a thread with [`READ_STACK_BYTES`] of stack, whatever thread
/// asked. A panic in the reader reaches the caller as before.
pub fn read(path: &str) -> Result<Document, String> {
    suite_common_core::zip_guard::check_xml_depth(std::path::Path::new(path), MAX_XML_DEPTH)
        .map_err(|e| format!("Cannot open .docx {path}: {e}"))?;
    let owned = path.to_string();
    let reader = std::thread::Builder::new()
        .name("docx-read".into())
        .stack_size(READ_STACK_BYTES)
        .spawn(move || read_on_this_thread(&owned))
        .map_err(|e| format!("Cannot open .docx {path}: {e}"))?;
    reader.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

fn read_on_this_thread(path: &str) -> Result<Document, String> {
    // Smart chips are content controls rdocx does not read runs from:
    // they become sentinel runs first (docx_chips).
    let (doc, (chips, revisions, text_boxes)) = open_with_chips(path).map_err(|e| format!("Cannot open .docx {}: {}", path, e))?;
    let comments = crate::docx_comments::bodies(&doc);
    let mut read = read_opened(path, doc)?;
    // Tracked changes first: a bracket may hold a chip's sentinel.
    crate::docx_revisions::restore(&mut read, &revisions);
    crate::docx_chips::restore(&mut read, &chips);
    crate::docx_text_boxes::restore(&mut read, &text_boxes);
    // Comments last: their markers may sit in a tracked change.
    if !comments.is_empty() {
        crate::docx_comments::restore(&mut read, comments);
    }
    crate::docx_toc::restore(&mut read);
    // Page-number fields in the header and footer, as "{page}"/"{total}".
    // A file of several sections has its first section's read so already
    // (`read_sections`).
    if read.sections.is_empty() {
        let (header, footer) = header_footer_templates(path);
        if header.is_some() {
            read.header = header;
        }
        if footer.is_some() {
            read.footer = footer;
        }
    }
    let ([header_extras, footer_extras], first) = header_footer_extras(path);
    // The first page's own header and footer, read whole from their parts.
    if let Some([header, footer]) = first {
        let story = |e: Option<PartExtras>| {
            let e = e.unwrap_or_else(PartExtras::empty);
            crate::model::HfStory { text: e.text, pictures: e.pictures, alignment: e.alignment, lines: e.lines }
        };
        (read.first_header, read.first_footer) = (Some(story(header)), Some(story(footer)));
    }
    for (extras, text, pictures, alignment, lines) in [
        (header_extras, &mut read.header, &mut read.header_pictures, &mut read.header_alignment, &mut read.header_lines),
        (footer_extras, &mut read.footer, &mut read.footer_pictures, &mut read.footer_alignment, &mut read.footer_lines),
    ] {
        (*pictures, *alignment) = (extras.pictures, extras.alignment);
        // A text box's lines, which rdocx does not read, as the template
        // has them; the part's own lines then go with its text.
        if text.as_deref().is_none_or(|t| !t.contains("{page}") && !t.contains("{total}")) && !extras.text.trim().is_empty() {
            *text = Some(extras.text.clone());
        }
        if text.as_deref().is_some_and(|t| t.split('\n').count() == extras.lines.len()) {
            *lines = extras.lines;
        }
    }
    Ok(read)
}

/// What the default header or footer part holds besides the text rdocx
/// reads: its pictures, how its text is aligned, and its text with each
/// line's look as the part has them.
struct PartExtras {
    pictures: Vec<Run>,
    alignment: Alignment,
    text: String,
    lines: Vec<crate::model::HfLine>,
}

impl PartExtras {
    fn empty() -> Self {
        PartExtras { pictures: Vec::new(), alignment: Alignment::Left, text: String::new(), lines: Vec::new() }
    }
}

/// The two parts of a page's header and footer, the first page's own
/// when it has them (`docx_fields::first_page_parts`).
type FirstParts = Option<[Option<PartExtras>; 2]>;

/// The pictures of the default header and footer (a letterhead's logo),
/// each kept in the media cache like a body picture (#455), how each
/// part's text is aligned, and its lines; and the same of the first
/// page's own, when it has them.
fn header_footer_extras(path: &str) -> ([PartExtras; 2], FirstParts) {
    let empty = PartExtras::empty;
    let none = || ([empty(), empty()], None);
    let Some(mut zip) = std::fs::File::open(path).ok().and_then(|f| zip::ZipArchive::new(f).ok()) else { return none() };
    let (Some(document), Some(rels)) = (part_text(&mut zip, "word/document.xml"), part_text(&mut zip, "word/_rels/document.xml.rels")) else {
        return none();
    };
    let styles = part_text(&mut zip, "word/styles.xml").unwrap_or_default();
    let (h, f) = crate::docx_fields::default_parts(&document, &rels);
    let first = crate::docx_fields::first_page_parts(&document, &rels);
    let mut extras = |part: Option<String>| -> PartExtras {
        let Some(part) = part else { return empty() };
        let Some(xml) = part_text(&mut zip, &part) else { return empty() };
        let (dir, file) = part.rsplit_once('/').unwrap_or(("", part.as_str()));
        let part_rels = part_text(&mut zip, &format!("{dir}/_rels/{file}.rels")).unwrap_or_default();
        let pictures = crate::docx_anchors::pictures_in(&xml)
            .into_iter()
            .filter_map(|pic| {
                let target = crate::docx_anchors::relationship_target(&part_rels, &pic.embed)?;
                let mut bytes = Vec::new();
                std::io::Read::read_to_end(&mut zip.by_name(&target).ok()?, &mut bytes).ok()?;
                let path = suite_common_core::media_cache::persist(&bytes).ok()?;
                Some(Run {
                    text: pic.alt,
                    style: RunStyle {
                        image: Some(path.to_string_lossy().into_owned()),
                        image_extent_emu: pic.extent,
                        image_anchor: pic.anchor,
                        ..Default::default()
                    },
                })
            })
            .collect();
        let (text, lines) = crate::docx_fields::story(&xml, &styles);
        PartExtras { pictures, alignment: crate::docx_fields::alignment(&xml, &styles), text, lines }
    };
    let defaults = [extras(h), extras(f)];
    let first = first.map(|(h, f)| [h.map(|p| extras(Some(p))), f.map(|p| extras(Some(p)))]);
    (defaults, first)
}

/// A zip part's text, if it is there.
fn part_text(zip: &mut zip::ZipArchive<impl std::io::Read + std::io::Seek>, name: &str) -> Option<String> {
    let mut s = String::new();
    std::io::Read::read_to_string(&mut zip.by_name(name).ok()?, &mut s).ok()?;
    Some(s)
}

/// The default header and footer as templates with their PAGE/NUMPAGES
/// fields ("{page}", "{total}"); `None` for a part without such fields.
fn header_footer_templates(path: &str) -> (Option<String>, Option<String>) {
    let Some(mut zip) = std::fs::File::open(path).ok().and_then(|f| zip::ZipArchive::new(f).ok()) else { return (None, None) };
    let (Some(document), Some(rels)) = (part_text(&mut zip, "word/document.xml"), part_text(&mut zip, "word/_rels/document.xml.rels")) else {
        return (None, None);
    };
    let (h, f) = crate::docx_fields::default_parts(&document, &rels);
    let mut template = |part: Option<String>| part.and_then(|p| part_text(&mut zip, &p)).and_then(|xml| crate::docx_fields::template(&xml));
    (template(h), template(f))
}

/// The package at `path`, with its chip controls as sentinel runs.
/// What `open_with_chips` took out of the markup for rdocx: the smart
/// chips, the tracked changes and the text boxes, restored after reading.
type Unwrapped = (Vec<(crate::chips::Chip, String)>, Vec<crate::model::Revision>, Vec<(crate::model::TextBox, crate::model::ImageAnchor)>);

fn open_with_chips(path: &str) -> Result<(rdocx::Document, Unwrapped), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut xml = String::new();
    let found = zip::ZipArchive::new(std::io::Cursor::new(&bytes))
        .ok()
        .and_then(|mut z| std::io::Read::read_to_string(&mut z.by_name("word/document.xml").ok()?, &mut xml).ok())
        .is_some();
    if !found || !(xml.contains("<w:sdt") || xml.contains("<w:ins ") || xml.contains("<w:del ") || xml.contains("<w:comment") || xml.contains("TOC \\") || xml.contains("<w:txbxContent") || xml.contains("<wps:cNvCnPr") || xml.contains("prst=\"line\"")) {
        return Ok((rdocx::Document::open(path).map_err(|e| e.to_string())?, (Vec::new(), Vec::new(), Vec::new())));
    }
    // A table of contents first: it lifts the paragraphs out of their
    // content control before chips look at controls.
    let toc = crate::docx_toc::unwrap(&xml);
    let (patched, chips) = crate::docx_chips::unwrap(toc.as_deref().unwrap_or(&xml));
    let (patched, revisions) = crate::docx_revisions::unwrap(&patched);
    let commented = crate::docx_comments::unwrap(&patched);
    let patched = commented.clone().unwrap_or(patched);
    // Text boxes rdocx does not read (docx_text_boxes).
    let (patched, text_boxes) = crate::docx_text_boxes::unwrap(&patched);
    if chips.is_empty() && revisions.is_empty() && commented.is_none() && toc.is_none() && text_boxes.is_empty() {
        return Ok((rdocx::Document::open(path).map_err(|e| e.to_string())?, (Vec::new(), Vec::new(), Vec::new())));
    }
    let bytes = with_part(&bytes, "word/document.xml", |_| patched.clone())?;
    Ok((rdocx::Document::from_bytes(&bytes).map_err(|e| e.to_string())?, (chips, revisions, text_boxes)))
}

/// Tabs in run text as Word's `w:tab` elements. rdocx writes a tab as a
/// character inside `w:t`, which Word and LibreOffice read as a space.
fn tabs_as_elements(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(at) = rest.find("<w:t>").into_iter().chain(rest.find("<w:t ")).min() {
        let Some(open_end) = rest[at..].find('>').map(|e| at + e + 1) else { break };
        let Some(close) = rest[open_end..].find("</w:t>").map(|c| open_end + c) else { break };
        out.push_str(&rest[..open_end]);
        let text = &rest[open_end..close];
        out.push_str(&text.replace('\t', "</w:t><w:tab/><w:t xml:space=\"preserve\">"));
        rest = &rest[close..];
    }
    out.push_str(rest);
    out
}

/// One tab stop as builders spell it: its alignment, and its leader.
/// rdocx names no middle dot: a middle dot writes as a dot, which the
/// reader reads back as a dot (ODT keeps the middle dot exactly).
fn emit_tab_stop<'a>(p: rdocx::Paragraph<'a>, stop: &TabStop) -> rdocx::Paragraph<'a> {
    let align = match stop.align {
        TabAlign::Center => rdocx::TabAlignment::Center,
        TabAlign::Right => rdocx::TabAlignment::Right,
        TabAlign::Decimal => rdocx::TabAlignment::Decimal,
        TabAlign::Left => rdocx::TabAlignment::Left,
    };
    let pos = rdocx::Length::pt(stop.pos_pt);
    match stop.leader {
        TabLeader::None => p.add_tab_stop(align, pos),
        TabLeader::Dot => p.add_tab_stop_with_leader(align, pos, rdocx::TabLeader::Dot),
        TabLeader::Hyphen => p.add_tab_stop_with_leader(align, pos, rdocx::TabLeader::Hyphen),
        TabLeader::Underscore => p.add_tab_stop_with_leader(align, pos, rdocx::TabLeader::Underscore),
        TabLeader::MiddleDot => p.add_tab_stop_with_leader(align, pos, rdocx::TabLeader::Dot),
    }
}

/// A document paragraph's properties rdocx cannot emit, in document
/// order: whether its line rule is atLeast (`set_line_spacing_at_least`
/// is crate-internal, so minima are written exact and fixed below) and
/// its per-side borders (only all-sides and bottom have builders).
struct ParaPost {
    at_least: bool,
    borders: Option<ParaBorders>,
}

impl ParaPost {
    fn of(style: &ParaStyle) -> Self {
        Self {
            at_least: matches!(style.line_height, Some(LineHeight::Minimum(_))),
            borders: if style.borders.is_empty() { None } else { Some(style.borders.clone()) },
        }
    }

    fn trivial() -> Self {
        Self { at_least: false, borders: None }
    }

    fn nontrivial(&self) -> bool {
        self.at_least || self.borders.is_some()
    }
}

/// Fix the properties of `specs` into the document's paragraphs, the
/// n-th `<w:p>` taking the n-th spec. Specs run with the paragraphs in
/// document order — body and table cells, separators included, covered
/// cells already removed — which is how rdocx serializes them.
fn para_post_pass(xml: &str, specs: &[ParaPost]) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    let mut n = 0usize;
    while let Some(at) = find_para(rest) {
        let Some(open_end) = rest[at..].find('>').map(|e| at + e + 1) else { break };
        out.push_str(&rest[..open_end]);
        rest = &rest[open_end..];
        let self_closed = out.ends_with("/>");
        let Some(spec) = specs.get(n) else { n += 1; continue };
        n += 1;
        if !spec.nontrivial() {
            continue;
        }
        // A blank paragraph is written self-closed: open it for its
        // borders (a blank line's padding is all it shows).
        if self_closed {
            if let Some(borders) = spec.borders.as_ref() {
                out.truncate(out.len() - 2);
                out.push_str(&format!("><w:pPr>{}</w:pPr></w:p>", pbdr_xml(borders)));
            }
            continue;
        }
        // The paragraph's extent: to its closing tag. A `w:p` never
        // nests, so the first close is its own.
        let Some(close) = rest.find("</w:p>") else { break };
        let (body, tail) = rest.split_at(close);
        let mut fixed = body.to_string();
        if spec.at_least {
            fixed = fixed.replacen("w:lineRule=\"exact\"", "w:lineRule=\"atLeast\"", 1);
        }
        if let Some(borders) = spec.borders.as_ref() {
            let pbdr = pbdr_xml(borders);
            if let Some(ppr) = find_ppr(&fixed) {
                let Some(end) = fixed[ppr..].find('>').map(|e| ppr + e + 1) else { break };
                if fixed[..end].ends_with("/>") {
                    fixed.replace_range(end - 2..end, &format!(">{pbdr}</w:pPr>"));
                } else {
                    // `w:pBdr` sorts with the paragraph properties, ahead
                    // of shading, tabs, spacing, indents and justification.
                    let slot = ["<w:shd", "<w:tabs", "<w:spacing", "<w:ind", "<w:jc"]
                        .into_iter()
                        .filter_map(|t| fixed[end..].find(t).map(|e| end + e))
                        .min()
                        .or_else(|| fixed.find("</w:pPr>"));
                    let Some(slot) = slot else { break };
                    fixed.insert_str(slot, &pbdr);
                }
            } else {
                fixed.insert_str(0, &format!("<w:pPr>{pbdr}</w:pPr>"));
            }
        }
        out.push_str(&fixed);
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// `<w:p` open tags only: `<w:pPr`, `<w:pStyle` and the rest are not
/// paragraphs. Returns the tag's start.
fn find_para(xml: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(at) = xml[from..].find("<w:p").map(|e| from + e) {
        match xml.as_bytes().get(at + 4) {
            Some(b'>') | Some(b' ') | Some(b'/') => return Some(at),
            _ => from = at + 4,
        }
    }
    None
}

/// `<w:pPr` open tags only: `<w:pPrChange` is the old value, not the
/// paragraph's. Returns the tag's start.
fn find_ppr(xml: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(at) = xml[from..].find("<w:pPr").map(|e| from + e) {
        match xml.as_bytes().get(at + 6) {
            Some(b'>') | Some(b' ') | Some(b'/') => return Some(at),
            _ => from = at + 6,
        }
    }
    None
}

/// A paragraph's per-side borders as a `w:pBdr` element. Widths are
/// eighths of a point, the content gap whole points.
fn pbdr_xml(borders: &ParaBorders) -> String {
    let edge = |name: &str, e: &ParaBorder| {
        let val = match e.style {
            BorderStyle::None => "none",
            BorderStyle::Single => "single",
            BorderStyle::Dashed => "dashed",
            BorderStyle::Dotted => "dotted",
            BorderStyle::Double => "double",
        };
        format!(
            "<w:{name} w:val=\"{val}\" w:sz=\"{}\" w:space=\"{}\" w:color=\"{}\"/>",
            (e.width_pt * 8.0).round().clamp(1.0, 96.0) as u32,
            e.space_pt.round().clamp(0.0, 31.0) as u32,
            e.color
        )
    };
    let mut out = String::from("<w:pBdr>");
    if let Some(e) = &borders.top {
        out.push_str(&edge("top", e));
    }
    if let Some(e) = &borders.left {
        out.push_str(&edge("left", e));
    }
    if let Some(e) = &borders.bottom {
        out.push_str(&edge("bottom", e));
    }
    if let Some(e) = &borders.right {
        out.push_str(&edge("right", e));
    }
    out.push_str("</w:pBdr>");
    out
}

/// A run's text as written: bracketed as a tracked change if it is one.
/// A run of `text` whose line breaks (Shift+Enter, `\n`) are Word's
/// `w:br`: raw, a newline is whitespace every reader collapses.
fn broken_run<'a>(p: &'a mut rdocx::Paragraph<'_>, text: &str) -> rdocx::Run<'a> {
    let mut pieces = text.split('\n');
    let mut r = p.add_run(pieces.next().unwrap_or_default());
    for piece in pieces {
        r.add_break(rdocx::BreakKind::Line);
        if !piece.is_empty() {
            r.add_text(piece);
        }
    }
    r
}

fn bracketed(revisions: &mut Vec<crate::model::Revision>, run: &Run) -> String {
    match &run.style.revision {
        Some(rev) => {
            revisions.push(rev.clone());
            crate::docx_revisions::bracket(revisions.len() - 1, &run.text)
        }
        None => run.text.clone(),
    }
}

/// `package` with part `name` rewritten by `f`.
fn with_part(package: &[u8], name: &str, f: impl Fn(&str) -> String) -> Result<Vec<u8>, String> {
    let mut zin = zip::ZipArchive::new(std::io::Cursor::new(package)).map_err(|e| e.to_string())?;
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for i in 0..zin.len() {
        let mut part = zin.by_index(i).map_err(|e| e.to_string())?;
        let part_name = part.name().map_err(|e| e.to_string())?.into_owned();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut part, &mut data).map_err(|e| e.to_string())?;
        if part_name == name {
            let xml = String::from_utf8(data).map_err(|e| e.to_string())?;
            data = f(&xml).into_bytes();
        }
        out.start_file(part_name, options).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut out, &data).map_err(|e| e.to_string())?;
    }
    Ok(out.finish().map_err(|e| e.to_string())?.into_inner())
}

fn read_opened(path: &str, doc: rdocx::Document) -> Result<Document, String> {
    THEME.with(|t| *t.borrow_mut() = theme_fonts(path));
    RESTARTED.with(|r| r.borrow_mut().clear());
    ANCHORS.with(|a| *a.borrow_mut() = anchor_placements(path));
    INLINE_SIDES.with(|s| *s.borrow_mut() = inline_sides(path));
    STYLE_FONTS.with(|s| *s.borrow_mut() = style_fonts(path));

    // Paragraph properties rdocx cannot hand back: strict-spelled indents
    // and tab-stop positions. The scan is positional, so it is only
    // trusted when it found exactly as many body paragraphs as rdocx did;
    // otherwise the two disagree about what a paragraph is and pairing
    // them would misattribute a property to its neighbour.
    let body = doc.paragraphs();
    let RawParas { body: raw, cells: raw_cells, tables_at, sections: raw_sections } = raw_paragraph_props(path);
    let raw = (raw.len() == body.len()).then_some(raw);
    let tables = doc.tables();
    // Where each table sits: the number of body paragraphs before it.
    // rdocx gives tables apart from the paragraphs; its body items give
    // their order, but not how many paragraphs a content control holds,
    // which the XML scan counts. When neither can be trusted, the tables
    // go after the body, as they always did.
    let tables_at: Option<Vec<usize>> = if raw.is_some() && tables_at.len() == tables.len() {
        Some(tables_at)
    } else {
        let mut seen = 0usize;
        let mut at = Vec::new();
        let mut known = true;
        for item in doc.body_items() {
            match item {
                rdocx::BodyItemRef::Paragraph(_) => seen += 1,
                rdocx::BodyItemRef::Table(_) => at.push(seen),
                rdocx::BodyItemRef::ContentControl(_) => known = false,
                rdocx::BodyItemRef::UnsupportedXml(_) => {}
            }
        }
        (known && seen == body.len() && at.len() == tables.len()).then_some(at)
    };

    let mut paragraphs = Vec::new();
    // Per kept body paragraph: its style id and whether it has contextual
    // spacing, for the pass after this loop.
    let mut contextual: Vec<(Option<String>, bool)> = Vec::new();
    let contextual_ids = contextual_styles(path);
    // Set when a paragraph ends with a run-level page break, and consumed
    // by the next paragraph this loop keeps.
    let mut carried_break = false;
    // The body positions of tables a dropped page break comes right
    // before: the break a paragraph of nothing but one makes, or one after
    // a paragraph's text.
    let mut table_breaks: Vec<usize> = Vec::new();
    // Per body paragraph: how many paragraphs were kept before it, which
    // is where a table that precedes it goes.
    let mut kept_before = Vec::with_capacity(body.len() + 1);
    // Per kept paragraph: the number Word gives it, when it is a numbered
    // list item (`WordCounter`), for the pass after the tables go in.
    let mut word_numbers: Vec<Option<u32>> = Vec::new();
    let mut counter = WordCounter::default();
    for (i, p) in body.iter().enumerate() {
        kept_before.push(paragraphs.len());
        // The paragraph the writer puts after a table that meets another
        // or ends the document is OOXML's, not the document's.
        if p.style_id() == Some(SEPARATOR_STYLE) && p.text().is_empty() {
            continue;
        }
        let mut pending_break = false;
        // Decorative rules (LibreOffice's HorizontalLine style) carry no text.
        if p.style_id() == Some("HorizontalLine") && p.text().is_empty() {
            continue;
        }
        let mut para = map_paragraph(&doc, p, None, raw.as_ref().and_then(|s| s.get(i)));
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
        // A table next takes the break: it starts the new page.
        let table_next = |carried: &mut bool, breaks_at: &mut Vec<usize>| {
            if *carried && tables_at.as_ref().is_some_and(|t| t.contains(&(i + 1))) {
                *carried = false;
                breaks_at.push(i + 1);
            }
        };
        if only_break && breaks.leading && !pending_break && i + 1 < body.len() {
            carried_break = true;
            table_next(&mut carried_break, &mut table_breaks);
            continue;
        }
        let style_id = p.style_id().map(str::to_string);
        let direct_contextual = raw.as_ref().and_then(|s| s.get(i)).is_some_and(|r| r.contextual);
        let is_contextual = direct_contextual || style_id.as_deref().is_some_and(|id| contextual_ids.contains(id));
        contextual.push((style_id, is_contextual));
        list_indent_from_declared(&doc, p, &mut para.style);
        word_numbers.push(counter.number(&doc, p, &mut para.style));
        paragraphs.push(para);
        carried_break = pending_break;
        table_next(&mut carried_break, &mut table_breaks);
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

    kept_before.push(paragraphs.len());

    // Table cells become paragraphs tagged with (table, row, col) — the
    // document stays flat (offset invariants intact) and the grid is fully
    // recoverable. Each table's paragraphs are gathered on their own and
    // put where the table sits, after the contextual pass above, which
    // counts body paragraphs only.
    let mut table_paragraphs: Vec<Vec<Paragraph>> = Vec::with_capacity(tables.len());
    // The cell half of the scan, trusted on the same terms as the body
    // half: only when it counted exactly the paragraphs rdocx walks here.
    let mut cell_paragraphs = 0usize;
    for table in &tables {
        for row in (0..table.row_count()).filter_map(|ri| table.row(ri)) {
            for ci in 0..row.cell_count() {
                cell_paragraphs += row.cell(ci).map_or(0, |c| c.paragraphs().count());
            }
        }
    }
    let raw_cells = (raw_cells.len() == cell_paragraphs).then_some(raw_cells);
    let mut cell_index = 0usize;
    let mut table_spans: std::collections::BTreeMap<u32, Vec<crate::model::CellSpan>> = Default::default();
    for (ti, table) in tables.iter().enumerate() {
        let mut paragraphs: Vec<Paragraph> = Vec::new();
        let spans = table_spans.entry(ti as u32).or_default();
        // Per grid column: the vertical merge open there (`w:vMerge`
        // restart), as an index into `spans`.
        let mut open: std::collections::HashMap<u32, usize> = Default::default();
        for ri in 0..table.row_count() {
            let Some(row) = table.row(ri) else { continue };
            // A cell's column is its place in the grid: the columns of the
            // cells before it, each `w:gridSpan` wide. Counting cells put
            // everything after a merged cell in the wrong column.
            let mut grid_col = 0u32;
            for ci in 0..row.cell_count() {
                let Some(cell) = row.cell(ci) else { continue };
                let col = grid_col;
                let width = cell.grid_span().unwrap_or(1).max(1);
                grid_col += width;
                match cell.v_merge() {
                    Some(rdocx::VMerge::Continue) => {
                        if let Some(span) = open.get(&col).and_then(|&k| spans.get_mut(k)) {
                            span.rows += 1;
                        }
                    }
                    Some(rdocx::VMerge::Restart) => {
                        open.insert(col, spans.len());
                        spans.push(crate::model::CellSpan { row: ri as u32, col, rows: 1, cols: width });
                    }
                    None => {
                        open.remove(&col);
                        if width > 1 {
                            spans.push(crate::model::CellSpan { row: ri as u32, col, rows: 1, cols: width });
                        }
                    }
                }
                // The grid positions a merged cell covers keep a paragraph.
                for covered in col + 1..col + width {
                    paragraphs.push(Paragraph {
                        style: ParaStyle {
                            table_cell: Some(crate::model::TableCell { table: ti as u32, row: ri as u32, col: covered }),
                            ..Default::default()
                        },
                        runs: vec![],
                    });
                }
                let mut wrote_any = false;
                let mut keep = |para: Paragraph| {
                    let mut para = para;
                    para.style.table_cell = Some(crate::model::TableCell {
                        table: ti as u32, row: ri as u32, col,
                    });
                    paragraphs.push(para);
                    wrote_any = true;
                };
                for item in cell.items() {
                    match item {
                        rdocx::CellItemRef::Paragraph(cp) => {
                            let raw_cell = raw_cells.as_ref().and_then(|s| s.get(cell_index));
                            cell_index += 1;
                            // An empty paragraph is a line of the cell:
                            // a form's answer box is a cell of them.
                            let mut para = map_paragraph(&doc, &cp, table.style_id(), raw_cell);
                            drop_cell_page_breaks(&mut para, &cp);
                            keep(para);
                        }
                        // A table nested in a cell belongs to its outer
                        // cell, as in the ODT reader: the model has no
                        // nesting, and dropping it lost its text (#1419).
                        // The save asks before it flattens one
                        // (`loss::content_a_save_drops`).
                        rdocx::CellItemRef::Table(nested) => {
                            for_each_nested_paragraph(&nested, &mut |cp| {
                                if !cp.text().is_empty() {
                                    keep(map_paragraph(&doc, cp, table.style_id(), None));
                                }
                            });
                        }
                        _ => {}
                    }
                }
                // Empty cells still occupy a grid position.
                if !wrote_any {
                    paragraphs.push(Paragraph {
                        style: ParaStyle {
                            table_cell: Some(crate::model::TableCell {
                                table: ti as u32, row: ri as u32, col,
                            }),
                            ..Default::default()
                        },
                        runs: vec![],
                    });
                }
            }
        }
        // Row-major within each row, as every reader of the grid expects.
        paragraphs.sort_by_key(|p| p.style.table_cell.map(|c| (c.row, c.col)));
        spans.retain(|s| s.rows > 1 || s.cols > 1);
        table_paragraphs.push(paragraphs);
    }
    table_spans.retain(|_, spans| !spans.is_empty());
    // Each table's paragraph count, for where the sections end.
    let table_lens: Vec<usize> = table_paragraphs.iter().map(Vec::len).collect();
    // In reverse, so each insertion leaves the earlier positions valid;
    // tables at one position keep their order.
    for (ti, mut cells) in table_paragraphs.into_iter().enumerate().rev() {
        // The first of the tables a page break comes before starts the page.
        if tables_at.as_ref().is_some_and(|t| table_breaks.contains(&t[ti]) && (ti == 0 || t[ti - 1] != t[ti])) {
            if let Some(first) = cells.first_mut() {
                first.style.page_break_before = true;
            }
        }
        let at = tables_at.as_ref().map_or(paragraphs.len(), |t| kept_before[t[ti].min(body.len())]);
        word_numbers.splice(at..at, std::iter::repeat_n(None, cells.len()));
        paragraphs.splice(at..at, cells);
    }
    number_as_word_does(&mut paragraphs, &word_numbers);

    if paragraphs.is_empty() {
        let mut d = Document::new();
        d.header = doc.header_text();
        d.footer = doc.footer_text();
        return Ok(d);
    }
    // Footnote texts land in the document list; docx ids remap to
    // zero-based indexes on the referencing runs (see map_paragraph).
    let footnotes: Vec<String> = doc.footnotes().into_iter().map(|(_, t)| t).collect();
    // Table ids are the tables' order, as the cells above are tagged.
    let table_columns = tables.iter().enumerate().filter_map(|(ti, t)| Some((ti as u32, column_widths(t)?))).collect();
    let table_rows = tables.iter().enumerate().filter_map(|(ti, t)| Some((ti as u32, row_heights(t)?))).collect();
    let table_fills = tables.iter().enumerate().filter_map(|(ti, t)| Some((ti as u32, cell_fills(t)?))).collect();
    let table_padding = tables.iter().enumerate().filter_map(|(ti, t)| Some((ti as u32, cell_padding(t)?))).collect();
    let table_unruled: std::collections::BTreeSet<u32> = tables.iter().enumerate().filter(|(_, t)| !table_ruled(t)).map(|(ti, _)| ti as u32).collect();
    let table_valign = tables.iter().enumerate().filter_map(|(ti, t)| Some((ti as u32, cell_valign(t)?))).collect();
    let rule_looks: Vec<(u32, RuleLook)> = tables.iter().enumerate().filter(|(ti, _)| !table_unruled.contains(&(*ti as u32))).map(|(ti, t)| (ti as u32, table_rule_look(t))).collect();
    let table_rule_colors = rule_looks.iter().filter_map(|(ti, (c, _))| Some((*ti, c.clone()?))).collect();
    let table_rule_widths = rule_looks.iter().filter_map(|(ti, (_, w))| Some((*ti, (*w)?))).collect();
    let table_rule_double = tables.iter().enumerate().filter(|(ti, t)| !table_unruled.contains(&(*ti as u32)) && table_rule_double(t)).map(|(ti, _)| ti as u32).collect();
    // The rows kept on one page (`w:cantSplit`), and heading rows
    // (`w:tblHeader`), which Writer never splits across pages.
    let table_whole_rows = tables
        .iter()
        .enumerate()
        .filter_map(|(ti, t)| {
            let rows: std::collections::BTreeSet<u32> =
                (0..t.row_count()).filter(|&r| t.row(r).is_some_and(|row| row.cant_split_value() == Some(true) || row.is_header())).map(|r| r as u32).collect();
            (!rows.is_empty()).then_some((ti as u32, rows))
        })
        .collect();
    // A table sized as a share of the text's width (`w:tblW w:type="pct"`).
    let table_relative = tables.iter().enumerate().filter(|(_, t)| matches!(t.width_mode(), Some(rdocx::TableWidth::Percentage(_)))).map(|(ti, _)| ti as u32).collect();
    let table_open_edges = tables.iter().enumerate().filter(|(ti, _)| !table_unruled.contains(&(*ti as u32))).filter_map(|(ti, t)| Some((ti as u32, cell_open_edges(t)?))).collect();
    // Where a table's text starts: Word 2013's rules (compatibility mode
    // 15) put its edge at `w:tblInd`, its text its left padding further
    // right; older ones its text at `w:tblInd`, its padding hanging left.
    let modern = doc.compatibility_settings().iter().any(|s| s.name == "compatibilityMode" && s.value.trim().parse::<u32>().is_ok_and(|m| m >= 15));
    let table_indent = tables
        .iter()
        .enumerate()
        .filter_map(|(ti, t)| {
            let edge = t.indent().map(|l| l.to_pt()).filter(|v| v.is_finite()).unwrap_or(0.0);
            let indent = edge + if modern { cell_padding(t).unwrap_or_default().left_pt } else { 0.0 };
            (indent.abs() > 0.005).then_some((ti as u32, indent))
        })
        .collect();

    let mut read = Document {
        paragraphs,
        footnotes,
        header: doc.header_text(),
        footer: doc.footer_text(),
        header_pictures: Vec::new(),
        footer_pictures: Vec::new(),
        header_alignment: Alignment::Left,
        footer_alignment: Alignment::Left, header_lines: Vec::new(), footer_lines: Vec::new(), first_header: None, first_footer: None, space_adds: false,
        page: read_page_geometry(&doc),
        base_font: read_base_font(&doc),
        heading_styles: read_heading_styles(&doc),
        comments: Vec::new(),
        table_columns,
        table_rows,
        table_spans,
        table_fills,
        table_padding,
        table_indent,
        table_unruled,
        table_rule_colors,
        table_rule_widths,
        table_rule_double,
        table_whole_rows,
        table_relative,
        table_open_edges,
        table_valign,
        sections: Vec::new(),
        next_section_id: 0,
    };
    // A file of several sections (`crate::sections`), read only from a
    // scan whose paragraphs and tables are rdocx's: a section's end is a
    // position in them. A file of one section reads as it always did.
    let sectioned = match (&raw, &tables_at) {
        (Some(_), Some(tables_at)) if raw_sections.len() >= 2 => read_sections(path, &raw_sections, &kept_before, tables_at, &table_lens, &read.paragraphs),
        _ => None,
    };
    if let Some((sections, header, footer)) = sectioned.filter(|(s, _, _)| crate::sections::is_valid(&read, s)) {
        read.page = Some(sections[0].geometry);
        read.header = header;
        read.footer = footer;
        read.next_section_id = sections.len() as u32;
        read.sections = sections;
    }
    Ok(read)
}

/// The sections of a file whose section properties (`raw`, in document
/// order) are more than one, with the first section's header and footer
/// text (the document's own, `Document::header`), or `None` when they
/// come to fewer than two sections.
///
/// A paragraph's `w:sectPr` ends its section after that body paragraph
/// (`b`): at `kept_before[b + 1]`, the paragraphs kept before the next
/// one, plus the cells of every table before it (`tables_at[t] <= b`); a
/// table right after it starts the next section. A section left with no
/// paragraph (its paragraph was a dropped separator or page break) goes,
/// its own header and footer passing to the next if that one has none,
/// as Word would show them there. Every break starts a new page here: a
/// continuous, even- or odd-page break reads as one.
///
/// A page value a section leaves out is the one before's (the first's,
/// `PageGeometry::default()`'s); columns are one, 18pt apart, unless
/// `w:cols` says otherwise, as `read_page_geometry` reads them. A
/// section's header and footer are its `default` references' text, PAGE
/// and NUMPAGES as "{page}" and "{total}"; one without a reference
/// continues the one before (`None`).
fn read_sections(
    path: &str,
    raw: &[RawSection],
    kept_before: &[usize],
    tables_at: &[usize],
    table_lens: &[usize],
    paragraphs: &[Paragraph],
) -> Option<(Vec<crate::model::DocSection>, Option<String>, Option<String>)> {
    let n = paragraphs.len();
    let body_len = kept_before.len().checked_sub(1)?;
    // A file whose last section has no `w:sectPr` of its own (the body
    // ends without one) still has that section, with nothing set.
    let tail = RawSection::default();
    let raw: Vec<&RawSection> = raw.iter().chain(raw.last().filter(|s| s.para.is_some()).map(|_| &tail)).collect();

    let mut zip = std::fs::File::open(path).ok().and_then(|f| zip::ZipArchive::new(f).ok());
    let rels = zip.as_mut().and_then(|z| part_text(z, "word/_rels/document.xml.rels")).unwrap_or_default();
    let mut story = |id: Option<&String>| -> Option<String> {
        let part = crate::docx_fields::relationship_target(&rels, id?)?;
        Some(crate::docx_fields::story_text(&part_text(zip.as_mut()?, &part)?))
    };
    // Whether `para` continues the table the paragraph before it is in:
    // a section cannot start there.
    let table = |i: usize| paragraphs.get(i).and_then(|p| p.style.table_cell).map(|c| c.table);
    let in_table = |i: usize| i > 0 && table(i).is_some() && table(i) == table(i - 1);

    let mut sections: Vec<crate::model::DocSection> = Vec::new();
    let mut previous = PageGeometry::default();
    let mut start = 0usize;
    // The header and footer of a section that went, for the next.
    let mut carried: (Option<String>, Option<String>) = (None, None);
    for s in raw {
        let mut end = match s.para {
            Some(b) => {
                let b = b.min(body_len.checked_sub(1)?);
                let cells: usize = tables_at.iter().zip(table_lens).filter(|(at, _)| **at <= b).map(|(_, len)| *len).sum();
                kept_before[b + 1] + cells
            }
            None => n,
        }
        .clamp(start, n);
        while end > start && end < n && in_table(end) {
            end -= 1;
        }
        let twips = |v: Option<f64>, or: f64| v.map_or(or, |t| t / 20.0);
        let geometry = PageGeometry {
            width_pt: twips(s.size[0], previous.width_pt),
            height_pt: twips(s.size[1], previous.height_pt),
            margin_top_pt: twips(s.margins[0], previous.margin_top_pt),
            margin_right_pt: twips(s.margins[1], previous.margin_right_pt),
            margin_bottom_pt: twips(s.margins[2], previous.margin_bottom_pt),
            margin_left_pt: twips(s.margins[3], previous.margin_left_pt),
            columns: s.columns.and_then(|c| u8::try_from(c).ok()).filter(|c| *c > 0).unwrap_or(1),
            column_gap_pt: twips(s.column_gap, 18.0),
            header_pt: s.header_footer[0].map(|t| t / 20.0).or(previous.header_pt),
            footer_pt: s.header_footer[1].map(|t| t / 20.0).or(previous.footer_pt),
            border: previous.border,
        };
        previous = geometry;
        let (carried_header, carried_footer) = std::mem::take(&mut carried);
        let header = story(s.header.as_ref()).or(carried_header);
        let footer = story(s.footer.as_ref()).or(carried_footer);
        if end <= start {
            carried = (header, footer);
            continue;
        }
        sections.push(crate::model::DocSection {
            id: sections.len() as u32,
            range: crate::model::ParagraphRange::new(start, end),
            geometry,
            header,
            footer,
        });
        start = end;
    }
    if sections.len() < 2 {
        return None;
    }
    // The first section's header and footer are the document's own.
    let header = sections[0].header.take();
    let footer = sections[0].footer.take();
    Some((sections, header, footer))
}

/// A table's shaded cells (`w:shd w:fill`), when any cell is; "auto" is
/// no fill.
fn cell_fills(table: &rdocx::TableRef<'_>) -> Option<Vec<crate::model::CellFill>> {
    let mut fills = Vec::new();
    for ri in 0..table.row_count() {
        let Some(row) = table.row(ri) else { continue };
        // At the cell's grid column, as the cells are read: the columns of
        // the cells before it, each `w:gridSpan` wide.
        let mut col = 0u32;
        for ci in 0..row.cell_count() {
            let Some(cell) = row.cell(ci) else { continue };
            let at = col;
            col += cell.grid_span().unwrap_or(1).max(1);
            let Some(fill) = cell.shading_fill().map(str::to_string) else { continue };
            let color = fill.trim_start_matches('#').to_uppercase();
            if color.len() == 6 && color.chars().all(|c| c.is_ascii_hexdigit()) {
                fills.push(crate::model::CellFill { row: ri as u32, col: at, color });
            }
        }
    }
    (!fills.is_empty()).then_some(fills)
}

/// The cells set lower than the top (`w:vAlign`), at their grid columns.
fn cell_valign(table: &rdocx::TableRef<'_>) -> Option<Vec<crate::model::CellVAlign>> {
    let mut cells = Vec::new();
    for ri in 0..table.row_count() {
        let Some(row) = table.row(ri) else { continue };
        let mut col = 0u32;
        for ci in 0..row.cell_count() {
            let Some(cell) = row.cell(ci) else { continue };
            let at = col;
            col += cell.grid_span().unwrap_or(1).max(1);
            let align = match cell.vertical_alignment() {
                Some(rdocx::VerticalAlignment::Center) => crate::model::VAlign::Center,
                Some(rdocx::VerticalAlignment::Bottom) => crate::model::VAlign::Bottom,
                _ => continue,
            };
            cells.push(crate::model::CellVAlign { row: ri as u32, col: at, align });
        }
    }
    (!cells.is_empty()).then_some(cells)
}

/// The cells whose own borders (`w:tcBorders`) leave a side without a
/// rule (`w:val="nil"` or `"none"`), at their grid columns. In a table
/// that rules nothing itself (no `w:tblBorders`, no ruled style), a side
/// a cell's borders leave out is bare too: Writer writes only the sides a
/// cell has.
fn cell_open_edges(table: &rdocx::TableRef<'_>) -> Option<Vec<crate::model::CellEdges>> {
    use rdocx::CellBorderEdge as C;
    use rdocx::TableBorderEdge as E;
    let visible = |b: Option<rdocx::TableBorderRef<'_>>| b.is_some_and(|b| !matches!(b.style(), "none" | "nil"));
    let table_rules = [E::Top, E::Bottom, E::Left, E::Right, E::InsideHorizontal, E::InsideVertical].into_iter().any(|e| visible(table.border(e)))
        || table.style_id().is_some_and(table_style_ruled);
    let mut edges = Vec::new();
    for ri in 0..table.row_count() {
        let Some(row) = table.row(ri) else { continue };
        let mut col = 0u32;
        for ci in 0..row.cell_count() {
            let Some(cell) = row.cell(ci) else { continue };
            let at = col;
            col += cell.grid_span().unwrap_or(1).max(1);
            let own = cell.has_borders();
            let open = |e: C| match cell.border(e) {
                Some(b) => matches!(b.style(), "none" | "nil"),
                None => own && !table_rules,
            };
            let open = crate::model::OpenEdges { top: open(C::Top), bottom: open(C::Bottom), left: open(C::Left), right: open(C::Right) };
            if open.any() {
                edges.push(crate::model::CellEdges { row: ri as u32, col: at, open });
            }
        }
    }
    (!edges.is_empty()).then_some(edges)
}

/// Whether a table draws any rule: a visible edge of its own borders
/// (`w:tblBorders`), its table style's (up the `w:basedOn` chain, as
/// "Table Grid" gives one), or any of its cells' (`w:tcBorders`). Word's
/// "Normal Table" declares none, and Word and Writer draw such a table
/// with no rules at all.
fn table_ruled(table: &rdocx::TableRef<'_>) -> bool {
    use rdocx::TableBorderEdge as E;
    let visible = |b: Option<rdocx::TableBorderRef<'_>>| b.is_some_and(|b| !matches!(b.style(), "none" | "nil"));
    if [E::Top, E::Bottom, E::Left, E::Right, E::InsideHorizontal, E::InsideVertical].into_iter().any(|e| visible(table.border(e))) {
        return true;
    }
    if table.style_id().is_some_and(table_style_ruled) {
        return true;
    }
    use rdocx::CellBorderEdge as C;
    (0..table.row_count()).filter_map(|r| table.row(r)).any(|row| {
        (0..row.cell_count()).filter_map(|c| row.cell(c)).any(|cell| [C::Top, C::Bottom, C::Left, C::Right].into_iter().any(|e| visible(cell.border(e))))
    })
}

/// A table's cell padding, when it is not Word's default: its cell margins
/// (`w:tblCellMar`), with its first cell's own (`w:tcMar`) over them.
fn cell_padding(table: &rdocx::TableRef<'_>) -> Option<crate::model::CellPadding> {
    let mut pad = crate::model::CellPadding::default();
    let first = table.row(0).and_then(|r| r.cell(0).and_then(|c| c.margins()));
    for m in [table.cell_margins(), first].into_iter().flatten() {
        let pt = |l: Option<rdocx::Length>| l.map(|l| l.to_pt()).filter(|v| v.is_finite() && *v >= 0.0);
        pad.top_pt = pt(m.top).unwrap_or(pad.top_pt);
        pad.bottom_pt = pt(m.bottom).unwrap_or(pad.bottom_pt);
        pad.left_pt = pt(m.left).unwrap_or(pad.left_pt);
        pad.right_pt = pt(m.right).unwrap_or(pad.right_pt);
    }
    (pad != crate::model::CellPadding::default()).then_some(pad)
}

/// A table's row heights, when any row gives one (`w:trHeight`).
fn row_heights(table: &rdocx::TableRef<'_>) -> Option<Vec<Option<crate::model::RowHeight>>> {
    let heights: Vec<Option<crate::model::RowHeight>> = (0..table.row_count())
        .map(|r| match table.row(r)?.height()? {
            rdocx::RowHeight::AtLeast(h) => Some(crate::model::RowHeight { pt: h.to_pt(), exact: false }),
            rdocx::RowHeight::Exact(h) => Some(crate::model::RowHeight { pt: h.to_pt(), exact: true }),
        })
        .map(|h| h.filter(|h| h.pt > 0.0))
        .collect();
    heights.iter().any(Option::is_some).then_some(heights)
}

/// A table's column widths in points: its grid's, else its first row's
/// cell widths, when every cell gives one in absolute units.
fn column_widths(table: &rdocx::TableRef<'_>) -> Option<Vec<f64>> {
    // The table's grid (`w:tblGrid`), as Writer lays it out: the first
    // row may be one cell across every column.
    let grid: Vec<f64> = table.grid_widths().iter().map(|w| w.to_pt()).collect();
    let spanned = (0..table.row_count())
        .filter_map(|r| table.row(r))
        .map(|row| (0..row.cell_count()).filter_map(|c| row.cell(c)).map(|c| c.grid_span().unwrap_or(1).max(1) as usize).sum::<usize>())
        .max()
        .unwrap_or(0);
    if grid.len() == spanned && grid.iter().all(|x| *x > 0.0) {
        return Some(grid);
    }
    let row = table.row(0)?;
    let widths: Option<Vec<f64>> = (0..row.cell_count()).map(|c| row.cell(c)?.width().map(|w| w.to_pt())).collect();
    widths.filter(|w| !w.is_empty() && w.iter().all(|x| *x > 0.0))
}

/// The style of the paragraph OOXML needs after a table that ends the
/// document or meets another, which the model does not have: the writer
/// adds it, 1pt high so Word draws the tables as Letters does, and the
/// reader drops it. An empty paragraph the document has after a table is
/// a line of the document, and both keep it.
const SEPARATOR_STYLE: &str = "LettersTableSeparator";

/// Whether a table followed by `after` needs the separator: nothing
/// follows it, or another table does.
fn needs_separator(after: &[Paragraph]) -> bool {
    after.first().is_none_or(|p| p.style.table_cell.is_some())
}

/// Write the separator paragraph (`SEPARATOR_STYLE`) after a table, with
/// its (trivial) entry in `post`, which runs with the paragraphs.
fn add_separator(out: &mut rdocx::Document, post: &mut Vec<ParaPost>) {
    out.add_paragraph("")
        .style(SEPARATOR_STYLE)
        .space_before(rdocx::Length::pt(0.0))
        .space_after(rdocx::Length::pt(0.0))
        .line_spacing(1.0);
    post.push(ParaPost::trivial());
}

/// A section's `w:sectPr`, in schema order: its header and footer
/// references (`refs`), a next-page break, its page size (landscape when
/// wider than tall), margins and columns, in twips.
fn section_properties_xml(refs: &str, page: &PageGeometry) -> String {
    let twips = |pt: f64| (pt * 20.0).round() as i64;
    let orient = if page.width_pt > page.height_pt { " w:orient=\"landscape\"" } else { "" };
    // The gap is written for one column too, so it reads back as it was.
    let num = if page.columns > 1 { format!(" w:num=\"{}\"", page.columns) } else { String::new() };
    format!(
        "<w:sectPr>{refs}<w:type w:val=\"nextPage\"/><w:pgSz w:w=\"{}\" w:h=\"{}\"{orient}/>\
         <w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" w:header=\"{}\" w:footer=\"{}\" w:gutter=\"0\"/>\
         <w:cols{num} w:space=\"{}\"/></w:sectPr>",
        twips(page.width_pt),
        twips(page.height_pt),
        twips(page.margin_top_pt),
        twips(page.margin_right_pt),
        twips(page.margin_bottom_pt),
        twips(page.margin_left_pt),
        twips(page.header_pt.unwrap_or(36.0)),
        twips(page.footer_pt.unwrap_or(36.0)),
        twips(page.column_gap_pt),
    )
}

/// `package` with each section's properties written over the `w:sectPr`
/// the writer left for it (`sections`, a document of several): the n-th
/// `w:sectPr` in document order is the n-th section's, the paragraph
/// breaks' and then the body's last.
///
/// The first section shows the document's own header and footer (rdocx's
/// parts, which its references on the body's last `w:sectPr` name) unless
/// it has its own. A section with its own text gets a part of its own
/// (`word/lettersSectionHeader{k}.xml`), its "{page}" and "{total}" as
/// fields; one without continues the one before and gets no reference.
/// A paragraph's `w:sectPr` goes last in its properties, where the
/// schema has it, whatever the post pass put after it.
fn with_sections(package: &[u8], sections: &[crate::model::DocSection]) -> Result<Vec<u8>, String> {
    const HEADER_TYPE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
    const FOOTER_TYPE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";
    const HEADER_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";
    const FOOTER_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer";
    const DOCUMENT: &str = "word/document.xml";
    const RELS: &str = "word/_rels/document.xml.rels";
    const TYPES: &str = "[Content_Types].xml";
    let mut zin = zip::ZipArchive::new(std::io::Cursor::new(package)).map_err(|e| e.to_string())?;
    let missing = |name: &str| format!("the package has no {name}");
    let document = part_text(&mut zin, DOCUMENT).ok_or_else(|| missing(DOCUMENT))?;
    let mut rels = part_text(&mut zin, RELS).ok_or_else(|| missing(RELS))?;
    let mut types = part_text(&mut zin, TYPES).ok_or_else(|| missing(TYPES))?;
    let spans = crate::docx_fields::section_properties(&document);
    if spans.len() != sections.len() {
        return Err(format!("{} section properties written for {} sections", spans.len(), sections.len()));
    }
    // rdocx's references to the document's own header and footer.
    fn default_tag<'a>(sect: &'a str, element: &str) -> Option<&'a str> {
        sect.match_indices(&format!("<{element} ")).find_map(|(at, _)| {
            let tag = &sect[at..at + sect[at..].find('>')? + 1];
            tag.contains("w:type=\"default\"").then_some(tag)
        })
    }
    let body_last = &document[spans[spans.len() - 1].clone()];
    let own = [default_tag(body_last, "w:headerReference"), default_tag(body_last, "w:footerReference")];

    let mut parts: Vec<(String, String)> = Vec::new();
    let mut new_rels = String::new();
    let mut new_types = String::new();
    let mut props: Vec<String> = Vec::with_capacity(sections.len());
    for (k, s) in sections.iter().enumerate() {
        let mut refs = String::new();
        for (is_header, text, own) in [(true, &s.header, own[0]), (false, &s.footer, own[1])] {
            let (element, kind, content_type, rel_type) =
                if is_header { ("w:headerReference", "Header", HEADER_TYPE, HEADER_REL) } else { ("w:footerReference", "Footer", FOOTER_TYPE, FOOTER_REL) };
            match text {
                Some(text) => {
                    let file = format!("lettersSection{kind}{k}.xml");
                    let id = format!("rIdLettersSection{kind}{k}");
                    let (xml, _) = crate::docx_anchors::header_footer_part(is_header, text, &[], Alignment::Left, &[], 0.0);
                    parts.push((format!("word/{file}"), crate::docx_fields::fields(&xml)));
                    new_rels.push_str(&format!("<Relationship Id=\"{id}\" Type=\"{rel_type}\" Target=\"{file}\"/>"));
                    new_types.push_str(&format!("<Override PartName=\"/word/{file}\" ContentType=\"{content_type}\"/>"));
                    refs.push_str(&format!("<{element} w:type=\"default\" r:id=\"{id}\"/>"));
                }
                None if k == 0 => refs.push_str(own.unwrap_or_default()),
                None => {}
            }
        }
        props.push(section_properties_xml(&refs, &s.geometry));
    }

    let mut xml = document;
    let last = spans.len() - 1;
    for (k, span) in spans.into_iter().enumerate().rev() {
        if k == last {
            xml.replace_range(span, &props[k]);
            continue;
        }
        let from = span.start;
        xml.replace_range(span, "");
        let close = xml[from..].find("</w:pPr>").map(|c| from + c);
        let change = xml[from..].find("<w:pPrChange").map(|c| from + c);
        let at = match (change, close) {
            (Some(change), Some(close)) if change < close => change,
            (_, Some(close)) => close,
            _ => from,
        };
        xml.insert_str(at, &props[k]);
    }
    if let Some(at) = rels.rfind("</Relationships>") {
        rels.insert_str(at, &new_rels);
    }
    if let Some(at) = types.rfind("</Types>") {
        types.insert_str(at, &new_types);
    }

    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for i in 0..zin.len() {
        let mut part = zin.by_index(i).map_err(|e| e.to_string())?;
        let name = part.name().map_err(|e| e.to_string())?.into_owned();
        let data = match name.as_str() {
            DOCUMENT => xml.as_bytes().to_vec(),
            RELS => rels.as_bytes().to_vec(),
            TYPES => types.as_bytes().to_vec(),
            _ => {
                let mut data = Vec::new();
                std::io::Read::read_to_end(&mut part, &mut data).map_err(|e| e.to_string())?;
                data
            }
        };
        out.start_file(name, options).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut out, &data).map_err(|e| e.to_string())?;
    }
    for (name, xml) in parts {
        out.start_file(name, options).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut out, xml.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(out.finish().map_err(|e| e.to_string())?.into_inner())
}

/// Read a DOCX and retain package members this reader does not interpret.
/// The report is suitable for a structured save-warning UI; the opaque
/// package can be passed to [`write_with_opaque`] after an unrelated edit.
pub fn read_with_report(path: &str) -> Result<(Document, suite_common_core::interop::CompatibilityReport, suite_common_core::interop::OpaquePackage), String> {
    let document = read(path)?;
    let opaque = suite_common_core::interop::OpaquePackage::capture(
        path,
        &[
            "[Content_Types].xml",
            "_rels/.rels",
            "word/document.xml",
            "word/_rels/document.xml.rels",
            // Comments are read (docx_comments) and written anew.
            "word/comments.xml",
            "word/commentsExtended.xml",
            "word/commentsIds.xml",
            "word/commentsExtensible.xml",
        ],
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
        header_pt: sect.header_distance.map(|v| v.0 as f64 / 20.0),
        footer_pt: sect.footer_distance.map(|v| v.0 as f64 / 20.0),
        border: None,
    })
    .map(|mut g| {
        g.border = sect.page_borders.as_deref().and_then(|b| page_border(b, &g));
        g
    })
}

/// A section's `w:pgBorders` as a page border, when it draws one. The
/// model keeps one rule for the four sides, the first visible edge's;
/// each side keeps its own distance from the text. `w:space` is that
/// distance when the border is measured from the text (the default),
/// and the distance from the page's edge when `w:offsetFrom="page"`,
/// which leaves what the margin has to spare.
fn page_border(b: &rdocx_oxml::document::CT_PageBorders, g: &PageGeometry) -> Option<PageBorder> {
    use rdocx_oxml::shared::ST_Border;
    let edges = [&b.top, &b.right, &b.bottom, &b.left];
    let rule = edges
        .iter()
        .filter_map(|e| e.as_ref())
        .find(|e| !matches!(e.val, ST_Border::None | ST_Border::Nil))?;
    let width_pt = rule.sz.map_or(0.5, |s| f64::from(s) / 8.0);
    let color = rule
        .color
        .as_deref()
        .filter(|c| c.len() == 6)
        .and_then(|c| u32::from_str_radix(c, 16).ok())
        .unwrap_or(0);
    let from_page = matches!(b.offset_from, Some(rdocx_oxml::document::ST_PageBorderOffset::Page));
    let margins = [g.margin_top_pt, g.margin_right_pt, g.margin_bottom_pt, g.margin_left_pt];
    let mut padding_pt = [0.0; 4];
    for (i, e) in edges.iter().enumerate() {
        let space = e.as_ref().and_then(|e| e.space).map_or(0.0, f64::from);
        padding_pt[i] = if from_page { (margins[i] - space - width_pt).max(0.0) } else { space };
    }
    Some(PageBorder { width_pt, color, padding_pt })
}

/// `w:pgBorders` for a page border, measured from the text as
/// `page_border` reads it. Word caps `w:space` at 31pt and `w:sz` at
/// 12pt (96 eighths).
fn page_borders(b: &PageBorder) -> rdocx_oxml::document::CT_PageBorders {
    let edge = |padding: f64| {
        let mut e = rdocx_oxml::borders::CT_BorderEdge::new(rdocx_oxml::shared::ST_Border::Single);
        e.sz = Some(((b.width_pt * 8.0).round() as u32).clamp(2, 96));
        e.space = Some(padding.round().clamp(0.0, 31.0) as u32);
        e.color = Some(format!("{:06X}", b.color & 0xFF_FFFF));
        Some(e)
    };
    rdocx_oxml::document::CT_PageBorders {
        offset_from: Some(rdocx_oxml::document::ST_PageBorderOffset::Text),
        top: edge(b.padding_pt[0]),
        right: edge(b.padding_pt[1]),
        bottom: edge(b.padding_pt[2]),
        left: edge(b.padding_pt[3]),
        ..Default::default()
    }
}

/// The text of a paragraph marking a table cell that a merged cell
/// covers, which `without_covered_cells` takes out of the saved XML.
const COVERED_CELL: &str = "\u{E00A}letters:covered-cell\u{E00A}";

/// `xml` without the table cells marked `COVERED_CELL`: the grid
/// positions a merged cell spans in its own row, which OOXML has no
/// cell for. The writer flattens nested tables, so a marked cell's
/// `w:tc` is the nearest one around the marker.
fn without_covered_cells(xml: &str) -> String {
    let mut out = xml.to_string();
    while let Some(at) = out.find(COVERED_CELL) {
        let start = [out[..at].rfind("<w:tc>"), out[..at].rfind("<w:tc ")].into_iter().flatten().max();
        let end = out[at..].find("</w:tc>").map(|e| at + e + "</w:tc>".len());
        match (start, end) {
            (Some(s), Some(e)) => out.replace_range(s..e, ""),
            // Not in a cell after all: drop the marker alone.
            _ => out.replace_range(at..at + COVERED_CELL.len(), ""),
        }
    }
    out
}

/// Write a Document to a .docx file.
pub fn write(doc: &Document, path: impl AsRef<std::path::Path>) -> Result<(), String> {
    let mut out = rdocx::Document::new();
    // Smart chips, written as sentinels and made content controls below.
    let mut chips: Vec<(crate::chips::Chip, String)> = Vec::new();
    let mut text_boxes: Vec<(crate::model::TextBox, crate::model::ImageAnchor)> = Vec::new();
    // Floating pictures, written inline and anchored afterwards: their
    // image relationship, alt text and placement.
    let mut floating: Vec<(String, String, crate::model::ImageAnchor)> = Vec::new();
    // Each inline picture's room beside it, in order (`docx_anchors::sides`).
    let mut inline_sides: Vec<(u64, u64)> = Vec::new();
    // Tracked changes, written as bracketed text and made w:ins/w:del below.
    let mut revisions: Vec<crate::model::Revision> = Vec::new();
    let paras = &doc.paragraphs;
    // A document of several sections (`crate::sections`) breaks after each
    // one's last paragraph, and its section properties are written whole
    // below (`with_sections`). One of one section writes as it always has.
    let sections: &[crate::model::DocSection] = if crate::sections::is_valid(doc, &doc.sections) { &doc.sections } else { &[] };
    // The section the next break starts.
    let mut next_section = 1usize;
    // Comment threads: marker runs where each one's text starts and ends,
    // made range elements below. `open` are the threads the text written
    // so far is in; a thread with no text is written, empty, first.
    let roots = crate::docx_comments::roots(doc);
    let mut open: Vec<u32> = Vec::new();
    let mut orphans: Vec<u32> = {
        let marked: std::collections::HashSet<u32> = paras.iter().flat_map(|p| &p.runs).flat_map(|r| r.style.comments.iter().copied()).collect();
        roots.iter().copied().filter(|id| !marked.contains(id)).collect()
    };
    let wanted = |run: &Run| -> Vec<u32> { crate::docx_comments::threads_of(&roots, &run.style.comments).collect() };
    // Footnote texts first: model index → docx id.
    let footnote_ids: Vec<i32> = doc.footnotes.iter().map(|t| out.add_footnote(t)).collect();
    // The text column's width, where a table of contents' page numbers go
    // (rdocx's default page: Letter with 1in margins).
    let text_width_pt = doc.page.map_or(468.0, |g| g.width_pt - g.margin_left_pt - g.margin_right_pt);
    let mut has_toc = false;

    // Per emitted document paragraph, in document order: the properties
    // rdocx cannot emit, fixed as XML below (`para_post_pass`).
    let mut post: Vec<ParaPost> = Vec::new();
    // Whether a merged cell covers a grid position (`COVERED_CELL`).
    let mut has_covered = false;
    // The numIds of the bullet and numbered list definitions, once made.
    let mut list_ids: [Option<u32>; 2] = [None, None];
    // What each numbered item shows, and what Word would show it as.
    let ordinals = crate::lists::ordinals(paras.iter().map(|p| &p.style));
    let mut word = WordNumbers::default();
    let mut i = 0;
    while i < paras.len() {
        // A new section starts here: the one before ends on the paragraph
        // last written. A table cannot carry the break, so one that ends a
        // section gets the separator after it when it has none already
        // (it has when another table follows); the reader drops it and
        // ends the section after the table, where it was.
        if sections.get(next_section).is_some_and(|s| s.range.start == i) {
            next_section += 1;
            if paras[i - 1].style.table_cell.is_some() && !needs_separator(&paras[i..]) {
                add_separator(&mut out, &mut post);
            }
            if let Some(mut last) = out.last_paragraph_mut() {
                last.set_section_break(rdocx::SectionBreak::NextPage);
            }
        }
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
            // A table that starts a page: a paragraph of nothing but a page
            // break before it, as Word writes one and the reader takes it.
            if group[0].style.page_break_before {
                out.add_paragraph("").add_run("").add_break(rdocx::BreakKind::Page);
                post.push(ParaPost::trivial());
            }
            let mut tbl = out.add_table(rows, cols);
            // The file's column widths, when the table still has as many
            // columns as it was read with; rdocx's own are equal.
            if let Some(widths) = doc.table_columns.get(&tc0.table).filter(|w| w.len() == cols) {
                for (c, w) in widths.iter().enumerate() {
                    tbl.set_column_width(c, rdocx::Length::pt(*w));
                }
            }
            // A table sized as a share of the text's width is written so:
            // all of it, its columns in proportion.
            if doc.table_relative.contains(&tc0.table) {
                tbl.set_width_pct(100.0);
            }
            if let Some(pad) = doc.table_padding.get(&tc0.table) {
                tbl.set_cell_margins(rdocx::Length::pt(pad.top_pt), rdocx::Length::pt(pad.right_pt), rdocx::Length::pt(pad.bottom_pt), rdocx::Length::pt(pad.left_pt));
            }
            // The file says Word 2013's rules, under which `w:tblInd` is the
            // table's edge: its left padding left of its text.
            let pad_left = doc.table_padding.get(&tc0.table).copied().unwrap_or_default().left_pt;
            tbl.set_indent(rdocx::Length::pt(doc.table_indent.get(&tc0.table).copied().unwrap_or(0.0) - pad_left));
            // A table is written with the rules it is drawn with: a 0.5pt
            // grid (rdocx writes none, and Word and Writer drew a Letters
            // table bare), or none for one the file drew without.
            if doc.table_unruled.contains(&tc0.table) {
                tbl.set_borders(rdocx::BorderStyle::None, 0, "auto");
            } else {
                let eighths = doc.table_rule_widths.get(&tc0.table).map_or(4, |w| (w * 8.0).round().clamp(1.0, 96.0) as u32);
                let line = if doc.table_rule_double.contains(&tc0.table) { rdocx::BorderStyle::Double } else { rdocx::BorderStyle::Single };
                tbl.set_borders(line, eighths, doc.table_rule_colors.get(&tc0.table).map_or("000000", String::as_str));
            }
            for fill in doc.table_fills.get(&tc0.table).into_iter().flatten() {
                if let Some(mut cell) = tbl.cell(fill.row as usize, fill.col as usize) {
                    cell.set_shading(&fill.color);
                }
            }
            for v in doc.table_valign.get(&tc0.table).into_iter().flatten() {
                if let Some(mut cell) = tbl.cell(v.row as usize, v.col as usize) {
                    cell.set_vertical_alignment(match v.align {
                        crate::model::VAlign::Center => rdocx::VerticalAlignment::Center,
                        crate::model::VAlign::Bottom => rdocx::VerticalAlignment::Bottom,
                    });
                }
            }
            // A cell's open sides, over the table's grid.
            for e in doc.table_open_edges.get(&tc0.table).into_iter().flatten().filter(|_| !doc.table_unruled.contains(&tc0.table)) {
                use rdocx::CellBorderEdge as C;
                if let Some(mut cell) = tbl.cell(e.row as usize, e.col as usize) {
                    for (open, side) in [(e.open.top, C::Top), (e.open.bottom, C::Bottom), (e.open.left, C::Left), (e.open.right, C::Right)] {
                        if open {
                            let _ = cell.set_border_checked(side, rdocx::BorderStyle::None, 0, "auto");
                        }
                    }
                }
            }
            for &r in doc.table_whole_rows.get(&tc0.table).into_iter().flatten() {
                if let Some(mut row) = tbl.row(r as usize) {
                    row.set_cant_split();
                }
            }
            if let Some(heights) = doc.table_rows.get(&tc0.table) {
                for (r, h) in heights.iter().enumerate().take(rows) {
                    let (Some(h), Some(mut row)) = (h, tbl.row(r)) else { continue };
                    if h.exact {
                        row.set_height_exact(rdocx::Length::pt(h.pt));
                    } else {
                        row.set_height(rdocx::Length::pt(h.pt));
                    }
                }
            }
            let mut filled = std::collections::HashSet::new();
            for p in group {
                let tc = p.style.table_cell.expect("grouped by table_cell");
                if let Some(mut cell) = tbl.cell(tc.row as usize, tc.col as usize) {
                    filled.insert((tc.row, tc.col));
                    let mut cp = cell.add_paragraph("");
                    // A cell paragraph's typography, which the reader maps
                    // the same way it maps a body paragraph's: without
                    // this a save dropped what a read kept.
                    if p.style.left_indent_pt != 0.0 {
                        cp = cp.indent_left(rdocx::Length::pt(p.style.left_indent_pt));
                    }
                    if p.style.right_indent_pt != 0.0 {
                        cp = cp.indent_right(rdocx::Length::pt(p.style.right_indent_pt));
                    }
                    if p.style.first_line_indent_pt != 0.0 {
                        cp = cp.first_line_indent(rdocx::Length::pt(p.style.first_line_indent_pt));
                    }
                    if p.style.space_before_pt != 0.0 {
                        cp = cp.space_before(rdocx::Length::pt(p.style.space_before_pt));
                    }
                    if p.style.space_after_pt != 0.0 {
                        cp = cp.space_after(rdocx::Length::pt(p.style.space_after_pt));
                    }
                    if p.style.keep_lines {
                        cp = cp.keep_together(true);
                    }
                    if let Some(control) = p.style.widow_control {
                        cp = cp.widow_control(control);
                    }
                    match p.style.line_height {
                        Some(LineHeight::Exact(pt)) => cp = cp.line_spacing(pt),
                        Some(LineHeight::Minimum(pt)) => cp = cp.line_spacing(pt),
                        None => {
                            if (p.style.line_spacing - 1.0).abs() > 0.01 {
                                cp = cp.line_spacing_multiple(p.style.line_spacing as f64);
                            }
                        }
                    }
                    if let Some(fill) = p.style.shading.as_deref().filter(|f| !f.is_empty()) {
                        cp = cp.shading(fill);
                    }
                    for stop in &p.style.tab_stops_pt {
                        cp = emit_tab_stop(cp, stop);
                    }
                    cp = match p.style.alignment {
                        Alignment::Center => cp.alignment(rdocx::Alignment::Center),
                        Alignment::Right => cp.alignment(rdocx::Alignment::Right),
                        Alignment::Justify => cp.alignment(rdocx::Alignment::Justify),
                        _ => cp,
                    };
                    post.push(ParaPost::of(&p.style));
                    // A cell's comments open and close inside it.
                    let mut in_cell: Vec<u32> = Vec::new();
                    for run in p.runs.iter().map(Some).chain([None]) {
                        let want = run.map(wanted).unwrap_or_default();
                        for (id, start) in crate::docx_comments::transition(&mut in_cell, &want) {
                            let _ = cp.add_run(&crate::docx_comments::marker(id, start));
                        }
                        let Some(run) = run else { break };
                        let mut r = broken_run(&mut cp, &run.text);
                        if run.style.bold { r = r.bold(true); } else if run.style.regular { r = r.bold(false); }
                        if !run.style.italic && run.style.upright { r = r.italic(false); }
                        if run.style.italic { r = r.italic(true); }
                        if run.style.caps { r = r.all_caps(true); }
                        if run.style.underline { r = r.underline(true); }
                        if run.style.strikethrough { r = r.strike(true); }
                        if run.style.highlight { r = r.highlight("yellow"); }
                        if run.style.code { r = r.style("SourceText"); }
                    }
                }
            }
            // A new cell already holds an empty paragraph: drop it once the
            // cell has its own, or every cell opens with a blank line in
            // Writer and Word (our reader skipped empty cell paragraphs,
            // which hid it; #1296). rdocx 0.16 keeps a cell's last
            // paragraph, so this has to come after the cell is filled.
            for (row, col) in filled {
                if let Some(mut cell) = tbl.cell(row as usize, col as usize) {
                    cell.remove_first_empty_paragraph();
                }
            }
            // Merged cells: the first spans its columns (`w:gridSpan`) and
            // starts a vertical merge (`w:vMerge`) the rows below continue.
            // The grid positions it covers in its own row are no cells in
            // OOXML: they are marked here and taken out below.
            for span in doc.table_spans.get(&tc0.table).into_iter().flatten() {
                for r in span.row..span.row + span.rows {
                    if let Some(mut cell) = tbl.cell(r as usize, span.col as usize) {
                        if span.cols > 1 {
                            cell.set_grid_span(span.cols);
                        }
                        if span.rows > 1 {
                            if r == span.row { cell.set_v_merge_restart() } else { cell.set_v_merge_continue() }
                        }
                    }
                    for c in span.col + 1..span.col + span.cols {
                        if let Some(mut cell) = tbl.cell(r as usize, c as usize) {
                            cell.add_paragraph(COVERED_CELL);
                            has_covered = true;
                        }
                    }
                }
            }
            // OOXML requires a paragraph in every cell, which a cell left
            // empty keeps from its creation, and one after a table that
            // ends the document or meets another; the reader drops it
            // again (`SEPARATOR_STYLE`).
            if needs_separator(&paras[i..]) {
                add_separator(&mut out, &mut post);
            }
            continue;
        }
        let para = &paras[i];
        i += 1;
        let level = u32::from(para.style.list_level);
        let mut p = match (para.style.list, list_ids[usize::from(para.style.list == ListKind::Numbered)]) {
            (ListKind::None, _) => out.add_paragraph(""),
            // rdocx's add_*_list_item clones the whole document on every
            // call (`clone_for_staging`), so a document with n list items
            // cost O(n²): 5,000 paragraphs took 30 s to save (#1208). It
            // only needs to allocate the list definition once; later items
            // are plain paragraphs on that numId, which is the XML the
            // builder would have written anyway.
            (ListKind::Bullet | ListKind::Numbered, Some(num_id)) => out.add_paragraph("").numbering(num_id, level),
            (ListKind::Bullet, None) => {
                let _ = out.add_bullet_list_item("", level);
                list_ids[0] = out.paragraphs().last().and_then(|p| p.numbering()).map(|(num_id, _)| num_id);
                out.last_paragraph_mut().expect("the list item just added")
            }
            // Every level "N.", as Letters draws a numbered item with no
            // label of its own: rdocx's own definition letters the second
            // level and numbers the third in roman, which Word and
            // LibreOffice showed and the reader read back as labels.
            (ListKind::Numbered, None) => {
                let levels = vec![rdocx::ListLevel::decimal(); 9];
                let id = out.add_numbering_definition(&levels).and_then(|d| out.add_numbering_instance(d, &[]));
                match id {
                    Ok(id) => {
                        list_ids[1] = Some(id);
                        out.add_paragraph("").numbering(id, level)
                    }
                    Err(_) => {
                        let _ = out.add_numbered_list_item("", level);
                        list_ids[1] = out.paragraphs().last().and_then(|p| p.numbering()).map(|(num_id, _)| num_id);
                        out.last_paragraph_mut().expect("the list item just added")
                    }
                }
            }
        };
        if let Some(level) = para.style.heading {
            p = p.style(&format!("Heading{}", level.clamp(1, 6)));
        }
        if para.style.block_quote {
            p = p.style("Quote");
        }
        if para.style.code_block.is_some() && para.style.heading.is_none() {
            p = p.style("PreformattedText");
        }
        if let Some(name) = &para.style.named_style {
            p = p.style(name);
        }
        if para.style.page_break_before {
            p = p.page_break_before(true);
        }
        if para.style.keep_with_next {
            p = p.keep_with_next(true);
        }
        if para.style.keep_lines {
            p = p.keep_together(true);
        }
        if let Some(control) = para.style.widow_control {
            p = p.widow_control(control);
        }
        match para.style.line_height {
            // Exact has a builder; atLeast does not
            // (`set_line_spacing_at_least` is crate-internal), so a
            // minimum is written exact here and fixed by the post pass
            // below (`para_post_pass`).
            Some(LineHeight::Exact(pt)) => p = p.line_spacing(pt),
            Some(LineHeight::Minimum(pt)) => {
                p = p.line_spacing(pt);
            }
            None => {
                if (para.style.line_spacing - 1.0).abs() > 0.01 {
                    p = p.line_spacing_multiple(para.style.line_spacing as f64);
                }
            }
        }
        if let Some(fill) = para.style.shading.as_deref().filter(|f| !f.is_empty()) {
            p = p.shading(fill);
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
        if para.style.list != ListKind::None {
            // A list item says where its text and marker go, as Letters
            // draws them: the text one list indent per level in from the
            // paragraph's own indent, the marker one hanging indent before
            // it. Left to rdocx's numbering definition (0.5in per level)
            // LibreOffice and Word drew our lists twice as deep as we do;
            // the edit-render journey's bullet showed it (#1201). The reader
            // takes the list's own indent back off.
            let text = (para.style.left_indent_pt + crate::lists::text_indent_pt(para.style.list_level)).max(0.0);
            let hanging = para.style.list_hanging_pt.unwrap_or(crate::lists::HANGING_PT);
            p = p.indent_left(rdocx::Length::pt(text)).hanging_indent(rdocx::Length::pt(hanging));
        } else if para.style.left_indent_pt != 0.0 {
            p = p.indent_left(rdocx::Length::pt(para.style.left_indent_pt));
        }
        if para.style.right_indent_pt != 0.0 {
            p = p.indent_right(rdocx::Length::pt(para.style.right_indent_pt));
        }
        if para.style.first_line_indent_pt != 0.0 && para.style.list == ListKind::None {
            p = p.first_line_indent(rdocx::Length::pt(para.style.first_line_indent_pt));
        }
        if para.style.space_before_pt != 0.0 {
            p = p.space_before(rdocx::Length::pt(para.style.space_before_pt));
        }
        if para.style.space_after_pt != 0.0 {
            p = p.space_after(rdocx::Length::pt(para.style.space_after_pt));
        }
        // Tab stops with their alignments and leaders.
        // A table of contents entry: Word's TOC style for its level, and
        // its page number at a right tab with a dot leader at the margin.
        if let Some(level) = para.style.toc {
            p = p.style(&format!("TOC{}", level.clamp(1, 9)));
            p = p.add_tab_stop_with_leader(rdocx::TabAlignment::Right, rdocx::Length::pt(text_width_pt), rdocx::TabLeader::Dot);
        } else {
            for stop in &para.style.tab_stops_pt {
                p = emit_tab_stop(p, stop);
            }
        }
        p = match para.style.alignment {
            Alignment::Left => p,
            Alignment::Center => p.alignment(rdocx::Alignment::Center),
            Alignment::Right => p.alignment(rdocx::Alignment::Right),
            Alignment::Justify => p.alignment(rdocx::Alignment::Justify),
        };
        // Properties rdocx cannot emit (atLeast line rules, per-side
        // borders) are fixed as XML below, in document order.
        post.push(ParaPost::of(&para.style));
        let _ = p; // release the builder borrow before append_hyperlink
        // A numbered item Word would number otherwise: a restart (#1205),
        // or the first item of a list after a paragraph that ended the one
        // before, which Word would count on from it. It gets its own
        // instance of the numbered list's definition, with a start
        // override at its level, which the items after it continue.
        if let (ListKind::Numbered, Some(mut num_id)) = (para.style.list, list_ids[1]) {
            // So does an item labelled otherwise than its level of the
            // instance: the new instance's override replaces the level, and
            // keeps the labels the instance gave its other levels.
            let number = ordinals[i - 1];
            let label = &para.style.list_label;
            if word.next(num_id, level) != number || word.label(num_id, level) != label.as_ref() {
                let definition = out.numbering_instance(num_id).map(|n| n.definition_id);
                let mut labels = word.labels.get(&num_id).cloned().unwrap_or_default();
                labels[level.min(8) as usize] = label.clone();
                let overrides: Vec<rdocx::NumberingLevelOverride> = (0..9u32)
                    .filter(|&l| l == level || labels[l as usize].is_some())
                    .map(|l| rdocx::NumberingLevelOverride {
                        level: l,
                        start: (l == level).then_some(number),
                        replacement: (labels[l as usize].is_some() || word.label(num_id, l).is_some())
                            .then(|| word_level(labels[l as usize].as_ref(), l, (l == level).then_some(number))),
                        paragraph_style_link: None,
                        has_unmodeled_properties: false,
                    })
                    .collect();
                if let Some(Ok(id)) = definition.map(|d| out.add_numbering_instance(d, &overrides)) {
                    out.last_paragraph_mut().expect("the list item").set_numbering(id, level);
                    list_ids[1] = Some(id);
                    word.restart(id, level, number);
                    word.labels.insert(id, labels);
                    num_id = id;
                }
            }
            word.count(num_id, level, number);
        }
        // A table of contents is Word's TOC field around its entries.
        let toc_edge = |k: usize| paras.get(k).is_none_or(|q| q.style.toc.is_none());
        if para.style.toc.is_some() && (i < 2 || toc_edge(i - 2)) {
            let _ = out.last_paragraph_mut().expect("paragraph").add_run(&crate::docx_toc::begin_marker());
            has_toc = true;
        }
        for id in std::mem::take(&mut orphans) {
            for start in [true, false] {
                let _ = out.last_paragraph_mut().expect("paragraph").add_run(&crate::docx_comments::marker(id, start));
            }
        }
        for run in &para.runs {
            for (id, start) in crate::docx_comments::transition(&mut open, &wanted(run)) {
                let _ = out.last_paragraph_mut().expect("paragraph").add_run(&crate::docx_comments::marker(id, start));
            }
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
                        // A floating picture stays in its paragraph, where
                        // its anchor is: written inline there, and made a
                        // `wp:anchor` once the part is written (docx_anchors).
                        if let Some(anchor) = run.style.image_anchor {
                            let rel = out.embed_image(&bytes, &name);
                            let mut p = out.last_paragraph_mut().expect("paragraph");
                            p.add_run("").add_picture(&rel, w, h);
                            floating.push((rel, run.text.clone(), anchor));
                            continue;
                        }
                        inline_sides.push(run.style.image_sides_emu.unwrap_or((0, 0)));
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
            // A text box: a sentinel run, the drawing once rdocx has
            // written the part (docx_text_boxes::wrap).
            if let Some(tb) = &run.style.text_box {
                let mark = crate::docx_text_boxes::sentinel(text_boxes.len());
                text_boxes.push((tb.as_ref().clone(), run.style.image_anchor.unwrap_or_default()));
                let _ = out.last_paragraph_mut().expect("paragraph").add_run(&mark);
                continue;
            }
            if let Some(chip) = &run.style.chip {
                let mark = crate::docx_chips::sentinel(chips.len());
                chips.push((chip.clone(), run.text.clone()));
                match &run.style.link {
                    Some(url) => out.append_hyperlink(&mark, url),
                    None => {
                        let _ = out.last_paragraph_mut().expect("paragraph").add_run(&mark);
                    }
                }
                continue;
            }
            if let Some(url) = &run.style.link {
                // Hyperlinks need a document-level relationship; styles on
                // link text are not yet carried through append_hyperlink.
                let text = bracketed(&mut revisions, run);
                out.append_hyperlink(&text, url);
                continue;
            }
            let mut p = out.last_paragraph_mut().expect("paragraph just added");
            let text = bracketed(&mut revisions, run);
            let mut r = broken_run(&mut p, &text);
            if run.style.bold { r = r.bold(true); } else if run.style.regular { r = r.bold(false); }
            if !run.style.italic && run.style.upright { r = r.italic(false); }
            if run.style.italic { r = r.italic(true); }
            if run.style.caps { r = r.all_caps(true); }
            if run.style.underline { r = r.underline(true); }
            if run.style.strikethrough { r = r.strike(true); }
            if run.style.highlight { r = r.highlight("yellow"); }
            if run.style.code { r = r.style("SourceText"); }
            if let Some(f) = &run.style.font_family { r = r.font(f); }
            if let Some(hp) = run.style.font_size_hp { r = r.size(hp as f64 / 2.0); }
            if let Some(c) = &run.style.color { r = r.color(c); }
            if let Some(tw) = run.style.letter_spacing_tw { r = r.character_spacing(rdocx::Length::pt(f64::from(tw) / 20.0)); }
            if let Some(hp) = run.style.position_hp { r = r.position(hp); }
            match run.style.vert_align {
                Some(crate::model::VertAlign::Superscript) => { r = r.superscript(); }
                Some(crate::model::VertAlign::Subscript) => { r = r.subscript(); }
                None => {}
            }
        }
        if para.style.toc.is_some() && toc_edge(i) {
            let _ = out.last_paragraph_mut().expect("paragraph").add_run(&crate::docx_toc::end_marker());
        }
        // Close the threads the next paragraph's text is not in (all of
        // them before a table, whose cells hold their own).
        let next: Vec<u32> = match paras.get(i) {
            Some(n) if n.style.table_cell.is_none() => n.runs.first().map(wanted).unwrap_or_default(),
            _ => Vec::new(),
        };
        let keep: Vec<u32> = open.iter().copied().filter(|id| next.contains(id)).collect();
        for (id, start) in crate::docx_comments::transition(&mut open, &keep) {
            let _ = out.last_paragraph_mut().expect("paragraph").add_run(&crate::docx_comments::marker(id, start));
        }
    }
    // A header or footer with pictures, aligned, or with lines of its own
    // look is written whole, its pictures in its first paragraph
    // (docx_anchors::header_footer_part).
    for (is_header, text, pictures, alignment, lines) in [
        (true, &doc.header, &doc.header_pictures, doc.header_alignment, &doc.header_lines),
        (false, &doc.footer, &doc.footer_pictures, doc.footer_alignment, &doc.footer_lines),
    ] {
        if pictures.is_empty() && alignment == Alignment::Left && lines.is_empty() {
            match (is_header, text) {
                (true, Some(h)) => out.set_header(h),
                (false, Some(f)) => out.set_footer(f),
                _ => {}
            }
            continue;
        }
        if text.is_none() && pictures.is_empty() {
            continue;
        }
        let (xml, images) = crate::docx_anchors::header_footer_part(is_header, text.as_deref().unwrap_or(""), pictures, alignment, lines, doc.page.unwrap_or_default().width_pt);
        let images: Vec<(&str, &[u8], &str)> = images.iter().map(|(r, b, n)| (r.as_str(), b.as_slice(), n.as_str())).collect();
        if is_header {
            out.set_raw_header_with_images(xml.into_bytes(), &images, rdocx::HdrFtrType::Default);
        } else {
            out.set_raw_footer_with_images(xml.into_bytes(), &images, rdocx::HdrFtrType::Default);
        }
    }
    // The first page's own header and footer: Word's "different first
    // page" gives it both, so the one it does not have of its own is the
    // others'. A document of several sections keeps its first section's
    // properties apart (below), and its first page the others' parts.
    if (doc.first_header.is_some() || doc.first_footer.is_some()) && sections.is_empty() {
        out.set_different_first_page(true);
        let page_w = doc.page.unwrap_or_default().width_pt;
        for (is_header, first, text, pictures, alignment, lines) in [
            (true, &doc.first_header, &doc.header, &doc.header_pictures, doc.header_alignment, &doc.header_lines),
            (false, &doc.first_footer, &doc.footer, &doc.footer_pictures, doc.footer_alignment, &doc.footer_lines),
        ] {
            let (xml, images) = match first {
                Some(s) => crate::docx_anchors::header_footer_part(is_header, &s.text, &s.pictures, s.alignment, &s.lines, page_w),
                None => crate::docx_anchors::header_footer_part(is_header, text.as_deref().unwrap_or(""), pictures, alignment, lines, page_w),
            };
            let images: Vec<(&str, &[u8], &str)> = images.iter().map(|(r, b, n)| (r.as_str(), b.as_slice(), n.as_str())).collect();
            if is_header {
                out.set_raw_header_with_images(xml.into_bytes(), &images, rdocx::HdrFtrType::First);
            } else {
                out.set_raw_footer_with_images(xml.into_bytes(), &images, rdocx::HdrFtrType::First);
            }
        }
    }
    if let Some(pg) = &doc.page {
        out.set_page_size(rdocx::Length::pt(pg.width_pt), rdocx::Length::pt(pg.height_pt));
        out.set_margins(
            rdocx::Length::pt(pg.margin_top_pt),
            rdocx::Length::pt(pg.margin_right_pt),
            rdocx::Length::pt(pg.margin_bottom_pt),
            rdocx::Length::pt(pg.margin_left_pt),
        );
        if pg.header_pt.is_some() || pg.footer_pt.is_some() {
            out.set_header_footer_distance(rdocx::Length::pt(pg.header_pt.unwrap_or(36.0)), rdocx::Length::pt(pg.footer_pt.unwrap_or(36.0)));
        }
        // A single column is the default section layout, and `w:cols`
        // with `w:num="1"` is noise; more than one has to be written or a
        // two-column document saves as one.
        if pg.columns > 1 {
            out.set_columns(pg.columns as u32, rdocx::Length::pt(pg.column_gap_pt));
        }
        if let Some(b) = pg.border {
            out.section_properties_mut().page_borders = Some(Box::new(page_borders(&b)));
        }
    }
    let bytes = out
        .to_bytes()
        .map_err(|e| format!("Cannot save {}: {}", path.as_ref().display(), e))?;
    let bytes = with_letters_styles(&bytes, &doc.base_font, &doc.heading_styles)
        .map_err(|e| format!("Cannot save {}: {}", path.as_ref().display(), e))?;
    let tabbed = paras.iter().flat_map(|p| &p.runs).any(|r| r.text.contains('\t'));
    let has_post = post.iter().any(ParaPost::nontrivial);
    let bytes = if chips.is_empty() && revisions.is_empty() && doc.comments.is_empty() && !has_toc && !tabbed && !has_covered && floating.is_empty() && !has_post && inline_sides.iter().all(|&s| s == (0, 0)) && text_boxes.is_empty() {
        bytes
    } else {
        with_part(&bytes, "word/document.xml", |xml| {
            let xml = if tabbed { tabs_as_elements(xml) } else { xml.to_string() };
            let xml = if has_covered { without_covered_cells(&xml) } else { xml };
            // After covered cells are gone: the remaining paragraphs run
            // with `post` in document order.
            let xml = if has_post { para_post_pass(&xml, &post) } else { xml };
            let xml = if floating.is_empty() { xml } else { crate::docx_anchors::float(&xml, &floating, 9001) };
            // After floating pictures are anchors: the inline ones are left.
            let xml = crate::docx_anchors::sides(&xml, &inline_sides);
            let xml = if text_boxes.is_empty() { xml } else { crate::docx_text_boxes::wrap(&xml, &text_boxes, 9400) };
            let xml = crate::docx_revisions::wrap(&crate::docx_chips::wrap(&xml, &chips), &revisions);
            let xml = crate::docx_comments::wrap(&xml, &doc.comments);
            if has_toc { crate::docx_toc::wrap(&xml) } else { xml }
        })
        .map_err(|e| format!("Cannot save {}: {}", path.as_ref().display(), e))?
    };
    let bytes = if doc.comments.is_empty() {
        bytes
    } else {
        crate::docx_comments::add_parts(&bytes, &doc.comments).map_err(|e| format!("Cannot save {}: {}", path.as_ref().display(), e))?
    };
    // A document of several sections: each one's page, header and footer,
    // over the document's last pass, and ahead of the fields below, which
    // find the document's header and footer through the first section.
    let bytes = if sections.is_empty() {
        bytes
    } else {
        with_sections(&bytes, sections).map_err(|e| format!("Cannot save {}: {}", path.as_ref().display(), e))?
    };
    // "{page}" and "{total}" in the header and footer become Word's PAGE
    // and NUMPAGES fields, so every page shows its own number.
    let fielded = |t: &Option<String>| t.as_deref().is_some_and(|t| t.contains("{page}") || t.contains("{total}"));
    let bytes = if fielded(&doc.header) || fielded(&doc.footer) {
        let parts = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).ok().and_then(|mut z| {
            Some(crate::docx_fields::default_parts(&part_text(&mut z, "word/document.xml")?, &part_text(&mut z, "word/_rels/document.xml.rels")?))
        });
        let mut bytes = bytes;
        for part in parts.into_iter().flat_map(|(h, f)| [h, f]).flatten() {
            bytes = with_part(&bytes, &part, crate::docx_fields::fields)
                .map_err(|e| format!("Cannot save {}: {}", path.as_ref().display(), e))?;
        }
        bytes
    } else {
        bytes
    };
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
        let name = f.name().map_err(|e| e.to_string())?.into_owned();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut data).map_err(|e| e.to_string())?;
        if name == "word/styles.xml" {
            let xml = String::from_utf8(data).map_err(|e| e.to_string())?;
            data = patch_styles_xml(&xml, &defaults, &headings).into_bytes();
        } else if name.starts_with("word/theme/") && name.ends_with(".xml") {
            let xml = String::from_utf8(data).map_err(|e| e.to_string())?;
            data = patch_theme_fonts(&xml, &family).into_bytes();
        }
        out.start_file(name, options).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut out, &data).map_err(|e| e.to_string())?;
    }
    Ok(out.finish().map_err(|e| e.to_string())?.into_inner())
}

/// Name `family` as the theme's heading and body fonts (`a:majorFont` and
/// `a:minorFont`'s Latin typeface). rdocx 0.14 writes Office's default
/// theme, whose fonts are Aptos Display and Aptos: the text still renders
/// in `family` (docDefaults names it outright), but Word offers the theme's
/// fonts as the document's "(Headings)" and "(Body)" fonts, and anything
/// that resolves a theme font would get Aptos, usually not installed.
fn patch_theme_fonts(xml: &str, family: &str) -> String {
    let mut xml = xml.to_string();
    for scheme in ["<a:majorFont>", "<a:minorFont>"] {
        let Some(at) = xml.find(scheme) else { continue };
        let Some(latin) = xml[at..].find("<a:latin ").map(|l| at + l) else { continue };
        let Some(end) = xml[latin..].find("/>").map(|e| latin + e + 2) else { continue };
        xml.replace_range(latin..end, &format!("<a:latin typeface=\"{family}\"/>"));
    }
    xml
}

/// Replace `styles.xml`'s docDefaults and its Heading1–6 styles.
fn patch_styles_xml(xml: &str, defaults: &str, headings: &str) -> String {
    let mut xml = xml.to_string();
    if let (Some(a), Some(b)) = (xml.find("<w:docDefaults"), xml.find("</w:docDefaults>")) {
        xml.replace_range(a..b + "</w:docDefaults>".len(), defaults);
    } else if let Some(at) = xml.find("<w:style ") {
        xml.insert_str(at, defaults);
    }
    // rdocx's template (0.15 on) defines Word's look for these. Letters
    // writes its own headings and quote below, and leaves Title and
    // Subtitle undefined so a Letters Title reopens with Letters' look
    // rather than as a file's own (`own_look`, #1599).
    let replaced = (1..=6).map(|n| format!("Heading{n}")).chain(["Title", "Subtitle", "Quote"].map(String::from));
    for style_id in replaced {
        let id = format!("w:styleId=\"{style_id}\"");
        while let Some(at) = xml.find(&id) {
            let Some(start) = xml[..at].rfind("<w:style ") else { break };
            let Some(len) = xml[start..].find("</w:style>") else { break };
            xml.replace_range(start..start + len + "</w:style>".len(), "");
        }
    }
    if let Some(end) = xml.rfind("</w:styles>") {
        xml.insert_str(end, headings);
    }
    // The quote, code block and inline code styles paragraphs and runs
    // name, which the template lacks (#1205). A style a paragraph names
    // but the file doesn't define is dropped by LibreOffice, so a block
    // quote and inline code read back as plain text after a pass through
    // Writer. The names are the ones Writer itself uses for these.
    for (id, def) in [
        ("Quote", "<w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:ind w:left=\"360\"/></w:pPr><w:rPr><w:i/></w:rPr></w:style>"),
        ("PreformattedText", "<w:style w:type=\"paragraph\" w:styleId=\"PreformattedText\"><w:name w:val=\"Preformatted Text\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/><w:pPr><w:spacing w:before=\"0\" w:after=\"0\"/></w:pPr><w:rPr><w:rFonts w:ascii=\"Liberation Mono\" w:hAnsi=\"Liberation Mono\" w:cs=\"Liberation Mono\"/><w:sz w:val=\"20\"/></w:rPr></w:style>"),
        ("SourceText", "<w:style w:type=\"character\" w:styleId=\"SourceText\"><w:name w:val=\"Source Text\"/><w:qFormat/><w:rPr><w:rFonts w:ascii=\"Liberation Mono\" w:hAnsi=\"Liberation Mono\" w:cs=\"Liberation Mono\"/></w:rPr></w:style>"),
    ] {
        if !xml.contains(&format!("w:styleId=\"{id}\"")) {
            if let Some(end) = xml.rfind("</w:styles>") {
                xml.insert_str(end, def);
            }
        }
    }
    // Word's table of contents entry styles, which the template lacks; a
    // table of contents' entries name them (their indents are direct).
    if !xml.contains("w:styleId=\"TOC1\"") {
        let toc: String = (1..=9)
            .map(|n| format!("<w:style w:type=\"paragraph\" w:styleId=\"TOC{n}\"><w:name w:val=\"toc {n}\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"39\"/><w:unhideWhenUsed/></w:style>"))
            .collect();
        if let Some(end) = xml.rfind("</w:styles>") {
            xml.insert_str(end, &toc);
        }
    }
    xml
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Write a DOCX and merge previously captured, non-conflicting package parts.
///
/// The merge is a single atomic replace (LET-002): the writer writes a
/// staging file beside `path`, the parts are merged into those staged bytes,
/// and only the merged bytes ever replace `path` — there is no moment on
/// disk with the edit but without the carried parts.
///
/// The live save path carries only the admitted safe subset
/// (`DocumentSession::carried_for_save` — custom XML, thumbnail, ODF
/// settings, per `suite_common_core::carry`); anything else the model cannot
/// keep routes to the LET-001 loss dialog instead of being passed through.
pub fn write_with_opaque(
    doc: &Document,
    path: impl AsRef<std::path::Path>,
    opaque: &suite_common_core::interop::OpaquePackage,
) -> Result<(), String> {
    let path = path.as_ref();
    crate::save::opaque_single_write(path, |p| write(doc, p), opaque)
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

/// A page break in a table cell breaks nothing: Writer lays the cell out
/// as if it were not there (the WGA form's logo cell opens with two, and
/// its logo sits at the top of the cell). Drop the flag and the newlines
/// the breaks before and after the cell's text left in it.
fn drop_cell_page_breaks(para: &mut Paragraph, p: &rdocx::ParagraphRef<'_>) {
    let (mut before, mut after, mut seen_text) = (0usize, 0usize, false);
    for r in p.runs() {
        for item in r.items() {
            match item {
                rdocx::RunItemRef::Break(rdocx::BreakKind::Page) if seen_text => after += 1,
                rdocx::RunItemRef::Break(rdocx::BreakKind::Page) => before += 1,
                rdocx::RunItemRef::Text(t) if !t.is_empty() => {
                    seen_text = true;
                    after = 0;
                }
                _ => {}
            }
        }
    }
    if before + after == 0 {
        return;
    }
    para.style.page_break_before = false;
    let strip = |runs: &mut dyn Iterator<Item = &mut Run>, mut n: usize, front: bool| {
        for r in runs {
            if n == 0 || r.style.image.is_some() || r.style.footnote.is_some() {
                break;
            }
            while n > 0 && (if front { r.text.starts_with('\n') } else { r.text.ends_with('\n') }) {
                if front { r.text.remove(0); } else { r.text.pop(); }
                n -= 1;
            }
            if !r.text.is_empty() {
                break;
            }
        }
    };
    strip(&mut para.runs.iter_mut(), before, true);
    strip(&mut para.runs.iter_mut().rev(), after, false);
    para.runs.retain(|r| !r.text.is_empty() || r.style.image.is_some() || r.style.footnote.is_some());
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

/// A numbering level labelled `label` ("N." for `None`), Word's level
/// text: the prefix, the level's own placeholder, the suffix.
fn word_level(label: Option<&ListLabel>, level: u32, start: Option<u32>) -> rdocx::ListLevel {
    let plain = ListLabel { suffix: ".".into(), ..Default::default() };
    let label = label.unwrap_or(&plain);
    let format = match label.format {
        NumberFormat::Decimal => rdocx::ListNumberFormat::Decimal,
        NumberFormat::LowerLetter => rdocx::ListNumberFormat::LowerLetter,
        NumberFormat::UpperLetter => rdocx::ListNumberFormat::UpperLetter,
        NumberFormat::LowerRoman => rdocx::ListNumberFormat::LowerRoman,
        NumberFormat::UpperRoman => rdocx::ListNumberFormat::UpperRoman,
        NumberFormat::None => rdocx::ListNumberFormat::None,
    };
    // A "%" of the label's own would read as a placeholder.
    let literal = |t: &str| t.replace('%', "");
    let own = if label.format == NumberFormat::None { String::new() } else { format!("%{}", level + 1) };
    let level = rdocx::ListLevel::new(format).level_text(format!("{}{own}{}", literal(&label.prefix), literal(&label.suffix)));
    match start {
        Some(n) => level.start(n),
        None => level,
    }
}

/// The numbers Word will give the numbered items written so far: per list
/// instance and level, the last number shown and where a level starts.
#[derive(Default)]
struct WordNumbers {
    counts: std::collections::HashMap<u32, [Option<u32>; 9]>,
    starts: std::collections::HashMap<(u32, u32), u32>,
    /// The levels an instance labels otherwise than "N." (`word_level`).
    labels: std::collections::HashMap<u32, [Option<ListLabel>; 9]>,
}

impl WordNumbers {
    fn label(&self, num_id: u32, level: u32) -> Option<&ListLabel> {
        self.labels.get(&num_id)?[level.min(8) as usize].as_ref()
    }

    /// The number Word gives the next item at `level` of `num_id`.
    fn next(&self, num_id: u32, level: u32) -> u32 {
        let last = self.counts.get(&num_id).and_then(|c| c[level.min(8) as usize]);
        last.map_or_else(|| self.starts.get(&(num_id, level)).copied().unwrap_or(1), |n| n + 1)
    }

    fn restart(&mut self, num_id: u32, level: u32, start: u32) {
        self.starts.insert((num_id, level), start);
    }

    /// Count an item numbered `n` at `level`, which restarts the levels
    /// below it.
    fn count(&mut self, num_id: u32, level: u32, n: u32) {
        let counts = self.counts.entry(num_id).or_default();
        let at = level.min(8) as usize;
        counts[at] = Some(n);
        counts[at + 1..].iter_mut().for_each(|c| *c = None);
    }
}

/// The numbers Word gives numbered list items, in body order.
///
/// Word keeps one count per list instance (`numId`) and level. A paragraph
/// that is not in the list does not end it: the minutes that number
/// "1.1", then a paragraph of discussion, then "1.2" are one list, which
/// this model's own count (`lists::ordinals`) would restart at every plain
/// paragraph. An item restarts the levels below its own, and a level's
/// first item starts at the level's `w:start`, or at the instance's
/// restart (`list_start`, read from its `w:startOverride`).
#[derive(Default)]
struct WordCounter {
    counts: std::collections::HashMap<u32, [Option<u32>; 9]>,
}

impl WordCounter {
    /// Count the item `p`, styled `style`, and give it its label
    /// (`list_label`); Word's number for it, when it is a numbered item.
    fn number(&mut self, doc: &rdocx::Document, p: &rdocx::ParagraphRef<'_>, style: &mut ParaStyle) -> Option<u32> {
        if style.list != ListKind::Numbered {
            return None;
        }
        let (num_id, level) = paragraph_numbering(doc, p)?;
        let at = usize::from(style.list_level);
        let counts = self.counts.entry(num_id).or_default();
        counts[at + 1..].iter_mut().for_each(|c| *c = None);
        let first = |l: u32| numbering_level(doc, num_id, l).map_or(1, |l| l.0);
        let n = style.list_start.unwrap_or_else(|| counts[at].map_or_else(|| first(level), |c| c + 1));
        counts[at] = Some(n);
        // The levels above this one, as Word shows them in "%1.%2": the
        // number each is at, or its first number before any item.
        let shown = |k: u32| counts.get(k as usize).copied().flatten().unwrap_or_else(|| first(k));
        let format = |k: u32| numbering_level(doc, num_id, k).map_or(NumberFormat::Decimal, |l| l.1);
        let text = numbering_level(doc, num_id, level).and_then(|l| l.2);
        style.list_label = text.and_then(|t| word_label(&t, level, format(level), |k| (shown(k), format(k))));
        Some(n)
    }
}

/// Level `level` of list instance `num_id`: its first number, format and
/// level text. The instance's own replacement of the level (a
/// `w:lvlOverride` holding a `w:lvl`) wins over its definition's.
fn numbering_level(doc: &rdocx::Document, num_id: u32, level: u32) -> Option<(u32, NumberFormat, Option<String>)> {
    if let Some(l) = level_replacement(doc, num_id, level) {
        let format = match l.format {
            rdocx::ListNumberFormat::LowerLetter => NumberFormat::LowerLetter,
            rdocx::ListNumberFormat::UpperLetter => NumberFormat::UpperLetter,
            rdocx::ListNumberFormat::LowerRoman => NumberFormat::LowerRoman,
            rdocx::ListNumberFormat::UpperRoman => NumberFormat::UpperRoman,
            rdocx::ListNumberFormat::None => NumberFormat::None,
            _ => NumberFormat::Decimal,
        };
        return Some((l.start.unwrap_or(1), format, l.level_text_value().map(str::to_string)));
    }
    let l = doc.numbering_level(num_id, level)?;
    Some((l.start, number_format(l.format_name), l.level_text.map(str::to_string)))
}

/// List instance `num_id`'s own replacement of level `level` (a
/// `w:lvlOverride` holding a `w:lvl`), which wins over its definition's:
/// a list whose definition only links a numbering style (`w:numStyleLink`)
/// gives its levels this way, "6.%2." numbered minutes among them.
fn level_replacement(doc: &rdocx::Document, num_id: u32, level: u32) -> Option<rdocx::ListLevel> {
    doc.numbering_instance(num_id)?.level_overrides.into_iter().find(|o| o.level == level)?.replacement
}

/// Whether level `level` of list instance `num_id` is bulleted: its
/// replacement's format, else its definition's.
fn level_is_bullet(doc: &rdocx::Document, num_id: u32, level: u32) -> Option<bool> {
    match level_replacement(doc, num_id, level) {
        Some(l) => Some(l.format == rdocx::ListNumberFormat::Bullet),
        None => doc.numbering_level(num_id, level).map(|l| l.format_name == "bullet"),
    }
}

/// Level `level`'s left and hanging indents, in points: its replacement's,
/// else its definition's.
fn level_indents(doc: &rdocx::Document, num_id: u32, level: u32) -> (Option<f64>, Option<f64>) {
    match level_replacement(doc, num_id, level) {
        Some(l) => {
            let (left, hanging, _) = l.indentation_value();
            (left.map(twips_pt), hanging.map(twips_pt))
        }
        None => doc.numbering_level(num_id, level).map_or((None, None), |l| (l.indent_left.map(twips_pt), l.indent_hanging.map(twips_pt))),
    }
}

/// A numbering format name (`w:numFmt`) as this model draws it; the many
/// it has no digits for are drawn as decimals, LibreOffice's fallback.
fn number_format(name: &str) -> NumberFormat {
    match name {
        "lowerLetter" => NumberFormat::LowerLetter,
        "upperLetter" => NumberFormat::UpperLetter,
        "lowerRoman" => NumberFormat::LowerRoman,
        "upperRoman" => NumberFormat::UpperRoman,
        "none" => NumberFormat::None,
        _ => NumberFormat::Decimal,
    }
}

/// The label of an item at `level` whose level text (`w:lvlText`) is
/// `text`: the text before the item's own placeholder is its prefix and
/// the text after its suffix, each with the placeholders of the levels
/// above as `above` gives them (`%1` is level 0). With no placeholder of
/// its own, as "2.1", the text is all prefix and no number shows. `None`
/// for "N.", the label every numbered item has without one.
fn word_label(text: &str, level: u32, format: NumberFormat, above: impl Fn(u32) -> (u32, NumberFormat)) -> Option<ListLabel> {
    let own = format!("%{}", level + 1);
    let fill = |part: &str| {
        let mut out = String::new();
        let mut chars = part.chars().peekable();
        while let Some(c) = chars.next() {
            match (c, chars.peek().and_then(|d| d.to_digit(10))) {
                ('%', Some(d @ 1..=9)) => {
                    chars.next();
                    let (n, f) = above(d - 1);
                    out.push_str(&crate::lists::format_number(f, n));
                }
                _ => out.push(c),
            }
        }
        out
    };
    let label = match text.find(&own) {
        Some(at) => ListLabel { prefix: fill(&text[..at]), format, suffix: fill(&text[at + own.len()..]) },
        None => ListLabel { prefix: fill(text), format: NumberFormat::None, suffix: String::new() },
    };
    (label != ListLabel { prefix: String::new(), format: NumberFormat::Decimal, suffix: ".".into() }).then_some(label)
}

/// Give each numbered item Word's number (`WordCounter`) where this
/// model's count would show another, by restarting the count there. Where
/// the two agree nothing changes, so a save writes the list as it was.
fn number_as_word_does(paragraphs: &mut [Paragraph], word_numbers: &[Option<u32>]) {
    // A restart this model's count makes anyway is no restart: the first
    // item of a list, which a save gives its own instance starting at 1.
    for k in 0..paragraphs.len() {
        let Some(start) = paragraphs[k].style.list_start else { continue };
        paragraphs[k].style.list_start = None;
        if crate::lists::ordinals(paragraphs[..=k].iter().map(|p| &p.style))[k] != start {
            paragraphs[k].style.list_start = Some(start);
        }
    }
    loop {
        let ours = crate::lists::ordinals(paragraphs.iter().map(|p| &p.style));
        let differs = word_numbers.iter().zip(&ours).position(|(w, o)| w.is_some_and(|w| w != *o));
        let Some(k) = differs else { return };
        paragraphs[k].style.list_start = word_numbers[k];
    }
}

/// Whether a run reads as highlighted: a named Word highlight other than
/// "none", or a shading fill that shows. Forms pasted from the web shade
/// whole paragraphs white (`w:shd w:fill="FFFFFF"`) on a white page; read
/// as a highlight, that text drew yellow where Word and LibreOffice draw
/// nothing.
fn run_is_highlighted(r: &rdocx::RunRef<'_>) -> bool {
    if let Some(name) = r.highlight_color() {
        return name != "none";
    }
    r.shading_fill().is_some_and(|fill| !fill.eq_ignore_ascii_case("auto") && !fill.eq_ignore_ascii_case("FFFFFF"))
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

/// A directly numbered list paragraph's indent, as the model keeps it: the
/// declared left edge of its text (the paragraph's own `w:ind`, else its
/// numbering level's) less the list indent the layout adds for its level.
/// The writer declares exactly `left_indent + text_indent(level)`, so its
/// own lists come back as written; a Word toolbar list (0.5in per level)
/// keeps the extra depth it is drawn with in Word and LibreOffice. A list's
/// hanging indent is the list's own, not a first-line indent.
///
/// Paragraphs numbered through a built-in list style ("List Bullet") are
/// left alone: those styles' 0.25in levels are what the layout already
/// draws, and their level is the style's, not the numbering's.
fn list_indent_from_declared(doc: &rdocx::Document, p: &rdocx::ParagraphRef<'_>, style: &mut ParaStyle) {
    if style.list == ListKind::None {
        return;
    }
    // The paragraph's own numbering, else its style's ("6.1." minutes
    // items numbered by their paragraph style).
    let direct = p.numbering().is_some_and(|(id, _)| id != 0);
    let Some((num_id, level)) = paragraph_numbering(doc, p) else { return };
    let text = crate::lists::text_indent_pt(style.list_level);
    // The paragraph's own indent, a zero one too: the business appointment
    // rules' items set `w:left="0"` over their level's 0.5in, and stand at
    // the margin. Else, as Word and Writer resolve it, a paragraph numbered
    // by its own `w:numPr` takes its level's indent over its style's (a
    // "List Paragraph" item at its level's 0.25in, not the style's 0.5in),
    // and one numbered by its style takes the style's own indent first.
    let own = p.indent_left().or_else(|| p.indent_start()).map(|l| l.to_pt());
    let of_style = Some(style.left_indent_pt).filter(|v| *v != 0.0);
    let of_level = level_indents(doc, num_id, level).0;
    let declared = match own {
        Some(v) => v,
        None if direct => of_level.or(of_style).unwrap_or(text),
        None => of_style.or(of_level).unwrap_or(text),
    };
    // Left of the list's own indent as far as the margin.
    style.left_indent_pt = (declared - text).max(-text);
    // The marker hangs as far as the paragraph says, else its level.
    let hanging = if style.first_line_indent_pt < 0.0 {
        Some(-style.first_line_indent_pt)
    } else {
        level_indents(doc, num_id, level).1
    };
    style.list_hanging_pt = hanging.filter(|h| h.is_finite() && *h > 0.0 && (h - crate::lists::HANGING_PT).abs() > 0.5);
    if style.first_line_indent_pt < 0.0 {
        style.first_line_indent_pt = 0.0;
    }
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

/// Every paragraph of `table`, its own nested tables' included, in
/// document order.
fn for_each_nested_paragraph(table: &rdocx::TableRef<'_>, f: &mut dyn FnMut(&rdocx::ParagraphRef<'_>)) {
    for ri in 0..table.row_count() {
        let Some(row) = table.row(ri) else { continue };
        for ci in 0..row.cell_count() {
            let Some(cell) = row.cell(ci) else { continue };
            for item in cell.items() {
                match item {
                    rdocx::CellItemRef::Paragraph(p) => f(&p),
                    rdocx::CellItemRef::Table(t) => for_each_nested_paragraph(&t, f),
                    _ => {}
                }
            }
        }
    }
}

/// `table_style` is the style of the table a cell paragraph is in.
/// `raw` is the scan's direct properties for this paragraph, when the
/// scan paired them (indents, tab stops, line rule, borders).
fn map_paragraph(doc: &rdocx::Document, p: &rdocx::ParagraphRef<'_>, table_style: Option<&str>, raw: Option<&RawPara>) -> Paragraph {
    let heading = p.style_id().and_then(style_id_to_heading);
    // LO uses "Quotations"; Word uses "Quote"/"IntenseQuote".
    let block_quote = matches!(p.style_id(), Some("Quote") | Some("Quotations") | Some("IntenseQuote") | Some("BlockQuote") | Some("BlockQuotation"));
    // LibreOffice emits PreformattedText for <pre>/code blocks.
    let code_block = matches!(p.style_id(), Some("PreformattedText") | Some("HTMLPreformatted"))
        .then(String::new);
    // What the paragraph's style chain (docDefaults, basedOn, its style)
    // says, for everything the paragraph does not set itself. Numbering
    // indents are left out: the model's list level carries them.
    let mut styled = doc.resolve_paragraph_properties(p.style_id());
    if let Some(ts) = table_style {
        under_table_style(doc, &mut styled, ts);
    }
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
    // A quote style the file defines is its own look too: its indent and
    // italics are the paragraph's (read below), with none added.
    let own_look = named_style.as_deref().is_some_and(style_defined) || (block_quote && p.style_id().is_some_and(style_defined));
    // Either spelling counts: the paragraph property, or a run-level
    // break before this paragraph's own text.
    let page_break_before = p.is_page_break_before() || run_page_break(p).leading;
    // Keep with next: the paragraph's own setting, else its style's.
    let keep_with_next = p.keep_with_next_value().or(styled.keep_next).unwrap_or(false);
    // Keep lines together, and widow/orphan control: the paragraph's own
    // setting, else its style's.
    let keep_lines = p.keep_together_value().or(styled.keep_lines).unwrap_or(false);
    let widow_control = p.widow_control_value().or(styled.widow_control);
    let numbering = paragraph_numbering(doc, p);
    let (list, list_level) = match numbering {
        // The item's own level says bullet or number: a numbered list's
        // second level is often bulleted, and the reverse.
        Some((num_id, level)) => (match level_is_bullet(doc, num_id, level).or_else(|| doc.numbering_is_bullet(num_id)) {
            Some(false) => ListKind::Numbered,
            // Unknown num_id defaults to bullet — the safer visual guess.
            _ => ListKind::Bullet,
        }, level.min(8) as u8),
        None => (ListKind::None, 0),
    };
    // Where a list restarts: a start override on this instance at this
    // level (#1205). Only the first item of that instance carries it.
    let list_start = numbering.and_then(|(num_id, level)| {
        let start = doc.numbering_instance(num_id)?.level_overrides.iter().find(|o| o.level == level)?.start?;
        RESTARTED.with(|r| r.borrow_mut().insert((num_id, level))).then_some(start)
    });

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
        // A picture, inline or floating: a floating one is placed where
        // its anchor says (`docx_anchors`), the next placement of its image.
        let picture = r.items().into_iter().find_map(|item| match item {
            rdocx::RunItemRef::Drawing(d) if d.relationship_kind() == Some(rdocx::DrawingRelationshipKind::Embedded) => {
                let extent = d.width().zip(d.height()).map(|(w, h)| (w.to_emu().max(0) as u64, h.to_emu().max(0) as u64));
                Some((d.relationship_id()?.to_string(), d.description().map(str::to_string), extent, d.is_anchor()))
            }
            rdocx::RunItemRef::UnsupportedXml(raw) => std::str::from_utf8(raw).ok().and_then(crate::docx_anchors::raw_picture),
            _ => None,
        });
        if let Some((rel_id, alt, extent, anchored)) = picture {
            if let Some(bytes) = doc.image_data(&rel_id) {
                // gh-268: the previous code wrote to a predictable
                // /tmp/letters-images/<content-hash>.png via fs::write, which
                // follows a pre-created symlink — a local user could point the
                // write anywhere. The media cache writes into a private 0700
                // per-process directory under an unpredictable name. The file
                // is NOT deleted when the model is done: it lives as long as
                // this process, identical bytes share one file, and the next
                // process sweeps the directory once this one is gone (#455).
                let path = match suite_common_core::media_cache::persist(&bytes) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                // The displayed size is the drawing's extent, not the
                // image's pixels: a 200px picture placed 2in wide is 2in.
                let image_anchor = anchored
                    .then(|| ANCHORS.with(|a| a.borrow_mut().get_mut(&rel_id).and_then(|q| q.pop_front())))
                    .flatten();
                // An inline picture's room beside it, the next of its image's.
                let image_sides_emu = (!anchored)
                    .then(|| INLINE_SIDES.with(|s| s.borrow_mut().get_mut(&rel_id).and_then(|q| q.pop_front())))
                    .flatten()
                    .filter(|&(l, r)| l > 0 || r > 0);
                runs.push(Run {
                    text: alt.unwrap_or_default(),
                    style: RunStyle {
                        image: Some(path.to_string_lossy().into_owned()),
                        image_extent_emu: extent
                            .filter(|(w, h)| *w > 0 && *h > 0),
                        image_sides_emu,
                        image_anchor,
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
            // The styles' properties, not rdocx's effective ones: those
            // also merge in the paragraph mark's (`w:pPr/w:rPr`), which
            // formats the pilcrow alone. A form whose marks were bold drew
            // every paragraph bold where Word and LibreOffice draw it plain.
            doc.resolve_run_properties(p.style_id(), r.style_id())
        };
        let family = r.font_name().map(|f| f.to_string()).or_else(|| {
            heading.is_none().then(|| style_family(p.style_id())).flatten().filter(|f| Some(f) != base.family.as_ref())
        });
        let size_hp = r
            .size()
            .map(|pt| (pt * 2.0).round() as u16)
            .or_else(|| eff.sz.map(|s| s.0.min(u32::from(u16::MAX)) as u16).filter(|hp| Some(*hp) != base.size_hp));
        // "auto" is Word's automatic (default) text colour, not a colour:
        // on the run itself it must not hide the style's either. In a
        // heading, though, the heading style's colour is drawn under every
        // run without one, so "auto" there is the run undoing it (a black
        // subtitle in a blue Heading 2): black, as Word and LibreOffice
        // draw it.
        let auto = r.color().is_some_and(|c| c.eq_ignore_ascii_case("auto"));
        let color = r
            .color()
            .filter(|c| !c.eq_ignore_ascii_case("auto"))
            .map(|c| c.trim_start_matches('#').to_uppercase())
            .or_else(|| (auto && heading.is_some()).then(|| "000000".to_string()))
            .or_else(|| eff.color.as_deref().filter(|c| !c.eq_ignore_ascii_case("auto")).map(|c| c.to_uppercase()));
        // LibreOffice writes a superscript or subscript as the run's own
        // position at a smaller size than the text around it (HTML's
        // `<sup>` as `w:position="8"` at 9.5pt in 12pt text), not as
        // `w:vertAlign`.
        let around = eff.sz.map(|s| s.0).or(base.size_hp.map(u32::from)).unwrap_or(24);
        let script = r
            .position()
            .filter(|&p| p != 0 && r.size().is_some_and(|pt| ((pt * 2.0).round() as u32) < around))
            .map(|p| if p > 0 { crate::model::VertAlign::Superscript } else { crate::model::VertAlign::Subscript });
        runs.push(Run {
            text,
            style: RunStyle {
                bold: r.bold_value().unwrap_or(eff.bold == Some(true)),
                // Not bold in a heading, which is drawn bold.
                regular: heading.is_some() && r.bold_value().or(eff.bold) == Some(false),
                italic: r.italic_value().unwrap_or(eff.italic == Some(true)),
                // Not italic in a heading, which may be drawn italic.
                upright: heading.is_some() && r.italic_value().or(eff.italic) == Some(false),
                caps: r.all_caps_value().unwrap_or(eff.caps == Some(true)),
                underline: r.is_underline(),
                strikethrough: r.is_strike(),
                highlight: run_is_highlighted(&r),
                code: r.style_id() == Some("SourceText"),
                link: link_for(idx),
                image: None,
                image_extent_emu: None,
                image_sides_emu: None,
                image_anchor: None,
                text_box: None,
                footnote: None,
                html: false,
                chip: None,
                revision: None,
                comments: Vec::new(),
                font_family: family,
                font_size_hp: size_hp,
                color,
                letter_spacing_tw: r.character_spacing().or(eff.spacing).map(|t| t.0).filter(|&t| t != 0),
                // Raised or lowered at its own size: the business
                // appointment rules' Normal lowers every run half a point,
                // which LibreOffice makes room for on every line.
                position_hp: if script.is_some() { None } else { r.position().or(eff.position).filter(|&p| p != 0) },
                vert_align: match r.vert_align() {
                    Some("superscript") => Some(crate::model::VertAlign::Superscript),
                    Some("subscript") => Some(crate::model::VertAlign::Subscript),
                    _ => script,
                },
            },
        });
    }
    // A blank paragraph is a line of its mark's font (`w:pPr/w:rPr`, else
    // its style's), as Word and Writer size it: the screening checklist
    // opens with two blank 22pt Title lines above its title, which drawn
    // at the body size stood it 32px high.
    // So is one holding nothing but floating text boxes (a form's rule
    // under its title) or floating pictures (the audit committee minutes'
    // logo, above three blank 14pt lines), whatever its anchoring run's
    // own size.
    let floating_only = !runs.is_empty()
        && runs.iter().all(|r| crate::docx_text_boxes::is_sentinel(&r.text) || (r.style.image.is_some() && r.style.image_anchor.is_some()));
    if (runs.is_empty() || floating_only) && heading.is_none() {
        let mark = p.mark();
        let styled = doc.resolve_run_properties(p.style_id(), None);
        let size_hp = mark
            .size()
            .map(|pt| (pt * 2.0).round() as u16)
            .or_else(|| styled.sz.map(|s| s.0.min(u32::from(u16::MAX)) as u16))
            .filter(|hp| Some(*hp) != base.size_hp);
        let family = mark.font_name().map(str::to_string).or_else(|| style_family(p.style_id())).filter(|f| Some(f) != base.family.as_ref());
        if floating_only {
            for r in &mut runs {
                (r.style.font_size_hp, r.style.font_family) = (size_hp, family.clone());
            }
        } else if size_hp.is_some() || family.is_some() {
            runs.push(Run { text: String::new(), style: RunStyle { font_size_hp: size_hp, font_family: family, ..Default::default() } });
        }
    }
    let mut para = Paragraph {
        style: ParaStyle {
            heading, alignment, list, code_block, block_quote,
            list_level, list_start,
            named_style, own_look, page_break_before, keep_with_next, keep_lines, widow_control,
            line_height: para_line_height(
                p.line_spacing_multiple(),
                p.line_spacing(),
                raw.and_then(|r| r.line_rule.as_deref()),
                &styled,
            ),
            shading: para_shading(p.shading_fill(), &styled),
            borders: para_borders(raw, &styled),
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
    if let Some(r) = raw {
        apply_strict_indents(&mut para.style, r);
    }
    normalize(&mut para);
    para
}

fn twips_pt(t: rdocx_oxml::Twips) -> f64 {
    f64::from(t.0) / 20.0
}

/// A cell paragraph's properties with its table's style beneath its own
/// paragraph style: Word applies the document defaults, then the table
/// style, then the paragraph style. Word's "Table Grid" sets no space
/// after and single lines; read without it, every cell of such a table
/// took the document's default 8pt after and 1.08 lines, and the table
/// drew a third taller than in Word or LibreOffice.
///
/// What the paragraph's style chain sets is what differs from the
/// defaults alone (a style naming the default's own value draws the
/// same either way, unless the table style differs; Word documents'
/// Normal style rarely sets spacing).
fn under_table_style(doc: &rdocx::Document, styled: &mut rdocx_oxml::properties::CT_PPr, table_style: &str) {
    // No style has an empty id: the defaults alone.
    let defaults = doc.resolve_paragraph_properties(Some(""));
    let table = doc.resolve_paragraph_properties(Some(table_style));
    if styled.space_before == defaults.space_before {
        styled.space_before = table.space_before;
    }
    if styled.space_after == defaults.space_after {
        styled.space_after = table.space_after;
    }
    if (styled.line_spacing, &styled.line_rule) == (defaults.line_spacing, &defaults.line_rule) {
        styled.line_spacing = table.line_spacing;
        styled.line_rule = table.line_rule.clone();
    }
    if styled.jc == defaults.jc {
        styled.jc = table.jc;
    }
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
    /// Table styles whose own `w:tblBorders` has a visible edge.
    ruled_table_styles: std::collections::HashSet<String>,
    /// The colour (not "auto" or black) and width (`w:sz`, eighths of a
    /// point) of the first visible edge of a table style's own
    /// `w:tblBorders`.
    table_style_rules: std::collections::HashMap<String, (Option<String>, Option<u32>)>,
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
        let mut in_tbl_borders = false;
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
                        "w:tblBorders" => in_tbl_borders = false,
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
                "w:tblBorders" if !empty => in_tbl_borders = true,
                "w:top" | "w:left" | "w:bottom" | "w:right" | "w:start" | "w:end" | "w:insideH" | "w:insideV" if in_tbl_borders => {
                    if let (Some(id), Some(val)) = (&style, attr(&e, "w:val")) {
                        if !matches!(val.as_str(), "none" | "nil") {
                            out.ruled_table_styles.insert(id.clone());
                            let size = attr(&e, "w:sz").and_then(|s| s.parse().ok());
                            out.table_style_rules.entry(id.clone()).or_insert((attr(&e, "w:color").and_then(|c| rule_color(&c)), size));
                        }
                    }
                }
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

/// Where the document's floating pictures sit (`docx_anchors`).
fn anchor_placements(path: &str) -> crate::docx_anchors::Placements {
    std::fs::File::open(path)
        .ok()
        .and_then(|f| zip::ZipArchive::new(f).ok())
        .and_then(|mut zip| part_text(&mut zip, "word/document.xml"))
        .map(|xml| crate::docx_anchors::placements(&xml))
        .unwrap_or_default()
}

/// The room beside each inline picture of the document at `path`
/// (`docx_anchors::inline_sides`).
fn inline_sides(path: &str) -> crate::docx_anchors::Sides {
    std::fs::File::open(path)
        .ok()
        .and_then(|f| zip::ZipArchive::new(f).ok())
        .and_then(|mut zip| part_text(&mut zip, "word/document.xml"))
        .map(|xml| crate::docx_anchors::inline_sides(&xml))
        .unwrap_or_default()
}

thread_local! {
    /// The inline pictures' room beside them not yet given to an image run
    /// of the document being read, per image relationship, in order.
    static INLINE_SIDES: std::cell::RefCell<crate::docx_anchors::Sides> = Default::default();
}

thread_local! {
    /// The floating pictures' placements not yet given to an image run of
    /// the document being read, per image relationship, in document order.
    static ANCHORS: std::cell::RefCell<crate::docx_anchors::Placements> = Default::default();
}

thread_local! {
    /// The `(num_id, level)` list restarts already given to a paragraph in
    /// the document being read: only the first item of a restart carries it.
    static RESTARTED: std::cell::RefCell<std::collections::HashSet<(u32, u32)>> = Default::default();
}

thread_local! {
    /// The styles' fonts of the document being read (set by `read`).
    static STYLE_FONTS: std::cell::RefCell<StyleFonts> = std::cell::RefCell::new(StyleFonts::default());
}

/// Whether table style `id`, or one it is based on, has a visible rule.
/// A border colour worth keeping: six hex digits, upper case; "auto" and
/// black (the rule drawn when none is given) are none.
fn rule_color(c: &str) -> Option<String> {
    let c = c.to_ascii_uppercase();
    (c.len() == 6 && c.bytes().all(|b| b.is_ascii_hexdigit()) && c != "000000").then_some(c)
}

/// A ruled table's rule colour and width, each when not the default.
type RuleLook = (Option<String>, Option<f64>);

/// Whether a ruled table's rules are double lines: its cells' first
/// visible border's line, else its own (`w:tblBorders`).
fn table_rule_double(table: &rdocx::TableRef<'_>) -> bool {
    use rdocx::CellBorderEdge as C;
    use rdocx::TableBorderEdge as E;
    let visible = |b: &rdocx::TableBorderRef<'_>| !matches!(b.style(), "none" | "nil");
    for r in 0..table.row_count() {
        let Some(row) = table.row(r) else { continue };
        for c in 0..row.cell_count() {
            let Some(cell) = row.cell(c) else { continue };
            if let Some(b) = [C::Top, C::Left, C::Bottom, C::Right].into_iter().filter_map(|e| cell.border(e)).find(visible) {
                return b.style() == "double";
            }
        }
    }
    [E::Top, E::Left, E::Bottom, E::Right, E::InsideHorizontal, E::InsideVertical]
        .into_iter()
        .filter_map(|e| table.border(e))
        .find(visible)
        .is_some_and(|b| b.style() == "double")
}

/// The colour a ruled table's rules are drawn in, when not black, and
/// their width in points, when not 0.5pt: its own first visible border's
/// (`w:tblBorders`), else its style's, through `w:basedOn` (Word's "Table
/// Grid Light" is BFBFBF).
fn table_rule_look(table: &rdocx::TableRef<'_>) -> RuleLook {
    use rdocx::TableBorderEdge as E;
    use rdocx::CellBorderEdge as C;
    let width = |sz: Option<u32>| sz.map(|s| f64::from(s) / 8.0).filter(|w| *w > 0.0 && (w - crate::layout::CELL_RULE_PT).abs() > 1e-6);
    let visible = |b: &rdocx::TableBorderRef<'_>| !matches!(b.style(), "none" | "nil");
    // A cell's own borders (`w:tcBorders`) win over the table's: the
    // questionnaire's respondent table rules itself white and each of its
    // cells black, and Word and Writer draw it black.
    let mut cells = None;
    'rows: for r in 0..table.row_count() {
        let Some(row) = table.row(r) else { continue };
        for c in 0..row.cell_count() {
            let Some(cell) = row.cell(c) else { continue };
            if let Some(b) = [C::Top, C::Left, C::Bottom, C::Right].into_iter().filter_map(|e| cell.border(e)).find(visible) {
                cells = Some((b.color().and_then(rule_color), width(b.size_eighths_pt())));
                break 'rows;
            }
        }
    }
    if let Some(look) = cells {
        return look;
    }
    let own = [E::Top, E::Left, E::Bottom, E::Right, E::InsideHorizontal, E::InsideVertical]
        .into_iter()
        .filter_map(|e| table.border(e))
        .find(visible);
    if let Some(b) = own {
        return (b.color().and_then(rule_color), width(b.size_eighths_pt()));
    }
    let Some(id) = table.style_id() else { return (None, None) };
    STYLE_FONTS.with(|sf| {
        let sf = sf.borrow();
        let mut cur = Some(id.to_string());
        for _ in 0..32 {
            let Some(c) = cur else { break };
            if sf.ruled_table_styles.contains(&c) {
                let (color, size) = sf.table_style_rules.get(&c).cloned().unwrap_or_default();
                return (color, width(size));
            }
            cur = sf.styles.get(&c).and_then(|(based_on, _)| based_on.clone());
        }
        (None, None)
    })
}

fn table_style_ruled(id: &str) -> bool {
    STYLE_FONTS.with(|sf| {
        let sf = sf.borrow();
        let mut cur = Some(id.to_string());
        for _ in 0..32 {
            let Some(c) = cur else { return false };
            if sf.ruled_table_styles.contains(&c) {
                return true;
            }
            cur = sf.styles.get(&c).and_then(|(based_on, _)| based_on.clone());
        }
        false
    })
}

/// Whether the file's styles part defines style `id`.
fn style_defined(id: &str) -> bool {
    STYLE_FONTS.with(|sf| sf.borrow().styles.contains_key(id))
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
    // A blank paragraph keeps the empty run that sizes its line.
    let blank = match p.runs.as_slice() {
        [r] if r.text.is_empty() && (r.style.font_size_hp.is_some() || r.style.font_family.is_some()) => Some(r.clone()),
        _ => None,
    };
    p.runs
        .retain(|r| !r.text.is_empty() || r.style.image.is_some() || r.style.footnote.is_some());
    if p.runs.is_empty() {
        p.runs.extend(blank);
    }
    crate::model::merge_adjacent_runs(&mut p.runs, |a, b| {
        a.style.footnote.is_none() && b.style.footnote.is_none() && a.style == b.style
    });
}

#[cfg(test)]
mod tests {
    /// 31 nested tables, inside rdocx's own cap of 32, read on whichever
    /// thread asks: on a 2 MiB test thread they overflowed the stack before
    /// the read moved to its own. Deeper, up to [`super::MAX_XML_DEPTH`],
    /// is refused by rdocx rather than crashing, and one level past it is
    /// refused unread (#1206).
    /// A docx Letters wrote, with `body` in place of its `w:body`'s content.
    fn docx_with_body(dir: &std::path::Path, body: &str, name: &str) -> std::path::PathBuf {
        use std::io::{Read, Write};
        let base = dir.join("base.docx");
        if !base.exists() {
            super::write(&Document::from_plain_text("x"), &base).unwrap();
        }
        let xml = format!(
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}</w:body></w:document>"
        );
        let mut source = zip::ZipArchive::new(std::fs::File::open(&base).unwrap()).unwrap();
        let path = dir.join(name);
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for i in 0..source.len() {
            let mut entry = source.by_index(i).unwrap();
            let entry_name = entry.name().unwrap().into_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            if entry_name == "word/document.xml" {
                bytes = xml.clone().into_bytes();
            }
            writer.start_file(entry_name, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(&bytes).unwrap();
        }
        writer.finish().unwrap();
        path
    }

    fn para(text: &str) -> String {
        format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>")
    }

    /// A page break before a table starts the table on the new page: the
    /// crime supervisor form puts a paragraph of nothing but a page break
    /// between two tables. The break was carried past the table to the
    /// paragraph after it, and the table ran on at the foot of page one.
    /// The break survives a save, and the paragraph after the table keeps
    /// its place.
    #[test]
    fn a_page_break_before_a_table_starts_it_on_the_new_page() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "{}<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p><w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>{}",
            para("before"), para("cell"), para("after")
        );
        let path = docx_with_body(dir.path(), &body, "break.docx");
        let breaks = |d: &Document| d.paragraphs.iter().map(|p| (p.text(), p.style.page_break_before)).collect::<Vec<_>>();
        let doc = super::read(path.to_str().unwrap()).unwrap();
        let want = vec![("before".to_string(), false), ("cell".to_string(), true), ("after".to_string(), false)];
        assert_eq!(breaks(&doc), want);
        let saved = dir.path().join("saved.docx");
        super::write(&doc, &saved).unwrap();
        assert_eq!(breaks(&super::read(saved.to_str().unwrap()).unwrap()), want);
    }

    /// A table's columns are its grid's (`w:tblGrid`), as Writer lays them
    /// out: the crime supervisor form's table opens with a title row of
    /// one cell across all five columns. Read from that row, the table had
    /// one width for five columns and was drawn with equal columns, its
    /// 1cm number column as wide as the rest. A grid that does not match
    /// the cells falls back to the first row's widths.
    #[test]
    fn a_tables_columns_are_its_grids() {
        let dir = tempfile::tempdir().unwrap();
        let cell = |w: u32, span: u32, text: &str| {
            let span = if span > 1 { format!("<w:gridSpan w:val=\"{span}\"/>") } else { String::new() };
            format!("<w:tc><w:tcPr><w:tcW w:w=\"{w}\" w:type=\"dxa\"/>{span}</w:tcPr>{}</w:tc>", para(text))
        };
        let table = |grid: &[u32]| {
            let grid: String = grid.iter().map(|w| format!("<w:gridCol w:w=\"{w}\"/>")).collect();
            format!(
                "<w:tbl><w:tblGrid>{grid}</w:tblGrid><w:tr>{}</w:tr><w:tr>{}{}{}</w:tr></w:tbl>{}",
                cell(6000, 3, "title"), cell(500, 1, "a"), cell(3000, 1, "b"), cell(2500, 1, "c"), para("after")
            )
        };
        let widths = |grid: &[u32], name| {
            let doc = super::read(docx_with_body(dir.path(), &table(grid), name).to_str().unwrap()).unwrap();
            doc.table_columns.get(&0).cloned()
        };
        assert_eq!(widths(&[567, 3000, 2500], "grid.docx"), Some(vec![28.35, 150.0, 125.0]));
        assert_eq!(widths(&[6000], "short.docx"), Some(vec![300.0]), "a grid of the wrong count gives way to the first row");
    }

    /// A table nested in a cell keeps its text, in its outer cell, in
    /// document order (#1419): its two cells used to be dropped.
    #[test]
    fn a_nested_tables_text_is_read_into_its_outer_cell() {
        let dir = tempfile::tempdir().unwrap();
        let body = format!(
            "{}<w:tbl><w:tr><w:tc>{}<w:tbl><w:tr><w:tc>{}</w:tc><w:tc>{}</w:tc></w:tr></w:tbl>{}</w:tc><w:tc>{}</w:tc></w:tr></w:tbl>{}",
            para("before"), para("outer A"), para("inner one"), para("inner two"), para("outer A, after"), para("outer B"), para("after")
        );
        let path = docx_with_body(dir.path(), &body, "nested.docx");
        let doc = super::read(path.to_str().unwrap()).unwrap();
        let cell = |row, col| -> Vec<String> {
            doc.paragraphs
                .iter()
                .filter(|p| p.style.table_cell.is_some_and(|c| (c.table, c.row, c.col) == (0, row, col)))
                .map(|p| p.text())
                .collect()
        };
        assert_eq!(cell(0, 0), ["outer A", "inner one", "inner two", "outer A, after"]);
        assert_eq!(cell(0, 1), ["outer B"]);
    }

    /// 31 nested tables, inside rdocx's own cap of 32, read on whichever
    /// thread asks: on a 2 MiB test thread they overflowed the stack before
    /// the read moved to its own. Deeper, up to [`super::MAX_XML_DEPTH`],
    /// is refused by rdocx rather than crashing, and one level past it is
    /// refused unread (#1206).
    #[test]
    fn nesting_is_read_up_to_the_limit_and_refused_past_it() {
        let dir = tempfile::tempdir().unwrap();
        let with_tables = |levels: usize, name: &str| {
            let body = format!(
                "{}<w:p><w:r><w:t>x</w:t></w:r></w:p>{}",
                "<w:tbl><w:tr><w:tc>".repeat(levels),
                "</w:tc></w:tr></w:tbl>".repeat(levels)
            );
            docx_with_body(dir.path(), &body, name)
        };
        let nested = with_tables(31, "nested.docx");
        let doc = super::read(nested.to_str().unwrap()).expect("31 nested tables read");
        // The innermost cell's text, in the outermost cell (#1419).
        let texts: Vec<String> = doc.paragraphs.iter().filter(|p| p.style.table_cell.is_some()).map(|p| p.text()).collect();
        assert_eq!(texts, ["x"]);

        // Three elements a level, between <w:document><w:body> and the
        // innermost cell's <w:p><w:r><w:t>.
        let deepest = (super::MAX_XML_DEPTH - 5) / 3;
        let inside = with_tables(deepest, "inside.docx");
        if let Err(error) = super::read(inside.to_str().unwrap()) {
            assert!(!error.contains("nests its elements"), "refused inside the limit: {error}");
        }
        let past = with_tables(deepest + 1, "past.docx");
        let error = super::read(past.to_str().unwrap()).expect_err("a docx past the limit is refused");
        assert!(error.contains("nests its elements"), "{error}");
    }

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
