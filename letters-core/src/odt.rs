// odt.rs — OpenDocument Text (.odt) read/write for the Letters model.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Scope (first slice, PARITY #20): paragraphs, headings 1–6, inline
// bold/italic/underline/strikethrough/highlight, font size, color,
// links, alignment, flat bullet/numbered lists, page breaks, and the
// document header/footer. Fidelity is measured against the LibreOffice
// oracle (tests/soffice_oracle.rs pattern), never ported from it.

use crate::model::*;
use quick_xml::events::{BytesRef, BytesText, Event};
use quick_xml::Reader;
use std::io::Write;
use suite_common_core::zip_guard::{BoundedArchive, ZipBudget};

const MIMETYPE: &str = "application/vnd.oasis.opendocument.text";

/// Read an ODT with a structured report and package parts that can be copied
/// through an unrelated edit.
pub fn read_with_report(path: &str) -> Result<(Document, suite_common_core::interop::CompatibilityReport, suite_common_core::interop::OpaquePackage), String> {
    let (document, pictures) = read_parts(path)?;
    let mut recognized = vec!["mimetype", "META-INF/manifest.xml", "content.xml", "styles.xml", "settings.xml"];
    recognized.extend(pictures.iter().map(String::as_str));
    let opaque = suite_common_core::interop::OpaquePackage::capture(path, &recognized)?;
    let mut report = suite_common_core::interop::CompatibilityReport::new("odt");
    for name in opaque.part_names() {
        report.record(suite_common_core::interop::UnsupportedFeature::new("uninterpreted-package-part", "Uninterpreted package part", name, suite_common_core::interop::FeatureDisposition::OpaquePassThrough, "will be copied through on an opaque save"));
    }
    Ok((document, report, opaque))
}

fn unescape_text(t: &BytesText) -> String {
    // quick-xml 0.42 dropped `decode()`: event payloads are already `&str`.
    quick_xml::escape::unescape(t)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| t.to_string())
}

// quick-xml 0.41 stopped inlining `&entity;`/`&#NN;` references into
// Event::Text — they now arrive as a separate Event::GeneralRef that must
// be resolved and appended alongside surrounding text, or escaped
// characters silently vanish from read documents.
fn resolve_general_ref(r: &BytesRef) -> String {
    if let Ok(Some(c)) = r.resolve_char_ref() {
        return c.to_string();
    }
    let name: &str = r;
    quick_xml::escape::resolve_predefined_entity(name)
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("&{name};"))
}

// ── XML helpers ──────────────────────────────────────────────────────

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ── Writing ──────────────────────────────────────────────────────────

/// Collect the distinct run styles used, in first-use order.
fn collect_run_styles(doc: &Document) -> Vec<RunStyle> {
    let mut styles: Vec<RunStyle> = Vec::new();
    for p in &doc.paragraphs {
        for r in &p.runs {
            let style = RunStyle { revision: None, comments: Vec::new(), ..r.style.clone() };
            if style != RunStyle::default() && style.chip.is_none() && !styles.contains(&style) {
                styles.push(style);
            }
        }
    }
    styles
}

fn run_style_props(st: &RunStyle) -> String {
    let mut props = String::new();
    if st.bold {
        props.push_str(" fo:font-weight=\"bold\"");
    }
    if st.italic {
        props.push_str(" fo:font-style=\"italic\"");
    }
    if st.underline {
        props.push_str(" style:text-underline-style=\"solid\"");
    }
    if st.strikethrough {
        props.push_str(" style:text-line-through-style=\"solid\"");
    }
    if st.highlight {
        props.push_str(" fo:background-color=\"#ffff00\"");
    }
    if let Some(hp) = st.font_size_hp {
        props.push_str(&format!(" fo:font-size=\"{}pt\"", hp as f32 / 2.0));
    }
    if let Some(color) = &st.color {
        props.push_str(&format!(" fo:color=\"#{color}\""));
    }
    if let Some(va) = st.vert_align {
        let pos = match va {
            VertAlign::Superscript => "super 58%",
            VertAlign::Subscript => "sub 58%",
        };
        props.push_str(&format!(" style:text-position=\"{pos}\""));
    }
    if st.code {
        props.push_str(" style:font-name=\"Monospace\"");
    }
    if let Some(fam) = &st.font_family {
        props.push_str(&format!(" fo:font-family=\"{}\"", esc(fam)));
    }
    props
}

fn para_style_props(st: &ParaStyle) -> String {
    let mut props = String::new();
    match st.alignment {
        Alignment::Left => {}
        Alignment::Center => props.push_str(" fo:text-align=\"center\""),
        Alignment::Right => props.push_str(" fo:text-align=\"end\""),
        Alignment::Justify => props.push_str(" fo:text-align=\"justify\""),
    }
    if st.page_break_before {
        props.push_str(" fo:break-before=\"page\"");
    }
    if st.keep_with_next {
        props.push_str(" fo:keep-with-next=\"always\"");
    }
    if (st.line_spacing - 1.0).abs() > 0.01 {
        props.push_str(&format!(" fo:line-height=\"{:.0}%\"", st.line_spacing * 100.0));
    }
    // ODF spells paragraph spacing `fo:margin-top`/`fo:margin-bottom`,
    // the same XSL-FO properties the page geometry below already uses.
    // This wrote `fo:space-before`/`fo:space-after`, which our own reader
    // understood and LibreOffice does not: the self round trip passed
    // because both halves shared an attribute nothing else reads, and the
    // spacing was silently gone the moment the file reached Writer.
    if st.space_before_pt.abs() > 0.01 { props.push_str(&format!(" fo:margin-top=\"{:.2}pt\"", st.space_before_pt)); }
    if st.space_after_pt.abs() > 0.01 { props.push_str(&format!(" fo:margin-bottom=\"{:.2}pt\"", st.space_after_pt)); }
    if st.left_indent_pt.abs() > 0.01 { props.push_str(&format!(" fo:margin-left=\"{:.2}pt\"", st.left_indent_pt)); }
    if st.right_indent_pt.abs() > 0.01 { props.push_str(&format!(" fo:margin-right=\"{:.2}pt\"", st.right_indent_pt)); }
    if st.first_line_indent_pt.abs() > 0.01 { props.push_str(&format!(" fo:text-indent=\"{:.2}pt\"", st.first_line_indent_pt)); }
    props
}

/// Child elements of `style:paragraph-properties`, as opposed to its
/// attributes.
///
/// ODF puts tab stops in a `style:tab-stops` child rather than an
/// attribute, so a paragraph style carrying them cannot be written as a
/// self-closing element. Returns the empty string when there are none,
/// which keeps every other style exactly as it was.
fn para_style_children(st: &ParaStyle) -> String {
    if st.tab_stops_pt.is_empty() {
        return String::new();
    }
    let mut out = String::from("<style:tab-stops>");
    for pos in &st.tab_stops_pt {
        // `style:type` defaults to left, which is the only kind the model
        // has; a position is all a stop needs.
        out.push_str(&format!("<style:tab-stop style:position=\"{pos:.2}pt\"/>"));
    }
    out.push_str("</style:tab-stops>");
    out
}

/// An image run's picture as the package stores it (#1292): its member
/// name under `Pictures/`, named by a hash of its bytes so a picture used
/// twice is stored once, its bytes and its media type. None when the
/// source can't be read; the run is then written as its alt text, as the
/// docx writer does.
fn picture(src: &str) -> Option<(String, Vec<u8>, &'static str)> {
    use std::hash::{Hash, Hasher};
    let bytes = std::fs::read(src).ok()?;
    let ext = std::path::Path::new(src)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_else(|| "png".into());
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "tif" | "tiff" => "image/tiff",
        _ => "application/octet-stream",
    };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    Some((format!("Pictures/{:016x}.{ext}", h.finish()), bytes, mime))
}

/// EMU per inch (OOXML's unit, which the model keeps image sizes in).
const EMU_PER_INCH: f64 = 914_400.0;

/// The size an image run is drawn at, in EMU: the size it was shown at,
/// else a PNG's own pixels at 96 dpi, else 4in x 3in (the docx writer's
/// fallback).
fn picture_extent(run: &Run, bytes: &[u8]) -> (u64, u64) {
    if let Some(e) = run.style.image_extent_emu {
        return e;
    }
    if bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let px = |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as u64;
        let (w, h) = (px(16), px(20));
        if w > 0 && h > 0 {
            return (w * 9525, h * 9525);
        }
    }
    (4 * 914_400, 3 * 914_400)
}

/// The automatic graphic style `name` placing a floating picture: each
/// axis aligned in, or offset from (`svg:x`/`svg:y` on the frame), the
/// page, the page inside its margins, or the paragraph; over the text or
/// behind it, never wrapping it.
fn floating_style(name: &str, a: &crate::model::ImageAnchor) -> String {
    use crate::model::{AnchorAlign, AnchorFrame};
    let rel = |f: AnchorFrame| match f {
        AnchorFrame::Page => "page",
        AnchorFrame::Margin => "page-content",
        AnchorFrame::Text => "paragraph",
    };
    let pos = |align: Option<AnchorAlign>, names: [&'static str; 4]| match align {
        None => names[0],
        Some(AnchorAlign::Start) => names[1],
        Some(AnchorAlign::Center) => names[2],
        Some(AnchorAlign::End) => names[3],
    };
    format!(
        "<style:style style:name=\"{name}\" style:family=\"graphic\"><style:graphic-properties style:wrap=\"run-through\" \
         style:run-through=\"{}\" style:horizontal-pos=\"{}\" style:horizontal-rel=\"{}\" style:vertical-pos=\"{}\" \
         style:vertical-rel=\"{}\"/></style:style>",
        if a.behind { "background" } else { "foreground" },
        pos(a.h_align, ["from-left", "left", "center", "right"]),
        rel(a.h_from),
        pos(a.v_align, ["from-top", "top", "middle", "bottom"]),
        rel(a.v_from),
    )
}

/// A graphic style's placement of a floating frame (its offsets are the
/// frame's own `svg:x`/`svg:y`).
fn graphic_placement(e: &quick_xml::events::BytesStart<'_>) -> crate::model::ImageAnchor {
    use crate::model::{AnchorAlign, AnchorFrame};
    let frame = |a: &str| match attr_val(e, a).as_deref() {
        Some("page") => AnchorFrame::Page,
        Some("page-content") => AnchorFrame::Margin,
        _ => AnchorFrame::Text,
    };
    let align = |a: &str| match attr_val(e, a).as_deref() {
        Some("left" | "top" | "inside") => Some(AnchorAlign::Start),
        Some("center" | "middle") => Some(AnchorAlign::Center),
        Some("right" | "bottom" | "outside") => Some(AnchorAlign::End),
        _ => None,
    };
    crate::model::ImageAnchor {
        h_from: frame("style:horizontal-rel"),
        h_align: align("style:horizontal-pos"),
        v_from: frame("style:vertical-rel"),
        v_align: align("style:vertical-pos"),
        behind: attr_val(e, "style:run-through").as_deref() == Some("background"),
        ..Default::default()
    }
}

/// A signed ODF length (a frame's `svg:x`, which may be negative) in EMU.
fn offset_emu(s: &str) -> Option<i64> {
    let s = s.trim();
    let (sign, magnitude) = s.strip_prefix('-').map_or((1, s), |m| (-1, m));
    let split = magnitude.find(|c: char| c.is_ascii_alphabetic())?;
    if magnitude[..split].trim().parse::<f64>().ok()? == 0.0 {
        return Some(0);
    }
    Some(sign * length_emu(magnitude)? as i64)
}

/// An ODF length ("2.5cm", "1in", "72pt", "10mm", "6pc", "96px") in EMU.
fn length_emu(s: &str) -> Option<u64> {
    let s = s.trim();
    let split = s.find(|c: char| c.is_ascii_alphabetic())?;
    let (n, unit) = s.split_at(split);
    let n: f64 = n.trim().parse().ok()?;
    let inches = match unit {
        "in" => n,
        "cm" => n / 2.54,
        "mm" => n / 25.4,
        "pt" => n / 72.0,
        "pc" => n / 6.0,
        "px" => n / 96.0,
        _ => return None,
    };
    (inches > 0.0).then(|| (inches * EMU_PER_INCH).round() as u64)
}

/// A table being written (#1296): its size and the cell being filled.
struct OpenTable {
    id: u32,
    rows: u32,
    cols: u32,
    row: u32,
    col: u32,
    /// Whether the current cell has a paragraph yet (ODF wants one).
    filled: bool,
    /// Each row's style attribute (empty for a row without a height).
    row_styles: Vec<String>,
    /// Each shaded cell's style attribute, by (row, col).
    cell_styles: std::collections::HashMap<(u32, u32), String>,
}

impl OpenTable {
    /// Open the cell at the current (row, col), with its fill's style.
    fn open_cell(&self, body: &mut String) {
        let style = self.cell_styles.get(&(self.row, self.col)).map_or("", String::as_str);
        body.push_str(&format!("<table:table-cell office:value-type=\"string\"{style}>"));
    }

    fn open_row(&self, body: &mut String) {
        let style = self.row_styles.get(self.row as usize).map_or("", String::as_str);
        body.push_str(&format!("<table:table-row{style}>"));
    }

    fn close_cell(&mut self, body: &mut String) {
        if !self.filled {
            body.push_str("<text:p/>");
        }
        body.push_str("</table:table-cell>");
    }

    /// Move to cell (row, col), closing the ones before it; a paragraph
    /// out of grid order stays in the current cell.
    fn move_to(&mut self, body: &mut String, row: u32, col: u32) {
        while (self.row, self.col) < (row, col) && self.row < self.rows {
            self.close_cell(body);
            if self.col + 1 < self.cols {
                self.col += 1;
            } else {
                body.push_str("</table:table-row>");
                self.row += 1;
                self.col = 0;
                if self.row == self.rows {
                    return;
                }
                self.open_row(body);
            }
            self.open_cell(body);
            self.filled = false;
        }
    }

    fn close(mut self, body: &mut String) {
        let (last_row, last_col) = (self.rows - 1, self.cols - 1);
        self.move_to(body, last_row, last_col);
        if self.row < self.rows {
            self.close_cell(body);
            body.push_str("</table:table-row>");
        }
        body.push_str("</table:table>");
    }
}

fn content_xml(doc: &Document) -> String {
    let run_styles = collect_run_styles(doc);
    let mut pictures_written = 0usize;

    let mut auto = String::new();
    for (i, st) in run_styles.iter().enumerate() {
        auto.push_str(&format!(
            "<style:style style:name=\"T{}\" style:family=\"text\">\
             <style:text-properties{}/></style:style>",
            i + 1,
            run_style_props(st)
        ));
    }
    // Paragraph automatic styles: one per used (alignment, break) combo.
    let mut para_autos: Vec<(String, String, Option<u8>)> = Vec::new();
    let mut para_style_idx: Vec<Option<usize>> = Vec::new();
    for p in &doc.paragraphs {
        let props = para_style_props(&p.style);
        let kids = para_style_children(&p.style);
        if props.is_empty() && kids.is_empty() {
            para_style_idx.push(None);
            continue;
        }
        // Keyed on the whole element: two paragraphs share a style only
        // when their attributes *and* their tab stops match. A heading's
        // inherits its level's look (#1297).
        let parent = p.style.heading.filter(|_| p.style.named_style.is_none() && p.style.toc.is_none());
        let elem = (props, kids, parent);
        let pos = para_autos.iter().position(|x| *x == elem).unwrap_or_else(|| {
            para_autos.push(elem.clone());
            para_autos.len() - 1
        });
        para_style_idx.push(Some(pos));
    }
    for (i, (props, kids, parent)) in para_autos.iter().enumerate() {
        let body = if kids.is_empty() {
            format!("<style:paragraph-properties{props}/>")
        } else {
            format!("<style:paragraph-properties{props}>{kids}</style:paragraph-properties>")
        };
        let parent = parent.map(|n| format!(" style:parent-style-name=\"Heading_20_{n}\"")).unwrap_or_default();
        auto.push_str(&format!(
            "<style:style style:name=\"P{}\" style:family=\"paragraph\"{parent}>{}</style:style>",
            i + 1,
            body
        ));
    }
    // Every level of both lists: a nested item takes its level's style,
    // and a level a style doesn't define draws unindented and unmarked.
    let bullets = ["•", "◦", "▪"];
    let level_props = |level: usize| {
        let indent = crate::lists::text_indent_pt(level as u8);
        format!(
            "<style:list-level-properties text:list-level-position-and-space-mode=\"label-alignment\">\
             <style:list-level-label-alignment text:label-followed-by=\"listtab\" fo:text-indent=\"-{h}pt\" fo:margin-left=\"{indent}pt\"/>\
             </style:list-level-properties>",
            h = crate::lists::HANGING_PT,
        )
    };
    auto.push_str("<text:list-style style:name=\"LB\">");
    for level in 0..10 {
        auto.push_str(&format!(
            "<text:list-level-style-bullet text:level=\"{}\" text:bullet-char=\"{}\">{}</text:list-level-style-bullet>",
            level + 1, bullets[level % bullets.len()], level_props(level)
        ));
    }
    auto.push_str("</text:list-style><text:list-style style:name=\"LN\">");
    for level in 0..10 {
        auto.push_str(&format!(
            "<text:list-level-style-number text:level=\"{}\" style:num-format=\"1\" style:num-suffix=\".\">{}</text:list-level-style-number>",
            level + 1, level_props(level)
        ));
    }
    auto.push_str("</text:list-style>");

    let mut body = String::new();
    // Tracked changes: one changed region per change (text:tracked-changes).
    let mut regions: Vec<String> = Vec::new();
    // Comment threads: an annotation (with its replies) where a thread's
    // text starts, an annotation end where it ends; a thread with no text
    // is an annotation without an end, first.
    let roots = crate::docx_comments::roots(doc);
    let mut open: Vec<u32> = Vec::new();
    let mut orphans: Vec<u32> = {
        let marked: std::collections::HashSet<u32> = doc.paragraphs.iter().flat_map(|p| &p.runs).flat_map(|r| r.style.comments.iter().copied()).collect();
        roots.iter().copied().filter(|id| !marked.contains(id)).collect()
    };
    let wanted = |run: &Run| -> Vec<u32> { crate::docx_comments::threads_of(&roots, &run.style.comments).collect() };
    let annotate = |id: u32, start: bool| -> String {
        let thread = doc.comments.iter().filter(|c| c.id == id || c.parent == Some(id));
        if start {
            thread.map(annotation_xml).collect()
        } else {
            thread.map(|c| format!("<office:annotation-end office:name=\"__Annotation__{}\"/>", c.id)).collect()
        }
    };
    // List grouping: consecutive items of one kind share one text:list,
    // and an item at level n sits in a text:list nested n deep (#1205: the
    // writer used to put every level in one flat list, so a nested item
    // came back at the top level). `depth` lists are open, each with an
    // open item.
    let mut open_list = ListKind::None;
    let mut depth = 0usize;
    // Tables: consecutive paragraphs of one table id are its cells, in
    // grid order, as the docx writer groups them (#1296).
    let mut table: Option<OpenTable> = None;
    let mut tables_written = 0u32;
    for (pi, p) in doc.paragraphs.iter().enumerate() {
        let cell = p.style.table_cell;
        let here = table.as_ref().map(|t| (t.id, t.row, t.col));
        if cell.map(|c| (c.table, c.row, c.col)) != here {
            // Lists don't cross a cell boundary.
            body.push_str(&"</text:list-item></text:list>".repeat(depth));
            depth = 0;
            open_list = ListKind::None;
            if table.as_ref().is_some_and(|t| cell.map(|c| c.table) != Some(t.id)) {
                table.take().expect("checked").close(&mut body);
            }
            if let Some(c) = cell {
                if table.is_none() {
                    let group = doc.paragraphs[pi..].iter().map_while(|q| q.style.table_cell.filter(|qc| qc.table == c.table));
                    let (rows, cols) = group.fold((0, 0), |(r, k), qc| (r.max(qc.row + 1), k.max(qc.col + 1)));
                    tables_written += 1;
                    // The file's column widths, as column styles, when the
                    // table still has as many columns as it was read with.
                    // A table style with their sum as its width: without
                    // one, Writer stretches the table to the text width and
                    // scales every column with it.
                    let mut table_style = String::new();
                    let columns = match doc.table_columns.get(&c.table).filter(|w| w.len() == cols as usize) {
                        Some(widths) => {
                            let total: f64 = widths.iter().sum();
                            auto.push_str(&format!(
                                "<style:style style:name=\"Table{tables_written}\" style:family=\"table\"><style:table-properties style:width=\"{total:.2}pt\" table:align=\"left\"/></style:style>"
                            ));
                            table_style = format!(" table:style-name=\"Table{tables_written}\"");
                            widths
                            .iter()
                            .enumerate()
                            .map(|(k, w)| {
                                let name = format!("Table{tables_written}.C{k}");
                                auto.push_str(&format!(
                                    "<style:style style:name=\"{name}\" style:family=\"table-column\"><style:table-column-properties style:column-width=\"{w:.2}pt\"/></style:style>"
                                ));
                                format!("<table:table-column table:style-name=\"{name}\"/>")
                            })
                            .collect::<String>()
                        }
                        None => format!("<table:table-column table:number-columns-repeated=\"{cols}\"/>"),
                    };
                    // The file's row heights, as row styles: a minimum
                    // height, or a fixed one.
                    let row_styles: Vec<String> = (0..rows as usize)
                        .map(|r| match doc.table_rows.get(&c.table).and_then(|h| h.get(r).copied().flatten()) {
                            Some(h) => {
                                let name = format!("Table{tables_written}.R{r}");
                                let prop = if h.exact { "style:row-height" } else { "style:min-row-height" };
                                auto.push_str(&format!(
                                    "<style:style style:name=\"{name}\" style:family=\"table-row\"><style:table-row-properties {prop}=\"{:.2}pt\"/></style:style>",
                                    h.pt
                                ));
                                format!(" table:style-name=\"{name}\"")
                            }
                            None => String::new(),
                        })
                        .collect();
                    body.push_str(&format!("<table:table table:name=\"Table{tables_written}\"{table_style}>{columns}"));
                    // The shaded cells, one table-cell style per colour.
                    let mut fill_styles: std::collections::HashMap<String, String> = Default::default();
                    let cell_styles = doc.table_fills.get(&c.table).into_iter().flatten().map(|f| {
                        let n = fill_styles.len();
                        let name = fill_styles.entry(f.color.clone()).or_insert_with(|| {
                            let name = format!("Table{tables_written}.F{n}");
                            auto.push_str(&format!(
                                "<style:style style:name=\"{name}\" style:family=\"table-cell\"><style:table-cell-properties fo:background-color=\"#{}\"/></style:style>",
                                esc(&f.color)
                            ));
                            name
                        });
                        ((f.row, f.col), format!(" table:style-name=\"{name}\""))
                    }).collect();
                    let t = OpenTable { id: c.table, rows, cols, row: 0, col: 0, filled: false, row_styles, cell_styles };
                    t.open_row(&mut body);
                    t.open_cell(&mut body);
                    table = Some(t);
                }
                if let Some(t) = table.as_mut() {
                    t.move_to(&mut body, c.row, c.col);
                }
            }
        }
        if let Some(t) = table.as_mut() {
            t.filled = true;
        }
        let kind = p.style.list;
        if kind != open_list {
            body.push_str(&"</text:list-item></text:list>".repeat(depth));
            depth = 0;
            open_list = kind;
        }
        if kind != ListKind::None {
            let target = usize::from(p.style.list_level) + 1;
            let start = p.style.list_start.map(|n| format!(" text:start-value=\"{n}\"")).unwrap_or_default();
            if depth == 0 {
                let style = if kind == ListKind::Numbered { "LN" } else { "LB" };
                body.push_str(&format!("<text:list text:style-name=\"{style}\">"));
                depth = 1;
                if target == 1 {
                    body.push_str(&format!("<text:list-item{start}>"));
                } else {
                    body.push_str("<text:list-item>");
                }
            } else {
                while depth > target {
                    body.push_str("</text:list-item></text:list>");
                    depth -= 1;
                }
                if depth == target {
                    body.push_str(&format!("</text:list-item><text:list-item{start}>"));
                }
            }
            while depth < target {
                depth += 1;
                let attr = if depth == target { start.as_str() } else { "" };
                body.push_str(&format!("<text:list><text:list-item{attr}>"));
            }
        }

        // LO built-in named styles win over our automatic styles; the
        // names (Title, Subtitle, Quotations) are ODF/LO conventions.
        let style_attr = if let Some(level) = p.style.toc {
            format!(" text:style-name=\"Contents_20_{}\"", level.clamp(1, 10))
        } else if let Some(name) = &p.style.named_style {
            format!(" text:style-name=\"{}\"", esc(name))
        } else if p.style.block_quote {
            " text:style-name=\"Quotations\"".to_string()
        } else if p.style.code_block.is_some() && p.style.heading.is_none() {
            " text:style-name=\"Preformatted_20_Text\"".to_string()
        } else {
            match (para_style_idx[pi], p.style.heading) {
                (Some(i), _) => format!(" text:style-name=\"P{}\"", i + 1),
                (None, Some(n)) => format!(" text:style-name=\"Heading_20_{n}\""),
                (None, None) => String::new(),
            }
        };

        let mut inner = String::new();
        for id in std::mem::take(&mut orphans) {
            inner.push_str(&annotate(id, true));
        }
        for r in &p.runs {
            for (id, start) in crate::docx_comments::transition(&mut open, &wanted(r)) {
                inner.push_str(&annotate(id, start));
            }
            // A picture is a frame anchored as a character, so it stays
            // where it is in the line; its bytes are in Pictures/ (#1292).
            if let Some(src) = &r.style.image {
                match picture(src) {
                    Some((name, bytes, _)) => {
                        pictures_written += 1;
                        let (w, h) = picture_extent(r, &bytes);
                        let title = if r.text.trim().is_empty() { String::new() } else { format!("<svg:title>{}</svg:title>", esc(&r.text)) };
                        // A floating picture is anchored to its paragraph
                        // and placed by a graphic style of its own.
                        let anchoring = match r.style.image_anchor {
                            Some(a) => {
                                let name = format!("fr{pictures_written}");
                                auto.push_str(&floating_style(&name, &a));
                                format!(
                                    "draw:style-name=\"{name}\" text:anchor-type=\"paragraph\" svg:x=\"{:.6}in\" svg:y=\"{:.6}in\"",
                                    a.x_emu as f64 / EMU_PER_INCH,
                                    a.y_emu as f64 / EMU_PER_INCH,
                                )
                            }
                            None => "text:anchor-type=\"as-char\"".to_string(),
                        };
                        inner.push_str(&format!(
                            "<draw:frame draw:name=\"Picture {pictures_written}\" {anchoring} \
                             svg:width=\"{:.6}in\" svg:height=\"{:.6}in\">\
                             <draw:image xlink:href=\"{name}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/>\
                             {title}</draw:frame>",
                            w as f64 / EMU_PER_INCH,
                            h as f64 / EMU_PER_INCH,
                        ));
                    }
                    None => inner.push_str(&esc(&r.text)),
                }
                continue;
            }
            // A footnote reference is an element, not text: ODF puts the
            // note's whole body inline at the reference point, and the
            // consumer renders the citation and the note area itself. The
            // run's own text carries nothing (the docx writer likewise
            // writes a reference and drops it), so it is not escaped in.
            if let Some(idx) = r.style.footnote {
                if let Some(text) = doc.footnotes.get(idx) {
                    let n = idx + 1;
                    inner.push_str(&format!(
                        "<text:note text:id=\"ftn{n}\" text:note-class=\"footnote\">\
                         <text:note-citation>{n}</text:note-citation>\
                         <text:note-body><text:p>{}</text:p></text:note-body>\
                         </text:note>",
                        esc(text)
                    ));
                }
                continue;
            }
            // A smart chip: a date is a fixed date field, which LibreOffice
            // and Word keep as a date; a link or person chip is its link,
            // named so it reopens as a chip.
            if let Some(chip) = &r.style.chip {
                inner.push_str(&chip_xml(chip, &r.text, r.style.link.as_deref()));
                continue;
            }
            // A tab is an element in ODF: a raw one is only whitespace.
            let mut run_xml = esc(&r.text).replace('\t', "<text:tab/>");
            let plain = RunStyle { revision: None, comments: Vec::new(), ..r.style.clone() };
            if plain != RunStyle::default() {
                let ti = run_styles.iter().position(|s| *s == plain).unwrap() + 1;
                run_xml = format!("<text:span text:style-name=\"T{ti}\">{run_xml}</text:span>");
            }
            // Inline code as Writer's "Source Text" (#1205): the monospace
            // font alone named a face the file never declared, so Writer
            // dropped it and the run came back as plain text.
            if r.style.code {
                run_xml = format!("<text:span text:style-name=\"Source_20_Text\">{run_xml}</text:span>");
            }
            if let Some(href) = &r.style.link {
                run_xml = format!(
                    "<text:a xlink:type=\"simple\" xlink:href=\"{}\">{}</text:a>",
                    esc(href),
                    run_xml
                );
            }
            match &r.style.revision {
                None => inner.push_str(&run_xml),
                Some(rev) => inner.push_str(&tracked_xml(&mut regions, rev, &run_xml)),
            }
        }

        // The threads the next paragraph's text is not in end here.
        let next: Vec<u32> = doc.paragraphs.get(pi + 1).and_then(|n| n.runs.first()).map(wanted).unwrap_or_default();
        let keep: Vec<u32> = open.iter().copied().filter(|id| next.contains(id)).collect();
        for (id, start) in crate::docx_comments::transition(&mut open, &keep) {
            inner.push_str(&annotate(id, start));
        }
        // A table of contents: LibreOffice's index, whose source says how
        // to regenerate it (headings 1-3: text, a dotted right tab, the
        // page), around the entries.
        let toc_at = |k: usize| doc.paragraphs.get(k).is_some_and(|q| q.style.toc.is_some());
        if p.style.toc.is_some() && (pi == 0 || !toc_at(pi - 1)) {
            body.push_str(&toc_open());
        }
        if let Some(level) = p.style.heading {
            body.push_str(&format!(
                "<text:h text:outline-level=\"{level}\"{style_attr}>{inner}</text:h>"
            ));
        } else {
            body.push_str(&format!("<text:p{style_attr}>{inner}</text:p>"));
        }
        if p.style.toc.is_some() && !toc_at(pi + 1) {
            body.push_str("</text:index-body></text:table-of-content>");
        }
    }
    body.push_str(&"</text:list-item></text:list>".repeat(depth));
    if let Some(t) = table.take() {
        t.close(&mut body);
    }
    let changes = if regions.is_empty() {
        String::new()
    } else {
        format!("<text:tracked-changes text:track-changes=\"false\">{}</text:tracked-changes>", regions.concat())
    };

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <office:document-content \
         xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
         xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
         xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" \
         xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" \
         xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" \
         xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" \
         xmlns:loext=\"urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
         office:version=\"1.2\">\
         <office:font-face-decls>\
         <style:font-face style:name=\"Monospace\" \
         svg:font-family=\"&apos;Liberation Mono&apos;, monospace\" \
         style:font-family-generic=\"modern\" style:font-pitch=\"fixed\"/>\
         </office:font-face-decls>\
         <office:automatic-styles>{auto}</office:automatic-styles>\
         <office:body><office:text>{changes}{body}</office:text></office:body>\
         </office:document-content>"
    )
}

/// A table of contents index's start: how to regenerate it, then its body.
fn toc_open() -> String {
    let templates: String = (1..=crate::toc::LEVELS)
        .map(|n| {
            format!(
                "<text:table-of-content-entry-template text:outline-level=\"{n}\" text:style-name=\"Contents_20_{n}\"><text:index-entry-text/><text:index-entry-tab-stop style:type=\"right\" style:leader-char=\".\"/><text:index-entry-page-number/></text:table-of-content-entry-template>"
            )
        })
        .collect();
    format!(
        "<text:table-of-content text:name=\"Table of Contents1\"><text:table-of-content-source text:outline-level=\"{}\">{templates}</text:table-of-content-source><text:index-body>",
        crate::toc::LEVELS
    )
}

/// The "Contents 1".."Contents 10" paragraph styles a table of contents'
/// entries name: each level indented one step more, with a dotted right
/// tab at the text column's edge for the page number.
fn contents_styles(doc: &Document) -> String {
    let g = doc.page.unwrap_or_default();
    let width = g.width_pt - g.margin_left_pt - g.margin_right_pt;
    (1..=10u8)
        .map(|n| {
            let indent = crate::toc::INDENT_PT * f64::from(n - 1);
            format!(
                "<style:style style:name=\"Contents_20_{n}\" style:display-name=\"Contents {n}\" style:family=\"paragraph\" style:class=\"index\"><style:paragraph-properties fo:margin-left=\"{indent:.2}pt\" fo:margin-right=\"0pt\" fo:text-indent=\"0pt\"><style:tab-stops><style:tab-stop style:position=\"{:.2}pt\" style:type=\"right\" style:leader-style=\"dotted\" style:leader-text=\".\"/></style:tab-stops></style:paragraph-properties></style:style>",
                width - indent
            )
        })
        .collect()
}

/// A comment as an ODF annotation; a reply names its thread as
/// LibreOffice does (`loext:parent-name`).
fn annotation_xml(c: &crate::model::Comment) -> String {
    let parent = c.parent.map(|p| format!(" loext:parent-name=\"__Annotation__{p}\"")).unwrap_or_default();
    let paras: String = c.text.split('\n').map(|l| format!("<text:p>{}</text:p>", esc(l))).collect();
    format!(
        "<office:annotation office:name=\"__Annotation__{}\"{parent} loext:resolved=\"{}\"><dc:creator>{}</dc:creator><dc:date>{}</dc:date>{paras}</office:annotation>",
        c.id,
        c.resolved,
        esc(&c.author),
        esc(&c.date)
    )
}

/// "{page}" and "{total}" in escaped header or footer text as ODF's page
/// number and page count fields.
fn page_fields(text: &str) -> String {
    text.replace("{page}", "<text:page-number text:select-page=\"current\">1</text:page-number>")
        .replace("{total}", "<text:page-count>1</text:page-count>")
}

/// A tracked change's body markup: the run between change marks for an
/// insertion, a change mark alone for a deletion (whose text goes into its
/// region, as ODF keeps deleted text out of the body). The region is added
/// to `regions`. A deletion of someone else's insertion is one region with
/// the deletion and then the insertion it deleted, as LibreOffice writes it.
fn tracked_xml(regions: &mut Vec<String>, rev: &crate::model::Revision, run_xml: &str) -> String {
    let id = format!("ct{}", regions.len() + 1);
    let info_of = |r: &crate::model::Revision| {
        format!(
            "<office:change-info><dc:creator>{}</dc:creator><dc:date>{}</dc:date></office:change-info>",
            esc(&r.author),
            esc(&r.date)
        )
    };
    let info = info_of(rev);
    let under = rev.under.as_deref().map(|u| format!("<text:insertion>{}</text:insertion>", info_of(u))).unwrap_or_default();
    let (region, body) = match rev.kind {
        crate::model::RevisionKind::Insert => (
            format!("<text:insertion>{info}</text:insertion>"),
            format!("<text:change-start text:change-id=\"{id}\"/>{run_xml}<text:change-end text:change-id=\"{id}\"/>"),
        ),
        crate::model::RevisionKind::Delete => (
            format!("<text:deletion>{info}<text:p>{run_xml}</text:p></text:deletion>{under}"),
            format!("<text:change text:change-id=\"{id}\"/>"),
        ),
    };
    regions.push(format!("<text:changed-region xml:id=\"{id}\" text:id=\"{id}\">{region}</text:changed-region>"));
    body
}

/// A smart chip's ODF: LibreOffice's own content control
/// (`loext:content-control`, what Writer makes of a Word content control),
/// tagged with what the chip is, holding its label (inside its link for a
/// link or person). A date chip is also a date control, so Writer shows a
/// date picker and saves it to .docx as a Word date control. A reader that
/// does not know the element still shows its text.
fn chip_xml(chip: &crate::chips::Chip, label: &str, link: Option<&str>) -> String {
    use crate::chips::ChipKind;
    let kind = match chip.kind {
        ChipKind::Date => "date",
        ChipKind::Person => "person",
        ChipKind::Link => "link",
    };
    let date = match chip.kind {
        ChipKind::Date => format!(
            " loext:date=\"true\" loext:date-format=\"d MMM yyyy\" loext:date-rfc-language-tag=\"en-GB\" loext:current-date=\"{}T00:00:00Z\"",
            esc(&chip.value)
        ),
        _ => String::new(),
    };
    let inner = match link {
        Some(href) => format!("<text:a xlink:type=\"simple\" xlink:href=\"{}\">{}</text:a>", esc(href), esc(label)),
        None => esc(label),
    };
    format!(
        "<loext:content-control loext:tag=\"{}\"{date}>{inner}</loext:content-control>",
        esc(&format!("{CHIP_NAME_PREFIX}{kind}:{}", chip.value))
    )
}

/// Prefix of a chip content control's tag.
const CHIP_NAME_PREFIX: &str = "letters-chip:";

/// The chip a content control's tag stands for.
fn chip_from_name(name: &str) -> Option<crate::chips::Chip> {
    use crate::chips::{Chip, ChipKind};
    let (kind, value) = name.strip_prefix(CHIP_NAME_PREFIX)?.split_once(':')?;
    let kind = match kind {
        "date" => ChipKind::Date,
        "link" => ChipKind::Link,
        "person" => ChipKind::Person,
        _ => return None,
    };
    Some(Chip { kind, value: value.to_string() })
}

fn styles_xml(doc: &Document) -> String {
    let layout = match &doc.page {
        Some(pg) => format!(
            "<style:page-layout-properties \
             fo:page-width=\"{:.2}pt\" fo:page-height=\"{:.2}pt\" \
             fo:margin-top=\"{:.2}pt\" fo:margin-bottom=\"{:.2}pt\" \
             fo:margin-left=\"{:.2}pt\" fo:margin-right=\"{:.2}pt\">\
             <style:columns fo:column-count=\"{}\" fo:column-gap=\"{:.2}pt\"/>\
             </style:page-layout-properties>",
            pg.width_pt, pg.height_pt, pg.margin_top_pt, pg.margin_bottom_pt,
            pg.margin_left_pt, pg.margin_right_pt, pg.columns.max(1), pg.column_gap_pt
        ),
        None => "<style:page-layout-properties/>".to_string(),
    };
    let mut hf = String::new();
    if doc.header.is_some() || doc.footer.is_some() {
        hf.push_str("<office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"pm1\">");
        if let Some(h) = &doc.header {
            hf.push_str(&format!(
                "<style:header><text:p>{}</text:p></style:header>",
                page_fields(&esc(h))
            ));
        }
        if let Some(f) = &doc.footer {
            hf.push_str(&format!(
                "<style:footer><text:p>{}</text:p></style:footer>",
                page_fields(&esc(f))
            ));
        }
        hf.push_str("</style:master-page></office:master-styles>");
    } else {
        hf.push_str("<office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"pm1\"/></office:master-styles>");
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <office:document-styles \
         xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
         xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
         xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" \
         office:version=\"1.2\">\
         <office:styles>{contents}</office:styles>\
         <office:automatic-styles>\
         <style:page-layout style:name=\"pm1\">{layout}\
         </style:page-layout></office:automatic-styles>{hf}\
         </office:document-styles>",
        contents = CODE_STYLES.to_string()
            + &heading_styles(doc)
            + &if doc.paragraphs.iter().any(|p| p.style.toc.is_some()) { contents_styles(doc) } else { String::new() }
    )
}

/// `Heading_20_1`..`6`, the styles headings name (#1297): the document's
/// own heading look if it has one, else Letters' (bold, scaled from the
/// body size), as the docx writer does. The odt used to carry no heading
/// look at all.
fn heading_styles(doc: &Document) -> String {
    let base_hp = doc.base_font.size_hp.unwrap_or((crate::layout::LayoutOptions::default().font_size_pt * 2.0) as u16);
    (1u8..=6)
        .map(|n| {
            let look = doc.heading_styles.get(usize::from(n) - 1).cloned().unwrap_or_else(|| RunStyle {
                bold: true,
                font_size_hp: Some((f64::from(base_hp) * crate::layout::heading_scale(n)).round() as u16),
                ..Default::default()
            });
            let look = RunStyle { font_size_hp: look.font_size_hp.or(Some(base_hp)), ..look };
            format!(
                "<style:style style:name=\"Heading_20_{n}\" style:display-name=\"Heading {n}\" style:family=\"paragraph\" \
                 style:default-outline-level=\"{n}\" style:class=\"text\"><style:paragraph-properties fo:keep-with-next=\"always\"/>\
                 <style:text-properties{}/></style:style>",
                run_style_props(&look)
            )
        })
        .collect()
}

/// Writer's code block and inline code styles, which code paragraphs and
/// runs name (#1205). Writer drops a reference to a spaced built-in name
/// ("Preformatted Text") that the file doesn't define, so a code block
/// came back from Writer as plain text.
const CODE_STYLES: &str = "<style:style style:name=\"Preformatted_20_Text\" style:display-name=\"Preformatted Text\" \
     style:family=\"paragraph\" style:class=\"html\"><style:paragraph-properties fo:margin-top=\"0pt\" fo:margin-bottom=\"0pt\"/>\
     <style:text-properties fo:font-family=\"'Liberation Mono'\" fo:font-size=\"10pt\"/></style:style>\
     <style:style style:name=\"Source_20_Text\" style:display-name=\"Source Text\" style:family=\"text\">\
     <style:text-properties fo:font-family=\"'Liberation Mono'\"/></style:style>";

const MANIFEST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.2\">\
<manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"application/vnd.oasis.opendocument.text\"/>\
<manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>\
<manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>\
</manifest:manifest>";

/// Write the document as .odt. Built fully in memory, then placed
/// atomically (see suite_common_core::atomic_save) — a rename before the
/// ZipWriter flushes its central directory would leave a corrupt archive.
pub fn write(doc: &Document, path: impl AsRef<std::path::Path>) -> Result<(), String> {
    let buf = std::io::Cursor::new(Vec::new());
    let mut z = zip::ZipWriter::new(buf);
    // Per ODF spec the mimetype entry comes first and uncompressed.
    z.start_file(
        "mimetype",
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )
    .map_err(|e| e.to_string())?;
    z.write_all(MIMETYPE.as_bytes()).map_err(|e| e.to_string())?;
    let opt = zip::write::SimpleFileOptions::default();
    // Every readable picture once, in the order the document uses them.
    let mut pictures: Vec<(String, Vec<u8>, &'static str)> = Vec::new();
    for src in doc.paragraphs.iter().flat_map(|p| &p.runs).filter_map(|r| r.style.image.as_deref()) {
        if let Some(pic) = picture(src).filter(|pic| !pictures.iter().any(|seen| seen.0 == pic.0)) {
            pictures.push(pic);
        }
    }
    let entries: String = pictures
        .iter()
        .map(|(name, _, mime)| format!("<manifest:file-entry manifest:full-path=\"{name}\" manifest:media-type=\"{mime}\"/>"))
        .collect();
    z.start_file("META-INF/manifest.xml", opt).map_err(|e| e.to_string())?;
    z.write_all(MANIFEST.replace("</manifest:manifest>", &format!("{entries}</manifest:manifest>")).as_bytes()).map_err(|e| e.to_string())?;
    for (name, bytes, _) in &pictures {
        z.start_file(name.as_str(), opt).map_err(|e| e.to_string())?;
        z.write_all(bytes).map_err(|e| e.to_string())?;
    }
    z.start_file("content.xml", opt).map_err(|e| e.to_string())?;
    z.write_all(content_xml(doc).as_bytes()).map_err(|e| e.to_string())?;
    z.start_file("styles.xml", opt).map_err(|e| e.to_string())?;
    z.write_all(styles_xml(doc).as_bytes()).map_err(|e| e.to_string())?;
    let bytes = z.finish().map_err(|e| e.to_string())?.into_inner();
    suite_common_core::atomic_save::atomic_write_bytes(path.as_ref(), &bytes)
}

/// Write an ODT while preserving package members captured by
/// [`read_with_report`].
pub fn write_with_opaque(doc: &Document, path: impl AsRef<std::path::Path>, opaque: &suite_common_core::interop::OpaquePackage) -> Result<(), String> {
    let path = path.as_ref();
    write(doc, path)?;
    opaque.append_to(path)
}

// ── Reading ──────────────────────────────────────────────────────────

fn attr_val(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    e.attributes().filter_map(|a| a.ok()).find_map(|a| {
        if a.key.into_inner() == name {
            // Attribute values are escaped XML too: a link to "?a=1&b=2"
            // is written "&amp;" and read back as "&amp;" without this.
            let raw = a.value.to_string();
            Some(quick_xml::escape::unescape(&raw).map(|s| s.into_owned()).unwrap_or(raw))
        } else {
            None
        }
    })
}

/// Parse the style:text-properties / paragraph-properties of automatic
/// styles into model styles keyed by style name.
struct AutoStyles {
    text: std::collections::HashMap<String, RunStyle>,
    para: std::collections::HashMap<String, AutoParaStyle>,
    /// Automatic paragraph style → its style:parent-style-name (LO
    /// rewrites named styles as autos inheriting from the built-in).
    para_parent: std::collections::HashMap<String, String>,
    /// Automatic text style → its parent, as for paragraphs.
    text_parent: std::collections::HashMap<String, String>,
    /// A paragraph style's own text properties: in styles.xml, how each
    /// `Heading_20_N` looks (#1297).
    para_text: std::collections::HashMap<String, RunStyle>,
    /// A table-column style's width, in points.
    column: std::collections::HashMap<String, f64>,
    /// A table-row style's height.
    row: std::collections::HashMap<String, crate::model::RowHeight>,
    /// A table-cell style's background colour, six hex digits.
    cell_fill: std::collections::HashMap<String, String>,
    /// A graphic style's placement of a floating frame.
    graphic: std::collections::HashMap<String, crate::model::ImageAnchor>,
}

/// Paragraph-level values read off one automatic style. Lengths are points.
#[derive(Clone, Debug, Default)]
struct AutoParaStyle {
    alignment: Alignment,
    page_break_before: bool,
    keep_with_next: bool,
    line_spacing: f32,
    space_before_pt: f64,
    space_after_pt: f64,
    left_indent_pt: f64,
    right_indent_pt: f64,
    first_line_indent_pt: f64,
    /// Positions of `style:tab-stops` children, in document order.
    tab_stops_pt: Vec<f64>,
    /// Whether the style itself sets its space before, space after and
    /// line height; what it leaves unset comes from its parent style.
    sets: [bool; 3],
}

/// Each `text:list-style` in `xml`: whether each of its levels is numbered,
/// by level (1-based in the file, 0-based here). LibreOffice names the list
/// styles it writes "WWNum1", "L2"…, so the kind can't be read off the name.
fn list_style_kinds(xml: &str) -> std::collections::HashMap<String, Vec<ListKind>> {
    let mut out: std::collections::HashMap<String, Vec<ListKind>> = Default::default();
    let mut reader = Reader::from_str(xml);
    let mut current: Option<String> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                "text:list-style" => current = attr_val(&e, "style:name"),
                name @ ("text:list-level-style-number" | "text:list-level-style-bullet" | "text:list-level-style-image") => {
                    let (Some(style), Some(level)) = (current.as_ref(), attr_val(&e, "text:level").and_then(|l| l.parse::<usize>().ok())) else { continue };
                    let kinds = out.entry(style.clone()).or_default();
                    if (1..=10).contains(&level) {
                        if kinds.len() < level {
                            kinds.resize(level, ListKind::Bullet);
                        }
                        let numbered = name == "text:list-level-style-number"
                            && attr_val(&e, "style:num-format").is_some_and(|f| !f.is_empty());
                        kinds[level - 1] = if numbered { ListKind::Numbered } else { ListKind::Bullet };
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) if e.name().as_ref() == "text:list-style" => current = None,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

fn parse_auto_styles(xml: &str) -> AutoStyles {
    let mut out = AutoStyles { text: Default::default(), para: Default::default(), para_parent: Default::default(), text_parent: Default::default(), para_text: Default::default(), column: Default::default(), row: Default::default(), cell_fill: Default::default(), graphic: Default::default() };
    let mut reader = Reader::from_str(xml);
    let mut cur_name: Option<String> = None;
    let mut cur_family = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                match e.name().as_ref() {
                    "style:style" => {
                        cur_name = attr_val(&e, "style:name");
                        cur_family = attr_val(&e, "style:family").unwrap_or_default();
                        if let (Some(n), Some(parent)) = (cur_name.clone(), attr_val(&e, "style:parent-style-name")) {
                            match cur_family.as_str() {
                                "paragraph" => { out.para_parent.insert(n, parent); }
                                "text" => { out.text_parent.insert(n, parent); }
                                _ => {}
                            }
                        }
                    }
                    "style:table-column-properties" => {
                        if let (Some(name), Some(w)) = (cur_name.clone(), attr_val(&e, "style:column-width").and_then(|v| parse_length_pt(&v))) {
                            out.column.insert(name, w);
                        }
                    }
                    "style:graphic-properties" if cur_family == "graphic" => {
                        if let Some(name) = cur_name.clone() {
                            out.graphic.insert(name, graphic_placement(&e));
                        }
                    }
                    "style:table-cell-properties" => {
                        let fill = attr_val(&e, "fo:background-color").map(|c| c.trim_start_matches('#').to_uppercase());
                        if let (Some(name), Some(color)) = (cur_name.clone(), fill.filter(|c| c.len() == 6 && c.chars().all(|ch| ch.is_ascii_hexdigit()))) {
                            out.cell_fill.insert(name, color);
                        }
                    }
                    // A minimum row height, or a fixed one (a height
                    // LibreOffice marks optimal is only a minimum).
                    "style:table-row-properties" => {
                        let len = |a: &str| attr_val(&e, a).and_then(|v| parse_length_pt(&v)).filter(|h| *h > 0.0);
                        let optimal = attr_val(&e, "style:use-optimal-row-height").as_deref() == Some("true");
                        let height = match (len("style:min-row-height"), len("style:row-height")) {
                            (Some(pt), _) => Some(crate::model::RowHeight { pt, exact: false }),
                            (None, Some(pt)) => Some(crate::model::RowHeight { pt, exact: !optimal }),
                            (None, None) => None,
                        };
                        if let (Some(name), Some(h)) = (cur_name.clone(), height) {
                            out.row.insert(name, h);
                        }
                    }
                    "style:text-properties" => {
                        if let Some(name) = cur_name.clone().filter(|_| cur_family == "text" || cur_family == "paragraph") {
                            let mut st = RunStyle::default();
                            if attr_val(&e, "fo:font-weight").as_deref() == Some("bold") {
                                st.bold = true;
                            }
                            if attr_val(&e, "fo:font-style").as_deref() == Some("italic") {
                                st.italic = true;
                            }
                            if attr_val(&e, "style:text-underline-style")
                                .map(|v| v != "none")
                                .unwrap_or(false)
                            {
                                st.underline = true;
                            }
                            if attr_val(&e, "style:text-line-through-style")
                                .map(|v| v != "none")
                                .unwrap_or(false)
                            {
                                st.strikethrough = true;
                            }
                            if let Some(bg) = attr_val(&e, "fo:background-color") {
                                if bg.to_lowercase() == "#ffff00" {
                                    st.highlight = true;
                                }
                            }
                            if let Some(sz) = attr_val(&e, "fo:font-size") {
                                if let Ok(pt) = sz.trim_end_matches("pt").parse::<f32>() {
                                    st.font_size_hp = Some((pt * 2.0).round() as u16);
                                }
                            }
                            if let Some(c) = attr_val(&e, "fo:color") {
                                st.color = Some(c.trim_start_matches('#').to_lowercase());
                            }
                            if let Some(fam) = attr_val(&e, "fo:font-family") {
                                let f = fam.to_lowercase();
                                if f.contains("mono") || f.contains("courier") {
                                    st.code = true;
                                } else {
                                    st.font_family =
                                        Some(fam.trim_matches('\'').to_string());
                                }
                            }
                            if let Some(fname) = attr_val(&e, "style:font-name") {
                                // Exactly what our writer emits for code
                                // spans; LO preserves or maps it to a
                                // mono face.
                                let f = fname.to_lowercase();
                                if f.contains("monospace")
                                    || f.contains("courier")
                                    || f.contains("liberation mono")
                                {
                                    st.code = true;
                                    st.font_family = None;
                                }
                            }
                            if let Some(tp) = attr_val(&e, "style:text-position") {
                                if tp.starts_with("super") {
                                    st.vert_align = Some(VertAlign::Superscript);
                                } else if tp.starts_with("sub") {
                                    st.vert_align = Some(VertAlign::Subscript);
                                }
                            }
                            // Writer names a paragraph style's face by its
                            // font declaration.
                            if cur_family == "paragraph" {
                                if let Some(fname) = attr_val(&e, "style:font-name").filter(|_| !st.code && st.font_family.is_none()) {
                                    st.font_family = Some(fname);
                                }
                                out.para_text.insert(name, st);
                            } else {
                                out.text.insert(name, st);
                            }
                        }
                    }
                    "style:paragraph-properties" => {
                        if let (Some(name), "paragraph") = (cur_name.clone(), cur_family.as_str()) {
                            let align = match attr_val(&e, "fo:text-align").as_deref() {
                                Some("center") => Alignment::Center,
                                Some("end") | Some("right") => Alignment::Right,
                                Some("justify") => Alignment::Justify,
                                _ => Alignment::Left,
                            };
                            let brk = attr_val(&e, "fo:break-before").as_deref() == Some("page");
                            let spacing = attr_val(&e, "fo:line-height")
                                .and_then(|v| v.trim_end_matches('%').parse::<f32>().ok())
                                .map(|pct| pct / 100.0)
                                .unwrap_or(1.0);
                            let length = |name: &str| attr_val(&e, name).and_then(|v| parse_length_pt(&v)).unwrap_or(0.0);
                            out.para.insert(name, AutoParaStyle {
                                alignment: align,
                                page_break_before: brk,
                                keep_with_next: attr_val(&e, "fo:keep-with-next").as_deref() == Some("always"),
                                line_spacing: spacing,
                                // `fo:space-before` is what this writer
                                // used to emit; still accepted so a
                                // document saved by an older build keeps
                                // its spacing.
                                space_before_pt: {
                                    let m = length("fo:margin-top");
                                    if m != 0.0 { m } else { length("fo:space-before") }
                                },
                                space_after_pt: {
                                    let m = length("fo:margin-bottom");
                                    if m != 0.0 { m } else { length("fo:space-after") }
                                },
                                left_indent_pt: length("fo:margin-left"),
                                right_indent_pt: length("fo:margin-right"),
                                first_line_indent_pt: length("fo:text-indent"),
                                // Filled from the `style:tab-stop`
                                // children that follow this element.
                                tab_stops_pt: Vec::new(),
                                sets: [
                                    attr_val(&e, "fo:margin-top").is_some() || attr_val(&e, "fo:space-before").is_some(),
                                    attr_val(&e, "fo:margin-bottom").is_some() || attr_val(&e, "fo:space-after").is_some(),
                                    attr_val(&e, "fo:line-height").is_some(),
                                ],
                            });
                        }
                    }
                    // A tab stop is a child of the paragraph properties
                    // just inserted above, so the entry is already there.
                    "style:tab-stop" => {
                        if let (Some(name), "paragraph") = (cur_name.clone(), cur_family.as_str()) {
                            if let Some(pos) =
                                attr_val(&e, "style:position").and_then(|v| parse_length_pt(&v))
                            {
                                if let Some(st) = out.para.get_mut(&name) {
                                    st.tab_stops_pt.push(pos);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    out
}

/// Parse an ODF length ("595.3pt", "21cm", "210mm", "8.5in") to points.
fn parse_length_pt(v: &str) -> Option<f64> {
    let v = v.trim();
    let (num, unit) = v.split_at(v.find(|c: char| c.is_ascii_alphabetic())?);
    let n: f64 = num.parse().ok()?;
    Some(match unit {
        "pt" => n,
        "cm" => n * 72.0 / 2.54,
        "mm" => n * 72.0 / 25.4,
        "in" => n * 72.0,
        _ => return None,
    })
}

/// A list that continues the numbering of the one before it with its
/// style starts where that one stopped: its first numbered item gets the
/// start value, so headings between them (which end a list in the model)
/// do not restart the count at 1. A Word conversion writes every numbered
/// paragraph between headings as such a list.
fn continue_numbering(paragraphs: &mut [Paragraph], top_lists: &[(usize, usize, Option<String>, bool)]) {
    let mut counts: std::collections::HashMap<Option<String>, u32> = Default::default();
    for (start, end, style, continues) in top_lists {
        let mut count = if *continues { counts.get(style).copied().unwrap_or(0) } else { 0 };
        let end = (*end).min(paragraphs.len());
        let mut first = true;
        for p in paragraphs.get_mut(*start..end).into_iter().flatten() {
            if p.style.list != ListKind::Numbered || p.style.list_level != 0 {
                continue;
            }
            if first && *continues && count > 0 && p.style.list_start.is_none() {
                p.style.list_start = Some(count + 1);
            }
            first = false;
            count = p.style.list_start.unwrap_or(count + 1);
        }
        counts.insert(style.clone(), count);
    }
}

/// The master page LibreOffice gives the first page: the one the first
/// paragraph or table's style names (`style:master-page-name`), else
/// "Standard", else the first one declared.
fn first_master_page(content: &str, styles: &str) -> Option<String> {
    let mut named: std::collections::HashMap<String, String> = Default::default();
    let mut masters: Vec<String> = Vec::new();
    let mut first_style: Option<String> = None;
    for xml in [content, styles] {
        let mut reader = Reader::from_str(xml);
        let mut in_text = false;
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                    "style:style" => {
                        if let (Some(n), Some(m)) = (attr_val(&e, "style:name"), attr_val(&e, "style:master-page-name").filter(|m| !m.is_empty())) {
                            named.insert(n, m);
                        }
                    }
                    "style:master-page" => masters.extend(attr_val(&e, "style:name")),
                    "office:text" => in_text = true,
                    "text:p" | "text:h" | "table:table" if in_text && first_style.is_none() => {
                        first_style = Some(attr_val(&e, if e.name().as_ref() == "table:table" { "table:style-name" } else { "text:style-name" }).unwrap_or_default());
                    }
                    _ => {}
                },
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
    }
    first_style
        .and_then(|s| named.get(&s).cloned())
        .filter(|m| masters.contains(m))
        .or_else(|| masters.iter().find(|m| *m == "Standard").cloned())
        .or_else(|| masters.first().cloned())
}

/// Column layout declared on a `text:section` rather than on the page.
///
/// ODF allows either, and the two live in different parts: a page-wide
/// layout carries `style:columns` inside `style:page-layout-properties`
/// in styles.xml, which is what this writer emits. LibreOffice, reading
/// a docx `w:cols`, models it as a section instead and writes
/// `style:section-properties` into content.xml — so a reader that only
/// looks at the page layout sees a two-column document as one column.
/// Nothing is lost in that conversion; it is simply recorded elsewhere.
///
/// Returns the first section's count and gap, the gap only when stated.
fn parse_section_columns(xml: &str) -> Option<(u8, Option<f64>)> {
    let mut reader = Reader::from_str(xml);
    let mut in_section_props = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) if e.name().as_ref() == "style:section-properties" => {
                in_section_props = true;
            }
            Ok(Event::End(e)) if e.name().as_ref() == "style:section-properties" => {
                in_section_props = false;
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if in_section_props && e.name().as_ref() == "style:columns" =>
            {
                let count = attr_val(&e, "fo:column-count").and_then(|v| v.parse::<u8>().ok())?;
                if count <= 1 {
                    return None;
                }
                let gap = attr_val(&e, "fo:column-gap").and_then(|v| parse_length_pt(&v));
                return Some((count, gap));
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

/// Read an .odt into the model.
pub fn read(path: &str) -> Result<Document, String> {
    read_parts(path).map(|(doc, _)| doc)
}

/// A picture frame being read: its size, its image's package member, its
/// alt text so far, whether that text is being read, and the depth of the
/// frame element (frames nest: a captioned picture is a text box frame
/// holding a picture frame).
struct FrameReading {
    extent: Option<(u64, u64)>,
    /// Where a frame not anchored as a character floats.
    anchor: Option<crate::model::ImageAnchor>,
    href: Option<String>,
    alt: String,
    in_alt: bool,
    depth: usize,
}

/// How headings 1-6 look, from styles.xml's `Heading_20_N` (#1297), each
/// with what it inherits from its parent styles; empty when the file
/// defines none, as the docx reader does.
/// The document's body font: the default paragraph style's font and size,
/// with the "Standard" (Default Paragraph Style) style's own over them, as
/// Writer resolves them. A font is named by its font-face declaration.
/// Without it, a Calibri document drew in the application's serif face.
fn read_base_font(styles: &str) -> crate::model::BaseFont {
    let mut faces: std::collections::HashMap<String, String> = Default::default();
    // (font name, size in points) from the default style, then Standard,
    // then Word's "Normal": a Word conversion has no Standard, and every
    // paragraph style inherits Normal's font, not the default style's.
    let mut found: [(Option<String>, Option<f64>); 3] = Default::default();
    let mut slot: Option<usize> = None;
    let mut reader = Reader::from_str(styles);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                "style:font-face" => {
                    if let (Some(n), Some(f)) = (attr_val(&e, "style:name"), attr_val(&e, "svg:font-family")) {
                        faces.insert(n, f.trim_matches(|c| c == '\'' || c == '"').to_string());
                    }
                }
                "style:default-style" => {
                    slot = (attr_val(&e, "style:family").as_deref() == Some("paragraph")).then_some(0);
                }
                "style:style" => {
                    let paragraph = attr_val(&e, "style:family").as_deref() == Some("paragraph");
                    slot = match attr_val(&e, "style:name").as_deref() {
                        Some("Standard") if paragraph => Some(1),
                        Some("Normal") if paragraph => Some(2),
                        _ => None,
                    };
                }
                "style:text-properties" => {
                    if let Some(i) = slot {
                        if let Some(n) = attr_val(&e, "style:font-name") {
                            found[i].0 = Some(n);
                        }
                        if let Some(pt) = attr_val(&e, "fo:font-size").and_then(|v| parse_length_pt(&v)) {
                            found[i].1 = Some(pt);
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) if matches!(e.name().as_ref(), "style:default-style" | "style:style") => slot = None,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    // Normal stands in for Standard only when the file has no Standard.
    let base = if found[1].0.is_some() || found[1].1.is_some() { 1 } else { 2 };
    let name = found[base].0.clone().or_else(|| found[0].0.clone());
    let size = found[base].1.or(found[0].1);
    crate::model::BaseFont {
        family: name.map(|n| faces.get(&n).cloned().unwrap_or(n)),
        size_hp: size.map(|pt| (pt * 2.0).round() as u16).filter(|hp| *hp > 0),
    }
}

/// The space before, space after and line height paragraph style `name`
/// has: its own where it sets them, else the nearest style up its parent
/// chain (automatic styles in `auto`, named ones in `named`) that does.
/// None where no style in the chain sets one.
fn inherited_spacing(name: &str, auto: &AutoStyles, named: &AutoStyles) -> (Option<f64>, Option<f64>, Option<f32>) {
    let (mut before, mut after, mut line) = (None, None, None);
    let mut cur = Some(name.to_string());
    for _ in 0..16 {
        let Some(n) = cur else { break };
        if let Some(st) = auto.para.get(&n).or_else(|| named.para.get(&n)) {
            if st.sets[0] && before.is_none() {
                before = Some(st.space_before_pt);
            }
            if st.sets[1] && after.is_none() {
                after = Some(st.space_after_pt);
            }
            if st.sets[2] && line.is_none() {
                line = Some(st.line_spacing);
            }
        }
        cur = auto.para_parent.get(&n).or_else(|| named.para_parent.get(&n)).cloned();
    }
    (before, after, line)
}

fn read_heading_styles(styles: &str) -> Vec<RunStyle> {
    let defs = parse_auto_styles(styles);
    let names = heading_style_names(styles);
    let look = |n: u8| -> Option<RunStyle> {
        let mut chain = Vec::new();
        let mut name = names[usize::from(n) - 1].clone();
        while chain.len() < 8 {
            let own = defs.para_text.get(&name);
            let parent = defs.para_parent.get(&name).cloned();
            if own.is_none() && parent.is_none() {
                break;
            }
            chain.push(own.cloned().unwrap_or_default());
            match parent {
                Some(p) => name = p,
                None => break,
            }
        }
        (!chain.is_empty()).then(|| {
            chain.into_iter().rev().fold(RunStyle::default(), |base, own| RunStyle {
                bold: base.bold || own.bold,
                italic: base.italic || own.italic,
                font_size_hp: own.font_size_hp.or(base.font_size_hp),
                color: own.color.or(base.color),
                font_family: own.font_family.or(base.font_family),
                ..Default::default()
            })
        })
    };
    if !names.iter().any(|n| defs.para_text.contains_key(n)) {
        return Vec::new();
    }
    (1..=6).map(|n| look(n).unwrap_or_default()).collect()
}

/// Each heading level's paragraph style: Writer's own `Heading_20_N`, else
/// the paragraph style declaring that outline level
/// (`style:default-outline-level`), as a Word conversion names them
/// ("Heading2", "Heading 2").
fn heading_style_names(styles: &str) -> [String; 6] {
    let mut by_level: [Option<String>; 6] = Default::default();
    let mut reader = Reader::from_str(styles);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == "style:style" => {
                let level = attr_val(&e, "style:default-outline-level").and_then(|l| l.parse::<usize>().ok()).filter(|l| (1..=6).contains(l));
                if let (Some(l), Some(name), Some("paragraph")) = (level, attr_val(&e, "style:name"), attr_val(&e, "style:family").as_deref()) {
                    by_level[l - 1].get_or_insert(name);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    std::array::from_fn(|i| {
        let own = format!("Heading_20_{}", i + 1);
        if styles.contains(&format!("style:name=\"{own}\"")) { own } else { by_level[i].clone().unwrap_or(own) }
    })
}

/// The document, and the package members its pictures were read from
/// (which are therefore not opaque, #1292).
fn read_parts(path: &str) -> Result<(Document, Vec<String>), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    // An ODT arrives from a download or an attachment; its parts are read
    // under a budget so a member that claims to decompress to gigabytes is
    // refused rather than allocated (#442).
    let mut budget = ZipBudget::default();
    budget.check_entry_count(zip.len())?;
    let content = zip.part_to_string("content.xml", &mut budget).map_err(|e| {
        if e.is_missing() { "no content.xml — not an ODT?".to_string() } else { e.to_string() }
    })?;
    let styles = zip.optional_part_to_string("styles.xml", &mut budget);

    let auto = parse_auto_styles(&content);
    let mut list_styles = list_style_kinds(&styles);
    list_styles.extend(list_style_kinds(&content));

    // styles.xml's named styles, the parents of content.xml's automatic ones.
    let named = parse_auto_styles(&styles);
    let mut doc = Document { paragraphs: Vec::new(), footnotes: Vec::new(), header: None, footer: None, page: None, base_font: read_base_font(&styles), heading_styles: read_heading_styles(&styles), comments: Vec::new(), table_columns: Default::default(), table_rows: Default::default(), table_fills: Default::default() };
    let mut reader = Reader::from_str(&content);
    let mut in_body = false;
    let mut para: Option<Paragraph> = None;
    // Span/link style stack: (style, depth marker)
    let mut span_stack: Vec<RunStyle> = Vec::new();
    // Body paragraphs open, each with its text base on `span_stack`.
    let mut paragraph_bases = 0usize;
    let mut link_stack: Vec<String> = Vec::new();
    let mut list_kind = ListKind::None;
    let mut list_level: u8 = 0;
    // The kind of each open text:list, so a nested list without a style of
    // its own takes its parent's, and closing it restores the parent's.
    let mut list_kinds: Vec<ListKind> = Vec::new();
    // The list style of each open text:list: a nested list naming none
    // continues its parent's, at its own level.
    let mut list_style_names: Vec<Option<String>> = Vec::new();
    // A list item's `text:start-value`, for the item's first paragraph.
    let mut pending_start: Option<u32> = None;
    // Each top-level list: where its paragraphs start and end, its style,
    // and whether it continues the numbering of the list before it with
    // that style (`text:continue-numbering`, `text:continue-list`).
    let mut top_lists: Vec<(usize, usize, Option<String>, bool)> = Vec::new();
    // A `text:note` nests its body *inside* the referencing paragraph, so
    // its `text:p` children have to be kept out of the body stream: while
    // a note is open, text accumulates into the note instead. The citation
    // (the rendered marker) is a consumer's business and is skipped, or it
    // would arrive as a stray "1" in the footnote text.
    let mut note: Option<String> = None;
    let mut note_paras = 0usize;
    let mut in_citation = false;
    // A smart chip being read: what it is and its label so far, and the
    // element that closes it (a content control, or a standard date field
    // from another application).
    let mut chip: Option<(crate::chips::Chip, String, &'static str)> = None;
    // Tracked changes. `text:tracked-changes` comes first in `office:text`:
    // each region's change (who, when, and a deletion's text) is read into
    // `regions`; the body then marks insertions (change-start/end, whose
    // revision applies to the text between) and deletions (a change mark
    // where the deleted text goes back in).
    let mut regions: std::collections::HashMap<String, (crate::model::Revision, Vec<Run>)> = std::collections::HashMap::new();
    // The region being read: its id, its changes in order (kind, author,
    // date) and a deletion's text.
    type Entry = (crate::model::RevisionKind, String, String);
    let mut region: Option<(String, Vec<Entry>, Vec<Run>)> = None;
    let mut in_changes = false;
    let mut change_field: Option<&'static str> = None;
    let mut rev_stack: Vec<crate::model::Revision> = Vec::new();
    // Comments: each annotation is read whole into `annotation` (the
    // comment, its parent's name, the field being read, its paragraphs so
    // far); one whose range ends somewhere (`ended`) puts a start marker in
    // the text, and its end an end marker, which `docx_comments::restore`
    // turns into marks.
    let ended: std::collections::HashSet<String> = content
        .match_indices("<office:annotation-end ")
        .filter_map(|(at, _)| {
            let tag = &content[at..at + content[at..].find('>')?];
            let v = tag.find("office:name=\"")? + "office:name=\"".len();
            Some(tag[v..v + tag[v..].find('"')?].to_string())
        })
        .collect();
    let mut names: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    // Ids: ours are in our annotations' names ("__Annotation__7"), so a
    // document reopens with its own; any other annotation takes a free one.
    let own_id = |name: &str| name.strip_prefix("__Annotation__").and_then(|n| n.parse::<u32>().ok()).filter(|n| *n > 0);
    let reserved: std::collections::HashSet<u32> = content
        .match_indices("<office:annotation ")
        .filter_map(|(at, _)| {
            let tag = &content[at..at + content[at..].find('>')?];
            let v = tag.find("office:name=\"")? + "office:name=\"".len();
            own_id(&tag[v..v + tag[v..].find('"')?])
        })
        .collect();
    let mut next_free = 1u32;
    // A table of contents index: inside its body (not its title), each
    // paragraph is an entry of the level its "Contents N" style names.
    let mut in_toc = false;
    let mut in_index_title = false;
    let mut comments: Vec<(crate::model::Comment, Option<String>)> = Vec::new();
    type Reading = (crate::model::Comment, Option<String>, Option<&'static str>, usize);
    let mut annotation: Option<Reading> = None;
    let mut frame: Option<FrameReading> = None;
    let mut frame_depth = 0usize;
    // The outermost table being read (#1296): its id, the current row and
    // column (-1 before the first), its element depth, and whether the
    // current cell has had a paragraph. A table nested in a cell belongs
    // to its outer cell.
    let mut table: Option<(u32, i64, i64, usize, bool)> = None;
    let mut table_depth = 0usize;
    let mut tables_read = 0u32;
    // Each outermost table's column widths; None once a column has none.
    let mut column_widths: std::collections::BTreeMap<u32, Option<Vec<f64>>> = Default::default();
    let mut row_heights: std::collections::BTreeMap<u32, Vec<Option<crate::model::RowHeight>>> = Default::default();
    let mut cell_fills: std::collections::BTreeMap<u32, Vec<crate::model::CellFill>> = Default::default();
    // A shaded cell's fill, at the cell just counted.
    let note_fill = |fills: &mut std::collections::BTreeMap<u32, Vec<crate::model::CellFill>>, t: &(u32, i64, i64, usize, bool), e: &quick_xml::events::BytesStart| {
        if let Some(color) = attr_val(e, "table:style-name").and_then(|n| auto.cell_fill.get(&n).cloned()) {
            fills.entry(t.0).or_default().push(crate::model::CellFill { row: t.1.max(0) as u32, col: t.2.max(0) as u32, color });
        }
    };
    let cell_of = |t: &Option<(u32, i64, i64, usize, bool)>| {
        t.map(|(id, r, c, _, _)| crate::model::TableCell { table: id, row: r.max(0) as u32, col: c.max(0) as u32 })
    };
    let mut consumed: Vec<String> = Vec::new();

    loop {
        let event = reader.read_event();
        if let Some((c, _, field, paras)) = annotation.as_mut() {
            let field_now = *field;
            let mut text = |t: &str| match field_now {
                Some("author") => c.author.push_str(t),
                Some("date") => c.date.push_str(t),
                Some(_) => {}
                None => c.text.push_str(t),
            };
            match &event {
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    "dc:creator" => *field = Some("author"),
                    "dc:date" => *field = Some("date"),
                    "meta:creator-initials" => *field = Some("initials"),
                    "text:p" | "text:h" => {
                        if *paras > 0 {
                            c.text.push('\n');
                        }
                        *paras += 1;
                    }
                    _ => {}
                },
                Ok(Event::End(e)) if e.name().as_ref() == "office:annotation" => {
                    let (mut c, parent, _, _) = annotation.take().expect("reading one");
                    c.date = crate::docx_comments::normalize_date(&c.date);
                    let spans = names.iter().any(|(n, id)| *id == c.id && ended.contains(n));
                    if let (None, true, Some(p)) = (&parent, spans, para.as_mut()) {
                        p.runs.push(Run::plain(crate::docx_comments::marker(c.id, true)));
                    }
                    comments.push((c, parent));
                }
                Ok(Event::End(e)) if matches!(e.name().as_ref(), "dc:creator" | "dc:date" | "meta:creator-initials") => *field = None,
                Ok(Event::Text(t)) => text(&unescape_text(t)),
                Ok(Event::GeneralRef(r)) => text(&resolve_general_ref(r)),
                Ok(Event::Empty(e)) if e.name().as_ref() == "text:s" => {
                    let n = attr_val(e, "text:c").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1);
                    text(&" ".repeat(n));
                }
                Ok(Event::Empty(e)) if e.name().as_ref() == "text:tab" => text("\t"),
                Err(e) => return Err(format!("XML parse error: {e}")),
                _ => {}
            }
            continue;
        }
        match event {
            Ok(Event::Start(e)) if e.name().as_ref() == "office:annotation" && para.is_some() && note.is_none() => {
                let name = attr_val(&e, "office:name");
                let id = match name.as_deref().and_then(own_id) {
                    Some(n) if !comments.iter().any(|(c, _)| c.id == n) => n,
                    _ => {
                        while reserved.contains(&next_free) || comments.iter().any(|(c, _)| c.id == next_free) {
                            next_free += 1;
                        }
                        next_free
                    }
                };
                if let Some(name) = name {
                    names.insert(name, id);
                }
                let comment = crate::model::Comment {
                    id,
                    author: String::new(),
                    date: String::new(),
                    text: String::new(),
                    resolved: attr_val(&e, "loext:resolved").as_deref() == Some("true"),
                    parent: None,
                };
                annotation = Some((comment, attr_val(&e, "loext:parent-name"), None, 0));
            }
            Ok(Event::Empty(e)) if e.name().as_ref() == "office:annotation-end" => {
                let root = attr_val(&e, "office:name")
                    .and_then(|n| names.get(&n).copied())
                    .filter(|id| comments.iter().any(|(c, p)| c.id == *id && p.is_none()));
                if let (Some(id), Some(p)) = (root, para.as_mut()) {
                    p.runs.push(Run::plain(crate::docx_comments::marker(id, false)));
                }
            }
            Ok(Event::Start(e)) if in_changes || e.name().as_ref() == "text:tracked-changes" => match e.name().as_ref() {
                "text:tracked-changes" => in_changes = true,
                "text:changed-region" => {
                    let id = attr_val(&e, "text:id").or_else(|| attr_val(&e, "xml:id")).unwrap_or_default();
                    region = Some((id, Vec::new(), Vec::new()));
                }
                "text:insertion" | "text:deletion" => {
                    if let Some(r) = region.as_mut() {
                        let kind = if e.name().as_ref() == "text:insertion" { crate::model::RevisionKind::Insert } else { crate::model::RevisionKind::Delete };
                        r.1.push((kind, String::new(), String::new()));
                    }
                }
                "dc:creator" => change_field = Some("author"),
                "dc:date" => change_field = Some("date"),
                "text:p" | "text:h" => para = Some(Paragraph::default()),
                "text:span" => {
                    let st = attr_val(&e, "text:style-name").and_then(|n| auto.text.get(&n).cloned()).unwrap_or_default();
                    span_stack.push(st);
                }
                _ => {}
            },
            Ok(Event::End(e)) if in_changes => match e.name().as_ref() {
                "text:tracked-changes" => in_changes = false,
                "text:changed-region" => {
                    // A region with a deletion is that deletion; an insertion
                    // in the same region is the one it deleted (LibreOffice
                    // writes a deletion of an insertion so).
                    if let Some((id, entries, runs)) = region.take() {
                        let rev = |e: &Entry| crate::model::Revision { kind: e.0, author: e.1.clone(), date: e.2.clone(), under: None };
                        let deletion = entries.iter().find(|e| e.0 == crate::model::RevisionKind::Delete);
                        let insertion = entries.iter().find(|e| e.0 == crate::model::RevisionKind::Insert);
                        let revision = match (deletion, insertion) {
                            (Some(d), under) => crate::model::Revision { under: under.map(|i| Box::new(rev(i))), ..rev(d) },
                            (None, Some(i)) => rev(i),
                            (None, None) => continue,
                        };
                        regions.insert(id, (revision, runs));
                    }
                }
                "dc:creator" | "dc:date" => change_field = None,
                "text:p" | "text:h" => {
                    if let (Some(p), Some(r)) = (para.take(), region.as_mut()) {
                        if !r.2.is_empty() && !p.runs.is_empty() {
                            r.2.push(Run::plain(" "));
                        }
                        r.2.extend(p.runs);
                    }
                }
                "text:span" => {
                    span_stack.pop();
                }
                _ => {}
            },
            Ok(Event::Text(t)) if in_changes => {
                let txt = unescape_text(&t);
                match (change_field, region.as_mut()) {
                    (Some("author"), Some(r)) => {
                        if let Some(e) = r.1.last_mut() {
                            e.1.push_str(&txt);
                        }
                    }
                    (Some("date"), Some(r)) => {
                        if let Some(e) = r.1.last_mut() {
                            e.2.push_str(&txt);
                        }
                    }
                    _ => push_text(&mut para, &span_stack, &link_stack, &[], &txt),
                }
            }
            Ok(Event::Empty(e)) if matches!(e.name().as_ref(), "text:change-start" | "text:change-end" | "text:change") && !in_changes => {
                let id = attr_val(&e, "text:change-id").unwrap_or_default();
                let Some((rev, runs)) = regions.get(&id).cloned() else { continue };
                match e.name().as_ref() {
                    "text:change-start" => rev_stack.push(rev),
                    "text:change-end" => {
                        rev_stack.pop();
                    }
                    _ => {
                        if let Some(p) = para.as_mut() {
                            for mut r in runs {
                                r.style.revision = Some(rev.clone());
                                match p.runs.last_mut() {
                                    Some(last) if last.style == r.style => last.text.push_str(&r.text),
                                    _ => p.runs.push(r),
                                }
                            }
                        }
                    }
                }
            }
            Ok(Event::Start(e)) => match e.name().as_ref() {
                "office:text" => in_body = true,
                "table:table" if in_body && note.is_none() => {
                    table_depth += 1;
                    if table.is_none() {
                        table = Some((tables_read, -1, -1, table_depth, false));
                        tables_read += 1;
                    }
                }
                "table:table-row" => {
                    if let Some(t) = table.as_mut().filter(|t| t.3 == table_depth) {
                        t.1 += 1;
                        t.2 = -1;
                        if let Some(h) = attr_val(&e, "table:style-name").and_then(|n| auto.row.get(&n).copied()) {
                            let heights = row_heights.entry(t.0).or_default();
                            heights.resize(t.1 as usize, None);
                            heights.push(Some(h));
                        }
                    }
                }
                "table:table-cell" | "table:covered-table-cell" => {
                    if let Some(t) = table.as_mut().filter(|t| t.3 == table_depth) {
                        t.2 += 1;
                        t.4 = false;
                        note_fill(&mut cell_fills, t, &e);
                    }
                }
                "draw:frame" => {
                    frame_depth += 1;
                    if frame.is_none() && para.is_some() && note.is_none() {
                        let size = |a: &str| attr_val(&e, a).as_deref().and_then(length_emu);
                        let floating = attr_val(&e, "text:anchor-type").is_some_and(|t| matches!(t.as_str(), "paragraph" | "char" | "page"));
                        let anchor = floating.then(|| {
                            let offset = |a: &str| attr_val(&e, a).as_deref().and_then(offset_emu).unwrap_or(0);
                            crate::model::ImageAnchor {
                                x_emu: offset("svg:x"),
                                y_emu: offset("svg:y"),
                                ..attr_val(&e, "draw:style-name").and_then(|n| auto.graphic.get(&n).copied()).unwrap_or_default()
                            }
                        });
                        frame = Some(FrameReading {
                            anchor,
                            extent: size("svg:width").zip(size("svg:height")),
                            href: None,
                            alt: String::new(),
                            in_alt: false,
                            depth: frame_depth,
                        });
                    }
                }
                "svg:title" | "svg:desc" => {
                    if let Some(f) = frame.as_mut().filter(|f| f.alt.is_empty()) {
                        f.in_alt = true;
                    }
                }
                "draw:image" => {
                    if let Some(f) = frame.as_mut().filter(|f| f.href.is_none()) {
                        f.href = attr_val(&e, "xlink:href");
                    }
                }
                "text:note" if para.is_some() => {
                    // The reference run is empty on purpose: it marks the
                    // position, and the text lives in `Document::footnotes`.
                    // Notes are read in document order, so the next free
                    // index is this note's.
                    if let Some(p) = para.as_mut() {
                        p.runs.push(Run {
                            text: String::new(),
                            style: RunStyle { footnote: Some(doc.footnotes.len()), ..Default::default() },
                        });
                    }
                    note = Some(String::new());
                    note_paras = 0;
                }
                "text:note-citation" => in_citation = true,
                "text:p" if note.is_some() => {
                    if note_paras > 0 {
                        if let Some(n) = note.as_mut() { n.push('\n'); }
                    }
                    note_paras += 1;
                }
                "text:p" | "text:h" if in_body => {
                    let mut style = ParaStyle::default();
                    if e.name().as_ref() == "text:h" {
                        let lvl = attr_val(&e, "text:outline-level")
                            .and_then(|v| v.parse::<u8>().ok())
                            .unwrap_or(1);
                        style.heading = Some(lvl.clamp(1, 6));
                    }
                    if let Some(name) = attr_val(&e, "text:style-name") {
                        if let Some(auto_para) = auto.para.get(&name) {
                            style.alignment = auto_para.alignment;
                            style.page_break_before = auto_para.page_break_before;
                            style.keep_with_next = auto_para.keep_with_next;
                            style.line_spacing = auto_para.line_spacing;
                            style.space_before_pt = auto_para.space_before_pt;
                            style.space_after_pt = auto_para.space_after_pt;
                            style.left_indent_pt = auto_para.left_indent_pt;
                            style.right_indent_pt = auto_para.right_indent_pt;
                            style.first_line_indent_pt = auto_para.first_line_indent_pt;
                            style.tab_stops_pt = auto_para.tab_stops_pt.clone();
                        }
                        // What the paragraph's own style leaves unset, its
                        // parent named style decides: Writer's "Standard" is
                        // where most documents keep their space after and
                        // line height.
                        let (before, after, line) = inherited_spacing(&name, &auto, &named);
                        if let Some(v) = before {
                            style.space_before_pt = v;
                        }
                        if let Some(v) = after {
                            style.space_after_pt = v;
                        }
                        if let Some(v) = line {
                            style.line_spacing = v;
                        }
                        // Direct built-in name, or an automatic style
                        // inheriting from one (LO's rewrite pattern).
                        let base = auto
                            .para_parent
                            .get(&name)
                            .cloned()
                            .unwrap_or_else(|| name.clone());
                        // ODF display names use _20_ for spaces.
                        let base = base.replace("_20_", " ");
                        match base.as_str() {
                            "Title" | "Subtitle" => style.named_style = Some(base),
                            // Writer's own quote style, and the name a docx
                            // "Quote" arrives under (#1205).
                            "Quotations" | "Quote" => style.block_quote = true,
                            "Preformatted Text" => style.code_block = Some(String::new()),
                            _ => {}
                        }
                    }
                    style.list = list_kind;
                    style.list_level = list_level.saturating_sub(1);
                    style.list_start = pending_start.take();
                    if in_toc && !in_index_title {
                        let name = attr_val(&e, "text:style-name").unwrap_or_default();
                        let base = auto.para_parent.get(&name).cloned().unwrap_or(name);
                        let level = base.rsplit(|c: char| !c.is_ascii_digit()).next().and_then(|d| d.parse::<u8>().ok()).filter(|l| (1..=10).contains(l)).unwrap_or(1);
                        style.toc = Some(level);
                        style.left_indent_pt = crate::toc::INDENT_PT * f64::from(level - 1);
                        style.tab_stops_pt.clear();
                    }
                    para = Some(Paragraph { style, runs: Vec::new() });
                    // The paragraph style's own text properties are how
                    // its text looks where no span says otherwise: a title
                    // written straight into a bold, 12pt paragraph.
                    let text_base = attr_val(&e, "text:style-name").and_then(|n| auto.para_text.get(&n).cloned()).unwrap_or_default();
                    span_stack.push(text_base);
                    paragraph_bases += 1;
                }
                "text:span" => {
                    let name = attr_val(&e, "text:style-name");
                    let own = name.as_ref().and_then(|n| auto.text.get(n).cloned()).unwrap_or_default();
                    let mut st = inherit(span_stack.last(), own);
                    // Inline code is Writer's "Source Text", named or as
                    // the parent of an automatic style (#1205).
                    if let Some(n) = &name {
                        if n == "Source_20_Text" || auto.text_parent.get(n).is_some_and(|p| p == "Source_20_Text") {
                            st.code = true;
                            st.font_family = None;
                        }
                    }
                    span_stack.push(st);
                }
                "text:a" => {
                    link_stack.push(attr_val(&e, "xlink:href").unwrap_or_default());
                }
                // A Word content control as LibreOffice writes it in ODF
                // (a .docx chip saved as .odt): ours by its tag, or any
                // date control by its date.
                "loext:content-control" if para.is_some() => {
                    let tagged = attr_val(&e, "loext:tag").as_deref().and_then(chip_from_name);
                    let dated = || {
                        let v = attr_val(&e, "loext:current-date")?.get(..10)?.to_string();
                        crate::chips::NaiveDate::parse_from_str(&v, "%Y-%m-%d").ok()?;
                        (attr_val(&e, "loext:date").as_deref() == Some("true"))
                            .then_some(crate::chips::Chip { kind: crate::chips::ChipKind::Date, value: v })
                    };
                    if let Some(c) = tagged.or_else(dated) {
                        chip = Some((c, String::new(), "loext:content-control"));
                    }
                }
                "text:date" if para.is_some() => {
                    let value = attr_val(&e, "text:date-value").and_then(|v| v.get(..10).map(str::to_string));
                    if let Some(v) = value.filter(|v| crate::chips::NaiveDate::parse_from_str(v, "%Y-%m-%d").is_ok()) {
                        chip = Some((crate::chips::Chip { kind: crate::chips::ChipKind::Date, value: v }, String::new(), "text:date"));
                    }
                }
                "text:table-of-content" => in_toc = true,
                "text:index-title" if in_toc => in_index_title = true,
                "text:list" if in_body => {
                    // Bullet vs numbered comes from the list style name we
                    // write; LO-authored lists fall back to bullet. A nested
                    // list naming no style is its parent's kind (#1205: it
                    // read as a bullet, so a numbered sub-list changed kind).
                    let name = attr_val(&e, "text:style-name").or_else(|| list_style_names.last().cloned().flatten());
                    let level = usize::from(list_level);
                    list_kind = match name.as_ref().and_then(|n| list_styles.get(n)) {
                        // The style's own definition of this level.
                        Some(kinds) => kinds.get(level).or(kinds.last()).copied().unwrap_or(ListKind::Bullet),
                        // Not defined here: our own names, else the parent's kind.
                        None => match &name {
                            Some(n) if n.contains('N') && !n.contains("LB") => ListKind::Numbered,
                            Some(_) => ListKind::Bullet,
                            None => list_kinds.last().copied().unwrap_or(ListKind::Bullet),
                        },
                    };
                    if list_level == 0 {
                        let continues = attr_val(&e, "text:continue-numbering").as_deref() == Some("true") || attr_val(&e, "text:continue-list").is_some();
                        top_lists.push((doc.paragraphs.len(), usize::MAX, name.clone(), continues));
                    }
                    list_style_names.push(name);
                    list_kinds.push(list_kind);
                    list_level = list_level.saturating_add(1);
                }
                "text:list-item" if in_body => {
                    pending_start = attr_val(&e, "text:start-value").and_then(|v| v.parse().ok());
                }
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.name().as_ref() {
                // A column of the outermost table: its width, repeated.
                "table:table-column" => {
                    if let Some(t) = table.as_ref().filter(|t| t.3 == table_depth) {
                        let width = attr_val(&e, "table:style-name").and_then(|n| auto.column.get(&n).copied());
                        let repeat = attr_val(&e, "table:number-columns-repeated").and_then(|n| n.parse::<usize>().ok()).unwrap_or(1).min(1024);
                        let cols = column_widths.entry(t.0).or_insert_with(|| Some(Vec::new()));
                        match (cols.as_mut(), width) {
                            (Some(c), Some(w)) => c.extend(std::iter::repeat_n(w, repeat)),
                            // One column without a width: the table has none.
                            _ => *cols = None,
                        }
                    }
                }
                // A cell with no paragraph still holds its grid position.
                "table:table-cell" | "table:covered-table-cell" => {
                    if let Some(t) = table.as_mut().filter(|t| t.3 == table_depth) {
                        t.2 += 1;
                        note_fill(&mut cell_fills, t, &e);
                        doc.paragraphs.push(Paragraph { style: ParaStyle { table_cell: cell_of(&table), ..Default::default() }, runs: Vec::new() });
                    }
                }
                "draw:image" => {
                    if let Some(f) = frame.as_mut().filter(|f| f.href.is_none()) {
                        f.href = attr_val(&e, "xlink:href");
                    }
                }
                "text:s" if chip.is_some() => {
                    let n = attr_val(&e, "text:c").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1);
                    if let Some(c) = chip.as_mut() { c.1.push_str(&" ".repeat(n)); }
                }
                "text:s" if para.is_some() => {
                    let n = attr_val(&e, "text:c")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(1);
                    push_text(&mut para, &span_stack, &link_stack, &rev_stack, &" ".repeat(n));
                }
                "text:tab" if para.is_some() => {
                    push_text(&mut para, &span_stack, &link_stack, &rev_stack, "\t");
                }
                "text:p" if note.is_some() => {}
                "text:p" | "text:h" if in_body => {
                    let style = ParaStyle { list: list_kind, list_level: list_level.saturating_sub(1), list_start: pending_start.take(), table_cell: cell_of(&table), ..Default::default() };
                    if let Some(t) = table.as_mut() {
                        t.4 = true;
                    }
                    doc.paragraphs.push(Paragraph { style, runs: Vec::new() });
                }
                _ => {}
            },
            Ok(Event::Text(t)) if frame.as_ref().is_some_and(|f| f.in_alt) => {
                if let Some(f) = frame.as_mut() {
                    f.alt.push_str(&unescape_text(&t));
                }
            }
            Ok(Event::Text(t)) => {
                if let Some(n) = note.as_mut() {
                    if !in_citation {
                        n.push_str(&unescape_text(&t));
                    }
                } else if let Some(c) = chip.as_mut() {
                    c.1.push_str(&unescape_text(&t));
                } else if para.is_some() {
                    let txt = unescape_text(&t);
                    push_text(&mut para, &span_stack, &link_stack, &rev_stack, &txt);
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if let Some(c) = chip.as_mut() {
                    c.1.push_str(&resolve_general_ref(&r));
                } else if para.is_some() {
                    let txt = resolve_general_ref(&r);
                    push_text(&mut para, &span_stack, &link_stack, &rev_stack, &txt);
                }
            }
            Ok(Event::End(e)) if chip.as_ref().is_some_and(|c| c.2 == e.name().as_ref()) => {
                if let (Some((c, label, _)), Some(p)) = (chip.take(), para.as_mut()) {
                    p.runs.push(crate::chips::chip_run(c, label));
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                "svg:title" | "svg:desc" => {
                    if let Some(f) = frame.as_mut() {
                        f.in_alt = false;
                    }
                }
                "draw:frame" => {
                    if frame.as_ref().is_some_and(|f| f.depth == frame_depth) {
                        let f = frame.take().expect("the frame just checked");
                        // A picture whose bytes are in the package: kept in
                        // the media cache, like a docx's (#455), so the
                        // model's path stays readable.
                        let href = f.href.filter(|h| !h.contains("://"));
                        let path = href.as_deref().and_then(|h| {
                            let bytes = zip.part_to_bytes(h.trim_start_matches("./"), &mut budget).ok()?;
                            suite_common_core::media_cache::persist(&bytes).ok()
                        });
                        if let (Some(path), Some(p)) = (path, para.as_mut()) {
                            consumed.push(href.expect("read from it").trim_start_matches("./").to_string());
                            p.runs.push(Run {
                                text: f.alt.trim().to_string(),
                                style: RunStyle {
                                    image: Some(path.to_string_lossy().into_owned()),
                                    image_extent_emu: f.extent,
                                    image_anchor: f.anchor,
                                    ..Default::default()
                                },
                            });
                        }
                    }
                    frame_depth = frame_depth.saturating_sub(1);
                }
                "text:note-citation" => in_citation = false,
                "text:note" => {
                    if let Some(text) = note.take() {
                        doc.footnotes.push(text);
                    }
                    note_paras = 0;
                }
                // A note's own paragraphs must not close the paragraph that
                // carries the reference.
                "text:p" if note.is_some() => {}
                "text:p" | "text:h" => {
                    if paragraph_bases > 0 {
                        paragraph_bases -= 1;
                        span_stack.pop();
                    }
                    if let Some(mut p) = para.take() {
                        // An entry's link goes to its heading, not a page.
                        if p.style.toc.is_some() {
                            for r in &mut p.runs {
                                if r.style.link.as_deref().is_some_and(|l| l.starts_with('#')) {
                                    r.style.link = None;
                                }
                            }
                            let mut runs: Vec<Run> = Vec::new();
                            for r in p.runs.drain(..) {
                                match runs.last_mut() {
                                    Some(l) if l.style == r.style && r.style.chip.is_none() && l.style.chip.is_none() => l.text.push_str(&r.text),
                                    _ => runs.push(r),
                                }
                            }
                            p.runs = runs;
                        }
                        if let Some(t) = table.as_mut() {
                            p.style.table_cell = cell_of(&Some(*t));
                            t.4 = true;
                        }
                        doc.paragraphs.push(p);
                    }
                }
                "table:table-cell" => {
                    if let Some(t) = table.filter(|t| t.3 == table_depth && !t.4) {
                        doc.paragraphs.push(Paragraph { style: ParaStyle { table_cell: cell_of(&Some(t)), ..Default::default() }, runs: Vec::new() });
                    }
                }
                "table:table" if table_depth > 0 => {
                    if table.is_some_and(|t| t.3 == table_depth) {
                        table = None;
                    }
                    table_depth -= 1;
                }
                "text:table-of-content" => in_toc = false,
                "text:index-title" => in_index_title = false,
                "text:span" => {
                    span_stack.pop();
                }
                "text:a" => {
                    link_stack.pop();
                }
                "text:list" => {
                    list_level = list_level.saturating_sub(1);
                    if list_level == 0 {
                        if let Some(top) = top_lists.last_mut() {
                            top.1 = doc.paragraphs.len();
                        }
                    }
                    list_kinds.pop();
                    list_style_names.pop();
                    list_kind = if list_level == 0 { ListKind::None } else { list_kinds.last().copied().unwrap_or(ListKind::Bullet) };
                },
                "office:text" => in_body = false,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error: {e}")),
            _ => {}
        }
    }
    if !comments.is_empty() {
        // A reply names its parent; a reply to a reply is in the thread.
        let parents: Vec<Option<u32>> = comments.iter().map(|(_, p)| p.as_ref().and_then(|n| names.get(n).copied())).collect();
        let root = |mut id: u32| {
            for _ in 0..parents.len() {
                match comments.iter().position(|(c, _)| c.id == id).and_then(|i| parents[i]) {
                    Some(p) if p != id => id = p,
                    _ => break,
                }
            }
            id
        };
        let bodies: Vec<crate::model::Comment> = comments
            .iter()
            .zip(&parents)
            .map(|((c, _), p)| crate::model::Comment { parent: p.map(root).filter(|r| *r != c.id), ..c.clone() })
            .collect();
        let mut bodies = bodies;
        bodies.sort_by_key(|c| c.id);
        crate::docx_comments::restore(&mut doc, bodies);
    }

    // Header/footer and page geometry from styles.xml: the first page's
    // master page only (a file may hold several, each with its own
    // header, and joining them repeated the header's text).
    if !styles.is_empty() {
        let master = first_master_page(&content, &styles);
        let mut reader = Reader::from_str(&styles);
        let mut in_header = false;
        let mut in_footer = false;
        // The master page being read, and whether it is the first page's.
        let mut in_master = false;
        let mut layouts: std::collections::HashMap<String, PageGeometry> = Default::default();
        let mut layout_name = String::new();
        let mut master_layout: Option<String> = None;
        let mut last_layout: Option<String> = None;
        // Inside an alternative text (`svg:title`/`svg:desc`) of a frame,
        // which is not the header's text.
        let mut in_alt = false;
        // Inside a page number or count field: its shown value is skipped.
        let mut in_field = false;
        // Returns the geometry instead of writing to `doc` so the borrow ends
        // with the call; `style:columns` needs `doc.page` mutably right after.
        let read_page_layout = |e: &quick_xml::events::BytesStart| -> Option<PageGeometry> {
            let width_pt = attr_val(e, "fo:page-width").and_then(|v| parse_length_pt(&v))?;
            let height_pt = attr_val(e, "fo:page-height").and_then(|v| parse_length_pt(&v))?;
            let d = PageGeometry::default();
            let m = |name: &str, fallback: f64| {
                attr_val(e, name).and_then(|v| parse_length_pt(&v)).unwrap_or(fallback)
            };
            Some(PageGeometry {
                width_pt,
                height_pt,
                margin_top_pt: m("fo:margin-top", d.margin_top_pt),
                margin_bottom_pt: m("fo:margin-bottom", d.margin_bottom_pt),
                margin_left_pt: m("fo:margin-left", d.margin_left_pt),
                margin_right_pt: m("fo:margin-right", d.margin_right_pt),
                columns: d.columns,
                column_gap_pt: d.column_gap_pt,
            })
        };
        loop {
            match reader.read_event() {
                Ok(Event::Empty(e)) if e.name().as_ref() == "style:page-layout-properties" => {
                    if let Some(page) = read_page_layout(&e) {
                        last_layout = Some(layout_name.clone());
                        layouts.insert(layout_name.clone(), page);
                    }
                }
                Ok(Event::Empty(e)) if e.name().as_ref() == "style:columns" => {
                    if let Some(page) = layouts.get_mut(&layout_name) {
                        page.columns = attr_val(&e, "fo:column-count")
                            .and_then(|v| v.parse::<u8>().ok()).unwrap_or(1).max(1);
                        page.column_gap_pt = attr_val(&e, "fo:column-gap")
                            .and_then(|v| parse_length_pt(&v)).unwrap_or(page.column_gap_pt);
                    }
                }
                Ok(Event::Start(e)) if e.name().as_ref() == "style:master-page" => {
                    in_master = attr_val(&e, "style:name") == master;
                    if in_master {
                        master_layout = attr_val(&e, "style:page-layout-name");
                    }
                }
                Ok(Event::Empty(e)) if e.name().as_ref() == "style:master-page" => {
                    if attr_val(&e, "style:name") == master {
                        master_layout = attr_val(&e, "style:page-layout-name");
                    }
                }
                Ok(Event::End(e)) if e.name().as_ref() == "style:master-page" => in_master = false,
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    "style:page-layout" => layout_name = attr_val(&e, "style:name").unwrap_or_default(),
                    "style:header" => in_header = in_master,
                    "style:footer" => in_footer = in_master,
                    "svg:title" | "svg:desc" => in_alt = true,
                    "text:page-number" | "text:page-count" if in_header || in_footer => {
                        let placeholder = if e.name().as_ref() == "text:page-number" { "{page}" } else { "{total}" };
                        let target = if in_header { &mut doc.header } else { &mut doc.footer };
                        target.get_or_insert_with(String::new).push_str(placeholder);
                        in_field = true;
                    }
                    "style:page-layout-properties" => {
                        if let Some(page) = read_page_layout(&e) {
                            last_layout = Some(layout_name.clone());
                            layouts.insert(layout_name.clone(), page);
                        }
                    }
                    _ => {}
                },
                Ok(Event::End(e)) => match e.name().as_ref() {
                    "style:header" => in_header = false,
                    "style:footer" => in_footer = false,
                    "svg:title" | "svg:desc" => in_alt = false,
                    "text:page-number" | "text:page-count" => in_field = false,
                    _ => {}
                },
                Ok(Event::Text(_)) if in_field || in_alt => {}
                Ok(Event::Text(t)) => {
                    let txt = unescape_text(&t);
                    if in_header && !txt.trim().is_empty() {
                        doc.header.get_or_insert_with(String::new).push_str(&txt);
                    }
                    if in_footer && !txt.trim().is_empty() {
                        doc.footer.get_or_insert_with(String::new).push_str(&txt);
                    }
                }
                Ok(Event::GeneralRef(_)) if in_field || in_alt => {}
                Ok(Event::GeneralRef(r)) => {
                    let txt = resolve_general_ref(&r);
                    if in_header {
                        doc.header.get_or_insert_with(String::new).push_str(&txt);
                    }
                    if in_footer {
                        doc.footer.get_or_insert_with(String::new).push_str(&txt);
                    }
                }
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
        }
        // The first page's layout; without a master page naming one that
        // exists, the last one read, as before.
        doc.page = master_layout.filter(|n| layouts.contains_key(n)).or(last_layout).and_then(|n| layouts.remove(&n));
    }

    continue_numbering(&mut doc.paragraphs, &top_lists);
    doc.ensure_non_empty();
    doc.table_columns = column_widths.into_iter().filter_map(|(t, w)| Some((t, w.filter(|w| !w.is_empty())?))).collect();
    doc.table_rows = row_heights;
    doc.table_fills = cell_fills;
    // A column count LibreOffice recorded on a section instead of on the
    // page layout. Only consulted when the page layout said nothing, so
    // an explicit page-wide count still wins.
    if let Some((count, gap)) = parse_section_columns(&content) {
        let page = doc.page.get_or_insert_with(PageGeometry::default);
        if page.columns <= 1 {
            page.columns = count;
            if let Some(gap) = gap {
                page.column_gap_pt = gap;
            }
        }
    }

    Ok((doc, consumed))
}

/// A span's text style over the text style around it: what the span
/// does not set, it takes from its paragraph (or outer span).
fn inherit(outer: Option<&RunStyle>, own: RunStyle) -> RunStyle {
    let Some(outer) = outer else { return own };
    RunStyle {
        bold: own.bold || outer.bold,
        italic: own.italic || outer.italic,
        underline: own.underline || outer.underline,
        strikethrough: own.strikethrough || outer.strikethrough,
        highlight: own.highlight || outer.highlight,
        font_family: own.font_family.or_else(|| outer.font_family.clone()),
        font_size_hp: own.font_size_hp.or(outer.font_size_hp),
        color: own.color.or_else(|| outer.color.clone()),
        vert_align: own.vert_align.or(outer.vert_align),
        ..own
    }
}

fn push_text(
    para: &mut Option<Paragraph>,
    span_stack: &[RunStyle],
    link_stack: &[String],
    rev_stack: &[crate::model::Revision],
    text: &str,
) {
    if text.is_empty() {
        return;
    }
    if let Some(p) = para.as_mut() {
        let mut style = span_stack.last().cloned().unwrap_or_default();
        style.revision = rev_stack.last().cloned();
        if let Some(href) = link_stack.last() {
            if !href.is_empty() {
                style.link = Some(href.clone());
            }
        }
        // Merge with the previous run when styles match (normalizes the
        // reader output so round-trip comparisons are stable).
        if let Some(last) = p.runs.last_mut() {
            if last.style == style {
                last.text.push_str(text);
                return;
            }
        }
        p.runs.push(Run { text: text.to_string(), style });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3x2 red PNG.
    const PNG: [u8; 78] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0x12, 0x16, 0xf1, 0x4d, 0x00, 0x00, 0x00, 0x15, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x3c, 0x21, 0x27, 0xc7, 0xc0, 0xc0, 0xc0, 0xc0, 0xc0, 0xc0, 0xc4, 0x00, 0x03, 0x00, 0x13, 0x2e, 0x01, 0x08, 0x6a, 0xc0, 0x65, 0x61, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82];

    /// Pictures are written as frames with their bytes in Pictures/ and
    /// read back in place, with their size and alt text (#1292). They used
    /// to be dropped on every save.
    #[test]
    fn pictures_round_trip_in_place_with_size_and_alt_text() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("red.png");
        std::fs::write(&src, PNG).unwrap();
        let image = |extent| Run {
            text: "A red square".into(),
            style: RunStyle { image: Some(src.to_string_lossy().into_owned()), image_extent_emu: extent, ..Default::default() },
        };
        let mut d = Document::from_plain_text("");
        d.paragraphs = vec![
            Paragraph { style: ParaStyle::default(), runs: vec![Run::plain("before "), image(Some((914_400, 457_200))), Run::plain(" after")] },
            // No size: the PNG's own 3x2 pixels at 96 dpi.
            Paragraph { style: ParaStyle::default(), runs: vec![image(None)] },
            // The same picture again is stored once.
            Paragraph { style: ParaStyle::default(), runs: vec![image(Some((914_400, 457_200)))] },
        ];
        let out = dir.path().join("pics.odt");
        write(&d, &out).unwrap();

        let mut zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        let members: Vec<String> = zip.file_names().filter(|n| n.starts_with("Pictures/")).map(str::to_string).collect();
        assert_eq!(members.len(), 1, "one picture used three times is stored once: {members:?}");
        let mut manifest = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("META-INF/manifest.xml").unwrap(), &mut manifest).unwrap();
        assert!(manifest.contains(&format!("manifest:full-path=\"{}\" manifest:media-type=\"image/png\"", members[0])), "{manifest}");

        let (rt, _, opaque) = read_with_report(out.to_str().unwrap()).unwrap();
        let pics: Vec<&Run> = rt.paragraphs.iter().flat_map(|p| &p.runs).filter(|r| r.style.image.is_some()).collect();
        assert_eq!(pics.len(), 3);
        for r in &pics {
            assert_eq!(std::fs::read(r.style.image.as_ref().unwrap()).unwrap(), PNG, "the picture's bytes");
            assert_eq!(r.text, "A red square", "alt text");
        }
        assert_eq!(pics[0].style.image_extent_emu, Some((914_400, 457_200)));
        assert_eq!(pics[1].style.image_extent_emu, Some((3 * 9525, 2 * 9525)));
        let first: Vec<&str> = rt.paragraphs[0].runs.iter().map(|r| if r.style.image.is_some() { "[pic]" } else { r.text.as_str() }).collect();
        assert_eq!(first, ["before ", "[pic]", " after"], "the picture stays where it is in the line");
        assert!(opaque.is_empty(), "a picture we read is not an opaque part: {:?}", opaque.part_names().collect::<Vec<_>>());
    }

    /// A floating picture is a frame anchored to its paragraph, placed by
    /// a graphic style of its own, and reads back placed the same.
    #[test]
    fn a_floating_picture_round_trips_its_placement() {
        use crate::model::{AnchorAlign, AnchorFrame, ImageAnchor};
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("red.png");
        std::fs::write(&src, PNG).unwrap();
        let placements = [
            ImageAnchor { h_from: AnchorFrame::Margin, h_align: Some(AnchorAlign::End), v_from: AnchorFrame::Text, y_emu: -660_400, ..Default::default() },
            ImageAnchor { h_from: AnchorFrame::Page, x_emu: 914_400, v_from: AnchorFrame::Page, v_align: Some(AnchorAlign::Center), behind: true, ..Default::default() },
        ];
        let mut d = Document::from_plain_text("");
        d.paragraphs = placements
            .iter()
            .map(|a| Paragraph {
                style: ParaStyle::default(),
                runs: vec![Run::plain("text "), Run {
                    text: "logo".into(),
                    style: RunStyle { image: Some(src.to_string_lossy().into_owned()), image_extent_emu: Some((914_400, 457_200)), image_anchor: Some(*a), ..Default::default() },
                }],
            })
            .collect();
        let out = dir.path().join("float.odt");
        write(&d, &out).unwrap();
        let rt = read(out.to_str().unwrap()).unwrap();
        let read: Vec<Option<ImageAnchor>> = rt.paragraphs.iter().flat_map(|p| &p.runs).filter(|r| r.style.image.is_some()).map(|r| r.style.image_anchor).collect();
        assert_eq!(read, placements.map(Some));
        assert_eq!(offset_emu("-1.5cm"), Some(-540_000));
        assert_eq!(offset_emu("0in"), Some(0));
    }

    /// Tables cross as tables (#1296): every cell in its grid position, an
    /// empty cell included, a cell of two paragraphs, run styling inside a
    /// cell, the text around the table, and a second table straight after.
    #[test]
    fn tables_round_trip_cell_by_cell() {
        let cell = |table, row, col, text: &str| Paragraph {
            style: ParaStyle { table_cell: Some(crate::model::TableCell { table, row, col }), ..Default::default() },
            runs: if text.is_empty() { Vec::new() } else { vec![Run::plain(text)] },
        };
        let mut bold = cell(0, 1, 2, "");
        bold.runs = vec![Run { text: "bold total".into(), style: RunStyle { bold: true, ..Default::default() } }];
        let mut d = Document::from_plain_text("before");
        d.paragraphs.extend([
            cell(0, 0, 0, "a1"), cell(0, 0, 1, "b1"), cell(0, 0, 2, "c1"),
            cell(0, 1, 0, "a2 first"), cell(0, 1, 0, "a2 second"), cell(0, 1, 1, ""), bold,
            cell(1, 0, 0, "second table"), cell(1, 0, 1, "x"),
        ]);
        d.paragraphs.push(Paragraph { style: ParaStyle::default(), runs: vec![Run::plain("after")] });
        let rt = round_trip(&d);
        type Shape = Vec<(Option<(u32, u32, u32)>, String, bool)>;
        let shape = |doc: &Document| -> Shape {
            doc.paragraphs.iter().map(|p| (p.style.table_cell.map(|c| (c.table, c.row, c.col)), p.text(), p.runs.iter().any(|r| r.style.bold))).collect()
        };
        assert_eq!(shape(&rt), shape(&d));
    }

    /// How headings look crosses an odt save (#1297): it used to be lost,
    /// so every heading reopened in Letters' default look.
    #[test]
    fn heading_looks_round_trip() {
        let mut d = Document::from_plain_text("Title\nbody");
        d.paragraphs[0].style.heading = Some(1);
        d.heading_styles = (1..=6)
            .map(|n| RunStyle { bold: n == 1, italic: n == 2, color: Some("c00000".into()), font_size_hp: Some(48 - 4 * n), font_family: Some("DejaVu Serif".into()), ..Default::default() })
            .collect();
        let rt = round_trip(&d);
        assert_eq!(rt.heading_styles, d.heading_styles);
        assert_eq!(rt.paragraphs[0].style.heading, Some(1));
    }

    /// An image whose file has gone is written as its alt text, as docx does.
    #[test]
    fn an_unreadable_picture_is_written_as_its_alt_text() {
        let dir = tempfile::tempdir().unwrap();
        let mut d = Document::from_plain_text("");
        d.paragraphs = vec![Paragraph {
            style: ParaStyle::default(),
            runs: vec![Run { text: "missing chart".into(), style: RunStyle { image: Some("/nonexistent/x.png".into()), ..Default::default() } }],
        }];
        let out = dir.path().join("gone.odt");
        write(&d, &out).unwrap();
        let rt = read(out.to_str().unwrap()).unwrap();
        assert_eq!(rt.paragraphs[0].text(), "missing chart");
        assert!(rt.paragraphs[0].runs.iter().all(|r| r.style.image.is_none()));
    }

    fn round_trip(doc: &Document) -> Document {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.odt");
        write(doc, path.to_str().unwrap()).expect("write odt");
        read(path.to_str().unwrap()).expect("read odt")
    }

    /// A table's column widths survive a save and reopen (and an ODT's
    /// own `table:table-column` widths, repeated columns included, are
    /// read), so a narrow number column stays narrow.
    #[test]
    fn table_column_widths_survive() {
        let mut d = Document::from_plain_text("after");
        let table = d.insert_table_at(0, 1, 3);
        d.table_columns.insert(table, vec![36.0, 200.0, 150.0]);
        let rt = round_trip(&d);
        let widths: Vec<Vec<f64>> = rt.table_columns.values().cloned().collect();
        assert_eq!(widths.len(), 1, "{:?}", rt.table_columns);
        assert!(widths[0].iter().zip([36.0, 200.0, 150.0]).all(|(a, b)| (a - b).abs() < 0.01), "{widths:?}");
        // Without widths, none are made up.
        let mut d = Document::from_plain_text("after");
        d.insert_table_at(0, 1, 2);
        assert!(round_trip(&d).table_columns.is_empty());
    }

    /// A table's row heights survive a save and reopen, a minimum as a
    /// minimum and a fixed height as fixed; rows without one stay without.
    #[test]
    fn table_row_heights_survive() {
        use crate::model::RowHeight;
        let mut d = Document::from_plain_text("after");
        let table = d.insert_table_at(0, 3, 2);
        let want = vec![Some(RowHeight { pt: 40.0, exact: false }), None, Some(RowHeight { pt: 18.5, exact: true })];
        d.table_rows.insert(table, want.clone());
        let rt = round_trip(&d);
        let got: Vec<_> = rt.table_rows.values().cloned().collect();
        assert_eq!(got, [want], "{:?}", rt.table_rows);
        let mut d = Document::from_plain_text("after");
        d.insert_table_at(0, 2, 2);
        assert!(round_trip(&d).table_rows.is_empty());
    }

    /// A table's shaded cells survive a save and reopen, one table-cell
    /// style per colour.
    #[test]
    fn table_cell_fills_survive() {
        use crate::model::CellFill;
        let mut d = Document::from_plain_text("after");
        let table = d.insert_table_at(0, 2, 2);
        let want = vec![CellFill { row: 0, col: 0, color: "D9D9D9".into() }, CellFill { row: 0, col: 1, color: "D9D9D9".into() }, CellFill { row: 1, col: 1, color: "DEEAF6".into() }];
        d.table_fills.insert(table, want.clone());
        let rt = round_trip(&d);
        assert_eq!(rt.table_fills.values().cloned().collect::<Vec<_>>(), [want]);
        let mut d = Document::from_plain_text("after");
        d.insert_table_at(0, 1, 1);
        assert!(round_trip(&d).table_fills.is_empty());
    }

    /// Smart chips reopen as chips: a date is a fixed `text:date`, a link
    /// or person chip a named `text:a` (a name-only person a bookmark).
    #[test]
    fn smart_chips_survive() {
        let d = crate::chips::sample_document();
        let rt = round_trip(&d);
        assert_eq!(rt.paragraphs[0].runs, d.paragraphs[0].runs);
    }

    /// Tracked changes reopen as they were, deleted text included (ODF keeps
    /// it in the change region, not the body), and a deletion of someone
    /// else's insertion too.
    #[test]
    fn tracked_changes_survive() {
        let d = crate::track::sample_document();
        let rt = round_trip(&d);
        assert_eq!(rt.paragraphs[0].runs, d.paragraphs[0].runs);
    }

    /// A table of contents is LibreOffice's index, and reopens as the same
    /// entries; a tab is written as an ODF tab, which a raw tab is not.
    #[test]
    fn a_table_of_contents_and_tabs_survive() {
        let mut d = crate::toc::sample_document();
        d.paragraphs[5].runs = vec![Run::plain("Why\tit matters.")];
        let rt = round_trip(&d);
        let entries = |d: &Document| crate::toc::blocks(d).into_iter().flat_map(|b| d.paragraphs[b].to_vec()).collect::<Vec<_>>();
        assert_eq!(entries(&rt), entries(&d));
        assert_eq!(rt.paragraphs[5].text(), "Why\tit matters.");
        assert_eq!(rt.paragraphs.len(), d.paragraphs.len());
    }

    /// Comments reopen as they were: overlapping annotations, a reply in
    /// its thread, a resolved thread, one of two lines, and one whose text
    /// was deleted (an annotation without an end).
    #[test]
    fn comments_survive() {
        let mut d = crate::comments::sample_document();
        let (ops, _) = crate::comments::add(&d, 0, 3, "Ada Lovelace", "2026-09-26T11:00:00Z", "Two lines\nof comment").unwrap();
        crate::edit::apply_all(&mut d, &ops).unwrap();
        crate::edit::apply_all(&mut d, &[crate::edit::Op::Delete { at: 0, len: 3 }]).unwrap();
        let rt = round_trip(&d);
        assert_eq!(crate::comments::threads(&rt), crate::comments::threads(&d));
        assert_eq!(rt.paragraphs, d.paragraphs);
    }

    /// An .odt made of these `content.xml` and `styles.xml`, read.
    fn read_package(content: &str, styles: &str) -> Document {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.odt");
        let mut out = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (name, data) in [("mimetype", "application/vnd.oasis.opendocument.text"), ("content.xml", content), ("styles.xml", styles)] {
            out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            out.write_all(data.as_bytes()).unwrap();
        }
        out.finish().unwrap();
        read(path.to_str().unwrap()).unwrap()
    }

    /// How Writer resolves a paragraph's look, as LibreOffice's own files
    /// rely on it: text written straight into a paragraph takes the
    /// paragraph style's text properties (a title in a bold 12pt
    /// paragraph style is bold 12pt), a span adds its own over them, a
    /// paragraph's space after and line height come from its parent
    /// "Standard" style when its own style leaves them unset, and the body
    /// font is the default style's, named by its font face. All four were
    /// lost: a Calibri agenda opened in the serif fallback with a plain
    /// title and tight table rows.
    #[test]
    fn paragraph_and_default_styles_are_inherited() {
        let content = "<office:document-content><office:automatic-styles>\
            <style:style style:name=\"P1\" style:parent-style-name=\"Standard\" style:family=\"paragraph\">\
              <style:paragraph-properties fo:text-align=\"center\" fo:margin-bottom=\"0in\"/>\
              <style:text-properties fo:font-weight=\"bold\" fo:font-size=\"12pt\"/></style:style>\
            <style:style style:name=\"P2\" style:parent-style-name=\"Standard\" style:family=\"paragraph\">\
              <style:paragraph-properties fo:text-align=\"center\"/></style:style>\
            <style:style style:name=\"T1\" style:family=\"text\"><style:text-properties fo:font-style=\"italic\"/></style:style>\
            </office:automatic-styles><office:body><office:text>\
            <text:p text:style-name=\"P1\">Title <text:span text:style-name=\"T1\">and more</text:span></text:p>\
            <text:p text:style-name=\"P2\">body</text:p>\
            </office:text></office:body></office:document-content>";
        let styles = "<office:document-styles><office:font-face-decls>\
            <style:font-face style:name=\"Calibri\" svg:font-family=\"Calibri\"/></office:font-face-decls><office:styles>\
            <style:default-style style:family=\"paragraph\"><style:text-properties style:font-name=\"Calibri\" fo:font-size=\"11pt\"/></style:default-style>\
            <style:style style:name=\"Standard\" style:family=\"paragraph\"><style:paragraph-properties fo:margin-bottom=\"0.1111in\" fo:line-height=\"103%\"/></style:style>\
            </office:styles></office:document-styles>";
        let d = read_package(content, styles);
        assert_eq!((d.base_font.family.as_deref(), d.base_font.size_hp), (Some("Calibri"), Some(22)));
        let title = &d.paragraphs[0];
        let runs: Vec<(&str, bool, bool, Option<u16>)> = title.runs.iter().map(|r| (r.text.as_str(), r.style.bold, r.style.italic, r.style.font_size_hp)).collect();
        assert_eq!(runs, [("Title ", true, false, Some(24)), ("and more", true, true, Some(24))]);
        assert_eq!(title.style.space_after_pt, 0.0, "its own style's zero wins over Standard's");
        let body = &d.paragraphs[1];
        assert!((body.style.space_after_pt - 8.0).abs() < 0.01, "Standard's space after: {}", body.style.space_after_pt);
        assert!((body.style.line_spacing - 1.03).abs() < 1e-4, "Standard's line height: {}", body.style.line_spacing);
        assert!(!body.runs[0].style.bold, "a paragraph style's text properties stay in its paragraph");
    }

    /// A standard ODF date field from another application opens as a date
    /// chip, keeping the text it shows.
    #[test]
    fn a_date_field_opens_as_a_date_chip() {
        let dir = tempfile::tempdir().unwrap();
        let ours = dir.path().join("ours.odt");
        write(&Document::from_plain_text("Due MARK."), ours.to_str().unwrap()).unwrap();
        // The same package, with a date field where MARK was.
        let theirs = dir.path().join("theirs.odt");
        let mut zin = zip::ZipArchive::new(std::fs::File::open(&ours).unwrap()).unwrap();
        let mut out = zip::ZipWriter::new(std::fs::File::create(&theirs).unwrap());
        for i in 0..zin.len() {
            let mut f = zin.by_index(i).unwrap();
            let name = f.name().to_string();
            let mut data = String::new();
            std::io::Read::read_to_string(&mut f, &mut data).unwrap();
            if name == "content.xml" {
                data = data.replace(
                    "MARK",
                    "<text:date style:data-style-name=\"N37\" text:date-value=\"2025-01-31T00:00:00\">31/01/2025</text:date>",
                );
            }
            out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            out.write_all(data.as_bytes()).unwrap();
        }
        out.finish().unwrap();
        let rt = read(theirs.to_str().unwrap()).unwrap();
        let chip = rt.paragraphs[0].runs.iter().find(|r| r.style.chip.is_some()).expect("a chip");
        assert_eq!(chip.text, "31/01/2025");
        assert_eq!(chip.style.chip.as_ref().unwrap().value, "2025-01-31");
    }

    /// A section's column count, which lives in content.xml.
    ///
    /// LibreOffice models a docx `w:cols` as an ODF section rather than a
    /// page-wide layout, so this is the shape a converted file arrives
    /// in. The gap is optional: a section may state a count alone.
    #[test]
    fn section_columns_are_read_from_content() {
        let xml = "<office:document-content><office:automatic-styles>\
                   <style:style style:name=\"Sect1\" style:family=\"section\">\
                   <style:section-properties style:editable=\"false\">\
                   <style:columns fo:column-count=\"2\" fo:column-gap=\"0.3335in\"/>\
                   </style:section-properties></style:style>\
                   </office:automatic-styles></office:document-content>";
        let (count, gap) = parse_section_columns(xml).expect("section columns");
        assert_eq!(count, 2);
        assert!((gap.expect("gap") - 24.0).abs() < 0.1, "gap: {gap:?}");
    }

    /// One column is not a column layout, and must not displace the page's.
    #[test]
    fn a_single_column_section_is_not_a_column_layout() {
        let xml = "<office:document-content>\
                   <style:section-properties>\
                   <style:columns fo:column-count=\"1\"/>\
                   </style:section-properties></office:document-content>";
        assert!(parse_section_columns(xml).is_none());
    }

    /// `style:columns` outside a section is the page's, read elsewhere.
    #[test]
    fn page_layout_columns_are_not_mistaken_for_a_section() {
        let xml = "<office:document-styles><style:page-layout-properties>\
                   <style:columns fo:column-count=\"3\"/>\
                   </style:page-layout-properties></office:document-styles>";
        assert!(parse_section_columns(xml).is_none());
    }

    /// Footnotes, which ODF nests inside the referencing paragraph.
    ///
    /// The writer and reader both ignored `text:note` entirely, so a
    /// footnote's text — authored content, not layout — was dropped on
    /// every odt save. The body paragraph must come back unchanged too:
    /// the note's own `text:p` children are nested *inside* it, so a
    /// reader that treats them as body text either splits the paragraph
    /// or pulls the footnote into it.
    #[test]
    fn footnotes_survive() {
        let mut d = Document::from_plain_text("");
        d.footnotes = vec!["the note text".to_string()];
        d.paragraphs[0] = Paragraph {
            style: ParaStyle::default(),
            runs: vec![
                Run::plain("body before"),
                Run { text: String::new(), style: RunStyle { footnote: Some(0), ..Default::default() } },
                Run::plain(" and after"),
            ],
        };
        let rt = round_trip(&d);
        assert_eq!(rt.footnotes, vec!["the note text".to_string()]);
        assert_eq!(rt.paragraphs.len(), 1, "note body leaked into the body stream: {:?}", rt.paragraphs);
        let text: String = rt.paragraphs[0].runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text, "body before and after");
        assert_eq!(
            rt.paragraphs[0].runs.iter().filter_map(|r| r.style.footnote).collect::<Vec<_>>(),
            vec![0],
            "the reference run did not come back"
        );
    }

    /// Two notes keep their own text and their own order.
    #[test]
    fn two_footnotes_keep_their_indexes() {
        let mut d = Document::from_plain_text("");
        d.footnotes = vec!["first".to_string(), "second".to_string()];
        d.paragraphs[0] = Paragraph {
            style: ParaStyle::default(),
            runs: vec![
                Run::plain("a"),
                Run { text: String::new(), style: RunStyle { footnote: Some(0), ..Default::default() } },
                Run::plain("b"),
                Run { text: String::new(), style: RunStyle { footnote: Some(1), ..Default::default() } },
            ],
        };
        let rt = round_trip(&d);
        assert_eq!(rt.footnotes, vec!["first".to_string(), "second".to_string()]);
        assert_eq!(
            rt.paragraphs[0].runs.iter().filter_map(|r| r.style.footnote).collect::<Vec<_>>(),
            vec![0, 1]
        );
    }

    /// Tab stops, which ODF puts in a child element of the paragraph
    /// properties rather than an attribute.
    ///
    /// Neither writer persisted these, so a paragraph's stops were lost
    /// on every save in both formats. Because `style:tab-stops` is a
    /// child, a paragraph carrying stops cannot be written as a
    /// self-closing `style:paragraph-properties` — which is what this
    /// writer emitted for every style it had.
    #[test]
    fn tab_stops_survive() {
        let mut d = Document::from_plain_text("tabbed");
        d.paragraphs[0].style.tab_stops_pt = vec![36.0, 108.0];
        let rt = round_trip(&d);
        assert_eq!(rt.paragraphs[0].style.tab_stops_pt.len(), 2, "stops lost");
        for (got, want) in rt.paragraphs[0].style.tab_stops_pt.iter().zip([36.0, 108.0]) {
            assert!((got - want).abs() < 0.01, "stop {got} != {want}");
        }
    }

    /// Stops belong to the paragraph that declared them.
    ///
    /// Paragraph automatic styles are deduplicated, and the key used to
    /// be the attribute string alone — which every paragraph here shares.
    /// Two paragraphs with different stops would then have collapsed onto
    /// one style and both read back with the first one's stops.
    #[test]
    fn different_tab_stops_do_not_share_one_style() {
        let mut d = Document::from_plain_text("first\nsecond");
        d.paragraphs[0].style.tab_stops_pt = vec![36.0];
        d.paragraphs[1].style.tab_stops_pt = vec![144.0];
        let rt = round_trip(&d);
        assert_eq!(rt.paragraphs[0].style.tab_stops_pt.len(), 1);
        assert_eq!(rt.paragraphs[1].style.tab_stops_pt.len(), 1);
        assert!((rt.paragraphs[0].style.tab_stops_pt[0] - 36.0).abs() < 0.01);
        assert!(
            (rt.paragraphs[1].style.tab_stops_pt[0] - 144.0).abs() < 0.01,
            "second paragraph got the first one's stops: {:?}",
            rt.paragraphs[1].style.tab_stops_pt
        );
    }

    #[test]
    fn plain_paragraphs() {
        let d = Document::from_plain_text("first\nsecond\n\nfourth");
        assert_eq!(round_trip(&d).to_plain_text(), d.to_plain_text());
    }

    #[test]
    fn headings_survive() {
        let mut d = Document::from_plain_text("Title\nSection\nbody");
        d.set_heading(0, Some(1));
        d.set_heading(1, Some(3));
        let rt = round_trip(&d);
        assert_eq!(rt.paragraphs[0].style.heading, Some(1));
        assert_eq!(rt.paragraphs[1].style.heading, Some(3));
        assert_eq!(rt.paragraphs[2].style.heading, None);
        assert_eq!(rt.to_plain_text(), d.to_plain_text());
    }

    #[test]
    fn inline_styles_survive() {
        let mut d = Document::from_plain_text("normal bold italic under strike");
        d.apply_run_style(7, 11, &StylePatch::set_bold(true));
        d.apply_run_style(12, 18, &StylePatch::set_italic(true));
        d.apply_run_style(19, 24, &StylePatch::set_underline(true));
        d.apply_run_style(25, 31, &StylePatch::set_strikethrough(true));
        let rt = round_trip(&d);
        assert_eq!(rt.to_plain_text(), d.to_plain_text());
        assert_eq!(rt.paragraphs[0].runs, d.paragraphs[0].runs);
    }

    #[test]
    fn font_size_color_highlight_survive() {
        let mut d = Document::from_plain_text("");
        d.paragraphs[0].runs = vec![
            Run {
                text: "sized".into(),
                style: RunStyle { font_size_hp: Some(28), ..Default::default() },
            },
            Run::plain(" "),
            Run {
                text: "colored".into(),
                style: RunStyle { color: Some("ff0000".into()), ..Default::default() },
            },
            Run::plain(" "),
            Run {
                text: "marked".into(),
                style: RunStyle { highlight: true, ..Default::default() },
            },
        ];
        let rt = round_trip(&d);
        assert_eq!(rt.paragraphs[0].runs, d.paragraphs[0].runs);
    }

    #[test]
    fn links_survive() {
        let mut d = Document::from_plain_text("");
        d.paragraphs[0].runs = vec![
            Run::plain("visit "),
            Run {
                text: "gnome".into(),
                style: RunStyle { link: Some("https://gnome.org".into()), ..Default::default() },
            },
            Run::plain(" now"),
        ];
        let rt = round_trip(&d);
        assert_eq!(rt.paragraphs[0].runs, d.paragraphs[0].runs);
    }

    #[test]
    fn alignment_and_page_break_survive() {
        let mut d = Document::from_plain_text("centered\nright\njustified\nbroken");
        d.paragraphs[0].style.alignment = Alignment::Center;
        d.paragraphs[1].style.alignment = Alignment::Right;
        d.paragraphs[2].style.alignment = Alignment::Justify;
        d.paragraphs[3].style.page_break_before = true;
        let rt = round_trip(&d);
        assert_eq!(rt.paragraphs[0].style.alignment, Alignment::Center);
        assert_eq!(rt.paragraphs[1].style.alignment, Alignment::Right);
        assert_eq!(rt.paragraphs[2].style.alignment, Alignment::Justify);
        assert!(rt.paragraphs[3].style.page_break_before);
    }

    #[test]
    fn lists_survive() {
        let mut d = Document::from_plain_text("intro\none\ntwo\nfirst\nsecond\noutro");
        d.paragraphs[1].style.list = ListKind::Bullet;
        d.paragraphs[2].style.list = ListKind::Bullet;
        d.paragraphs[3].style.list = ListKind::Numbered;
        d.paragraphs[4].style.list = ListKind::Numbered;
        let rt = round_trip(&d);
        let kinds: Vec<ListKind> = rt.paragraphs.iter().map(|p| p.style.list).collect();
        assert_eq!(
            kinds,
            vec![
                ListKind::None,
                ListKind::Bullet,
                ListKind::Bullet,
                ListKind::Numbered,
                ListKind::Numbered,
                ListKind::None
            ]
        );
        assert_eq!(rt.to_plain_text(), d.to_plain_text());
    }

    #[test]
    fn header_footer_survive() {
        let mut d = Document::from_plain_text("body");
        d.header = Some("Report — {page} of {total}".into());
        d.footer = Some("Confidential".into());
        let rt = round_trip(&d);
        assert_eq!(rt.header.as_deref(), Some("Report — {page} of {total}"));
        assert_eq!(rt.footer.as_deref(), Some("Confidential"));
    }

    /// A Word conversion's heading styles are named "Heading2", not
    /// Writer's `Heading_20_2`, and say their level with
    /// `style:default-outline-level`; their look (Arial, bold, blue) is
    /// the headings' as it is in LibreOffice, not the bold serif fallback.
    #[test]
    fn heading_styles_are_found_by_outline_level() {
        let styles = "<office:document-styles><office:styles>\
            <style:style style:name=\"Normal\" style:family=\"paragraph\"/>\
            <style:style style:name=\"Heading2\" style:display-name=\"Heading 2\" style:family=\"paragraph\" \
              style:parent-style-name=\"Normal\" style:default-outline-level=\"2\">\
              <style:text-properties style:font-name=\"Arial\" fo:font-weight=\"bold\" fo:color=\"#2E74B5\" fo:font-size=\"18pt\"/></style:style>\
            </office:styles></office:document-styles>";
        let content = "<office:document-content><office:body><office:text>\
            <text:h text:style-name=\"Heading2\" text:outline-level=\"2\">Summary</text:h>\
            </office:text></office:body></office:document-content>";
        let d = read_package(content, styles);
        let h2 = &d.heading_styles[1];
        assert!(h2.bold, "{h2:?}");
        assert_eq!(h2.color.as_deref().map(|c| c.trim_start_matches('#').to_uppercase()), Some("2E74B5".into()), "{h2:?}");
        assert_eq!(h2.font_size_hp, Some(36), "{h2:?}");
        assert_eq!(h2.font_family.as_deref(), Some("Arial"), "{h2:?}");
    }

    /// A Word conversion has no "Standard" style: its paragraphs inherit
    /// Word's "Normal" (Palatino 12pt), not the default style (Times
    /// 10pt), so Normal gives the body font. Standard still wins where a
    /// file has both.
    #[test]
    fn words_normal_style_gives_the_body_font_without_standard() {
        let styles = |standard: &str| format!(
            "<office:document-styles><office:styles>\
             <style:default-style style:family=\"paragraph\"><style:text-properties style:font-name=\"Times New Roman\" fo:font-size=\"10pt\"/></style:default-style>\
             <style:style style:name=\"Normal\" style:family=\"paragraph\"><style:text-properties style:font-name=\"Palatino Linotype\" fo:font-size=\"12pt\"/></style:style>\
             {standard}</office:styles></office:document-styles>"
        );
        let content = "<office:document-content><office:body><office:text><text:p>body</text:p></office:text></office:body></office:document-content>";
        let d = read_package(content, &styles(""));
        assert_eq!((d.base_font.family.as_deref(), d.base_font.size_hp), (Some("Palatino Linotype"), Some(24)));
        let d = read_package(content, &styles("<style:style style:name=\"Standard\" style:family=\"paragraph\"><style:text-properties style:font-name=\"Carlito\" fo:font-size=\"11pt\"/></style:style>"));
        assert_eq!((d.base_font.family.as_deref(), d.base_font.size_hp), (Some("Carlito"), Some(22)));
    }

    /// A numbered list split by headings, each part continuing the one
    /// before (`text:continue-numbering`, as a Word conversion writes
    /// every run of numbered paragraphs), counts on across them: 1, 2,
    /// heading, 3. A list that does not continue starts again at 1.
    #[test]
    fn a_continued_list_counts_on_across_headings() {
        let styles = "<office:document-styles><office:styles><text:list-style style:name=\"L1\">\
            <text:list-level-style-number text:level=\"1\" style:num-format=\"1\"/></text:list-style>\
            </office:styles></office:document-styles>";
        let list = |cont: &str, items: &[&str]| format!(
            "<text:list text:style-name=\"L1\"{cont}>{}</text:list>",
            items.iter().map(|t| format!("<text:list-item><text:p>{t}</text:p></text:list-item>")).collect::<String>()
        );
        let content = format!(
            "<office:document-content><office:body><office:text>{}<text:h text:outline-level=\"2\">Heading</text:h>{}\
             <text:h text:outline-level=\"2\">Again</text:h>{}{}</office:text></office:body></office:document-content>",
            list("", &["one", "two"]),
            list(" text:continue-numbering=\"true\"", &["three"]),
            list("", &["fresh"]),
            list(" text:continue-numbering=\"true\"", &["second"]),
        );
        let d = read_package(&content, styles);
        let numbered: Vec<(String, Option<u32>)> = d.paragraphs.iter().filter(|p| p.style.list == ListKind::Numbered).map(|p| (p.text(), p.style.list_start)).collect();
        assert_eq!(numbered, [("one".into(), None), ("two".into(), None), ("three".into(), Some(3)), ("fresh".into(), None), ("second".into(), Some(2))]);
        let ordinals = crate::lists::ordinals(d.paragraphs.iter().map(|p| &p.style));
        let shown: Vec<u32> = ordinals.into_iter().filter(|n| *n > 0).collect();
        assert_eq!(shown, [1, 2, 3, 1, 2]);
    }

    /// A file with several master pages (a Word conversion gives each
    /// section one) shows the first page's: its header, footer and page
    /// layout, not every master's joined, and a footer text box's
    /// alternative text is not footer text. The first paragraph's style
    /// names the master; "Standard" is the fallback.
    #[test]
    fn the_first_pages_master_gives_the_header_and_page() {
        let master = |name: &str, layout: &str, header: &str| format!(
            "<style:master-page style:name=\"{name}\" style:page-layout-name=\"{layout}\">\
             <style:header><text:p>{header}</text:p></style:header>\
             <style:footer><text:p><draw:frame><draw:text-box><text:p>OFFICIAL</text:p></draw:text-box>\
             <svg:title/><svg:desc>OFFICIAL</svg:desc></draw:frame></text:p></style:footer></style:master-page>"
        );
        let layout = |name: &str, left: &str| format!(
            "<style:page-layout style:name=\"{name}\"><style:page-layout-properties fo:page-width=\"8.27in\" \
             fo:page-height=\"11.69in\" fo:margin-left=\"{left}\"/></style:page-layout>"
        );
        let styles = format!(
            "<office:document-styles><office:automatic-styles>{}{}</office:automatic-styles>\
             <office:master-styles>{}{}</office:master-styles></office:document-styles>",
            layout("PL0", "1in"), layout("PL1", "2in"), master("MP0", "PL0", "FIRST"), master("MP1", "PL1", "SECOND"),
        );
        let content = |master: &str| format!(
            "<office:document-content><office:automatic-styles>\
             <style:style style:name=\"P1\" style:family=\"paragraph\" style:master-page-name=\"{master}\"/>\
             </office:automatic-styles><office:body><office:text><text:p text:style-name=\"P1\">body</text:p>\
             </office:text></office:body></office:document-content>"
        );
        for (first, header, left) in [("MP0", "FIRST", 72.0), ("MP1", "SECOND", 144.0), ("", "FIRST", 72.0)] {
            let d = read_package(&content(first), &styles);
            assert_eq!(d.header.as_deref(), Some(header), "first page's master {first:?}");
            assert_eq!(d.footer.as_deref(), Some("OFFICIAL"));
            assert!((d.page.unwrap().margin_left_pt - left).abs() < 1e-6, "{first:?}: {:?}", d.page);
        }
    }

    #[test]
    fn special_chars_escaped() {
        let d = Document::from_plain_text("a < b & c > \"d\"");
        assert_eq!(round_trip(&d).to_plain_text(), d.to_plain_text());
    }

    #[test]
    fn page_geometry_survives() {
        let mut d = Document::from_plain_text("body");
        d.page = Some(PageGeometry {
            width_pt: 612.0,   // US Letter
            height_pt: 792.0,
            margin_top_pt: 36.0,
            margin_bottom_pt: 54.0,
            margin_left_pt: 90.0,
            margin_right_pt: 45.0,
            columns: 1,
            column_gap_pt: 18.0,
        });
        let rt = round_trip(&d);
        let pg = rt.page.expect("page geometry lost");
        assert!(pg.approx_eq(&d.page.unwrap()), "geometry drifted: {pg:?}");
    }

    #[test]
    fn structured_paragraph_layout_survives() {
        let mut d = Document::from_plain_text("layout");
        d.paragraphs[0].style.space_before_pt = 6.0;
        d.paragraphs[0].style.space_after_pt = 9.0;
        d.paragraphs[0].style.left_indent_pt = 24.0;
        d.paragraphs[0].style.first_line_indent_pt = -12.0;
        let rt = round_trip(&d);
        let st = &rt.paragraphs[0].style;
        assert!((st.space_before_pt - 6.0).abs() < 0.01);
        assert!((st.space_after_pt - 9.0).abs() < 0.01);
        assert!((st.left_indent_pt - 24.0).abs() < 0.01);
        assert!((st.first_line_indent_pt + 12.0).abs() < 0.01);
    }

    #[test]
    fn no_page_geometry_reads_none() {
        let d = Document::from_plain_text("body");
        assert_eq!(round_trip(&d).page, None);
    }

    #[test]
    fn font_family_survives() {
        let mut d = Document::from_plain_text("");
        d.paragraphs[0].runs = vec![
            Run::plain("sans "),
            Run {
                text: "serif".into(),
                style: RunStyle {
                    font_family: Some("Liberation Serif".into()),
                    ..Default::default()
                },
            },
        ];
        let rt = round_trip(&d);
        assert_eq!(rt.paragraphs[0].runs, d.paragraphs[0].runs);
    }

    #[test]
    fn line_spacing_survives() {
        let mut d = Document::from_plain_text("single\ndouble");
        d.paragraphs[1].style.line_spacing = 2.0;
        let rt = round_trip(&d);
        assert!((rt.paragraphs[0].style.line_spacing - 1.0).abs() < 0.01);
        assert!((rt.paragraphs[1].style.line_spacing - 2.0).abs() < 0.01);
    }

    #[test]
    fn empty_paragraphs_preserved() {
        let d = Document::from_plain_text("a\n\n\nb");
        assert_eq!(round_trip(&d).to_plain_text(), "a\n\n\nb");
    }
}
