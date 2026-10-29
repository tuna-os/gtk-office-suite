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
    } else if st.regular {
        props.push_str(" fo:font-weight=\"normal\"");
    }
    if st.italic {
        props.push_str(" fo:font-style=\"italic\"");
    }
    if st.caps {
        props.push_str(" fo:text-transform=\"uppercase\"");
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
    if st.keep_lines {
        props.push_str(" fo:keep-together=\"always\"");
    }
    match st.widow_control {
        Some(false) => props.push_str(" fo:widows=\"1\" fo:orphans=\"1\""),
        Some(true) => props.push_str(" fo:widows=\"2\" fo:orphans=\"2\""),
        None => {}
    }
    match st.line_height {
        // A fixed height is a length; a minimum is
        // `style:line-height-at-least`, which Writer keeps.
        Some(LineHeight::Exact(pt)) => props.push_str(&format!(" fo:line-height=\"{pt:.2}pt\"")),
        Some(LineHeight::Minimum(pt)) => props.push_str(&format!(" style:line-height-at-least=\"{pt:.2}pt\"")),
        None => {
            if (st.line_spacing - 1.0).abs() > 0.01 {
                props.push_str(&format!(" fo:line-height=\"{:.0}%\"", st.line_spacing * 100.0));
            }
        }
    }
    if let Some(fill) = st.shading.as_deref().filter(|f| !f.is_empty()) {
        props.push_str(&format!(" fo:background-color=\"#{fill}\""));
    }
    let border = |e: &ParaBorder| {
        let style = match e.style {
            BorderStyle::None => "none",
            BorderStyle::Single => "solid",
            BorderStyle::Dashed => "dashed",
            BorderStyle::Dotted => "dotted",
            BorderStyle::Double => "double",
        };
        format!("{:.2}pt {style} #{}", e.width_pt, e.color)
    };
    if let Some(e) = &st.borders.top {
        props.push_str(&format!(" fo:border-top=\"{}\"", border(e)));
    }
    if let Some(e) = &st.borders.left {
        props.push_str(&format!(" fo:border-left=\"{}\"", border(e)));
    }
    if let Some(e) = &st.borders.bottom {
        props.push_str(&format!(" fo:border-bottom=\"{}\"", border(e)));
    }
    if let Some(e) = &st.borders.right {
        props.push_str(&format!(" fo:border-right=\"{}\"", border(e)));
    }
    // ODF spells paragraph spacing `fo:margin-top`/`fo:margin-bottom`,
    // the same XSL-FO properties the page geometry below already uses.
    // This wrote `fo:space-before`/`fo:space-after`, which our own reader
    // understood and LibreOffice does not: the self round trip passed
    // because both halves shared an attribute nothing else reads, and the
    // spacing was silently gone the moment the file reached Writer.
    if st.space_before_pt.abs() > 0.01 { props.push_str(&format!(" fo:margin-top=\"{:.2}pt\"", st.space_before_pt)); }
    if st.space_after_pt.abs() > 0.01 { props.push_str(&format!(" fo:margin-bottom=\"{:.2}pt\"", st.space_after_pt)); }
    // A list item placed off its level writes where its text and marker
    // stand, which replace its list level's (label-alignment) in Writer
    // and in the reader.
    let own_list_place = st.list != ListKind::None && (st.left_indent_pt.abs() > 0.01 || st.list_hanging_pt.is_some());
    if own_list_place {
        let hanging = st.list_hanging_pt.unwrap_or(crate::lists::HANGING_PT);
        props.push_str(&format!(
            " fo:margin-left=\"{:.2}pt\" fo:text-indent=\"{:.2}pt\"",
            st.left_indent_pt + crate::lists::text_indent_pt(st.list_level),
            -hanging
        ));
    } else {
        if st.left_indent_pt.abs() > 0.01 { props.push_str(&format!(" fo:margin-left=\"{:.2}pt\"", st.left_indent_pt)); }
        if st.first_line_indent_pt.abs() > 0.01 { props.push_str(&format!(" fo:text-indent=\"{:.2}pt\"", st.first_line_indent_pt)); }
    }
    if st.right_indent_pt.abs() > 0.01 { props.push_str(&format!(" fo:margin-right=\"{:.2}pt\"", st.right_indent_pt)); }
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
    for stop in &st.tab_stops_pt {
        // `style:type` defaults to left, so only other alignments are
        // spelled out; leaders ride `style:leader-style` with their
        // character, as Writer writes them.
        let kind = match stop.align {
            TabAlign::Center => " style:type=\"center\"",
            TabAlign::Right => " style:type=\"right\"",
            TabAlign::Decimal => " style:type=\"char\"",
            TabAlign::Left => "",
        };
        let leader = match stop.leader {
            TabLeader::Dot => " style:leader-style=\"dotted\" style:leader-text=\".\"",
            TabLeader::Hyphen => " style:leader-style=\"dashed\" style:leader-text=\"-\"",
            TabLeader::Underscore => " style:leader-style=\"solid\" style:leader-text=\"_\"",
            TabLeader::MiddleDot => " style:leader-style=\"dotted\" style:leader-text=\"·\"",
            TabLeader::None => "",
        };
        out.push_str(&format!("<style:tab-stop style:position=\"{:.2}pt\"{kind}{leader}/>", stop.pos_pt));
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

/// A picture run as a frame (picture number `n`): anchored as a character,
/// so it stays where it is in the line, or, floating, anchored to its
/// paragraph and placed by a graphic style of its own (`{prefix}{n}`, added
/// to `auto`). Its bytes are in Pictures/ (#1292). None when its source
/// can't be read.
fn picture_frame(r: &Run, n: usize, prefix: &str, auto: &mut String) -> Option<String> {
    let (name, bytes, _) = picture(r.style.image.as_deref()?)?;
    let (w, h) = picture_extent(r, &bytes);
    let title = if r.text.trim().is_empty() { String::new() } else { format!("<svg:title>{}</svg:title>", esc(&r.text)) };
    let anchoring = match r.style.image_anchor {
        Some(a) => {
            let style = format!("{prefix}{n}");
            auto.push_str(&floating_style(&style, &a));
            format!(
                "draw:style-name=\"{style}\" text:anchor-type=\"paragraph\" svg:x=\"{:.6}in\" svg:y=\"{:.6}in\"",
                a.x_emu as f64 / EMU_PER_INCH,
                a.y_emu as f64 / EMU_PER_INCH,
            )
        }
        // The room kept beside it, as the margins of a graphic style.
        None => match r.style.image_sides_emu {
            Some((left, right)) => {
                let style = format!("{prefix}{n}");
                auto.push_str(&format!(
                    "<style:style style:name=\"{style}\" style:family=\"graphic\"><style:graphic-properties fo:margin-left=\"{:.2}pt\" fo:margin-right=\"{:.2}pt\"/></style:style>",
                    left as f64 / 12700.0,
                    right as f64 / 12700.0,
                ));
                format!("draw:style-name=\"{style}\" text:anchor-type=\"as-char\"")
            }
            None => "text:anchor-type=\"as-char\"".to_string(),
        },
    };
    Some(format!(
        "<draw:frame draw:name=\"Picture {n}\" {anchoring} svg:width=\"{:.6}in\" svg:height=\"{:.6}in\">\
         <draw:image xlink:href=\"{name}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/>\
         {title}</draw:frame>",
        w as f64 / EMU_PER_INCH,
        h as f64 / EMU_PER_INCH,
    ))
}

/// A text box run as a frame (text box number `n`) holding a
/// `draw:text-box`, anchored to its paragraph and placed, framed, filled
/// and padded by a graphic style of its own (`tb{n}`, added to `auto`).
/// Its text is a paragraph per line.
fn text_box_frame(r: &Run, n: usize, auto: &mut String) -> Option<String> {
    let tb = r.style.text_box.as_deref()?;
    let anchor = r.style.image_anchor.unwrap_or_default();
    let style = format!("tb{n}");
    let emu_pt = |v: u32| v as f64 / 12700.0;
    let mut look = String::new();
    match &tb.border {
        Some(b) => look.push_str(&format!(" fo:border=\"{:.2}pt solid #{}\"", emu_pt(b.width_emu), b.color)),
        None => look.push_str(" fo:border=\"none\""),
    }
    match &tb.fill {
        Some(c) => look.push_str(&format!(" draw:fill=\"solid\" draw:fill-color=\"#{c}\"")),
        None => look.push_str(" draw:fill=\"none\""),
    }
    let [top, right, bottom, left] = tb.padding_emu;
    look.push_str(&format!(
        " fo:padding-top=\"{:.2}pt\" fo:padding-right=\"{:.2}pt\" fo:padding-bottom=\"{:.2}pt\" fo:padding-left=\"{:.2}pt\"",
        emu_pt(top), emu_pt(right), emu_pt(bottom), emu_pt(left)
    ));
    // Under Writer's "Frame" style it is a text frame to Writer, which
    // keeps its rule; without a parent Writer reads it as a drawing shape
    // and drops the rule.
    let frame_style = floating_style(&style, &anchor)
        .replacen("style:family=\"graphic\"", "style:family=\"graphic\" style:parent-style-name=\"Frame\"", 1)
        .replacen("/></style:style>", &format!("{look}/></style:style>"), 1);
    auto.push_str(&frame_style);
    let text: String = tb.text.split('\n').map(|line| format!("<text:p>{}</text:p>", esc(line))).collect();
    Some(format!(
        "<draw:frame draw:style-name=\"{style}\" draw:name=\"Text Box {n}\" text:anchor-type=\"paragraph\" \
         svg:x=\"{:.6}in\" svg:y=\"{:.6}in\" svg:width=\"{:.6}in\" svg:height=\"{:.6}in\"><draw:text-box>{text}</draw:text-box></draw:frame>",
        anchor.x_emu as f64 / EMU_PER_INCH,
        anchor.y_emu as f64 / EMU_PER_INCH,
        tb.extent_emu.0 as f64 / EMU_PER_INCH,
        tb.extent_emu.1 as f64 / EMU_PER_INCH,
    ))
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
        "<style:style style:name=\"{name}\" style:family=\"graphic\"><style:graphic-properties style:wrap=\"{}\" \
         style:run-through=\"{}\" style:horizontal-pos=\"{}\" style:horizontal-rel=\"{}\" style:vertical-pos=\"{}\" \
         style:vertical-rel=\"{}\"/></style:style>",
        match a.wrap {
            crate::model::Wrap::None => "run-through",
            crate::model::Wrap::Parallel => "parallel",
            crate::model::Wrap::TopBottom => "none",
        },
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
        // Text beside it on one side or both is read as beside it on its
        // wider side, which is where Writer puts a short paragraph.
        wrap: match attr_val(e, "style:wrap").as_deref() {
            Some("parallel" | "dynamic" | "biggest" | "left" | "right") => crate::model::Wrap::Parallel,
            Some("none") => crate::model::Wrap::TopBottom,
            _ => crate::model::Wrap::None,
        },
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
    /// The table's merged cells.
    spans: Vec<CellSpan>,
    /// Each shaded cell's style attribute, by (row, col).
    cell_styles: std::collections::HashMap<(u32, u32), String>,
}

impl OpenTable {
    /// Whether the current cell is one a merged cell covers.
    fn covered(&self) -> bool {
        self.spans.iter().any(|s| s.contains(self.row, self.col) && (s.row, s.col) != (self.row, self.col))
    }

    /// Open the current cell: a merged cell says how far it spans, and a
    /// position one covers is a `table:covered-table-cell`.
    fn open_cell(&mut self, body: &mut String) {
        self.filled = false;
        if self.covered() {
            body.push_str("<table:covered-table-cell>");
            return;
        }
        let span = self.spans.iter().find(|s| (s.row, s.col) == (self.row, self.col));
        let attrs = span.map_or(String::new(), |s| {
            format!(" table:number-columns-spanned=\"{}\" table:number-rows-spanned=\"{}\"", s.cols, s.rows)
        });
        let style = self.cell_styles.get(&(self.row, self.col)).map_or("", String::as_str);
        body.push_str(&format!("<table:table-cell office:value-type=\"string\"{style}{attrs}>"));
    }

    fn open_row(&self, body: &mut String) {
        let style = self.row_styles.get(self.row as usize).map_or("", String::as_str);
        body.push_str(&format!("<table:table-row{style}>"));
    }

    fn close_cell(&mut self, body: &mut String) {
        if self.covered() {
            body.push_str("</table:covered-table-cell>");
            return;
        }
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

/// The list styles numbered items need for their labels
/// (`ParaStyle::list_label`). An ODF list style labels each of its levels
/// one way, so a run of numbered items takes one style while its levels
/// keep their labels, and the next style where a level changes label.
struct LabelledLists {
    /// Each set of level labels after "LN"'s, which is all "N.".
    sets: Vec<[Option<ListLabel>; 10]>,
    /// Per paragraph: 0 for "LN", k for `sets[k - 1]` ("LNk").
    style: Vec<usize>,
    /// Per paragraph: where a list split for its label starts counting,
    /// so the item keeps its number in the new list.
    start: Vec<Option<u32>>,
}

fn labelled_lists(paragraphs: &[Paragraph]) -> LabelledLists {
    let ordinals = crate::lists::ordinals(paragraphs.iter().map(|p| &p.style));
    let mut out = LabelledLists { sets: Vec::new(), style: vec![0; paragraphs.len()], start: vec![None; paragraphs.len()] };
    // The list being built: its first paragraph and its levels' labels,
    // `None` within a level that has an item and no label.
    let mut run: Option<(usize, [Option<Option<ListLabel>>; 10])> = None;
    let close = |out: &mut LabelledLists, run: Option<(usize, [Option<Option<ListLabel>>; 10])>, end: usize| {
        let Some((first, levels)) = run else { return };
        let labels: [Option<ListLabel>; 10] = levels.map(Option::flatten);
        let k = if labels.iter().all(Option::is_none) {
            0
        } else {
            match out.sets.iter().position(|s| *s == labels) {
                Some(k) => k + 1,
                None => {
                    out.sets.push(labels);
                    out.sets.len()
                }
            }
        };
        out.style[first..end].iter_mut().for_each(|s| *s = k);
    };
    for (i, p) in paragraphs.iter().enumerate() {
        if p.style.list != ListKind::Numbered {
            close(&mut out, run.take(), i);
            continue;
        }
        let level = usize::from(p.style.list_level.min(9));
        let label = &p.style.list_label;
        if let Some((_, levels)) = &run {
            if levels[level].as_ref().is_some_and(|l| l != label) {
                close(&mut out, run.take(), i);
                out.start[i] = Some(ordinals[i]);
            }
        }
        let (_, levels) = run.get_or_insert_with(|| (i, Default::default()));
        levels[level].get_or_insert_with(|| label.clone());
    }
    close(&mut out, run.take(), paragraphs.len());
    out
}

/// ODF's `style:num-format` for `format`.
fn odf_number_format(format: NumberFormat) -> &'static str {
    match format {
        NumberFormat::Decimal => "1",
        NumberFormat::LowerLetter => "a",
        NumberFormat::UpperLetter => "A",
        NumberFormat::LowerRoman => "i",
        NumberFormat::UpperRoman => "I",
        NumberFormat::None => "",
    }
}

fn content_xml(doc: &Document) -> String {
    let run_styles = collect_run_styles(doc);
    let mut pictures_written = 0usize;
    let mut text_boxes_written = 0usize;

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
    // A paragraph automatic style: its properties and tab stops, its
    // heading level's style as parent, and `extra` attributes.
    let para_auto = |(props, kids, parent): &(String, String, Option<u8>), name: &str, extra: &str| -> String {
        let body = if kids.is_empty() {
            format!("<style:paragraph-properties{props}/>")
        } else {
            format!("<style:paragraph-properties{props}>{kids}</style:paragraph-properties>")
        };
        let parent = parent.map(|n| format!(" style:parent-style-name=\"Heading_20_{n}\"")).unwrap_or_default();
        format!("<style:style style:name=\"{name}\" style:family=\"paragraph\"{parent}{extra}>{body}</style:style>")
    };
    for (i, elem) in para_autos.iter().enumerate() {
        auto.push_str(&para_auto(elem, &format!("P{}", i + 1), ""));
    }
    // The paragraph or table each section after the first starts with,
    // and the section's index: its style names the section's master page,
    // so it starts a new page on the section's (LET-005S, `styles_xml`).
    let section_start: std::collections::HashMap<usize, usize> = doc.sections.iter().enumerate().skip(1).map(|(k, s)| (s.range.start, k)).collect();
    let master_attr = |k: usize| format!(" style:master-page-name=\"{}\"", section_master(k));
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
    auto.push_str("</text:list-style>");
    // "LN", every level "N.", and one more for each set of labels a list
    // gives its levels (`labelled_lists`).
    let labelled = labelled_lists(&doc.paragraphs);
    let plain: [Option<ListLabel>; 10] = Default::default();
    let label_sets = std::iter::once(&plain).chain(&labelled.sets);
    for (k, labels) in label_sets.enumerate() {
        let name = if k == 0 { "LN".to_string() } else { format!("LN{k}") };
        auto.push_str(&format!("<text:list-style style:name=\"{name}\">"));
        for (level, label) in labels.iter().enumerate() {
            let label = label.clone().unwrap_or(ListLabel { suffix: ".".into(), ..Default::default() });
            let prefix = Some(&label.prefix).filter(|p| !p.is_empty()).map(|p| format!(" style:num-prefix=\"{}\"", esc(p))).unwrap_or_default();
            let suffix = Some(&label.suffix).filter(|p| !p.is_empty()).map(|p| format!(" style:num-suffix=\"{}\"", esc(p))).unwrap_or_default();
            auto.push_str(&format!(
                "<text:list-level-style-number text:level=\"{}\" style:num-format=\"{}\"{prefix}{suffix}>{}</text:list-level-style-number>",
                level + 1, odf_number_format(label.format), level_props(level)
            ));
        }
        auto.push_str("</text:list-style>");
    }

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
    let mut open_style = String::new();
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
                    // A table starting a section names its master page.
                    let master = section_start.get(&pi).map(|k| master_attr(*k)).unwrap_or_default();
                    // Its edge sits its padding left of its text (the
                    // padding written below, Writer's none without it).
                    let indent = doc.table_indent.get(&c.table).copied().unwrap_or(0.0);
                    let margin = Some(indent - doc.table_padding.get(&c.table).map_or(0.0, |p| p.left_pt)).filter(|m| m.abs() > 0.005);
                    let margin_attr = margin.map(|m| format!(" fo:margin-left=\"{m:.2}pt\"")).unwrap_or_default();
                    let columns = match doc.table_columns.get(&c.table).filter(|w| w.len() == cols as usize) {
                        Some(widths) => {
                            let total: f64 = widths.iter().sum();
                            auto.push_str(&format!(
                                "<style:style style:name=\"Table{tables_written}\" style:family=\"table\"{master}><style:table-properties style:width=\"{total:.2}pt\"{margin_attr} table:align=\"{}\"/></style:style>",
                                // A share of the text's width fills it.
                                if doc.table_relative.contains(&c.table) { "margins" } else { "left" }
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
                        None => {
                            if margin.is_some() {
                                auto.push_str(&format!("<style:style style:name=\"Table{tables_written}\" style:family=\"table\"{master}><style:table-properties{margin_attr} table:align=\"margins\"/></style:style>"));
                                table_style = format!(" table:style-name=\"Table{tables_written}\"");
                            } else if !master.is_empty() {
                                auto.push_str(&format!("<style:style style:name=\"Table{tables_written}\" style:family=\"table\"{master}/>"));
                                table_style = format!(" table:style-name=\"Table{tables_written}\"");
                            }
                            format!("<table:table-column table:number-columns-repeated=\"{cols}\"/>")
                        }
                    };
                    // The file's row heights, as row styles: a minimum
                    // height, or a fixed one.
                    // A row kept on one page says so too.
                    let row_styles: Vec<String> = (0..rows as usize)
                        .map(|r| {
                            let height = doc.table_rows.get(&c.table).and_then(|h| h.get(r).copied().flatten()).map(|h| {
                                let prop = if h.exact { "style:row-height" } else { "style:min-row-height" };
                                format!(" {prop}=\"{:.2}pt\"", h.pt)
                            });
                            let whole = doc.table_whole_rows.get(&c.table).is_some_and(|w| w.contains(&(r as u32)));
                            if height.is_none() && !whole {
                                return String::new();
                            }
                            let name = format!("Table{tables_written}.R{r}");
                            auto.push_str(&format!(
                                "<style:style style:name=\"{name}\" style:family=\"table-row\"><style:table-row-properties{}{}/></style:style>",
                                height.unwrap_or_default(),
                                if whole { " fo:keep-together=\"always\"" } else { "" }
                            ));
                            format!(" table:style-name=\"{name}\"")
                        })
                        .collect();
                    body.push_str(&format!("<table:table table:name=\"Table{tables_written}\"{table_style}>{columns}"));
                    // The shaded cells, one table-cell style per colour,
                    // and the table's cell padding in every cell's style.
                    let padding = doc.table_padding.get(&c.table).map_or(String::new(), |p| {
                        format!(
                            " fo:padding-top=\"{:.4}pt\" fo:padding-bottom=\"{:.4}pt\" fo:padding-left=\"{:.4}pt\" fo:padding-right=\"{:.4}pt\"",
                            p.top_pt, p.bottom_pt, p.left_pt, p.right_pt
                        )
                    });
                    // A ruled table (any made here) rules every cell, as
                    // Writer draws a cell only with a border of its own.
                    let ruled = !doc.table_unruled.contains(&c.table);
                    let pad_only = padding.clone();
                    let color = doc.table_rule_colors.get(&c.table).map_or("000000", String::as_str);
                    let width = doc.table_rule_widths.get(&c.table).copied().unwrap_or(crate::layout::CELL_RULE_PT);
                    let padding = if ruled { format!("{padding} fo:border=\"{width}pt solid #{}\"", esc(color)) } else { padding };
                    let mut fill_styles: std::collections::HashMap<String, String> = Default::default();
                    let mut cell_styles: std::collections::HashMap<(u32, u32), String> = doc.table_fills.get(&c.table).into_iter().flatten().map(|f| {
                        let n = fill_styles.len();
                        let name = fill_styles.entry(f.color.clone()).or_insert_with(|| {
                            let name = format!("Table{tables_written}.F{n}");
                            auto.push_str(&format!(
                                "<style:style style:name=\"{name}\" style:family=\"table-cell\"><style:table-cell-properties fo:background-color=\"#{}\"{padding}/></style:style>",
                                esc(&f.color)
                            ));
                            name
                        });
                        ((f.row, f.col), format!(" table:style-name=\"{name}\""))
                    }).collect();
                    if !padding.is_empty() {
                        let name = format!("Table{tables_written}.P");
                        auto.push_str(&format!("<style:style style:name=\"{name}\" style:family=\"table-cell\"><style:table-cell-properties{padding}/></style:style>"));
                        for r in 0..rows {
                            for col in 0..cols {
                                cell_styles.entry((r, col)).or_insert_with(|| format!(" table:style-name=\"{name}\""));
                            }
                        }
                    }
                    // A cell with sides left open, or set lower than the
                    // top, has a style of its own naming each side and
                    // where its content sits.
                    let open_at = |row: u32, col: u32| doc.table_open_edges.get(&c.table).into_iter().flatten().find(|e| (e.row, e.col) == (row, col)).map(|e| e.open);
                    let valign_at = |row: u32, col: u32| doc.table_valign.get(&c.table).into_iter().flatten().find(|v| (v.row, v.col) == (row, col)).map(|v| v.align);
                    let mut own: Vec<(u32, u32)> = doc.table_open_edges.get(&c.table).into_iter().flatten().filter(|_| ruled).map(|e| (e.row, e.col))
                        .chain(doc.table_valign.get(&c.table).into_iter().flatten().map(|v| (v.row, v.col)))
                        .collect();
                    own.sort_unstable();
                    own.dedup();
                    for (n, (row, col)) in own.into_iter().enumerate() {
                        let name = format!("Table{tables_written}.E{n}");
                        let open = open_at(row, col).unwrap_or_default();
                        let side = |open: bool| if open || !ruled { "none" } else { "0.5pt solid #000000" };
                        let fill = doc.table_fills.get(&c.table).into_iter().flatten().find(|f| (f.row, f.col) == (row, col)).map_or(String::new(), |f| format!(" fo:background-color=\"#{}\"", esc(&f.color)));
                        let valign = match valign_at(row, col) {
                            Some(crate::model::VAlign::Center) => " style:vertical-align=\"middle\"",
                            Some(crate::model::VAlign::Bottom) => " style:vertical-align=\"bottom\"",
                            None => "",
                        };
                        auto.push_str(&format!(
                            "<style:style style:name=\"{name}\" style:family=\"table-cell\"><style:table-cell-properties{fill}{pad_only}{valign} fo:border-top=\"{}\" fo:border-bottom=\"{}\" fo:border-left=\"{}\" fo:border-right=\"{}\"/></style:style>",
                            side(open.top), side(open.bottom), side(open.left), side(open.right)
                        ));
                        cell_styles.insert((row, col), format!(" table:style-name=\"{name}\""));
                    }
                    let spans = doc.table_spans.get(&c.table).cloned().unwrap_or_default();
                    let mut t = OpenTable { id: c.table, rows, cols, row: 0, col: 0, filled: false, row_styles, cell_styles, spans };
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
        let list_style = match (kind, labelled.style[pi]) {
            (ListKind::Numbered, 0) => "LN".to_string(),
            (ListKind::Numbered, k) => format!("LN{k}"),
            _ => "LB".to_string(),
        };
        if kind != open_list || (kind == ListKind::Numbered && list_style != open_style) {
            body.push_str(&"</text:list-item></text:list>".repeat(depth));
            depth = 0;
            open_list = kind;
            open_style = list_style.clone();
        }
        if kind != ListKind::None {
            let target = usize::from(p.style.list_level) + 1;
            let start = p.style.list_start.or(labelled.start[pi]).map(|n| format!(" text:start-value=\"{n}\"")).unwrap_or_default();
            if depth == 0 {
                body.push_str(&format!("<text:list text:style-name=\"{list_style}\">"));
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
        let (style_name, own_auto) = if let Some(level) = p.style.toc {
            (Some(format!("Contents_20_{}", level.clamp(1, 10))), None)
        } else if let Some(name) = &p.style.named_style {
            (Some(esc(name)), None)
        } else if p.style.block_quote {
            (Some("Quotations".to_string()), None)
        } else if p.style.code_block.is_some() && p.style.heading.is_none() {
            (Some("Preformatted_20_Text".to_string()), None)
        } else {
            match (para_style_idx[pi], p.style.heading) {
                (Some(i), _) => (Some(format!("P{}", i + 1)), Some(i)),
                (None, Some(n)) => (Some(format!("Heading_20_{n}")), None),
                (None, None) => (None, None),
            }
        };
        // A paragraph starting a section has a style of its own naming the
        // section's master page, otherwise the one it would have: a copy of
        // its automatic style, or a child of its named one.
        let style_name = match section_start.get(&pi).filter(|_| cell.is_none()) {
            Some(&k) => {
                let name = format!("PS{k}");
                auto.push_str(&match own_auto {
                    Some(i) => para_auto(&para_autos[i], &name, &master_attr(k)),
                    None => {
                        let parent = style_name.map(|n| format!(" style:parent-style-name=\"{n}\"")).unwrap_or_default();
                        format!("<style:style style:name=\"{name}\" style:family=\"paragraph\"{parent}{}/>", master_attr(k))
                    }
                });
                Some(name)
            }
            None => style_name,
        };
        let style_attr = style_name.map(|n| format!(" text:style-name=\"{n}\"")).unwrap_or_default();

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
            if r.style.text_box.is_some() {
                text_boxes_written += 1;
                inner.push_str(&text_box_frame(r, text_boxes_written, &mut auto).unwrap_or_default());
                continue;
            }
            if r.style.image.is_some() {
                match picture_frame(r, pictures_written + 1, "fr", &mut auto) {
                    Some(frame) => {
                        pictures_written += 1;
                        inner.push_str(&frame);
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
            // A tab and a line break are elements in ODF: raw, they are
            // only whitespace.
            let mut run_xml = esc(&r.text).replace('\t', "<text:tab/>").replace('\n', "<text:line-break/>");
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

/// The master page section `k` names (LET-005S): the first section's is
/// "Standard", the one every reader gives a first page naming none.
fn section_master(k: usize) -> String {
    if k == 0 { "Standard".to_string() } else { format!("LettersSection{k}") }
}

/// The page layout section `k`'s master page names.
fn section_layout(k: usize) -> String {
    if k == 0 { "pm1".to_string() } else { format!("pmS{k}") }
}

/// A page layout's properties: the page's size, margins and columns, or
/// none (the application's page).
fn page_layout_properties(page: Option<&PageGeometry>) -> String {
    match page {
        Some(pg) => {
            // The model's margins run to the text; ODF's run to a page
            // border, with its rule and padding inside them.
            let inset = |side: usize| pg.border.map_or(0.0, |b| b.width_pt + b.padding_pt[side]);
            let border = pg.border.map_or(String::new(), |b| {
                format!(
                    " fo:border=\"{:.2}pt solid #{:06X}\" fo:padding-top=\"{:.2}pt\" fo:padding-right=\"{:.2}pt\" fo:padding-bottom=\"{:.2}pt\" fo:padding-left=\"{:.2}pt\"",
                    b.width_pt, b.color & 0xFF_FFFF, b.padding_pt[0], b.padding_pt[1], b.padding_pt[2], b.padding_pt[3]
                )
            });
            format!(
                "<style:page-layout-properties \
                 fo:page-width=\"{:.2}pt\" fo:page-height=\"{:.2}pt\" \
                 fo:margin-top=\"{:.2}pt\" fo:margin-bottom=\"{:.2}pt\" \
                 fo:margin-left=\"{:.2}pt\" fo:margin-right=\"{:.2}pt\"{border}>\
                 <style:columns fo:column-count=\"{}\" fo:column-gap=\"{:.2}pt\"/>\
                 </style:page-layout-properties>",
                pg.width_pt, pg.height_pt,
                (pg.margin_top_pt - inset(0)).max(0.0), (pg.margin_bottom_pt - inset(2)).max(0.0),
                (pg.margin_left_pt - inset(3)).max(0.0), (pg.margin_right_pt - inset(1)).max(0.0),
                pg.columns.max(1), pg.column_gap_pt
            )
        }
        None => "<style:page-layout-properties/>".to_string(),
    }
}

fn styles_xml(doc: &Document) -> String {
    // One page layout and master page per section (LET-005S), each master
    // with the header and footer its section's pages show; a document of
    // one section has "pm1" and "Standard", with its page, header and
    // footer.
    struct Master<'a> {
        name: String,
        layout: String,
        page: Option<PageGeometry>,
        header: (Option<&'a str>, bool),
        footer: (Option<&'a str>, bool),
    }
    let masters: Vec<Master> = if doc.sections.is_empty() {
        vec![Master { name: section_master(0), layout: section_layout(0), page: doc.page, header: (doc.header.as_deref(), true), footer: (doc.footer.as_deref(), true) }]
    } else {
        doc.sections
            .iter()
            .enumerate()
            .map(|(k, s)| Master {
                name: section_master(k),
                layout: section_layout(k),
                page: Some(s.geometry),
                header: crate::sections::header(doc, k),
                footer: crate::sections::footer(doc, k),
            })
            .collect()
    };
    let mut hf = String::new();
    // A header's or footer's pictures are frames at the start of its
    // paragraph, a floating one placed by a graphic style of styles.xml's.
    let mut graphic = String::new();
    let mut para_styles = String::new();
    let mut frames = |pictures: &[Run], prefix: &str| -> String {
        pictures.iter().enumerate().filter_map(|(k, r)| picture_frame(r, k + 1, prefix, &mut graphic)).collect()
    };
    // Whether a master has a header (or footer): the document's own when
    // it has text, even empty, or pictures; a section's own text unless
    // it is "" (no header).
    let shows = |story: (Option<&str>, bool), pictures: &[Run]| {
        if story.1 { story.0.is_some() || !pictures.is_empty() } else { story.0.is_some_and(|t| !t.is_empty()) }
    };
    // A section's header and footer sit in its margins as Writer lays a
    // page out: the page margin runs to the header, whose minimum height
    // keeps the body where the section's margin (Word's, the model's)
    // puts it; the reader adds it back. A document of one section writes
    // its page as before.
    // The page's own distance to its header or footer, else the layout's.
    let defaults = crate::layout::LayoutOptions::default();
    let layouts: String = masters
        .iter()
        .map(|m| {
            let room = |shown: bool, margin: f64, distance: f64| (!doc.sections.is_empty() && shown && margin > distance).then_some((margin - distance, distance));
            let header = m.page.and_then(|p| room(shows(m.header, &doc.header_pictures), p.margin_top_pt, p.header_pt.unwrap_or(defaults.header_distance_pt)));
            let footer = m.page.and_then(|p| room(shows(m.footer, &doc.footer_pictures), p.margin_bottom_pt, p.footer_pt.unwrap_or(defaults.footer_distance_pt)));
            let page = m.page.map(|mut p| {
                if let Some((_, distance)) = header {
                    p.margin_top_pt = distance;
                }
                if let Some((_, distance)) = footer {
                    p.margin_bottom_pt = distance;
                }
                p
            });
            let mut out = format!("<style:page-layout style:name=\"{}\">{}", m.layout, page_layout_properties(page.as_ref()));
            if let Some((h, _)) = header {
                out.push_str(&format!("<style:header-style><style:header-footer-properties fo:min-height=\"{h:.2}pt\" fo:margin-bottom=\"0pt\"/></style:header-style>"));
            }
            if let Some((h, _)) = footer {
                out.push_str(&format!("<style:footer-style><style:header-footer-properties fo:min-height=\"{h:.2}pt\" fo:margin-top=\"0pt\"/></style:footer-style>"));
            }
            out.push_str("</style:page-layout>");
            out
        })
        .collect();
    let own_shown = masters.iter().any(|m| (m.header.1 && shows(m.header, &doc.header_pictures)) || (m.footer.1 && shows(m.footer, &doc.footer_pictures)));
    // An aligned header's or footer's paragraph names a style of its own.
    let aligned = |name: &str, a: Alignment, styles: &mut String| -> String {
        let align = match a {
            Alignment::Left => return String::new(),
            Alignment::Center => "center",
            Alignment::Right => "end",
            Alignment::Justify => "justify",
        };
        styles.push_str(&format!("<style:style style:name=\"{name}\" style:family=\"paragraph\"><style:paragraph-properties fo:text-align=\"{align}\"/></style:style>"));
        format!(" text:style-name=\"{name}\"")
    };
    let (header_style, footer_style) = if own_shown {
        let header = aligned("LettersHeader", doc.header_alignment, &mut para_styles);
        (header, aligned("LettersFooter", doc.footer_alignment, &mut para_styles))
    } else {
        (String::new(), String::new())
    };
    // Each line of the document's own header and footer with a look of
    // its own names a style of its own: its alignment, tab stops and the
    // look of its text.
    // A floating line is a frame: a text box the page's width, set against
    // its top or bottom by a graphic style of its own, its text inset by
    // the style's padding. The frame's opening markup goes with the line.
    let page_w = doc.page.unwrap_or_default().width_pt;
    let line_styles = |prefix: &str, lines: &[crate::model::HfLine], shown: bool, styles: &mut String| -> Vec<(String, Option<String>)> {
        if !shown {
            return Vec::new();
        }
        lines
            .iter()
            .enumerate()
            .map(|(k, line)| {
                let name = format!("{prefix}{}", k + 1);
                let align = match line.alignment {
                    Alignment::Left => "",
                    Alignment::Center => " fo:text-align=\"center\"",
                    Alignment::Right => " fo:text-align=\"end\"",
                    Alignment::Justify => " fo:text-align=\"justify\"",
                };
                let tabs = para_style_children(&ParaStyle { tab_stops_pt: line.tab_stops.clone(), ..Default::default() });
                // Its spacing spelled out, both sides: a style that sets
                // one has none of the default style's other.
                let mut spacing = format!(" fo:margin-top=\"{:.2}pt\" fo:margin-bottom=\"{:.2}pt\"", line.space_before_pt, line.space_after_pt);
                match line.line_height {
                    Some(LineHeight::Exact(pt)) => spacing.push_str(&format!(" fo:line-height=\"{pt:.2}pt\"")),
                    Some(LineHeight::Minimum(pt)) => spacing.push_str(&format!(" style:line-height-at-least=\"{pt:.2}pt\"")),
                    None => {}
                }
                styles.push_str(&format!(
                    "<style:style style:name=\"{name}\" style:family=\"paragraph\"><style:paragraph-properties{align}{spacing}>{tabs}</style:paragraph-properties><style:text-properties{}/></style:style>",
                    run_style_props(&line.look)
                ));
                let frame = line.float.map(|f| {
                    let (v_pos, top, bottom) = match f {
                        crate::model::HfFloat::Top(d) => ("top", d, 0.0),
                        crate::model::HfFloat::Bottom(d) => ("bottom", 0.0, d),
                    };
                    let h_pos = match line.alignment {
                        Alignment::Center => "center",
                        Alignment::Right => "right",
                        _ => "left",
                    };
                    styles.push_str(&format!(
                        "<style:style style:name=\"{name}Box\" style:family=\"graphic\"><style:graphic-properties style:vertical-pos=\"{v_pos}\" style:vertical-rel=\"page\" style:horizontal-pos=\"{h_pos}\" style:horizontal-rel=\"page\" fo:padding-top=\"{top:.2}pt\" fo:padding-bottom=\"{bottom:.2}pt\" fo:padding-left=\"0pt\" fo:padding-right=\"0pt\" fo:border=\"none\" draw:stroke=\"none\" draw:fill=\"none\" draw:textarea-vertical-align=\"{v_pos}\" style:wrap=\"run-through\" style:run-through=\"foreground\"/></style:style>"
                    ));
                    let height = line.look.font_size_hp.map_or(12.0, |hp| f64::from(hp) / 2.0) * 1.25 + top + bottom;
                    format!("<draw:frame draw:style-name=\"{name}Box\" draw:name=\"{name}Box\" text:anchor-type=\"paragraph\" svg:width=\"{page_w:.2}pt\" svg:height=\"{height:.2}pt\" draw:z-index=\"0\"><draw:text-box>")
                });
                (format!(" text:style-name=\"{name}\""), frame)
            })
            .collect()
    };
    let header_lines = line_styles("LettersHeaderLine", &doc.header_lines, own_shown, &mut para_styles);
    let footer_lines = line_styles("LettersFooterLine", &doc.footer_lines, own_shown, &mut para_styles);
    // The first page's own header and footer, on the first master: their
    // alignment, lines and pictures named apart from the others'.
    let mut first_page = [String::new(), String::new()];
    let [first_header_out, first_footer_out] = &mut first_page;
    for (out, story, element, prefix) in [(first_header_out, &doc.first_header, "style:header-first", "LettersFirstHeader"), (first_footer_out, &doc.first_footer, "style:footer-first", "LettersFirstFooter")] {
        let Some(story) = story else { continue };
        if story.text.is_empty() && story.pictures.is_empty() {
            *out = format!("<{element}/>");
            continue;
        }
        let style = aligned(prefix, story.alignment, &mut para_styles);
        let lines = line_styles(&format!("{prefix}Line"), &story.lines, true, &mut para_styles);
        let pics = frames(&story.pictures, &format!("{prefix}Fr"));
        *out = format!("<{element}>{}</{element}>", hf_paragraphs(&story.text, &style, &lines, &pics));
    }
    hf.push_str("<office:master-styles>");
    for (k, m) in masters.iter().enumerate() {
        let header = shows(m.header, &doc.header_pictures);
        let footer = shows(m.footer, &doc.footer_pictures);
        let [first_header, first_footer] = if k == 0 { std::mem::take(&mut first_page) } else { Default::default() };
        if !header && !footer && first_header.is_empty() && first_footer.is_empty() {
            hf.push_str(&format!("<style:master-page style:name=\"{}\" style:page-layout-name=\"{}\"/>", m.name, m.layout));
            continue;
        }
        hf.push_str(&format!("<style:master-page style:name=\"{}\" style:page-layout-name=\"{}\">", m.name, m.layout));
        // The pictures and alignment are the document's own header's and
        // footer's; a later master repeating them names its frames' styles
        // apart from the first's.
        let suffix = if k == 0 { String::new() } else { format!("{k}_") };
        if header {
            let (pics, style, lines) = if m.header.1 { (frames(&doc.header_pictures, &format!("hfr{suffix}")), header_style.as_str(), header_lines.as_slice()) } else { (String::new(), "", &[][..]) };
            hf.push_str(&format!("<style:header>{}</style:header>", hf_paragraphs(m.header.0.unwrap_or(""), style, lines, &pics)));
        }
        hf.push_str(&first_header);
        if footer {
            let (pics, style, lines) = if m.footer.1 { (frames(&doc.footer_pictures, &format!("ffr{suffix}")), footer_style.as_str(), footer_lines.as_slice()) } else { (String::new(), "", &[][..]) };
            hf.push_str(&format!("<style:footer>{}</style:footer>", hf_paragraphs(m.footer.0.unwrap_or(""), style, lines, &pics)));
        }
        hf.push_str(&first_footer);
        hf.push_str("</style:master-page>");
    }
    hf.push_str("</office:master-styles>");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <office:document-styles \
         xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
         xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
         xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" \
         xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" \
         xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" \
         xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         office:version=\"1.2\">\
         <office:styles>{contents}</office:styles>\
         <office:automatic-styles>\
         {layouts}{graphic}{para_styles}</office:automatic-styles>{hf}\
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
    let runs = doc.paragraphs.iter().flat_map(|p| &p.runs).chain(&doc.header_pictures).chain(&doc.footer_pictures).chain([&doc.first_header, &doc.first_footer].into_iter().flatten().flat_map(|s| &s.pictures));
    for src in runs.filter_map(|r| r.style.image.as_deref()) {
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
pub fn write_with_opaque(doc: &Document, path: impl AsRef<std::path::Path>, opaque: &suite_common_core::interop::OpaquePackage) -> Result<(), String> {
    let path = path.as_ref();
    crate::save::opaque_single_write(path, |p| write(doc, p), opaque)
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
    /// Paragraph styles that say whether their paragraphs keep their
    /// spacing beside another of their style (`style:contextual-spacing`).
    contextual: std::collections::HashMap<String, bool>,
    /// A text style's font declaration (`style:font-name`), which Writer
    /// names a span's face by: a header's or footer's line takes it.
    text_font_name: std::collections::HashMap<String, String>,
    /// A paragraph style's own text properties: in styles.xml, how each
    /// `Heading_20_N` looks (#1297).
    para_text: std::collections::HashMap<String, RunStyle>,
    /// A table-column style's width, in points.
    column: std::collections::HashMap<String, f64>,
    /// A table-row style's height.
    row: std::collections::HashMap<String, crate::model::RowHeight>,
    /// A table-cell style's background colour, six hex digits.
    cell_fill: std::collections::HashMap<String, String>,
    /// Table-cell styles with a visible border on any side.
    cell_ruled: std::collections::HashSet<String>,
    /// A ruled cell style's rule colour, when not black.
    cell_rule_color: std::collections::HashMap<String, String>,
    /// A ruled cell style's rule width in points, when not 0.5pt.
    cell_rule_width: std::collections::HashMap<String, f64>,
    /// A ruled table-cell style's sides without a border.
    cell_open: std::collections::HashMap<String, crate::model::OpenEdges>,
    /// A table-cell style's content set lower than the top.
    cell_valign: std::collections::HashMap<String, crate::model::VAlign>,
    /// A graphic style's placement of a floating frame.
    graphic: std::collections::HashMap<String, crate::model::ImageAnchor>,
    /// Table-row styles that keep their row on one page
    /// (`fo:keep-together="always"`).
    row_whole: std::collections::HashSet<String>,
    /// Table styles that size the table as a share of the text's width
    /// (`style:rel-width`, or `table:align="margins"`).
    table_relative: std::collections::HashSet<String>,
    /// Table styles' `fo:margin-left`, in points: how far right of the
    /// text's edge the table's edge sits.
    table_margin: std::collections::HashMap<String, f64>,
    /// A graphic style's padding above and below its text, in points: a
    /// text box's inset.
    graphic_padding: std::collections::HashMap<String, (f64, f64)>,
    /// A graphic style's room left and right of its frame, in points
    /// (`fo:margin-left`, `fo:margin-right`).
    graphic_sides: std::collections::HashMap<String, (f64, f64)>,
    /// A graphic style's frame for a text box: its rule, its fill (a
    /// solid `draw:fill`) and its padding, in EMU.
    graphic_box: std::collections::HashMap<String, BoxLook>,
    /// A table-cell style's padding, when it gives one.
    cell_padding: std::collections::HashMap<String, crate::model::CellPadding>,
    /// A paragraph or table style's `style:master-page-name`: the master
    /// page a block with the style starts a new page on ("" names none).
    master: std::collections::HashMap<String, String>,
}

/// Paragraph-level values read off one automatic style. Lengths are points.
#[derive(Clone, Debug, Default)]
struct AutoParaStyle {
    alignment: Alignment,
    page_break_before: bool,
    keep_with_next: bool,
    keep_lines: bool,
    widow_control: Option<bool>,
    line_spacing: f32,
    line_height: Option<LineHeight>,
    shading: Option<String>,
    borders: ParaBorders,
    space_before_pt: f64,
    space_after_pt: f64,
    left_indent_pt: f64,
    right_indent_pt: f64,
    first_line_indent_pt: f64,
    /// `style:tab-stops` children, in document order.
    tab_stops_pt: Vec<TabStop>,
    /// Whether the style itself sets its space before, space after and
    /// line height; what it leaves unset comes from its parent style.
    sets: [bool; 3],
    /// Whether it sets its left, right and first-line indents.
    sets_indent: [bool; 3],
}

/// Note the merged cell `e` opens at the current cell of `table` (id,
/// row, column, …), if it spans more than one row or column. A covered
/// cell spans nothing.
fn note_span(spans: &mut std::collections::BTreeMap<u32, Vec<CellSpan>>, e: &quick_xml::events::BytesStart, table: &(u32, i64, i64, usize, bool)) {
    if e.name().as_ref() != "table:table-cell" {
        return;
    }
    let n = |a: &str| attr_val(e, a).and_then(|v| v.parse::<u32>().ok()).unwrap_or(1).clamp(1, 1024);
    let (cols, rows) = (n("table:number-columns-spanned"), n("table:number-rows-spanned"));
    if (cols > 1 || rows > 1) && table.1 >= 0 && table.2 >= 0 {
        spans.entry(table.0).or_default().push(CellSpan { row: table.1 as u32, col: table.2 as u32, rows, cols });
    }
}

/// Each `text:list-style` in `xml`: the label of each numbered level that
/// has one other than "N." (`style:num-prefix`, `style:num-format` and
/// `style:num-suffix`), by level, 0-based.
fn list_style_labels(xml: &str) -> std::collections::HashMap<String, Vec<Option<ListLabel>>> {
    let mut out: std::collections::HashMap<String, Vec<Option<ListLabel>>> = Default::default();
    let mut reader = Reader::from_str(xml);
    let mut current: Option<String> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                "text:list-style" => current = attr_val(&e, "style:name"),
                "text:list-level-style-number" => {
                    let (Some(style), Some(level)) = (current.as_ref(), attr_val(&e, "text:level").and_then(|l| l.parse::<usize>().ok())) else { continue };
                    if !(1..=10).contains(&level) {
                        continue;
                    }
                    let format = match attr_val(&e, "style:num-format").as_deref() {
                        Some("a") => NumberFormat::LowerLetter,
                        Some("A") => NumberFormat::UpperLetter,
                        Some("i") => NumberFormat::LowerRoman,
                        Some("I") => NumberFormat::UpperRoman,
                        Some("") | None => NumberFormat::None,
                        _ => NumberFormat::Decimal,
                    };
                    let label = ListLabel {
                        prefix: attr_val(&e, "style:num-prefix").unwrap_or_default(),
                        format,
                        suffix: attr_val(&e, "style:num-suffix").unwrap_or_default(),
                    };
                    let labels = out.entry(style.clone()).or_default();
                    if labels.len() < level {
                        labels.resize(level, None);
                    }
                    labels[level - 1] = (label != ListLabel { suffix: ".".into(), ..Default::default() }).then_some(label);
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
                        // A number level with no digits is still a label
                        // when it has text: Word's "2.1" with no number.
                        let has_text = |a: &str| attr_val(&e, a).is_some_and(|t| !t.is_empty());
                        let numbered = name == "text:list-level-style-number"
                            && (has_text("style:num-format") || has_text("style:num-prefix") || has_text("style:num-suffix"));
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

/// Each `text:list-style` in `xml`: where each level puts its text, in
/// points from the paragraph's indent (`fo:margin-left` of its
/// label-alignment properties, Word's 0.5in for a first level), by level,
/// 0-based. None where a level declares no position.
fn list_style_indents(xml: &str) -> std::collections::HashMap<String, Vec<Option<f64>>> {
    let mut out: std::collections::HashMap<String, Vec<Option<f64>>> = Default::default();
    let mut reader = Reader::from_str(xml);
    let mut current: Option<String> = None;
    let mut level: Option<usize> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                "text:list-style" => current = attr_val(&e, "style:name"),
                "text:list-level-style-number" | "text:list-level-style-bullet" | "text:list-level-style-image" => {
                    level = attr_val(&e, "text:level").and_then(|l| l.parse::<usize>().ok()).filter(|l| (1..=10).contains(l));
                }
                "style:list-level-label-alignment" => {
                    let (Some(style), Some(l)) = (current.as_ref(), level) else { continue };
                    if let Some(margin) = attr_val(&e, "fo:margin-left").and_then(|v| parse_length_pt(&v)) {
                        let levels = out.entry(style.clone()).or_default();
                        if levels.len() < l {
                            levels.resize(l, None);
                        }
                        levels[l - 1] = Some(margin);
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

/// The key `style:default-style` (paragraph family) is kept under among
/// the named styles: the root every paragraph style's chain ends at.
const DEFAULT_PARA: &str = "\u{0}default-paragraph";

fn parse_auto_styles(xml: &str) -> AutoStyles {
    let mut out = AutoStyles { text: Default::default(), para: Default::default(), para_parent: Default::default(), text_parent: Default::default(), contextual: Default::default(), text_font_name: Default::default(), graphic_padding: Default::default(), graphic_sides: Default::default(), graphic_box: Default::default(), table_relative: Default::default(), table_margin: Default::default(), para_text: Default::default(), column: Default::default(), row: Default::default(), cell_fill: Default::default(), cell_ruled: Default::default(), cell_rule_color: Default::default(), cell_rule_width: Default::default(), cell_open: Default::default(), cell_valign: Default::default(), graphic: Default::default(), master: Default::default(), cell_padding: Default::default(), row_whole: Default::default() };
    let mut reader = Reader::from_str(xml);
    let mut cur_name: Option<String> = None;
    let mut cur_family = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                match e.name().as_ref() {
                    "style:default-style" => {
                        cur_family = attr_val(&e, "style:family").unwrap_or_default();
                        cur_name = (cur_family == "paragraph").then(|| DEFAULT_PARA.to_string());
                    }
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
                        if let (Some(n), Some(m)) = (cur_name.clone(), attr_val(&e, "style:master-page-name")) {
                            out.master.insert(n, m);
                        }
                    }
                    "style:table-column-properties" => {
                        if let (Some(name), Some(w)) = (cur_name.clone(), attr_val(&e, "style:column-width").and_then(|v| parse_length_pt(&v))) {
                            out.column.insert(name, w);
                        }
                    }
                    "style:graphic-properties" if cur_family == "graphic" => {
                        if let Some(name) = cur_name.clone() {
                            out.graphic.insert(name.clone(), graphic_placement(&e));
                            let pad = |a: &str| attr_val(&e, a).or_else(|| attr_val(&e, "fo:padding")).and_then(|v| parse_length_pt(&v)).unwrap_or(0.0);
                            out.graphic_padding.insert(name.clone(), (pad("fo:padding-top"), pad("fo:padding-bottom")));
                            let side = |a: &str| attr_val(&e, a).and_then(|v| parse_length_pt(&v)).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(0.0);
                            out.graphic_sides.insert(name.clone(), (side("fo:margin-left"), side("fo:margin-right")));
                            out.graphic_box.insert(name, box_look(&e));
                        }
                    }
                    "style:table-properties" if cur_family == "table" => {
                        if let Some(name) = cur_name.clone() {
                            if let Some(m) = attr_val(&e, "fo:margin-left").and_then(|v| parse_length_pt(&v)).filter(|m| m.is_finite()) {
                                out.table_margin.insert(name.clone(), m);
                            }
                            if attr_val(&e, "style:rel-width").is_some() || attr_val(&e, "table:align").as_deref() == Some("margins") {
                                out.table_relative.insert(name);
                            }
                        }
                    }
                    "style:table-cell-properties" => {
                        // fo:padding for every side, a side's own over it;
                        // a side neither names has none (ODF's default).
                        let side = |a: &str| attr_val(&e, a).and_then(|v| parse_length_pt(&v)).filter(|v| v.is_finite() && *v >= 0.0);
                        let all = side("fo:padding");
                        let sides = ["fo:padding-top", "fo:padding-bottom", "fo:padding-left", "fo:padding-right"].map(|a| side(a).or(all));
                        if let (Some(name), true) = (cur_name.clone(), sides.iter().any(Option::is_some)) {
                            let [top_pt, bottom_pt, left_pt, right_pt] = sides.map(|v| v.unwrap_or(0.0));
                            out.cell_padding.insert(name, crate::model::CellPadding { top_pt, bottom_pt, left_pt, right_pt });
                        }
                        let ruled = ["fo:border", "fo:border-top", "fo:border-bottom", "fo:border-left", "fo:border-right"]
                            .iter()
                            .filter_map(|a| attr_val(&e, a))
                            .any(|v| !v.trim().eq_ignore_ascii_case("none") && parse_length_pt(v.split_whitespace().next().unwrap_or("")).is_none_or(|w| w > 0.0));
                        if let (Some(name), true) = (cur_name.clone(), ruled) {
                            // A side is ruled by its own border, else by
                            // fo:border; neither, or "none", leaves it open.
                            let visible = |v: &str| !v.trim().eq_ignore_ascii_case("none") && parse_length_pt(v.split_whitespace().next().unwrap_or("")).is_none_or(|w| w > 0.0);
                            let all = attr_val(&e, "fo:border");
                            let open = |a: &str| !attr_val(&e, a).or_else(|| all.clone()).is_some_and(|v| visible(&v));
                            let open = crate::model::OpenEdges { top: open("fo:border-top"), bottom: open("fo:border-bottom"), left: open("fo:border-left"), right: open("fo:border-right") };
                            if open.any() {
                                out.cell_open.insert(name.clone(), open);
                            }
                            // The first visible border's colour and width
                            // ("0.5pt solid #bfbfbf").
                            let first = ["fo:border", "fo:border-top", "fo:border-left", "fo:border-bottom", "fo:border-right"].iter().filter_map(|a| attr_val(&e, a)).find(|v| visible(v));
                            let color = first
                                .as_deref()
                                .and_then(|v| v.split_whitespace().find_map(|w| w.strip_prefix('#').map(str::to_ascii_uppercase)))
                                .filter(|c| c.len() == 6 && c.bytes().all(|b| b.is_ascii_hexdigit()) && c != "000000");
                            if let Some(color) = color {
                                out.cell_rule_color.insert(name.clone(), color);
                            }
                            let width = first.as_deref().and_then(|v| v.split_whitespace().find_map(parse_length_pt)).filter(|w| *w > 0.0 && (w - crate::layout::CELL_RULE_PT).abs() > 1e-3);
                            if let Some(width) = width {
                                out.cell_rule_width.insert(name.clone(), width);
                            }
                            out.cell_ruled.insert(name);
                        }
                        let valign = match attr_val(&e, "style:vertical-align").as_deref() {
                            Some("middle") => Some(crate::model::VAlign::Center),
                            Some("bottom") => Some(crate::model::VAlign::Bottom),
                            _ => None,
                        };
                        if let (Some(name), Some(v)) = (cur_name.clone(), valign) {
                            out.cell_valign.insert(name, v);
                        }
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
                        if let Some(name) = cur_name.clone().filter(|_| attr_val(&e, "fo:keep-together").as_deref() == Some("always")) {
                            out.row_whole.insert(name);
                        }
                    }
                    "style:text-properties" => {
                        if let Some(name) = cur_name.clone().filter(|_| cur_family == "text" || cur_family == "paragraph") {
                            let mut st = RunStyle::default();
                            match attr_val(&e, "fo:font-weight").as_deref() {
                                Some("bold") => st.bold = true,
                                // Undoes a parent's bold, and a heading's.
                                Some("normal") => st.regular = true,
                                _ => {}
                            }
                            if attr_val(&e, "fo:font-style").as_deref() == Some("italic") {
                                st.italic = true;
                            }
                            if attr_val(&e, "fo:text-transform").as_deref() == Some("uppercase") {
                                st.caps = true;
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
                            } else if attr_val(&e, "style:use-window-font-color").as_deref() == Some("true") {
                                // The automatic (window) colour undoes a
                                // parent's: the W1 notes' headings are
                                // black over Heading 1's blue, as
                                // LibreOffice draws them. Black, as the
                                // DOCX reader takes "auto" in a heading.
                                st.color = Some("000000".into());
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
                                // A span's too: the statistical release's
                                // Verdana is named only so.
                                if let Some(fname) = attr_val(&e, "style:font-name").filter(|_| !st.code) {
                                    if st.font_family.is_none() {
                                        st.font_family = Some(fname.clone());
                                    }
                                    out.text_font_name.insert(name.clone(), fname);
                                }
                                out.text.insert(name, st);
                            }
                        }
                    }
                    "style:paragraph-properties" => {
                        if let (Some(name), "paragraph") = (cur_name.clone(), cur_family.as_str()) {
                            if let Some(v) = attr_val(&e, "style:contextual-spacing") {
                                out.contextual.insert(name.clone(), v == "true");
                            }
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
                                keep_lines: attr_val(&e, "fo:keep-together").as_deref() == Some("always"),
                                widow_control: {
                                    let widows = attr_val(&e, "fo:widows").and_then(|v| v.parse::<u32>().ok());
                                    let orphans = attr_val(&e, "fo:orphans").and_then(|v| v.parse::<u32>().ok());
                                    match (widows, orphans) {
                                        (None, None) => None,
                                        (w, o) => Some(w.unwrap_or(2) > 1 || o.unwrap_or(2) > 1),
                                    }
                                },
                                line_spacing: spacing,
                                line_height: attr_val(&e, "style:line-height-at-least")
                                    .and_then(|v| parse_length_pt(&v))
                                    .map(LineHeight::Minimum)
                                    .or_else(|| {
                                        attr_val(&e, "fo:line-height")
                                            .filter(|v| !v.trim_end().ends_with('%'))
                                            .and_then(|v| parse_length_pt(&v))
                                            .map(LineHeight::Exact)
                                    }),
                                shading: attr_val(&e, "fo:background-color").and_then(|c| {
                                    let c = c.trim_start_matches('#').to_uppercase();
                                    if c.len() == 6 && c.bytes().all(|b| b.is_ascii_hexdigit()) {
                                        Some(c)
                                    } else {
                                        None
                                    }
                                }),
                                borders: {
                                    let all = attr_val(&e, "fo:border").as_deref().and_then(parse_para_border);
                                    let side = |name: &str| {
                                        attr_val(&e, name).as_deref().and_then(parse_para_border).or_else(|| all.clone())
                                    };
                                    ParaBorders {
                                        top: side("fo:border-top"),
                                        left: side("fo:border-left"),
                                        bottom: side("fo:border-bottom"),
                                        right: side("fo:border-right"),
                                    }
                                },
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
                                // The space above and below are one
                                // setting in Writer: a style that sets
                                // either has none of its parent's other
                                // (a heading's space above, and no
                                // space after from Normal).
                                sets: [
                                    ["fo:margin-top", "fo:space-before", "fo:margin-bottom", "fo:space-after"].iter().any(|a| attr_val(&e, a).is_some()),
                                    ["fo:margin-top", "fo:space-before", "fo:margin-bottom", "fo:space-after"].iter().any(|a| attr_val(&e, a).is_some()),
                                    attr_val(&e, "fo:line-height").is_some(),
                                ],
                                sets_indent: ["fo:margin-left", "fo:margin-right", "fo:text-indent"].map(|a| attr_val(&e, a).is_some()),
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
                                let align = match attr_val(&e, "style:type").as_deref() {
                                    Some("center") => TabAlign::Center,
                                    Some("right") => TabAlign::Right,
                                    Some("char") => TabAlign::Decimal,
                                    _ => TabAlign::Left,
                                };
                                let text = attr_val(&e, "style:leader-text")
                                    .or_else(|| attr_val(&e, "style:leader-char"));
                                let leader = match (
                                    attr_val(&e, "style:leader-style").as_deref(),
                                    text.as_deref(),
                                ) {
                                    (_, Some(".")) => TabLeader::Dot,
                                    (_, Some("·")) => TabLeader::MiddleDot,
                                    (_, Some("-")) => TabLeader::Hyphen,
                                    (_, Some("_")) => TabLeader::Underscore,
                                    (Some("dotted"), _) => TabLeader::Dot,
                                    (Some("dashed"), _) => TabLeader::Hyphen,
                                    (Some("solid"), _) => TabLeader::Underscore,
                                    _ => TabLeader::None,
                                };
                                if let Some(st) = out.para.get_mut(&name) {
                                    st.tab_stops_pt.push(TabStop { pos_pt: pos, align, leader });
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

/// One `fo:border-*` value ("0.75pt solid #000000") as a modelled
/// border, if its look is one the model keeps. `none` (and anything
/// else unmodelled: groove, ridge, inset, wave) reads as no border
/// here, and the save dialog names it (`loss`), never a silent
/// restyle.
fn parse_para_border(v: &str) -> Option<ParaBorder> {
    let mut parts = v.split_whitespace();
    let (width, style, color) = (parts.next()?, parts.next()?, parts.next()?);
    let style = match style {
        "solid" => BorderStyle::Single,
        "dashed" => BorderStyle::Dashed,
        "dotted" => BorderStyle::Dotted,
        "double" => BorderStyle::Double,
        _ => return None,
    };
    let width_pt = parse_length_pt(width)?;
    let color = color.trim_start_matches('#').to_uppercase();
    if color.len() != 6 || !color.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(ParaBorder { style, color, width_pt, space_pt: 0.0 })
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
/// A header's or footer's text as its paragraphs, one a line, each in
/// `style`; the pictures' frames sit in the first.
fn hf_paragraphs(text: &str, style: &str, lines: &[(String, Option<String>)], pics: &str) -> String {
    // A floating line's frame goes in the paragraph of the next line that
    // is not (or one of its own after the last), where a reader finds it
    // ahead of that line.
    let mut out = String::new();
    let mut frames = String::new();
    let mut first = true;
    for (k, line) in text.split('\n').enumerate() {
        let (style, frame) = lines.get(k).map_or((style, None), |(s, f)| (s.as_str(), f.as_deref()));
        let content = page_fields(&odf_spaces(&esc(line)));
        if let Some(frame) = frame {
            frames.push_str(&format!("{frame}<text:p{style}>{content}</text:p></draw:text-box></draw:frame>"));
            continue;
        }
        let pics = if std::mem::take(&mut first) { pics } else { "" };
        out.push_str(&format!("<text:p{style}>{pics}{}{content}</text:p>", std::mem::take(&mut frames)));
    }
    if !frames.is_empty() || first {
        out.push_str(&format!("<text:p>{}{frames}</text:p>", if first { pics } else { "" }));
    }
    out
}

/// Text with its tabs and its runs of spaces as ODF's elements: raw, they
/// are whitespace a reader collapses to one space.
fn odf_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut spaces = 0;
    // A run of spaces is one element: a space left as text beside an
    // element (a page field) is whitespace a reader drops.
    let flush = |out: &mut String, n: usize, at_start: bool| match n {
        0 => {}
        1 if !at_start => out.push(' '),
        n => out.push_str(&format!("<text:s text:c=\"{n}\"/>")),
    };
    for c in text.chars() {
        if c == ' ' {
            spaces += 1;
            continue;
        }
        let at_start = out.is_empty();
        flush(&mut out, spaces, at_start);
        spaces = 0;
        if c == '\t' {
            out.push_str("<text:tab/>");
        } else {
            out.push(c);
        }
    }
    let at_start = out.is_empty();
    flush(&mut out, spaces, at_start);
    out
}

/// Text of a header or footer added to its line; a later cell of a table
/// row (`gap`) follows the line's text after a space.
fn append_hf(target: &mut Option<String>, txt: &str, gap: bool) {
    let t = target.get_or_insert_with(String::new);
    if gap && !t.is_empty() && !t.ends_with('\n') {
        t.push(' ');
    }
    t.push_str(txt);
}

/// Whether a header's or footer's text so far holds nothing to show.
fn blank(text: &Option<String>) -> bool {
    text.as_deref().is_none_or(|t| t.trim().is_empty())
}

/// The line a header's or footer's paragraph of style `style` starts: its
/// alignment and tab stops, and the look of its text, through the styles
/// it is based on (`named`: styles.xml's, its automatic ones among them).
fn hf_line(style: Option<&str>, named: &AutoStyles) -> crate::model::HfLine {
    let Some(name) = style else { return Default::default() };
    let mut line = crate::model::HfLine { look: para_text_style(name, named, named), ..Default::default() };
    let (mut aligned, mut tabbed) = (false, false);
    let mut cur = Some(name.to_string());
    for _ in 0..8 {
        let Some(n) = cur else { break };
        if let Some(st) = named.para.get(&n) {
            if !aligned && st.alignment != Alignment::Left {
                (line.alignment, aligned) = (st.alignment, true);
            }
            if !tabbed && !st.tab_stops_pt.is_empty() {
                (line.tab_stops, tabbed) = (st.tab_stops_pt.clone(), true);
            }
        }
        cur = named.para_parent.get(&n).cloned();
    }
    let (before, after, _) = inherited_spacing(name, named, named);
    (line.space_before_pt, line.space_after_pt) = (before.unwrap_or(0.0), after.unwrap_or(0.0));
    line.line_height = inherited_para(name, named, named).line_height;
    line
}

/// A text box of the document's own header or footer being read (a
/// classification label): its lines, each with its text, look and whether
/// that look has its first text's yet, and where the box floats. Its lines
/// go ahead of the line of the paragraph that anchors it.
struct HfBox {
    lines: Vec<(String, crate::model::HfLine, bool)>,
    float: Option<crate::model::HfFloat>,
    across: Option<Alignment>,
}

/// Text of a header's or footer's text box added to its last line (a
/// line of its own if it has none yet); its first text gives the line its
/// span's look.
fn hf_box_text(b: &mut HfBox, txt: &str, spans: &[Option<String>], named: &AutoStyles) {
    if b.lines.is_empty() {
        b.lines.push((String::new(), Default::default(), false));
    }
    let (text, line, styled) = b.lines.last_mut().expect("a line");
    if !*styled && !txt.trim().is_empty() {
        *styled = true;
        if let Some(span) = spans.iter().rev().flatten().next() {
            hf_span_look(&mut line.look, span, named);
        }
    }
    text.push_str(txt);
}

/// A header's or footer's text box read: its lines with text go ahead of
/// the line being read (the paragraph that anchors the box), floating
/// where the box does and across the page as it sits. A box that does not
/// float against the page has its text on that line.
fn finish_hf_box(b: HfBox, text: &mut Option<String>, lines: &mut Vec<crate::model::HfLine>) {
    let shown: Vec<(String, crate::model::HfLine)> = b
        .lines
        .into_iter()
        .filter(|(t, ..)| !t.trim().is_empty())
        .map(|(t, mut line, _)| {
            line.float = b.float;
            if let Some(a) = b.across {
                line.alignment = a;
            }
            (t, line)
        })
        .collect();
    if shown.is_empty() {
        return;
    }
    let t = text.get_or_insert_with(String::new);
    // A box placed against its paragraph sits over that paragraph's line:
    // its text is that line's.
    if b.float.is_none() {
        t.extend(shown.iter().map(|(s, _)| s.as_str()));
        return;
    }
    let at = t.rfind('\n').map_or(0, |i| i + 1);
    let joined: String = shown.iter().map(|(s, _)| format!("{s}\n")).collect();
    t.insert_str(at, &joined);
    let k = lines.len().saturating_sub(1);
    lines.splice(k..k, shown.into_iter().map(|(_, l)| l));
}

/// Where a header's or footer's text box of graphic style `style` (at
/// `svg:y` `y`) floats and how it sits across the page: set against the
/// page's top or bottom, its text inset by the style's padding; `None`
/// for a box placed against anything else.
fn hf_box_place(style: Option<&str>, y: Option<&str>, named: &AutoStyles) -> (Option<crate::model::HfFloat>, Option<Alignment>) {
    use crate::model::{AnchorAlign, AnchorFrame, HfFloat};
    let Some(place) = style.and_then(|n| named.graphic.get(n)) else { return (None, None) };
    let (top, bottom) = style.and_then(|n| named.graphic_padding.get(n)).copied().unwrap_or((0.0, 0.0));
    let float = match (place.v_from, place.v_align) {
        (AnchorFrame::Page, Some(AnchorAlign::Start)) => Some(HfFloat::Top(top)),
        (AnchorFrame::Page, Some(AnchorAlign::End)) => Some(HfFloat::Bottom(bottom)),
        (AnchorFrame::Page, None) => y.and_then(parse_length_pt).map(|y| HfFloat::Top(y + top)),
        _ => None,
    };
    let across = match (place.h_from, place.h_align) {
        (AnchorFrame::Page | AnchorFrame::Margin, Some(AnchorAlign::Center)) => Some(Alignment::Center),
        (AnchorFrame::Page | AnchorFrame::Margin, Some(AnchorAlign::End)) => Some(Alignment::Right),
        (AnchorFrame::Page | AnchorFrame::Margin, Some(AnchorAlign::Start)) => Some(Alignment::Left),
        _ => None,
    };
    (float, across)
}

/// Span style `name`'s own look over a header's or footer's line's: its
/// first text is drawn so.
fn hf_span_look(look: &mut RunStyle, name: &str, named: &AutoStyles) {
    if let Some(face) = named.text_font_name.get(name) {
        look.font_family = Some(face.clone());
    }
    let Some(span) = named.text.get(name) else { return };
    look.bold |= span.bold;
    look.italic |= span.italic;
    if span.font_size_hp.is_some() {
        look.font_size_hp = span.font_size_hp;
    }
    if span.color.is_some() {
        look.color.clone_from(&span.color);
    }
    if span.font_family.is_some() {
        look.font_family.clone_from(&span.font_family);
    }
}

/// A paragraph of a header or footer starts a line of its text: of the
/// master page being read, and of the document's own when it is the
/// first page's (`header` or `footer`). Its first paragraph starts it.
fn start_hf_line(master: Option<&mut MasterReading>, doc: &mut Document, any_header: bool, header: bool, footer: bool) {
    let line = |t: &mut Option<String>| match t {
        Some(s) => s.push('\n'),
        None => *t = Some(String::new()),
    };
    if let Some(m) = master {
        line(if any_header { &mut m.header } else { &mut m.footer });
    }
    if header {
        line(&mut doc.header);
    }
    if footer {
        line(&mut doc.footer);
    }
}

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

/// styles.xml with the first page's master (`master`) made to read as
/// Writer lays it out when it names a different `style:next-style-name`:
/// the next master's header and footer run on the pages after the first,
/// and the first master's own become the first page's
/// (`style:header-first`/`style:footer-first`). A first master without a
/// header or footer gives a first page without one.
fn follow_next_master(styles: &str, master: &str) -> Option<String> {
    #[derive(Default)]
    struct Master {
        next: Option<String>,
        content: Option<(usize, usize)>,
        // Its own header and footer elements, and whether it has a first
        // page's one already.
        header: Option<(usize, usize)>,
        footer: Option<(usize, usize)>,
        has_first: bool,
    }
    let mut masters: std::collections::HashMap<String, Master> = Default::default();
    let mut reader = Reader::from_str(styles);
    let mut cur: Option<String> = None;
    let mut content_start = 0;
    // The master's child element being read: its name, start and depth.
    let mut child: Option<(String, usize)> = None;
    let mut depth = 0usize;
    loop {
        let before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(e)) if cur.is_none() && e.name().as_ref() == "style:master-page" => {
                let name = attr_val(&e, "style:name").unwrap_or_default();
                masters.insert(name.clone(), Master { next: attr_val(&e, "style:next-style-name"), ..Default::default() });
                cur = Some(name);
                content_start = reader.buffer_position() as usize;
                depth = 0;
            }
            Ok(Event::Empty(e)) if cur.is_none() && e.name().as_ref() == "style:master-page" => {
                let name = attr_val(&e, "style:name").unwrap_or_default();
                masters.insert(name, Master { next: attr_val(&e, "style:next-style-name"), ..Default::default() });
            }
            Ok(Event::End(e)) if depth == 0 && e.name().as_ref() == "style:master-page" => {
                if let Some(m) = cur.take().and_then(|n| masters.get_mut(&n)) {
                    m.content = Some((content_start, before));
                }
            }
            Ok(Event::Start(e)) if cur.is_some() => {
                if depth == 0 {
                    child = Some((e.name().as_ref().to_string(), before));
                }
                depth += 1;
            }
            Ok(Event::End(_)) if cur.is_some() => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if let Some((name, start)) = child.take() {
                        let span = (start, reader.buffer_position() as usize);
                        let m = cur.as_ref().and_then(|n| masters.get_mut(n)).expect("the master being read");
                        match name.as_str() {
                            "style:header" => m.header = Some(span),
                            "style:footer" => m.footer = Some(span),
                            "style:header-first" | "style:footer-first" => m.has_first = true,
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::Empty(e)) if cur.is_some() && depth == 0 => {
                let span = (before, reader.buffer_position() as usize);
                let m = cur.as_ref().and_then(|n| masters.get_mut(n)).expect("the master being read");
                match e.name().as_ref() {
                    "style:header" => m.header = Some(span),
                    "style:footer" => m.footer = Some(span),
                    "style:header-first" | "style:footer-first" => m.has_first = true,
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    let first = masters.get(master)?;
    let next = masters.get(first.next.as_deref().filter(|n| *n != master)?)?;
    let (start, end) = first.content?;
    if first.has_first {
        return None;
    }
    let part = |span: Option<(usize, usize)>| span.map_or("", |(s, e)| &styles[s..e]);
    // The first master's own part, renamed to the first page's.
    // One the first master lacks is a first page without, where the
    // pages after have one.
    let as_first = |span: Option<(usize, usize)>, after: Option<(usize, usize)>, tag: &str| -> String {
        match span {
            None if after.is_none() => String::new(),
            None => format!("<style:{tag}-first/>"),
            Some((s, e)) => {
                let text = &styles[s..e];
                let open = format!("<style:{tag}");
                let mut out = format!("<style:{tag}-first{}", &text[open.len()..]);
                let close = format!("</style:{tag}>");
                if out.ends_with(&close) {
                    out.truncate(out.len() - close.len());
                    out.push_str(&format!("</style:{tag}-first>"));
                }
                out
            }
        }
    };
    let mut merged = String::with_capacity(styles.len());
    merged.push_str(&styles[..start]);
    merged.push_str(part(next.header));
    merged.push_str(part(next.footer));
    merged.push_str(&as_first(first.header, next.header, "header"));
    merged.push_str(&as_first(first.footer, next.footer, "footer"));
    merged.push_str(&styles[end..]);
    Some(merged)
}

/// The master page a body block whose style is `name` starts a new page
/// on (LET-005S): the style's `style:master-page-name`, else the one a
/// parent style names. None where no style in the chain names one, or the
/// nearest names "" (no new master).
fn block_master(name: &str, auto: &AutoStyles, named: &AutoStyles) -> Option<String> {
    let mut cur = Some(name.to_string());
    for _ in 0..16 {
        let n = cur?;
        if let Some(m) = auto.master.get(&n).or_else(|| named.master.get(&n)) {
            return Some(m.clone()).filter(|m| !m.is_empty());
        }
        cur = auto.para_parent.get(&n).or_else(|| named.para_parent.get(&n)).cloned();
    }
    None
}

/// A header's or footer's text, pictures, alignment and lines as the
/// reader has them on the document.
type HfParts = (Option<String>, Vec<Run>, Alignment, Vec<HfLine>);

/// Take the header's (`header`) or footer's parts off the document.
fn take_hf(doc: &mut Document, header: bool) -> HfParts {
    if header {
        (doc.header.take(), std::mem::take(&mut doc.header_pictures), std::mem::take(&mut doc.header_alignment), std::mem::take(&mut doc.header_lines))
    } else {
        (doc.footer.take(), std::mem::take(&mut doc.footer_pictures), std::mem::take(&mut doc.footer_alignment), std::mem::take(&mut doc.footer_lines))
    }
}

/// Put the header's (`header`) or footer's parts back on the document.
fn put_hf(doc: &mut Document, header: bool, (text, pictures, alignment, lines): HfParts) {
    if header {
        (doc.header, doc.header_pictures, doc.header_alignment, doc.header_lines) = (text, pictures, alignment, lines);
    } else {
        (doc.footer, doc.footer_pictures, doc.footer_alignment, doc.footer_lines) = (text, pictures, alignment, lines);
    }
}

/// A master page of styles.xml as sections need it: its page layout's
/// name, its header's and footer's text (`{page}`/`{total}` for the
/// fields) and whether each holds a picture.
#[derive(Default)]
struct MasterReading {
    layout: Option<String>,
    /// Whether the master page has a header and a footer shown: only then
    /// does its page layout's header or footer area take room.
    has_header: bool,
    has_footer: bool,
    header: Option<String>,
    footer: Option<String>,
    header_pictures: bool,
    footer_pictures: bool,
}

/// The sections a body's master page switches make (LET-005S): `switches`
/// are (first model paragraph, master named) of each top-level block whose
/// style names a master page, in order. The first page's master (`first`)
/// starts section 0, whose page is `doc.page` and whose header and footer
/// are the document's; each switch to another declared master starts a
/// section on that master's page layout. A switch to the master already
/// in use is a page break (`page_break_before`), not a section. A
/// section's header or footer is its master's text ("" for none) where it
/// differs from the section before's, else None (continued). Empty when
/// the body never changes master.
fn master_sections(
    doc: &mut Document,
    switches: &[(usize, String)],
    first: Option<&str>,
    masters: &std::collections::HashMap<String, MasterReading>,
    pages: &std::collections::HashMap<String, PageGeometry>,
) -> Vec<DocSection> {
    let Some(first) = first else { return Vec::new() };
    let mut starts: Vec<(usize, &str)> = vec![(0, first)];
    for (at, name) in switches {
        if *at == 0 || !masters.contains_key(name) {
            continue;
        }
        if starts.last().is_some_and(|(_, m)| *m == name.as_str()) {
            if let Some(p) = doc.paragraphs.get_mut(*at) {
                p.style.page_break_before = true;
            }
        } else {
            starts.push((*at, name.as_str()));
        }
    }
    if starts.len() < 2 {
        return Vec::new();
    }
    // What a master's pages show in its header (or footer): its text, and
    // whether it has pictures (which only the first page's master's keep).
    let story_of = |name: &str, header: bool| -> (String, bool) {
        let m = masters.get(name);
        let text = m.and_then(|m| if header { m.header.clone() } else { m.footer.clone() }).unwrap_or_default();
        (text, m.is_some_and(|m| if header { m.header_pictures } else { m.footer_pictures }))
    };
    let story = |k: usize, header: bool| -> Option<String> {
        let own = story_of(starts[k].1, header);
        (k > 0 && own != story_of(starts[k - 1].1, header)).then_some(own.0)
    };
    let first_page = doc.page.unwrap_or_default();
    let total = doc.paragraphs.len();
    starts
        .iter()
        .enumerate()
        .map(|(k, &(start, name))| DocSection {
            id: k as u32,
            range: ParagraphRange::new(start, starts.get(k + 1).map_or(total, |s| s.0)),
            geometry: if k == 0 {
                first_page
            } else {
                pages.get(name).copied().unwrap_or_default()
            },
            header: story(k, true),
            footer: story(k, false),
        })
        .collect()
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

/// Each section style's columns, as `parse_section_columns` reads the
/// first: `text:section` style name → count (more than one) and gap, the
/// gap only when stated.
fn section_style_columns(xml: &str) -> std::collections::HashMap<String, (u8, Option<f64>)> {
    let mut out = std::collections::HashMap::new();
    let mut reader = Reader::from_str(xml);
    let mut style: Option<String> = None;
    let mut in_section_props = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) if e.name().as_ref() == "style:style" => style = attr_val(&e, "style:name"),
            Ok(Event::End(e)) if e.name().as_ref() == "style:style" => style = None,
            Ok(Event::Start(e)) if e.name().as_ref() == "style:section-properties" => in_section_props = true,
            Ok(Event::End(e)) if e.name().as_ref() == "style:section-properties" => in_section_props = false,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if in_section_props && e.name().as_ref() == "style:columns" => {
                let count = attr_val(&e, "fo:column-count").and_then(|v| v.parse::<u8>().ok()).filter(|c| *c > 1);
                if let (Some(name), Some(count)) = (style.clone(), count) {
                    out.insert(name, (count, attr_val(&e, "fo:column-gap").and_then(|v| parse_length_pt(&v))));
                }
            }
            Ok(Event::Eof) | Err(_) => return out,
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
    /// The room kept beside a frame anchored as a character, in EMU.
    sides: Option<(u64, u64)>,
    /// Where a frame not anchored as a character floats.
    anchor: Option<crate::model::ImageAnchor>,
    href: Option<String>,
    alt: String,
    in_alt: bool,
    depth: usize,
    /// A text box's frame and its text so far: Some once its
    /// `draw:text-box` opens.
    text_box: Option<String>,
    look: BoxLook,
}

/// How a text box's frame is drawn: its rule, its fill and its padding
/// (top, right, bottom, left) in EMU.
type BoxLook = (Option<crate::model::BoxRule>, Option<String>, [u32; 4]);

/// A graphic style's text-box frame. The fill is `draw:fill`'s: Writer
/// draws a frame whose style gives only `fo:background-color` unfilled
/// (the opt-out form's answer boxes are white in it).
fn box_look(e: &quick_xml::events::BytesStart) -> BoxLook {
    let rule = ["fo:border", "fo:border-top", "fo:border-left"]
        .iter()
        .find_map(|a| attr_val(e, a).as_deref().and_then(parse_para_border))
        .filter(|b| b.style != BorderStyle::None && b.width_pt > 0.0)
        .map(|b| crate::model::BoxRule { width_emu: (b.width_pt * 12700.0).round() as u32, color: b.color })
        // A drawing shape's outline (`draw:stroke`), as Writer writes a
        // Word text box's: 0.75pt black unless it says.
        .or_else(|| {
            attr_val(e, "draw:stroke").filter(|s| s != "none")?;
            Some(crate::model::BoxRule {
                width_emu: attr_val(e, "svg:stroke-width").and_then(|v| parse_length_pt(&v)).filter(|w| w.is_finite() && *w > 0.0).map_or(9525, |w| (w * 12700.0).round() as u32),
                color: attr_val(e, "svg:stroke-color").map(|c| c.trim_start_matches('#').to_uppercase()).filter(|c| c.len() == 6 && c.bytes().all(|b| b.is_ascii_hexdigit())).unwrap_or_else(|| "000000".into()),
            })
        });
    let fill = (attr_val(e, "draw:fill").as_deref() == Some("solid"))
        .then(|| attr_val(e, "draw:fill-color"))
        .flatten()
        .map(|c| c.trim_start_matches('#').to_uppercase())
        .filter(|c| c.len() == 6 && c.bytes().all(|b| b.is_ascii_hexdigit()));
    let pad = |a: &str| {
        attr_val(e, a)
            .or_else(|| attr_val(e, "fo:padding"))
            .and_then(|v| parse_length_pt(&v))
            .filter(|v| v.is_finite() && *v >= 0.0)
            .map_or(0, |v| (v * 12700.0).round() as u32)
    };
    (rule, fill, ["fo:padding-top", "fo:padding-right", "fo:padding-bottom", "fo:padding-left"].map(pad))
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
    // Writer keeps a style's contextual spacing in the same setting as its
    // space above and below: a style that sets the one and not the others
    // takes the document default's spacing, not its parent's. The W1
    // notes' List Paragraph sets contextual spacing under a Normal of 6pt
    // above, and Writer opens its lists with no space above and closes
    // them with the default's 8pt below.
    let mut spacing_settled = false;
    let mut cur = Some(name.to_string());
    for _ in 0..16 {
        let Some(n) = cur else { break };
        if let Some(st) = auto.para.get(&n).or_else(|| named.para.get(&n)) {
            if !spacing_settled {
                if st.sets[0] && before.is_none() {
                    before = Some(st.space_before_pt);
                }
                if st.sets[1] && after.is_none() {
                    after = Some(st.space_after_pt);
                }
                spacing_settled = auto.contextual.contains_key(&n) || named.contextual.contains_key(&n);
            }
            if st.sets[2] && line.is_none() {
                line = Some(st.line_spacing);
            }
        }
        cur = auto.para_parent.get(&n).or_else(|| named.para_parent.get(&n)).cloned();
    }
    // Every chain ends at the document's default paragraph style.
    if let Some(st) = named.para.get(DEFAULT_PARA) {
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
    (before, after, line)
}

/// The left, right and first-line indents paragraph style `name` has:
/// its own where it sets them, else the nearest style up its parent chain
/// that does ("Table Paragraph"'s small left indent in every cell of the
/// vessel declaration). None where no style in the chain sets one.
fn inherited_indents(name: &str, auto: &AutoStyles, named: &AutoStyles) -> [Option<f64>; 3] {
    let mut out = [None; 3];
    let mut cur = Some(name.to_string());
    for _ in 0..16 {
        let Some(n) = cur else { break };
        if let Some(st) = auto.para.get(&n).or_else(|| named.para.get(&n)) {
            let own = [st.left_indent_pt, st.right_indent_pt, st.first_line_indent_pt];
            for k in 0..3 {
                if st.sets_indent[k] && out[k].is_none() {
                    out[k] = Some(own[k]);
                }
            }
        }
        cur = auto.para_parent.get(&n).or_else(|| named.para_parent.get(&n)).cloned();
    }
    out
}

/// What paragraph style `name` inherits from its parent chain for the
/// keep and decoration properties: the nearest style up the chain
/// (automatic styles in `auto`, named ones in `named`) that sets each
/// one. Writer keeps these on the named style — a Heading's
/// keep-with-next, a quote's border — while the automatic style on the
/// paragraph carries only what that paragraph overrides.
#[derive(Default)]
struct InheritedPara {
    keep_with_next: bool,
    keep_lines: bool,
    widow_control: Option<bool>,
    line_height: Option<LineHeight>,
    shading: Option<String>,
    borders: ParaBorders,
}

fn inherited_para(name: &str, auto: &AutoStyles, named: &AutoStyles) -> InheritedPara {
    let mut out = InheritedPara::default();
    let mut line_decided = false;
    let mut cur = Some(name.to_string());
    for _ in 0..16 {
        let Some(n) = cur else { break };
        if let Some(st) = auto.para.get(&n).or_else(|| named.para.get(&n)) {
            out.keep_with_next |= st.keep_with_next;
            out.keep_lines |= st.keep_lines;
            if out.widow_control.is_none() {
                out.widow_control = st.widow_control;
            }
            // The nearest style that sets a line height decides it: a
            // heading's 100% over Normal's exact one.
            if !line_decided && (st.sets[2] || st.line_height.is_some()) {
                (out.line_height, line_decided) = (st.line_height, true);
            }
            if out.shading.is_none() {
                out.shading.clone_from(&st.shading);
            }
            if out.borders.top.is_none() {
                out.borders.top.clone_from(&st.borders.top);
            }
            if out.borders.left.is_none() {
                out.borders.left.clone_from(&st.borders.left);
            }
            if out.borders.bottom.is_none() {
                out.borders.bottom.clone_from(&st.borders.bottom);
            }
            if out.borders.right.is_none() {
                out.borders.right.clone_from(&st.borders.right);
            }
        }
        cur = auto.para_parent.get(&n).or_else(|| named.para_parent.get(&n)).cloned();
    }
    out
}

/// How a paragraph style's text looks: its own text properties, then what
/// it leaves unset from the named styles it is based on, as Writer draws a
/// form's "Coloured Box Headline" (bold, 14pt, based on Normal). The base
/// paragraph style (Standard, Word's Normal, or one based on nothing) is
/// the document's base font (`read_base_font`), not its runs'.
fn para_text_style(name: &str, auto: &AutoStyles, named: &AutoStyles) -> RunStyle {
    let mut cur = auto.para_parent.get(name).or_else(|| named.para_parent.get(name)).cloned();
    let base = cur.is_none() || matches!(name, "Standard" | "Normal");
    let own = auto.para_text.get(name).or_else(|| named.para_text.get(name).filter(|_| !base));
    let mut style = own.cloned().unwrap_or_default();
    let mut depth = 0;
    while let Some(n) = cur {
        let parent = auto.para_parent.get(&n).or_else(|| named.para_parent.get(&n)).cloned();
        if depth >= 8 || parent.is_none() || matches!(n.as_str(), "Standard" | "Normal") {
            break;
        }
        if let Some(own) = auto.para_text.get(&n).or_else(|| named.para_text.get(&n)) {
            style.bold |= own.bold;
            style.italic |= own.italic;
            style.caps |= own.caps;
            style.font_size_hp = style.font_size_hp.or(own.font_size_hp);
            style.color = style.color.clone().or_else(|| own.color.clone());
            style.font_family = style.font_family.clone().or_else(|| own.font_family.clone());
        }
        cur = parent;
        depth += 1;
    }
    style
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
    let mut list_labels = list_style_labels(&styles);
    list_labels.extend(list_style_labels(&content));
    let mut list_indents = list_style_indents(&styles);
    list_indents.extend(list_style_indents(&content));
    // The label of a numbered item at `level` (1-based) of the list style
    // `name`.
    let label_of = |name: Option<&String>, level: u8| -> Option<ListLabel> {
        list_labels.get(name?)?.get(usize::from(level.checked_sub(1)?))?.clone()
    };
    // A list item's kind and label at `level` (1-based): its own list
    // style's (`item_styles`) when it overrides the list's, else the
    // list's.
    let item_list = |names: &[Option<String>], items: &[Option<String>], kind: ListKind, level: u8| -> (ListKind, Option<ListLabel>) {
        let own = level.checked_sub(1).and_then(|l| items.get(usize::from(l))).cloned().flatten();
        let kind = own
            .as_ref()
            .and_then(|n| list_styles.get(n))
            .and_then(|kinds| kinds.get(usize::from(level.max(1) - 1)).copied())
            .unwrap_or(kind);
        let name = own.as_ref().or_else(|| names.last().and_then(Option::as_ref));
        (kind, if kind == ListKind::Numbered { label_of(name, level) } else { None })
    };

    // styles.xml's named styles, the parents of content.xml's automatic ones.
    let named = parse_auto_styles(&styles);
    let mut doc = Document { paragraphs: Vec::new(), footnotes: Vec::new(), header: None, footer: None, header_pictures: Vec::new(), footer_pictures: Vec::new(), header_alignment: Default::default(), footer_alignment: Default::default(), header_lines: Vec::new(), footer_lines: Vec::new(), page: None, base_font: read_base_font(&styles), heading_styles: read_heading_styles(&styles), comments: Vec::new(), table_columns: Default::default(), table_rows: Default::default(), table_spans: Default::default(), table_fills: Default::default(), sections: Vec::new(), next_section_id: 0, table_padding: Default::default(), table_unruled: Default::default(), table_rule_colors: Default::default(), table_rule_widths: Default::default(), table_relative: Default::default(), table_open_edges: Default::default(), table_valign: Default::default(), table_whole_rows: Default::default(), table_indent: Default::default(), first_header: None, first_footer: None, space_adds: true };
    let mut reader = Reader::from_str(&content);
    let mut in_body = false;
    let mut para: Option<Paragraph> = None;
    // Span/link style stack: (style, depth marker)
    let mut span_stack: Vec<RunStyle> = Vec::new();
    // Body paragraphs open, each with its text base on `span_stack`.
    let mut paragraph_bases = 0usize;
    // Each body paragraph's named style and contextual spacing, by index.
    let mut para_ctx: Option<(String, bool)> = None;
    let mut spacing_ctx: std::collections::HashMap<usize, (String, bool)> = Default::default();
    let mut link_stack: Vec<String> = Vec::new();
    let mut list_kind = ListKind::None;
    let mut list_level: u8 = 0;
    // The kind of each open text:list, so a nested list without a style of
    // its own takes its parent's, and closing it restores the parent's.
    let mut list_kinds: Vec<ListKind> = Vec::new();
    // The list style of each open text:list: a nested list naming none
    // continues its parent's, at its own level.
    let mut list_style_names: Vec<Option<String>> = Vec::new();
    // Per open list level: the list style its current item overrides the
    // list's with (`text:style-override`), if any.
    let mut item_styles: Vec<Option<String>> = Vec::new();
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
    // Inside a body frame's `draw:text-box`, and how many of its
    // paragraphs have been read.
    let mut in_text_box = false;
    let mut box_paragraphs = 0usize;
    // The text box being read is a drawing shape (`draw:custom-shape` or
    // `draw:rect`, Writer's form of a Word text box) and not a frame, and
    // whether its geometry is a rectangle.
    let mut shape_box = false;
    let mut shape_rect = false;
    // The outermost table being read (#1296): its id, the current row and
    // column (-1 before the first), its element depth, and whether the
    // current cell has had a paragraph. A table nested in a cell belongs
    // to its outer cell.
    let mut table: Option<(u32, i64, i64, usize, bool)> = None;
    let mut table_depth = 0usize;
    // Inside the outermost table's heading rows (`table:table-header-rows`),
    // which Writer never splits across pages: read as rows kept whole.
    let mut in_header_rows = false;
    let mut tables_read = 0u32;
    // Each outermost table's column widths; None once a column has none.
    let mut column_widths: std::collections::BTreeMap<u32, Option<Vec<f64>>> = Default::default();
    let mut row_heights: std::collections::BTreeMap<u32, Vec<Option<crate::model::RowHeight>>> = Default::default();
    // Each table's merged cells (`table:number-columns-spanned`,
    // `table:number-rows-spanned`).
    let mut cell_spans: std::collections::BTreeMap<u32, Vec<CellSpan>> = Default::default();
    let mut cell_fills: std::collections::BTreeMap<u32, Vec<crate::model::CellFill>> = Default::default();
    // A shaded cell's fill, at the cell just counted.
    // Each table's cell padding: its first cell's.
    let mut cell_padding: std::collections::BTreeMap<u32, crate::model::CellPadding> = Default::default();
    // Each table's style's `fo:margin-left`, by table id.
    let mut table_margins: std::collections::BTreeMap<u32, f64> = Default::default();
    // The tables with a ruled cell: Writer rules a cell only by its style.
    let mut ruled_tables: std::collections::HashSet<u32> = Default::default();
    // A table's rule colour and width: its first ruled cell's.
    type RuleLooks = (std::collections::BTreeMap<u32, String>, std::collections::BTreeMap<u32, f64>);
    let mut rule_looks: RuleLooks = Default::default();
    let note_rule = |ruled: &mut std::collections::HashSet<u32>, looks: &mut RuleLooks, t: &(u32, i64, i64, usize, bool), e: &quick_xml::events::BytesStart| {
        let Some(n) = attr_val(e, "table:style-name").filter(|n| auto.cell_ruled.contains(n) || named.cell_ruled.contains(n)) else { return };
        if ruled.insert(t.0) {
            if let Some(c) = auto.cell_rule_color.get(&n).or_else(|| named.cell_rule_color.get(&n)) {
                looks.0.insert(t.0, c.clone());
            }
            if let Some(w) = auto.cell_rule_width.get(&n).or_else(|| named.cell_rule_width.get(&n)) {
                looks.1.insert(t.0, *w);
            }
        }
    };
    let note_fill = |fills: &mut std::collections::BTreeMap<u32, Vec<crate::model::CellFill>>, t: &(u32, i64, i64, usize, bool), e: &quick_xml::events::BytesStart| {
        if let Some(color) = attr_val(e, "table:style-name").and_then(|n| auto.cell_fill.get(&n).cloned()) {
            fills.entry(t.0).or_default().push(crate::model::CellFill { row: t.1.max(0) as u32, col: t.2.max(0) as u32, color });
        }
    };
    // A cell's sides without a rule, in a table with rules.
    let mut open_edges: std::collections::BTreeMap<u32, Vec<crate::model::CellEdges>> = Default::default();
    let note_open = |edges: &mut std::collections::BTreeMap<u32, Vec<crate::model::CellEdges>>, t: &(u32, i64, i64, usize, bool), e: &quick_xml::events::BytesStart| {
        if let Some(open) = attr_val(e, "table:style-name").and_then(|n| auto.cell_open.get(&n).or_else(|| named.cell_open.get(&n)).copied()) {
            edges.entry(t.0).or_default().push(crate::model::CellEdges { row: t.1.max(0) as u32, col: t.2.max(0) as u32, open });
        }
    };
    let mut valigns: std::collections::BTreeMap<u32, Vec<crate::model::CellVAlign>> = Default::default();
    let note_valign = |cells: &mut std::collections::BTreeMap<u32, Vec<crate::model::CellVAlign>>, t: &(u32, i64, i64, usize, bool), e: &quick_xml::events::BytesStart| {
        if let Some(align) = attr_val(e, "table:style-name").and_then(|n| auto.cell_valign.get(&n).or_else(|| named.cell_valign.get(&n)).copied()) {
            cells.entry(t.0).or_default().push(crate::model::CellVAlign { row: t.1.max(0) as u32, col: t.2.max(0) as u32, align });
        }
    };
    let cell_of = |t: &Option<(u32, i64, i64, usize, bool)>| {
        t.map(|(id, r, c, _, _)| crate::model::TableCell { table: id, row: r.max(0) as u32, col: c.max(0) as u32 })
    };
    let mut consumed: Vec<String> = Vec::new();
    // Each top-level body block (paragraph, heading or table, not text in
    // a table, frame or note) whose style names a master page: the model
    // paragraph it starts at and the master (LET-005S, `master_sections`).
    let mut switches: Vec<(usize, String)> = Vec::new();
    // Each top-level `text:section`: its style and the model paragraphs it
    // holds (`open_text_sections` while open, None for one inside a table,
    // frame or note). LibreOffice writes a Word section's columns so.
    let mut open_text_sections: Vec<Option<(String, usize)>> = Vec::new();
    let mut text_sections: Vec<(String, usize, usize)> = Vec::new();

    // The close of a self-closing paragraph read as an open one.
    let mut pending_end: Option<quick_xml::events::BytesEnd<'static>> = None;
    loop {
        let event = match pending_end.take() {
            Some(end) => Ok(Event::End(end)),
            None => reader.read_event(),
        };
        // A self-closing body paragraph (Writer's blank line) is an empty
        // one opened and closed, so it gets its style (size, spacing) like
        // any other, not a bare default.
        let event = match event {
            Ok(Event::Empty(e)) if in_body && note.is_none() && annotation.is_none() && matches!(e.name().as_ref(), "text:p" | "text:h") => {
                pending_end = Some(e.to_end().into_owned());
                Ok(Event::Start(e))
            }
            other => other,
        };
        // A text box's own paragraphs are its text, not the body's: they
        // must not close the paragraph that anchors it.
        if let Some(f) = frame.as_mut().filter(|f| f.depth == frame_depth) {
            if let Some(text) = f.text_box.as_mut() {
                match &event {
                    Ok(Event::End(e)) if e.name().as_ref() == "draw:text-box" => in_text_box = false,
                    // The shape's own end closes its box, and is read below.
                    Ok(Event::End(e)) if shape_box && matches!(e.name().as_ref(), "draw:custom-shape" | "draw:rect") => in_text_box = false,
                    _ if !in_text_box => {}
                    Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == "draw:enhanced-geometry" => {
                        shape_rect = attr_val(e, "draw:type").is_some_and(|t| matches!(t.as_str(), "rect" | "ooxml-rect" | "rectangle"));
                    }
                    Ok(Event::Start(e)) if matches!(e.name().as_ref(), "text:p" | "text:h") => {
                        if box_paragraphs > 0 {
                            text.push('\n');
                        }
                        box_paragraphs += 1;
                    }
                    Ok(Event::Text(t)) => text.push_str(&unescape_text(t)),
                    Ok(Event::GeneralRef(r)) => text.push_str(&resolve_general_ref(r)),
                    Ok(Event::Empty(e)) => match e.name().as_ref() {
                        "text:s" => text.push_str(&" ".repeat(attr_val(e, "text:c").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).min(1024))),
                        "text:tab" => text.push('\t'),
                        "text:line-break" => text.push('\n'),
                        _ => {}
                    },
                    _ => {}
                }
                if in_text_box || matches!(&event, Ok(Event::End(e)) if e.name().as_ref() == "draw:text-box") {
                    continue;
                }
            } else if let Ok(Event::Start(e)) | Ok(Event::Empty(e)) = &event {
                if e.name().as_ref() == "draw:text-box" {
                    f.text_box = Some(String::new());
                    in_text_box = matches!(&event, Ok(Event::Start(_)));
                    box_paragraphs = 0;
                    continue;
                }
            }
        }
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
                    if table.is_none() && para.is_none() && frame_depth == 0 {
                        if let Some(m) = attr_val(&e, "table:style-name").and_then(|n| block_master(&n, &auto, &named)) {
                            switches.push((doc.paragraphs.len(), m));
                        }
                    }
                    table_depth += 1;
                    if table.is_none() {
                        if attr_val(&e, "table:style-name").is_some_and(|n| auto.table_relative.contains(&n) || named.table_relative.contains(&n)) {
                            doc.table_relative.insert(tables_read);
                        }
                        if let Some(m) = attr_val(&e, "table:style-name").and_then(|n| auto.table_margin.get(&n).or(named.table_margin.get(&n)).copied()) {
                            table_margins.insert(tables_read, m);
                        }
                        table = Some((tables_read, -1, -1, table_depth, false));
                        tables_read += 1;
                    }
                }
                "table:table-header-rows" if table.as_ref().is_some_and(|t| t.3 == table_depth) => in_header_rows = true,
                "table:table-row" => {
                    if let Some(t) = table.as_mut().filter(|t| t.3 == table_depth) {
                        t.1 += 1;
                        t.2 = -1;
                        if in_header_rows || attr_val(&e, "table:style-name").is_some_and(|n| auto.row_whole.contains(&n) || named.row_whole.contains(&n)) {
                            doc.table_whole_rows.entry(t.0).or_default().insert(t.1 as u32);
                        }
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
                        note_span(&mut cell_spans, &e, t);
                        note_fill(&mut cell_fills, t, &e);
                        note_rule(&mut ruled_tables, &mut rule_looks, t, &e);
                        note_open(&mut open_edges, t, &e);
                        note_valign(&mut valigns, t, &e);
                        if (t.1, t.2) == (0, 0) {
                            if let Some(pad) = attr_val(&e, "table:style-name").and_then(|n| auto.cell_padding.get(&n).copied()) {
                                cell_padding.insert(t.0, pad);
                            }
                        }
                    }
                }
                "draw:frame" | "draw:custom-shape" | "draw:rect" => {
                    frame_depth += 1;
                    let shape = e.name().as_ref() != "draw:frame";
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
                        let sides = (!floating)
                            .then(|| attr_val(&e, "draw:style-name").and_then(|n| auto.graphic_sides.get(&n).copied()))
                            .flatten()
                            .filter(|&(l, r)| l > 0.0 || r > 0.0)
                            .map(|(l, r)| ((l * 12700.0).round() as u64, (r * 12700.0).round() as u64));
                        frame = Some(FrameReading {
                            anchor,
                            sides,
                            extent: size("svg:width").zip(size("svg:height")),
                            href: None,
                            alt: String::new(),
                            in_alt: false,
                            depth: frame_depth,
                            // A shape's paragraphs are its text, as a
                            // frame's text box's are.
                            text_box: shape.then(String::new),
                            look: attr_val(&e, "draw:style-name").and_then(|n| auto.graphic_box.get(&n).cloned()).unwrap_or_default(),
                        });
                        if shape {
                            (in_text_box, shape_box, box_paragraphs) = (true, true, 0);
                            shape_rect = e.name().as_ref() == "draw:rect";
                        }
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
                    if table.is_none() && para.is_none() && frame_depth == 0 {
                        if let Some(m) = attr_val(&e, "text:style-name").and_then(|n| block_master(&n, &auto, &named)) {
                            switches.push((doc.paragraphs.len(), m));
                        }
                    }
                    para_ctx = attr_val(&e, "text:style-name").map(|n| contextual_spacing(&n, &auto, &named));
                    let mut style = ParaStyle::default();
                    if e.name().as_ref() == "text:h" {
                        let lvl = attr_val(&e, "text:outline-level")
                            .and_then(|v| v.parse::<u8>().ok())
                            .unwrap_or(1);
                        style.heading = Some(lvl.clamp(1, 6));
                    }
                    // A paragraph naming no style has the default style's
                    // spacing.
                    if attr_val(&e, "text:style-name").is_none() {
                        let (before, after, line) = inherited_spacing(DEFAULT_PARA, &auto, &named);
                        style.space_before_pt = before.unwrap_or(style.space_before_pt);
                        style.space_after_pt = after.unwrap_or(style.space_after_pt);
                        style.line_spacing = line.unwrap_or(style.line_spacing);
                    }
                    if let Some(name) = attr_val(&e, "text:style-name") {
                        if let Some(auto_para) = auto.para.get(&name) {
                            style.alignment = auto_para.alignment;
                            style.page_break_before = auto_para.page_break_before;
                            style.keep_with_next = auto_para.keep_with_next;
                            style.keep_lines = auto_para.keep_lines;
                            style.widow_control = auto_para.widow_control;
                            style.line_spacing = auto_para.line_spacing;
                            style.line_height = auto_para.line_height;
                            style.shading = auto_para.shading.clone();
                            style.borders = auto_para.borders.clone();
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
                        let inh = inherited_para(&name, &auto, &named);
                        style.keep_with_next |= inh.keep_with_next;
                        style.keep_lines |= inh.keep_lines;
                        if style.widow_control.is_none() {
                            style.widow_control = inh.widow_control;
                        }
                        if style.line_height.is_none() {
                            style.line_height = inh.line_height;
                        }
                        if style.shading.is_none() {
                            style.shading = inh.shading;
                        }
                        let borders = &mut style.borders;
                        if borders.top.is_none() {
                            borders.top = inh.borders.top;
                        }
                        if borders.left.is_none() {
                            borders.left = inh.borders.left;
                        }
                        if borders.bottom.is_none() {
                            borders.bottom = inh.borders.bottom;
                        }
                        if borders.right.is_none() {
                            borders.right = inh.borders.right;
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
                    let (kind, label) = item_list(&list_style_names, &item_styles, list_kind, list_level);
                    // The indents a parent style gives (a list item's
                    // come from its list style, a quote's from its look).
                    if let Some(name) = attr_val(&e, "text:style-name").filter(|_| kind == ListKind::None && !style.block_quote) {
                        let [left, right, first] = inherited_indents(&name, &auto, &named);
                        style.left_indent_pt = left.unwrap_or(style.left_indent_pt);
                        style.right_indent_pt = right.unwrap_or(style.right_indent_pt);
                        style.first_line_indent_pt = first.unwrap_or(style.first_line_indent_pt);
                    }
                    style.list = kind;
                    style.list_level = list_level.saturating_sub(1);
                    style.list_start = pending_start.take();
                    style.list_label = label;
                    // Where the list style puts the item's text, unless the
                    // paragraph places it itself: the model's indent is
                    // that less the list indent the layout adds (as for a
                    // Word list, `docx::list_indent_from_declared`). A
                    // Word-made .odt's 0.5in level is drawn at 0.5in.
                    if kind != ListKind::None {
                        let own = item_styles.get(usize::from(list_level.max(1) - 1)).cloned().flatten();
                        let name = own.as_ref().or_else(|| list_style_names.last().and_then(Option::as_ref));
                        let declared = name.and_then(|n| list_indents.get(n)).and_then(|l| l.get(usize::from(list_level.max(1) - 1)).copied().flatten());
                        let text = crate::lists::text_indent_pt(style.list_level);
                        if style.left_indent_pt == 0.0 {
                            if let Some(declared) = declared {
                                style.left_indent_pt = (declared - text).max(0.0);
                                if style.first_line_indent_pt < 0.0 {
                                    style.first_line_indent_pt = 0.0;
                                }
                            }
                        } else if declared.is_some() {
                            // A label-aligned list's paragraph that sets its
                            // own indents replaces the level's, as Writer
                            // draws it: its text at the paragraph's left
                            // margin, the marker hanging by its first-line
                            // indent (a Word-made list's 0.2958in items
                            // stand their bullets at the page margin).
                            style.left_indent_pt = (style.left_indent_pt - text).max(-text);
                            let hanging = (-style.first_line_indent_pt).max(0.0);
                            style.list_hanging_pt = Some(hanging).filter(|h| (h - crate::lists::HANGING_PT).abs() > 0.5);
                            style.first_line_indent_pt = 0.0;
                        }
                    }
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
                    let text_base = match attr_val(&e, "text:style-name") {
                        // A heading looks as its level's style says
                        // (`heading_styles`); only its own automatic
                        // style's text properties are its runs'.
                        Some(n) if para.as_ref().is_some_and(|p| p.style.heading.is_some()) => auto.para_text.get(&n).cloned().unwrap_or_default(),
                        Some(n) => para_text_style(&n, &auto, &named),
                        None => RunStyle::default(),
                    };
                    span_stack.push(text_base);
                    paragraph_bases += 1;
                }
                "text:span" => {
                    let name = attr_val(&e, "text:style-name");
                    let own = name.as_deref().map(|n| text_style(n, &auto, &named)).unwrap_or_default();
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
                "text:section" => {
                    let top = in_body && table.is_none() && para.is_none() && frame_depth == 0 && note.is_none();
                    open_text_sections.push(attr_val(&e, "text:style-name").filter(|_| top).map(|n| (n, doc.paragraphs.len())));
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
                    // LibreOffice gives an item a list style of its own this
                    // way: a Word list instance's override of one level.
                    let at = usize::from(list_level.max(1) - 1);
                    item_styles.resize(at + 1, None);
                    item_styles[at] = attr_val(&e, "text:style-override");
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
                        note_span(&mut cell_spans, &e, t);
                        note_fill(&mut cell_fills, t, &e);
                        note_rule(&mut ruled_tables, &mut rule_looks, t, &e);
                        note_open(&mut open_edges, t, &e);
                        note_valign(&mut valigns, t, &e);
                        if (t.1, t.2) == (0, 0) {
                            if let Some(pad) = attr_val(&e, "table:style-name").and_then(|n| auto.cell_padding.get(&n).copied()) {
                                cell_padding.insert(t.0, pad);
                            }
                        }
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
                // A line break within the paragraph (Shift+Enter): the
                // statistical release's two-line title.
                "text:line-break" if para.is_some() => {
                    push_text(&mut para, &span_stack, &link_stack, &rev_stack, "\n");
                }
                "text:p" if note.is_some() => {}
                "text:p" | "text:h" if in_body => {
                    if table.is_none() && para.is_none() && frame_depth == 0 {
                        if let Some(m) = attr_val(&e, "text:style-name").and_then(|n| block_master(&n, &auto, &named)) {
                            switches.push((doc.paragraphs.len(), m));
                        }
                    }
                    let (kind, list_label) = item_list(&list_style_names, &item_styles, list_kind, list_level);
                    let style = ParaStyle { list: kind, list_level: list_level.saturating_sub(1), list_start: pending_start.take(), list_label, table_cell: cell_of(&table), ..Default::default() };
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
                "table:table-header-rows" if table.as_ref().is_some_and(|t| t.3 == table_depth) => in_header_rows = false,
                "svg:title" | "svg:desc" => {
                    if let Some(f) = frame.as_mut() {
                        f.in_alt = false;
                    }
                }
                "draw:frame" | "draw:custom-shape" | "draw:rect" => {
                    if frame.as_ref().is_some_and(|f| f.depth == frame_depth && f.text_box.is_some()) {
                        let f = frame.take().expect("the frame just checked");
                        // A shape that is neither a rectangle nor holds text
                        // (a line, an arrow) is not a box.
                        let kept = !std::mem::take(&mut shape_box) || shape_rect || f.text_box.as_deref().is_some_and(|t| !t.trim().is_empty());
                        let (border, fill, padding_emu) = f.look;
                        if let Some(p) = para.as_mut().filter(|_| kept) {
                            p.runs.push(Run {
                                text: String::new(),
                                style: RunStyle {
                                    text_box: Some(Box::new(crate::model::TextBox {
                                        extent_emu: f.extent.unwrap_or((0, 0)),
                                        border,
                                        fill,
                                        padding_emu,
                                        text: f.text_box.unwrap_or_default(),
                                    })),
                                    // One placed as a character floats at its
                                    // paragraph.
                                    image_anchor: Some(f.anchor.unwrap_or_default()),
                                    ..Default::default()
                                },
                            });
                        }
                    } else if frame.as_ref().is_some_and(|f| f.depth == frame_depth) {
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
                                    image_sides_emu: f.sides,
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
                    // A blank paragraph keeps its style's text look in an
                    // empty run: its line is as tall as that font, as in
                    // Writer (a 14pt blank line, not the body size).
                    let blank_look = span_stack.last().filter(|_| paragraph_bases > 0).cloned();
                    if paragraph_bases > 0 {
                        paragraph_bases -= 1;
                        span_stack.pop();
                    }
                    if let Some(mut p) = para.take() {
                        if p.runs.is_empty() && p.style.table_cell.is_none() {
                            if let Some(look) = blank_look.filter(|l| l.font_size_hp.is_some() || l.font_family.is_some()) {
                                p.runs.push(Run { text: String::new(), style: look });
                            }
                        }
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
                        if let Some(c) = para_ctx.take() {
                            spacing_ctx.insert(doc.paragraphs.len(), c);
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
                "text:section" => {
                    if let Some(Some((name, start))) = open_text_sections.pop() {
                        text_sections.push((name, start, doc.paragraphs.len()));
                    }
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
                    item_styles.truncate(usize::from(list_level));
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
    // master page gives the document's (a file may hold several, each with
    // its own header, and joining them repeated the header's text). Every
    // master's layout and header and footer text is kept too, for the
    // sections a body that changes master makes (`master_sections`).
    if !styles.is_empty() {
        let master = first_master_page(&content, &styles);
        let followed = master.as_deref().and_then(|m| follow_next_master(&styles, m));
        let mut reader = Reader::from_str(followed.as_deref().unwrap_or(&styles));
        let mut in_header = false;
        let mut in_footer = false;
        // The master page being read, and whether it is the first page's.
        let mut in_master = false;
        let mut cur_master: Option<String> = None;
        let mut master_pages: std::collections::HashMap<String, MasterReading> = Default::default();
        // In a header or footer of any master page (`in_header` and
        // `in_footer` are the first page's master's only).
        let mut in_any_header = false;
        let mut in_any_footer = false;
        // Inside the first page's master's `style:header-first` (true) or
        // `style:footer-first` (false): the running part read so far, set
        // aside while the first page's own is read in its place, and the
        // master being read.
        let mut hf_first: Option<(bool, HfParts, Option<String>)> = None;
        let mut hf_first_skip = false;
        let mut layouts: std::collections::HashMap<String, PageGeometry> = Default::default();
        let mut layout_name = String::new();
        let mut master_layout: Option<String> = None;
        // Each page layout's header and footer areas, which take room only
        // on a master page that has a header or footer.
        let mut hf_areas: std::collections::HashMap<String, (Option<f64>, Option<f64>)> = Default::default();
        let mut last_layout: Option<String> = None;
        // Inside a page layout's `style:header-style` (true) or
        // `style:footer-style` (false).
        let mut hf_style: Option<bool> = None;
        // Inside an alternative text (`svg:title`/`svg:desc`) of a frame,
        // which is not the header's text.
        let mut in_alt = false;
        // Inside a page number or count field: its shown value is skipped.
        let mut in_field = false;
        // A picture of the header or footer being read: in the header or
        // not, its size, placement, image and alt text.
        let mut hf_frame: Option<(bool, FrameReading)> = None;
        // The style of the header's or footer's paragraph being read: the
        // first one with text gives the part its alignment.
        let mut hf_para_style: Option<String> = None;
        // Inside a text box of the header or footer: its own paragraphs'
        // styles are not the part's.
        let mut hf_box_depth = 0usize;
        // In a table row of a header or footer: whether a paragraph of the
        // row has started its line yet. A row's cells read as one line, as
        // they sit side by side; a later cell's text follows a space.
        let mut hf_row: Option<bool> = None;
        let mut hf_cell_gap = false;
        // The spans open in the document's header or footer, and whether
        // its line being read has taken its first text's look yet.
        let mut hf_spans: Vec<Option<String>> = Vec::new();
        let mut hf_styled = false;
        // A text box of the document's own header or footer being read,
        // and where the frame around a text box places it.
        let mut hf_box: Option<HfBox> = None;
        let mut hf_frame_place: (Option<crate::model::HfFloat>, Option<Alignment>) = (None, None);
        let para_alignment = |name: &str| -> Alignment {
            let mut cur = Some(name.to_string());
            for _ in 0..8 {
                let Some(n) = cur else { break };
                if let Some(a) = named.para.get(&n).map(|st| st.alignment).filter(|a| *a != Alignment::Left) {
                    return a;
                }
                cur = named.para_parent.get(&n).cloned();
            }
            Alignment::Left
        };
        // Returns the geometry instead of writing to `doc` so the borrow ends
        // with the call; `style:columns` needs `doc.page` mutably right after.
        let read_page_layout = |e: &quick_xml::events::BytesStart| -> Option<PageGeometry> {
            let width_pt = attr_val(e, "fo:page-width").and_then(|v| parse_length_pt(&v))?;
            let height_pt = attr_val(e, "fo:page-height").and_then(|v| parse_length_pt(&v))?;
            let d = PageGeometry::default();
            let m = |name: &str, fallback: f64| {
                attr_val(e, name).and_then(|v| parse_length_pt(&v)).unwrap_or(fallback)
            };
            // A page border sits on the margin, its padding inside it:
            // the text starts past both, which is where the model's
            // margins put it.
            let border = ["fo:border", "fo:border-top", "fo:border-right", "fo:border-bottom", "fo:border-left"]
                .iter()
                .find_map(|a| attr_val(e, a).as_deref().and_then(parse_para_border))
                .filter(|b| b.style != BorderStyle::None && b.width_pt > 0.0)
                .map(|b| PageBorder {
                    width_pt: b.width_pt,
                    color: u32::from_str_radix(&b.color, 16).unwrap_or(0),
                    padding_pt: ["fo:padding-top", "fo:padding-right", "fo:padding-bottom", "fo:padding-left"]
                        .map(|a| m(a, m("fo:padding", 0.0))),
                });
            let inset = |side: usize| border.map_or(0.0, |b| b.width_pt + b.padding_pt[side]);
            Some(PageGeometry {
                width_pt,
                height_pt,
                margin_top_pt: m("fo:margin-top", d.margin_top_pt) + inset(0),
                margin_bottom_pt: m("fo:margin-bottom", d.margin_bottom_pt) + inset(2),
                margin_left_pt: m("fo:margin-left", d.margin_left_pt) + inset(3),
                margin_right_pt: m("fo:margin-right", d.margin_right_pt) + inset(1),
                columns: d.columns,
                column_gap_pt: d.column_gap_pt,
                border,
                ..d
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
                // LibreOffice writes its columns with `style:column`
                // children (a start tag); the count and gap are the same.
                Ok(Event::Empty(e)) | Ok(Event::Start(e)) if e.name().as_ref() == "style:columns" => {
                    if let Some(page) = layouts.get_mut(&layout_name) {
                        page.columns = attr_val(&e, "fo:column-count")
                            .and_then(|v| v.parse::<u8>().ok()).unwrap_or(1).max(1);
                        page.column_gap_pt = attr_val(&e, "fo:column-gap")
                            .and_then(|v| parse_length_pt(&v)).unwrap_or(page.column_gap_pt);
                    }
                }
                // Writer's page margin runs to the header; the header's
                // height (its spacing to the body included, as Writer
                // counts it) sits between it and the body, and Word's top
                // margin (what the model holds) is to the body. So a
                // header area adds to the top margin, a footer area to the
                // bottom one.
                Ok(Event::Start(e)) if e.name().as_ref() == "style:header-style" || e.name().as_ref() == "style:footer-style" => {
                    hf_style = Some(e.name().as_ref() == "style:header-style");
                }
                Ok(Event::End(e)) if e.name().as_ref() == "style:header-style" || e.name().as_ref() == "style:footer-style" => hf_style = None,
                Ok(Event::Empty(e)) | Ok(Event::Start(e)) if hf_style.is_some() && e.name().as_ref() == "style:header-footer-properties" => {
                    let len = |name: &str| attr_val(&e, name).and_then(|v| parse_length_pt(&v)).unwrap_or(0.0);
                    let height = len("fo:min-height").max(len("svg:height"));
                    let area = hf_areas.entry(layout_name.clone()).or_default();
                    let side = if hf_style == Some(true) { &mut area.0 } else { &mut area.1 };
                    *side = Some(side.unwrap_or(0.0) + height);
                }
                Ok(Event::Start(e)) if e.name().as_ref() == "style:master-page" => {
                    cur_master = attr_val(&e, "style:name");
                    in_master = cur_master == master;
                    if in_master {
                        master_layout = attr_val(&e, "style:page-layout-name");
                    }
                    if let Some(n) = cur_master.clone() {
                        master_pages.insert(n, MasterReading { layout: attr_val(&e, "style:page-layout-name"), ..Default::default() });
                    }
                }
                // A blank paragraph of a header or footer is a line of its
                // own, as Writer gives it one.
                Ok(Event::Empty(e)) if (in_any_header || in_any_footer) && hf_box_depth == 0 && matches!(e.name().as_ref(), "text:p" | "text:h") => {
                    if hf_row.is_none_or(|started| !started) {
                        hf_row = hf_row.map(|_| true);
                        let master = cur_master.as_ref().and_then(|n| master_pages.get_mut(n));
                        start_hf_line(master, &mut doc, in_any_header, in_header, in_footer);
                        let line = hf_line(attr_val(&e, "text:style-name").as_deref(), &named);
                        if in_header {
                            doc.header_lines.push(line);
                        } else if in_footer {
                            doc.footer_lines.push(line);
                        }
                    }
                }
                // Spaces and tabs are elements, not text.
                Ok(Event::Empty(e)) if (in_any_header || in_any_footer) && !in_field && matches!(e.name().as_ref(), "text:s" | "text:tab") => {
                    let gap = if e.name().as_ref() == "text:tab" {
                        "\t".to_string()
                    } else {
                        " ".repeat(attr_val(&e, "text:c").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1))
                    };
                    if let Some(m) = cur_master.as_ref().and_then(|n| master_pages.get_mut(n)) {
                        let target = if in_any_header { &mut m.header } else { &mut m.footer };
                        target.get_or_insert_with(String::new).push_str(&gap);
                    }
                    if let Some(b) = hf_box.as_mut().filter(|_| (in_header || in_footer) && hf_box_depth > 0) {
                        hf_box_text(b, &gap, &hf_spans, &named);
                    } else if in_header || in_footer {
                        let target = if in_header { &mut doc.header } else { &mut doc.footer };
                        target.get_or_insert_with(String::new).push_str(&gap);
                    }
                }
                Ok(Event::Empty(e)) if e.name().as_ref() == "style:master-page" => {
                    let name = attr_val(&e, "style:name");
                    if name == master {
                        master_layout = attr_val(&e, "style:page-layout-name");
                    }
                    if let Some(n) = name {
                        master_pages.insert(n, MasterReading { layout: attr_val(&e, "style:page-layout-name"), ..Default::default() });
                    }
                }
                Ok(Event::End(e)) if e.name().as_ref() == "style:master-page" => {
                    in_master = false;
                    cur_master = None;
                }
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == "draw:frame" && (in_header || in_footer) => {
                    let size = |a: &str| attr_val(&e, a).as_deref().and_then(length_emu);
                    let floating = attr_val(&e, "text:anchor-type").is_some_and(|t| matches!(t.as_str(), "paragraph" | "char" | "page"));
                    let anchor = floating.then(|| {
                        let offset = |a: &str| attr_val(&e, a).as_deref().and_then(offset_emu).unwrap_or(0);
                        crate::model::ImageAnchor {
                            x_emu: offset("svg:x"),
                            y_emu: offset("svg:y"),
                            ..attr_val(&e, "draw:style-name").and_then(|n| named.graphic.get(&n).copied()).unwrap_or_default()
                        }
                    });
                    hf_frame = Some((in_header, FrameReading { extent: size("svg:width").zip(size("svg:height")), sides: None, anchor, href: None, alt: String::new(), in_alt: false, depth: 0, text_box: None, look: Default::default() }));
                    hf_frame_place = hf_box_place(attr_val(&e, "draw:style-name").as_deref(), attr_val(&e, "svg:y").as_deref(), &named);
                }
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == "draw:image" => {
                    if let Some((_, f)) = hf_frame.as_mut().filter(|(_, f)| f.href.is_none()) {
                        f.href = attr_val(&e, "xlink:href");
                    }
                    if let Some(m) = cur_master.as_ref().and_then(|n| master_pages.get_mut(n)) {
                        m.header_pictures |= in_any_header;
                        m.footer_pictures |= in_any_footer;
                    }
                }
                Ok(Event::End(e)) if e.name().as_ref() == "draw:frame" => {
                    // A picture whose bytes are in the package, kept in the
                    // media cache like a body picture's.
                    if let Some((header, f)) = hf_frame.take() {
                        let href = f.href.filter(|h| !h.contains("://"));
                        let path = href.as_deref().and_then(|h| {
                            let bytes = zip.part_to_bytes(h.trim_start_matches("./"), &mut budget).ok()?;
                            suite_common_core::media_cache::persist(&bytes).ok()
                        });
                        if let Some(path) = path {
                            consumed.push(href.expect("read from it").trim_start_matches("./").to_string());
                            let run = Run {
                                text: f.alt.trim().to_string(),
                                style: RunStyle { image: Some(path.to_string_lossy().into_owned()), image_extent_emu: f.extent, image_anchor: f.anchor, ..Default::default() },
                            };
                            if header { doc.header_pictures.push(run) } else { doc.footer_pictures.push(run) }
                        }
                    }
                }
                // The first page's own header or footer: read as the running
                // one is, in its place. One not displayed is a first page
                // without.
                Ok(Event::Start(e)) if in_master && hf_first.is_none() && matches!(e.name().as_ref(), "style:header-first" | "style:footer-first") => {
                    let header = e.name().as_ref() == "style:header-first";
                    hf_first = Some((header, take_hf(&mut doc, header), cur_master.take()));
                    if attr_val(&e, "style:display").as_deref() == Some("false") {
                        hf_first_skip = true;
                    }
                    (in_header, in_any_header, in_footer, in_any_footer) = (header, header, !header, !header);
                }
                Ok(Event::End(e)) if matches!(e.name().as_ref(), "style:header-first" | "style:footer-first") => {
                    if let Some((header, running, master)) = hf_first.take() {
                        let (text, pictures, alignment, lines) = take_hf(&mut doc, header);
                        let story = if hf_first_skip { HfStory::default() } else { HfStory { text: text.unwrap_or_default(), pictures, alignment, lines } };
                        hf_first_skip = false;
                        *if header { &mut doc.first_header } else { &mut doc.first_footer } = Some(story);
                        put_hf(&mut doc, header, running);
                        cur_master = master;
                        (in_header, in_any_header, in_footer, in_any_footer) = (false, false, false, false);
                    }
                }
                Ok(Event::Empty(e)) if in_master && matches!(e.name().as_ref(), "style:header-first" | "style:footer-first") => {
                    *if e.name().as_ref() == "style:header-first" { &mut doc.first_header } else { &mut doc.first_footer } = Some(HfStory::default());
                }
                Ok(Event::Start(e)) => match e.name().as_ref() {
                    "style:page-layout" => layout_name = attr_val(&e, "style:name").unwrap_or_default(),
                    "style:header" => {
                        in_header = in_master;
                        in_any_header = true;
                        if let Some(m) = cur_master.as_ref().and_then(|n| master_pages.get_mut(n)).filter(|_| attr_val(&e, "style:display").as_deref() != Some("false")) {
                            m.has_header = true;
                        }
                    }
                    "style:footer" => {
                        in_footer = in_master;
                        in_any_footer = true;
                        if let Some(m) = cur_master.as_ref().and_then(|n| master_pages.get_mut(n)).filter(|_| attr_val(&e, "style:display").as_deref() != Some("false")) {
                            m.has_footer = true;
                        }
                    }
                    "text:p" | "text:h" if in_any_header || in_any_footer => {
                        if (in_header || in_footer) && hf_box_depth == 0 {
                            hf_para_style = attr_val(&e, "text:style-name");
                        }
                        if let Some(b) = hf_box.as_mut().filter(|_| (in_header || in_footer) && hf_box_depth > 0) {
                            b.lines.push((String::new(), hf_line(attr_val(&e, "text:style-name").as_deref(), &named), false));
                        }
                        if hf_box_depth == 0 {
                            let new_line = hf_row.is_none_or(|started| !started);
                            hf_row = hf_row.map(|_| true);
                            hf_cell_gap = !new_line;
                            if new_line {
                                let master = cur_master.as_ref().and_then(|n| master_pages.get_mut(n));
                                start_hf_line(master, &mut doc, in_any_header, in_header, in_footer);
                                hf_styled = false;
                            }
                            // A line is its first text's paragraph's: a
                            // table row's line is the cell's that holds
                            // its text, not its first, empty cell's.
                            if new_line || !hf_styled {
                                let line = hf_line(attr_val(&e, "text:style-name").as_deref(), &named);
                                let lines = if in_header { Some(&mut doc.header_lines) } else if in_footer { Some(&mut doc.footer_lines) } else { None };
                                if let Some(lines) = lines {
                                    if new_line {
                                        lines.push(line);
                                    } else if let Some(last) = lines.last_mut() {
                                        *last = line;
                                    }
                                }
                            }
                        }
                    }
                    "text:span" if in_header || in_footer => hf_spans.push(attr_val(&e, "text:style-name")),
                    "table:table-row" if in_any_header || in_any_footer => hf_row = Some(false),
                    "draw:text-box" if in_header || in_footer => {
                        hf_box_depth += 1;
                        if hf_box.is_none() {
                            hf_box = Some(HfBox { lines: Vec::new(), float: hf_frame_place.0, across: hf_frame_place.1 });
                        }
                    }
                    // Writer's own text box: its paragraphs are inside it.
                    "draw:custom-shape" if in_header || in_footer => {
                        hf_box_depth += 1;
                        if hf_box.is_none() {
                            let (float, across) = hf_box_place(attr_val(&e, "draw:style-name").as_deref(), attr_val(&e, "svg:y").as_deref(), &named);
                            hf_box = Some(HfBox { lines: Vec::new(), float, across });
                        }
                    }
                    "svg:title" | "svg:desc" => in_alt = true,
                    "text:page-number" | "text:page-count" if in_any_header || in_any_footer => {
                        let placeholder = if e.name().as_ref() == "text:page-number" { "{page}" } else { "{total}" };
                        if let Some(m) = cur_master.as_ref().and_then(|n| master_pages.get_mut(n)) {
                            let target = if in_any_header { &mut m.header } else { &mut m.footer };
                            target.get_or_insert_with(String::new).push_str(placeholder);
                        }
                        if (in_header || in_footer) && hf_box_depth == 0 && !hf_styled {
                            hf_styled = true;
                            let lines = if in_header { &mut doc.header_lines } else { &mut doc.footer_lines };
                            if let (Some(line), Some(span)) = (lines.last_mut(), hf_spans.iter().rev().flatten().next()) {
                                hf_span_look(&mut line.look, span, &named);
                            }
                        }
                        if in_header || in_footer {
                            let align = hf_para_style.as_deref().map_or(Alignment::Left, para_alignment);
                            let (target, alignment) = if in_header { (&mut doc.header, &mut doc.header_alignment) } else { (&mut doc.footer, &mut doc.footer_alignment) };
                            if blank(target) {
                                *alignment = align;
                            }
                            target.get_or_insert_with(String::new).push_str(placeholder);
                        }
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
                    "draw:text-box" | "draw:custom-shape" if hf_box_depth > 0 => {
                        hf_box_depth -= 1;
                        if hf_box_depth == 0 {
                            if let Some(b) = hf_box.take() {
                                let (text, lines) = if in_header { (&mut doc.header, &mut doc.header_lines) } else { (&mut doc.footer, &mut doc.footer_lines) };
                                finish_hf_box(b, text, lines);
                            }
                        }
                    }
                    "table:table-row" => hf_row = None,
                    "style:header" => {
                        in_header = false;
                        in_any_header = false;
                    }
                    "style:footer" => {
                        in_footer = false;
                        in_any_footer = false;
                    }
                    "svg:title" | "svg:desc" => in_alt = false,
                    "text:page-number" | "text:page-count" => in_field = false,
                    "text:span" if in_header || in_footer => {
                        hf_spans.pop();
                    }
                    _ => {}
                },
                Ok(Event::Text(t)) if in_alt => {
                    if let Some((_, f)) = hf_frame.as_mut().filter(|(_, f)| f.alt.is_empty()) {
                        f.alt = unescape_text(&t);
                    }
                }
                Ok(Event::Text(_)) if in_field => {}
                Ok(Event::Text(t)) => {
                    let txt = unescape_text(&t);
                    let gap = hf_cell_gap && !txt.trim().is_empty();
                    if gap {
                        hf_cell_gap = false;
                    }
                    if let Some(m) = cur_master.as_ref().and_then(|n| master_pages.get_mut(n)).filter(|_| !txt.trim().is_empty()) {
                        if in_any_header {
                            append_hf(&mut m.header, &txt, gap);
                        }
                        if in_any_footer {
                            append_hf(&mut m.footer, &txt, gap);
                        }
                    }
                    if let Some(b) = hf_box.as_mut().filter(|_| (in_header || in_footer) && hf_box_depth > 0) {
                        hf_box_text(b, &txt, &hf_spans, &named);
                        continue;
                    }
                    if (in_header || in_footer) && hf_box_depth == 0 && !hf_styled && !txt.trim().is_empty() {
                        hf_styled = true;
                        let lines = if in_header { &mut doc.header_lines } else { &mut doc.footer_lines };
                        if let (Some(line), Some(span)) = (lines.last_mut(), hf_spans.iter().rev().flatten().next()) {
                            hf_span_look(&mut line.look, span, &named);
                        }
                    }
                    let align = hf_para_style.as_deref().map_or(Alignment::Left, para_alignment);
                    if in_header && !txt.trim().is_empty() {
                        if blank(&doc.header) {
                            doc.header_alignment = align;
                        }
                        append_hf(&mut doc.header, &txt, gap);
                    }
                    if in_footer && !txt.trim().is_empty() {
                        if blank(&doc.footer) {
                            doc.footer_alignment = align;
                        }
                        append_hf(&mut doc.footer, &txt, gap);
                    }
                }
                Ok(Event::GeneralRef(_)) if in_field || in_alt => {}
                Ok(Event::GeneralRef(r)) => {
                    let txt = resolve_general_ref(&r);
                    if let Some(m) = cur_master.as_ref().and_then(|n| master_pages.get_mut(n)) {
                        if in_any_header {
                            m.header.get_or_insert_with(String::new).push_str(&txt);
                        }
                        if in_any_footer {
                            m.footer.get_or_insert_with(String::new).push_str(&txt);
                        }
                    }
                    if let Some(b) = hf_box.as_mut().filter(|_| (in_header || in_footer) && hf_box_depth > 0) {
                        hf_box_text(b, &txt, &hf_spans, &named);
                    } else if in_header {
                        doc.header.get_or_insert_with(String::new).push_str(&txt);
                    } else if in_footer {
                        doc.footer.get_or_insert_with(String::new).push_str(&txt);
                    }
                }
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
        }
        // A header or footer of blank paragraphs shows nothing, but Writer
        // still gives the document's own its lines' room (a section's ""
        // is no header at all). Its pictures' row is the room of one that
        // holds pictures.
        for (t, lines, pictures) in [(&mut doc.header, &doc.header_lines, &doc.header_pictures), (&mut doc.footer, &doc.footer_lines, &doc.footer_pictures)] {
            if blank(t) {
                *t = (t.is_some() && !lines.is_empty() && pictures.is_empty()).then(|| "\n".repeat(lines.len() - 1));
            }
        }
        // The first page's own the same way: one of only pictures in the
        // line is their row. A picture set against the page takes none,
        // so the blank lines it floats over keep theirs.
        for s in [&mut doc.first_header, &mut doc.first_footer].into_iter().flatten() {
            if s.text.trim().is_empty() {
                let inline = !s.pictures.is_empty() && s.pictures.iter().all(|p| p.style.image_anchor.is_none());
                if s.lines.is_empty() || inline {
                    s.text.clear();
                    s.lines.clear();
                } else {
                    s.text = "\n".repeat(s.lines.len() - 1);
                }
            }
        }
        for t in master_pages.values_mut().flat_map(|m| [&mut m.header, &mut m.footer]) {
            if blank(t) {
                *t = None;
            }
        }
        // A master page's header and footer areas sit between its page
        // layout's margins and the body: each one it shows adds to its
        // margin. Its page is that layout so grown.
        // A first page with its own header or footer (`first`) places it
        // at that distance too, though the pages after show none.
        let grown = |layout: &str, m: Option<&MasterReading>, first: (bool, bool)| -> Option<PageGeometry> {
            let mut page = layouts.get(layout).copied()?;
            let (header, footer) = hf_areas.get(layout).copied().unwrap_or_default();
            // The page margin a header or footer area grows is how far it
            // sits from the page's edge.
            if let Some(header) = header.filter(|_| m.is_some_and(|m| m.has_header)) {
                page.header_pt = Some(page.margin_top_pt);
                page.margin_top_pt += header;
            } else if header.is_some() && first.0 {
                page.header_pt = Some(page.margin_top_pt);
            }
            if let Some(footer) = footer.filter(|_| m.is_some_and(|m| m.has_footer)) {
                page.footer_pt = Some(page.margin_bottom_pt);
                page.margin_bottom_pt += footer;
            } else if footer.is_some() && first.1 {
                page.footer_pt = Some(page.margin_bottom_pt);
            }
            Some(page)
        };
        let master_geometry: std::collections::HashMap<String, PageGeometry> =
            master_pages.iter().filter_map(|(n, m)| Some((n.clone(), grown(m.layout.as_deref()?, Some(m), (false, false))?))).collect();
        // The first page's layout; without a master page naming one that
        // exists, the last one read, as before.
        doc.page = match master_layout.filter(|n| layouts.contains_key(n)) {
            Some(layout) => grown(&layout, master.as_ref().and_then(|n| master_pages.get(n)), (doc.first_header.is_some(), doc.first_footer.is_some())),
            None => last_layout.and_then(|n| layouts.get(&n).copied()),
        };
        // A body that changes master page is several sections, the first
        // on the page just read; a list that breaks the contract (a switch
        // where no paragraph starts) leaves the document one section.
        let mut sections = master_sections(&mut doc, &switches, master.as_deref(), &master_pages, &master_geometry);
        // A section whose page has one column takes the columns of a
        // `text:section` wrapping its paragraphs: LibreOffice writes a Word
        // section's columns as one, from the section's start to at most
        // its end (empty paragraphs after it are still the section's).
        if !sections.is_empty() {
            let columns = section_style_columns(&content);
            for s in sections.iter_mut().filter(|s| s.geometry.columns <= 1) {
                let wrapping = text_sections
                    .iter()
                    .filter(|(_, start, end)| *start == s.range.start && *end > *start && *end <= s.range.end)
                    .find_map(|(name, _, _)| columns.get(name));
                if let Some(&(count, gap)) = wrapping {
                    s.geometry.columns = count;
                    if let Some(gap) = gap {
                        s.geometry.column_gap_pt = gap;
                    }
                }
            }
        }
        if !sections.is_empty() && crate::sections::is_valid(&doc, &sections) {
            doc.page = Some(sections[0].geometry);
            doc.next_section_id = sections.len() as u32;
            doc.sections = sections;
        }
    }

    continue_numbering(&mut doc.paragraphs, &top_lists);
    // Contextual spacing: no space between two paragraphs of one style
    // in one text (the body or a cell) where the style asks for it, as
    // the DOCX reader does. The form's list items, each ending in a line
    // break, stand one blank line apart, not that and Normal's space.
    for k in 1..doc.paragraphs.len() {
        let (Some((a, a_on)), Some((b, b_on))) = (spacing_ctx.get(&(k - 1)), spacing_ctx.get(&k)) else { continue };
        if a != b || doc.paragraphs[k - 1].style.table_cell != doc.paragraphs[k].style.table_cell {
            continue;
        }
        if *a_on {
            doc.paragraphs[k - 1].style.space_after_pt = 0.0;
        }
        if *b_on {
            doc.paragraphs[k].style.space_before_pt = 0.0;
        }
    }
    // A regular weight matters only where the paragraph is drawn bold (a
    // heading, the Title); LibreOffice writes "normal" on body text too.
    for p in &mut doc.paragraphs {
        let bold_look = p.style.heading.is_some() || p.style.named_style.as_deref().is_some_and(|n| n.eq_ignore_ascii_case("title"));
        if !bold_look {
            p.runs.iter_mut().for_each(|r| r.style.regular = false);
        }
    }
    doc.ensure_non_empty();
    doc.table_columns = column_widths.into_iter().filter_map(|(t, w)| Some((t, w.filter(|w| !w.is_empty())?))).collect();
    doc.table_rows = row_heights;
    doc.table_spans = cell_spans;
    doc.table_fills = cell_fills;
    // Writer's table edge sits at its margin, its first cell's text that
    // cell's padding (none unless named) further right.
    let tables: std::collections::BTreeSet<u32> = table_margins.keys().chain(cell_padding.keys()).copied().collect();
    doc.table_indent = tables
        .into_iter()
        .map(|t| (t, table_margins.get(&t).copied().unwrap_or(0.0) + cell_padding.get(&t).map_or(0.0, |p| p.left_pt)))
        .filter(|(_, indent)| indent.abs() > 0.005)
        .collect();
    doc.table_padding = cell_padding;
    doc.table_unruled = doc.paragraphs.iter().filter_map(|p| p.style.table_cell.map(|c| c.table)).filter(|t| !ruled_tables.contains(t)).collect();
    (doc.table_rule_colors, doc.table_rule_widths) = rule_looks;
    doc.table_open_edges = open_edges;
    doc.table_valign = valigns;
    // A column count LibreOffice recorded on a section instead of on the
    // page layout. Only consulted when the page layout said nothing, so
    // an explicit page-wide count still wins; a document of several
    // sections keeps each section's own page layout's.
    if let Some((count, gap)) = parse_section_columns(&content).filter(|_| doc.sections.is_empty()) {
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

/// A paragraph's named style, and whether its spacing gives way beside a
/// paragraph of the same style (`style:contextual-spacing`, the nearest
/// style in its chain that says): an automatic style is its parent's.
fn contextual_spacing(name: &str, auto: &AutoStyles, named: &AutoStyles) -> (String, bool) {
    let is_auto = |n: &str| auto.para.contains_key(n) || auto.para_parent.contains_key(n) || auto.para_text.contains_key(n);
    let mut style = None;
    let mut on = None;
    let mut cur = Some(name.to_string());
    for _ in 0..16 {
        let Some(n) = cur else { break };
        if style.is_none() && !is_auto(&n) {
            style = Some(n.clone());
        }
        if on.is_none() {
            on = auto.contextual.get(&n).or_else(|| named.contextual.get(&n)).copied();
        }
        cur = auto.para_parent.get(&n).or_else(|| named.para_parent.get(&n)).cloned();
    }
    (style.unwrap_or_else(|| name.to_string()), on.unwrap_or(false))
}

/// A span's text style over the text style around it: what the span
/// does not set, it takes from its paragraph (or outer span).
/// A text style's look: an automatic style's or a named one's (styles.xml's
/// "Responses", blue), over what its parents give it.
fn text_style(name: &str, auto: &AutoStyles, named: &AutoStyles) -> RunStyle {
    let mut chain = Vec::new();
    let mut cur = Some(name.to_string());
    for _ in 0..16 {
        let Some(n) = cur else { break };
        if let Some(st) = auto.text.get(&n).or_else(|| named.text.get(&n)) {
            chain.push(st.clone());
        }
        cur = auto.text_parent.get(&n).or_else(|| named.text_parent.get(&n)).cloned();
    }
    chain.into_iter().rev().fold(None, |outer: Option<RunStyle>, own| Some(inherit(outer.as_ref(), own))).unwrap_or_default()
}

fn inherit(outer: Option<&RunStyle>, own: RunStyle) -> RunStyle {
    let Some(outer) = outer else { return own };
    RunStyle {
        bold: own.bold || (outer.bold && !own.regular),
        regular: own.regular || (outer.regular && !own.bold),
        italic: own.italic || outer.italic,
        caps: own.caps || outer.caps,
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
        let members: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).filter(|n| n.starts_with("Pictures/")).collect();
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

    /// A letterhead's logo comes back in the header, before its text, and a
    /// footer's floating picture keeps its place; the bytes are stored in
    /// Pictures/ once with the body's.
    #[test]
    fn header_and_footer_pictures_round_trip() {
        use crate::model::{AnchorAlign, AnchorFrame, ImageAnchor};
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("logo.png");
        std::fs::write(&src, PNG).unwrap();
        let picture = |alt: &str, anchor| Run {
            text: alt.into(),
            style: RunStyle { image: Some(src.to_string_lossy().into_owned()), image_extent_emu: Some((1_343_025, 933_450)), image_anchor: anchor, ..Default::default() },
        };
        let seal = ImageAnchor { h_from: AnchorFrame::Margin, h_align: Some(AnchorAlign::End), v_from: AnchorFrame::Text, ..Default::default() };
        let mut d = Document::from_plain_text("body");
        d.header = Some("Page {page}".into());
        d.header_pictures = vec![picture("Department for Education", None)];
        d.footer_pictures = vec![picture("seal", Some(seal))];
        let out = dir.path().join("hf.odt");
        write(&d, &out).unwrap();
        let rt = read(out.to_str().unwrap()).unwrap();
        assert_eq!(rt.header.as_deref(), Some("Page {page}"));
        assert_eq!(rt.footer, None, "a footer of a picture alone has no text");
        let logo = &rt.header_pictures;
        assert_eq!(logo.len(), 1, "{:?}", rt.header_pictures);
        assert_eq!((logo[0].text.as_str(), logo[0].style.image_anchor), ("Department for Education", None));
        let (w, h) = logo[0].style.image_extent_emu.unwrap();
        assert!(w.abs_diff(1_343_025) < 10 && h.abs_diff(933_450) < 10, "{:?}", (w, h));
        assert_eq!(std::fs::read(logo[0].style.image.as_ref().unwrap()).unwrap(), PNG);
        assert_eq!(rt.footer_pictures.iter().map(|r| r.style.image_anchor).collect::<Vec<_>>(), [Some(seal)]);
    }

    /// A header's and footer's alignment survive a save and reopen.
    #[test]
    fn header_and_footer_alignment_round_trip() {
        let mut d = Document::from_plain_text("body");
        d.header = Some("FINANCE BILL".into());
        d.header_alignment = Alignment::Right;
        d.footer = Some("Page {page}".into());
        d.footer_alignment = Alignment::Center;
        let rt = round_trip(&d);
        assert_eq!((rt.header.as_deref(), rt.header_alignment), (Some("FINANCE BILL"), Alignment::Right));
        assert_eq!((rt.footer.as_deref(), rt.footer_alignment), (Some("Page {page}"), Alignment::Center));
    }

    /// Each line of a header and footer keeps its alignment, look and tab
    /// stops through a save and reopen, and its tabs and runs of spaces
    /// their place (raw, they are whitespace a reader collapses).
    #[test]
    fn header_and_footer_lines_round_trip() {
        use crate::model::HfLine;
        let mut d = Document::from_plain_text("body");
        d.header = Some("The Crime Contract 2025\nName of ship:\tID:".into());
        d.header_lines = vec![
            HfLine { alignment: Alignment::Center, look: RunStyle { bold: true, font_size_hp: Some(32), font_family: Some("Liberation Sans".into()), ..Default::default() }, ..Default::default() },
            HfLine { look: RunStyle { font_size_hp: Some(18), color: Some("231f20".into()), ..Default::default() }, tab_stops: vec![TabStop { pos_pt: 523.3, align: TabAlign::Right, leader: TabLeader::None }], ..Default::default() },
        ];
        d.footer = Some("{page}/{total}  MSF 2020".into());
        d.footer_lines = vec![HfLine { alignment: Alignment::Center, look: RunStyle { font_size_hp: Some(18), ..Default::default() }, ..Default::default() }];
        let rt = round_trip(&d);
        assert_eq!((rt.header.as_deref(), rt.footer.as_deref()), (d.header.as_deref(), d.footer.as_deref()));
        assert_eq!((&rt.header_lines, &rt.footer_lines), (&d.header_lines, &d.footer_lines));
    }

    /// A row kept on one page stays so through a save and reopen (its row
    /// style's `fo:keep-together="always"`), beside a row height.
    #[test]
    fn a_row_kept_whole_round_trips() {
        let mut d = Document::from_plain_text("after");
        let table = d.insert_table_at(0, 3, 1);
        d.table_whole_rows.entry(table).or_default().extend([0, 2]);
        d.table_rows.insert(table, vec![None, None, Some(crate::model::RowHeight { pt: 40.0, exact: false })]);
        let rt = round_trip(&d);
        let id = rt.paragraphs.iter().find_map(|p| p.style.table_cell).unwrap().table;
        assert_eq!(rt.table_whole_rows.get(&id).map(|r| r.iter().copied().collect::<Vec<_>>()), Some(vec![0, 2]));
        assert_eq!(rt.table_rows.get(&id).and_then(|r| r.get(2).copied().flatten()).map(|h| h.pt), Some(40.0));
    }

    /// A table sized as a share of the text's width stays so through a
    /// save and reopen (`table:align="margins"`); one with the file's own
    /// widths does not become one.
    #[test]
    fn a_table_sized_to_the_text_round_trips() {
        let mut d = Document::from_plain_text("after");
        let shared = d.insert_table_at(0, 1, 2);
        d.table_columns.insert(shared, vec![200.0, 300.0]);
        d.table_relative.insert(shared);
        let rt = round_trip(&d);
        let table = rt.paragraphs.iter().find_map(|p| p.style.table_cell).unwrap().table;
        assert_eq!(rt.table_relative.iter().copied().collect::<Vec<_>>(), [table]);
        d.table_relative.clear();
        assert!(round_trip(&d).table_relative.is_empty());
    }

    /// Capitals survive a save and reopen, the text in its own case.
    #[test]
    fn capitals_round_trip() {
        let mut d = Document::from_plain_text("Crime supervisor standard\nplain");
        d.paragraphs[0].runs[0].style.caps = true;
        let rt = round_trip(&d);
        assert_eq!((rt.paragraphs[0].text(), rt.paragraphs[0].runs[0].style.caps), ("Crime supervisor standard".to_string(), true));
        assert!(!rt.paragraphs[1].runs[0].style.caps);
    }

    /// A floating header or footer line survives a save and reopen as a
    /// text box set against the page, its text ahead of the line whose
    /// paragraph holds it.
    #[test]
    fn floating_header_and_footer_lines_round_trip() {
        use crate::model::{HfFloat, HfLine};
        let mut d = Document::from_plain_text("body");
        d.header = Some("OFFICIAL\nThe Crime Contract 2025".into());
        d.header_lines = vec![HfLine { alignment: Alignment::Center, look: RunStyle { font_size_hp: Some(24), ..Default::default() }, float: Some(HfFloat::Top(15.0)), ..Default::default() }, HfLine::default()];
        d.footer = Some("OFFICIAL\n{page}".into());
        d.footer_lines = vec![HfLine { alignment: Alignment::Right, float: Some(HfFloat::Bottom(10.0)), ..Default::default() }, HfLine::default()];
        let rt = round_trip(&d);
        assert_eq!((rt.header.as_deref(), rt.footer.as_deref()), (d.header.as_deref(), d.footer.as_deref()));
        assert_eq!((&rt.header_lines, &rt.footer_lines), (&d.header_lines, &d.footer_lines));
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

    /// Merged cells survive a save: `table:number-columns-spanned` and
    /// `-rows-spanned` on the cell, and a `table:covered-table-cell` at
    /// each position it covers.
    #[test]
    fn merged_cells_survive() {
        let mut d = Document::from_plain_text("after");
        let table = d.insert_table_at(0, 3, 3);
        for p in d.paragraphs.iter_mut() {
            if let Some(c) = p.style.table_cell {
                if (c.row, c.col) == (0, 0) || c.row > 0 && c.col > 0 || (c.row, c.col) == (1, 0) {
                    p.runs = vec![Run::plain(format!("r{}c{}", c.row, c.col))];
                }
            }
        }
        let spans = vec![CellSpan { row: 0, col: 0, rows: 1, cols: 3 }, CellSpan { row: 1, col: 0, rows: 2, cols: 1 }];
        d.table_spans.insert(table, spans.clone());
        let rt = round_trip(&d);
        assert_eq!(rt.table_spans.values().collect::<Vec<_>>(), [&spans]);
        let texts = |d: &Document| -> Vec<(u32, u32, String)> {
            d.paragraphs.iter().filter_map(|p| p.style.table_cell.map(|c| (c.row, c.col, p.text()))).filter(|(_, _, t)| !t.is_empty()).collect()
        };
        assert_eq!(texts(&rt), texts(&d));
    }

    /// A numbered item's label survives a save: each set of level labels
    /// is a list style, and a label that changes within a list splits it
    /// there, the item keeping its number.
    #[test]
    fn list_labels_survive() {
        let label = |prefix: &str, format, suffix: &str| Some(ListLabel { prefix: prefix.into(), format, suffix: suffix.into() });
        let mut d = Document::from_plain_text("one\na\ntwo\nthree\nfour\nbetween\n2.1");
        for (k, level, l) in [
            (0, 0, label("(", NumberFormat::Decimal, ")")),
            (1, 1, label("1.", NumberFormat::LowerLetter, "")),
            (2, 0, label("(", NumberFormat::Decimal, ")")),
            (3, 0, label("", NumberFormat::UpperRoman, ".")),
            (4, 0, None),
            (6, 0, label("2.1", NumberFormat::None, "")),
        ] {
            d.paragraphs[k].style.list = ListKind::Numbered;
            d.paragraphs[k].style.list_level = level;
            d.paragraphs[k].style.list_label = l;
        }
        let markers = |d: &Document| {
            let ordinals = crate::lists::ordinals(d.paragraphs.iter().map(|p| &p.style));
            d.paragraphs.iter().zip(ordinals).map(|(p, n)| crate::lists::marker_for(&p.style, n).unwrap_or_default()).collect::<Vec<_>>()
        };
        assert_eq!(markers(&d), ["(1)", "1.a", "(2)", "III.", "4.", "", "2.1"]);
        let rt = round_trip(&d);
        assert_eq!(markers(&rt), markers(&d));
        let labels = |d: &Document| d.paragraphs.iter().map(|p| p.style.list_label.clone()).collect::<Vec<_>>();
        assert_eq!(labels(&rt), labels(&d));
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

    /// A ruled table's cell that leaves a side bare keeps it bare: the
    /// expression-of-interest form's header row is two cells, the first
    /// with no right border and the second no left, which Writer draws as
    /// one band. Ruled all round, a line split the band. The bare sides
    /// survive a save, the cell's shading with them.
    #[test]
    fn a_cells_bare_side_stays_bare() {
        use crate::model::{CellEdges, OpenEdges};
        let content = "<office:document-content><office:automatic-styles>\
             <style:style style:name=\"A\" style:family=\"table-cell\"><style:table-cell-properties fo:border-top=\"0.5pt solid #000000\" fo:border-left=\"0.5pt solid #000000\" fo:border-bottom=\"0.5pt solid #000000\" fo:border-right=\"none\" fo:background-color=\"#CFDCE3\"/></style:style>\
             <style:style style:name=\"B\" style:family=\"table-cell\"><style:table-cell-properties fo:border=\"0.5pt solid #000000\" fo:border-left=\"none\"/></style:style>\
             <style:style style:name=\"C\" style:family=\"table-cell\"><style:table-cell-properties fo:border=\"0.5pt solid #000000\"/></style:style>\
             </office:automatic-styles><office:body><office:text><table:table>\
             <table:table-row><table:table-cell table:style-name=\"A\"><text:p>EOI project details</text:p></table:table-cell><table:table-cell table:style-name=\"B\"><text:p/></table:table-cell></table:table-row>\
             <table:table-row><table:table-cell table:style-name=\"C\"><text:p>Title</text:p></table:table-cell><table:table-cell table:style-name=\"C\"><text:p/></table:table-cell></table:table-row>\
             </table:table><text:p/></office:text></office:body></office:document-content>";
        let d = read_package(content, "<office:document-styles/>");
        let want = vec![
            CellEdges { row: 0, col: 0, open: OpenEdges { right: true, ..Default::default() } },
            CellEdges { row: 0, col: 1, open: OpenEdges { left: true, ..Default::default() } },
        ];
        assert_eq!(d.table_open_edges.values().collect::<Vec<_>>(), [&want]);
        assert!(d.table_unruled.is_empty());
        let rt = round_trip(&d);
        assert_eq!(rt.table_open_edges.values().cloned().collect::<Vec<_>>(), [want]);
        assert_eq!(rt.table_fills.values().flatten().count(), 1, "the bare cell keeps its shading");
    }

    /// A cell set lower than the top (`style:vertical-align`) keeps it,
    /// through a save too: the vessel declaration's labels sit in the
    /// middle of their rows.
    #[test]
    fn a_cells_vertical_alignment_survives() {
        use crate::model::{CellVAlign, VAlign};
        let content = "<office:document-content><office:automatic-styles>\
             <style:style style:name=\"M\" style:family=\"table-cell\"><style:table-cell-properties fo:border=\"0.5pt solid #000000\" style:vertical-align=\"middle\"/></style:style>\
             <style:style style:name=\"B\" style:family=\"table-cell\"><style:table-cell-properties fo:border=\"0.5pt solid #000000\" style:vertical-align=\"bottom\"/></style:style>\
             <style:style style:name=\"T\" style:family=\"table-cell\"><style:table-cell-properties fo:border=\"0.5pt solid #000000\" style:vertical-align=\"top\"/></style:style>\
             </office:automatic-styles><office:body><office:text><table:table><table:table-row>\
             <table:table-cell table:style-name=\"M\"><text:p>a</text:p></table:table-cell><table:table-cell table:style-name=\"B\"><text:p>b</text:p></table:table-cell><table:table-cell table:style-name=\"T\"><text:p>c</text:p></table:table-cell>\
             </table:table-row></table:table><text:p/></office:text></office:body></office:document-content>";
        let d = read_package(content, "<office:document-styles/>");
        let want = vec![CellVAlign { row: 0, col: 0, align: VAlign::Center }, CellVAlign { row: 0, col: 1, align: VAlign::Bottom }];
        assert_eq!(d.table_valign.values().collect::<Vec<_>>(), [&want]);
        let rt = round_trip(&d);
        assert_eq!(rt.table_valign.values().collect::<Vec<_>>(), [&want]);
        assert!(rt.table_unruled.is_empty(), "the cells keep their rules");
    }

    /// A table's cell padding survives a save and reopen, in every cell's
    /// style, a shaded cell's included.
    #[test]
    fn table_cell_padding_survives() {
        use crate::model::{CellFill, CellPadding};
        let mut d = Document::from_plain_text("after");
        let table = d.insert_table_at(0, 2, 2);
        let want = CellPadding { top_pt: 5.65, bottom_pt: 5.65, left_pt: 5.65, right_pt: 4.0 };
        d.table_padding.insert(table, want);
        d.table_fills.insert(table, vec![CellFill { row: 0, col: 0, color: "D9D9D9".into() }]);
        let rt = round_trip(&d);
        assert_eq!(rt.table_padding.values().copied().collect::<Vec<_>>(), [want]);
        assert_eq!(rt.table_fills.values().flatten().count(), 1);
        assert!(rt.table_indent.is_empty(), "its text still at the margin: {:?}", rt.table_indent);
        d.table_indent.insert(table, 30.0);
        assert_eq!(round_trip(&d).table_indent.values().map(|v| (v * 100.0).round() / 100.0).collect::<Vec<_>>(), [30.0], "and an indent survives");
        let mut d = Document::from_plain_text("after");
        d.insert_table_at(0, 1, 1);
        assert!(round_trip(&d).table_padding.is_empty());
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

    /// A table Letters made is written with ruled cells, as Writer draws
    /// a cell's rule only from its style, and reads back ruled; one read
    /// as unruled is written and read back without rules.
    #[test]
    fn table_rules_survive_both_ways() {
        let mut d = Document::from_plain_text("after");
        let table = d.insert_table_at(0, 1, 2);
        assert!(round_trip(&d).table_unruled.is_empty());
        d.table_unruled.insert(table);
        assert_eq!(round_trip(&d).table_unruled.len(), 1);
    }

    /// A table's rules keep their colour and width through a save:
    /// Writer's grey cell borders ("0.5pt solid #bfbfbf") were drawn
    /// black, and a 0.75pt grid's rows each stood a quarter point short.
    #[test]
    fn a_table_keeps_its_rule_colour() {
        let mut d = Document::from_plain_text("after");
        let table = d.insert_table_at(0, 1, 2);
        d.table_rule_colors.insert(table, "BFBFBF".into());
        d.table_rule_widths.insert(table, 0.75);
        let rt = round_trip(&d);
        let id = rt.paragraphs.iter().find_map(|p| p.style.table_cell).unwrap().table;
        assert_eq!(rt.table_rule_colors.get(&id).map(String::as_str), Some("BFBFBF"));
        assert_eq!(rt.table_rule_widths.get(&id).copied(), Some(0.75), "and their width");
        d.table_rule_colors.clear();
        d.table_rule_widths.clear();
        let rt = round_trip(&d);
        assert!(rt.table_rule_colors.is_empty() && rt.table_rule_widths.is_empty(), "black 0.5pt rules are a table's own");
    }

    /// A table's heading rows (`table:table-header-rows`) are read as rows
    /// kept whole, as Writer never splits one across pages; the rows after
    /// them are not.
    #[test]
    fn heading_rows_are_kept_whole() {
        let content = "<office:document-content><office:body><office:text><table:table table:name=\"T\">\
            <table:table-column/><table:table-header-rows><table:table-row><table:table-cell><text:p>head</text:p></table:table-cell></table:table-row></table:table-header-rows>\
            <table:table-row><table:table-cell><text:p>body</text:p></table:table-cell></table:table-row></table:table>\
            <text:p>after</text:p></office:text></office:body></office:document-content>";
        let d = read_package(content, "<office:document-styles/>");
        let id = d.paragraphs.iter().find_map(|p| p.style.table_cell).unwrap().table;
        assert_eq!(d.table_whole_rows.get(&id).map(|r| r.iter().copied().collect::<Vec<_>>()), Some(vec![0]));
    }

    /// A first page's master that names a different next master gives
    /// the first page its own header and footer and the pages after it
    /// the next master's, as Writer lays it out: a form's logo header and
    /// version footer showed on every page (render-lab
    /// `real-expression-of-interest-form`).
    #[test]
    fn a_first_master_hands_on_to_its_next_style() {
        let styles = "<office:document-styles><office:master-styles>\
            <style:master-page style:name=\"MP0\" style:page-layout-name=\"pm1\"><style:footer><text:p>later</text:p></style:footer></style:master-page>\
            <style:master-page style:name=\"MPF0\" style:page-layout-name=\"pm1\" style:next-style-name=\"MP0\">\
            <style:header><text:p>logo</text:p></style:header><style:footer><text:p>V1.2</text:p></style:footer></style:master-page>\
            </office:master-styles></office:document-styles>";
        let content = "<office:document-content><office:automatic-styles><style:style style:name=\"P1\" style:family=\"paragraph\" style:master-page-name=\"MPF0\"/></office:automatic-styles>\
            <office:body><office:text><text:p text:style-name=\"P1\">body</text:p></office:text></office:body></office:document-content>";
        let d = read_package(content, styles);
        assert_eq!((d.header.as_deref(), d.footer.as_deref()), (None, Some("later")));
        assert_eq!(d.first_header.as_ref().map(|s| s.text.as_str()), Some("logo"));
        assert_eq!(d.first_footer.as_ref().map(|s| s.text.as_str()), Some("V1.2"));
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
    /// A list style's label-alignment position is where its items' text
    /// goes: a Word-made .odt's first level (margin 0.5in, label 0.25in
    /// back) is drawn with its text at 0.5in and its number at 0.25in, as
    /// Writer draws it, not at the built-in 18pt (render-lab
    /// `real-explanatory-note`, `real-salary-template`).
    #[test]
    fn list_items_sit_where_their_list_style_puts_them() {
        let content = "<office:document-content><office:automatic-styles>\
            <text:list-style style:name=\"L1\"><text:list-level-style-number text:level=\"1\" style:num-suffix=\".\" style:num-format=\"1\">\
              <style:list-level-properties text:list-level-position-and-space-mode=\"label-alignment\">\
              <style:list-level-label-alignment text:label-followed-by=\"listtab\" fo:margin-left=\"0.5in\" fo:text-indent=\"-0.25in\"/>\
              </style:list-level-properties></text:list-level-style-number></text:list-style>\
            </office:automatic-styles><office:body><office:text>\
            <text:list text:style-name=\"L1\"><text:list-item><text:p>one</text:p></text:list-item></text:list>\
            </office:text></office:body></office:document-content>";
        let d = read_package(content, "<office:document-styles/>");
        let item = &d.paragraphs[0];
        assert_eq!(item.style.list, ListKind::Numbered);
        let text_x = crate::layout::paragraph_box_x(item);
        assert!((text_x - 36.0).abs() < 0.01, "text at 0.5in: {text_x}");
        // A list Letters wrote reads back at its own indent.
        let mut d = Document::from_plain_text("a\nb");
        d.paragraphs[0].style.list = ListKind::Bullet;
        d.paragraphs[1].style.list = ListKind::Bullet;
        d.paragraphs[1].style.list_level = 1;
        let rt = round_trip(&d);
        assert_eq!(rt.paragraphs[0].style.left_indent_pt, 0.0);
        assert_eq!(rt.paragraphs[1].style.left_indent_pt, 0.0);
    }

    /// A label-aligned list's item whose paragraph sets its own indents
    /// stands where the paragraph says, as Writer draws it: the W1 notes'
    /// bullets (items at 0.2958in hanging 0.2958in, over a 0.5in level)
    /// sit at the page margin, their text at 0.2958in. Such an item
    /// writes its place back as Writer reads it.
    #[test]
    fn a_list_items_own_indents_replace_its_levels() {
        let content = "<office:document-content><office:automatic-styles>\
            <style:style style:name=\"P1\" style:family=\"paragraph\" style:list-style-name=\"L1\"><style:paragraph-properties fo:margin-left=\"0.2958in\" fo:text-indent=\"-0.2958in\"/></style:style>\
            <text:list-style style:name=\"L1\"><text:list-level-style-bullet text:level=\"1\" text:bullet-char=\"•\">\
              <style:list-level-properties text:list-level-position-and-space-mode=\"label-alignment\">\
              <style:list-level-label-alignment text:label-followed-by=\"listtab\" fo:margin-left=\"0.5in\" fo:text-indent=\"-0.25in\"/>\
              </style:list-level-properties></text:list-level-style-bullet></text:list-style>\
            </office:automatic-styles><office:body><office:text>\
            <text:list text:style-name=\"L1\"><text:list-item><text:p text:style-name=\"P1\">a director</text:p></text:list-item></text:list>\
            </office:text></office:body></office:document-content>";
        let d = read_package(content, "<office:document-styles/>");
        let item = &d.paragraphs[0];
        assert_eq!(item.style.list, ListKind::Bullet);
        let text_x = crate::layout::paragraph_box_x(item);
        assert!((text_x - 21.2976).abs() < 0.01, "text at 0.2958in: {text_x}");
        let hanging = item.style.list_hanging_pt.expect("the paragraph's hanging");
        assert!((text_x - hanging).abs() < 0.01, "the bullet at the margin: {hanging}");
        assert_eq!(item.style.first_line_indent_pt, 0.0);
        let rt = round_trip(&d).paragraphs[0].style.clone();
        assert!((rt.left_indent_pt - item.style.left_indent_pt).abs() < 0.01 && rt.list_hanging_pt.is_some_and(|h| (h - hanging).abs() < 0.01), "{rt:?}");
    }

    /// Writer's default paragraph style is the root of every style chain
    /// (a Word-made .odt keeps its space after and line height there),
    /// and a self-closing paragraph is a blank line with its style's
    /// spacing and font size, not a bare default (render-lab
    /// `real-salary-template`: its letter lost all paragraph spacing).
    #[test]
    fn the_default_style_roots_spacing_and_blank_lines_keep_their_style() {
        let content = "<office:document-content><office:automatic-styles>\
            <style:style style:name=\"P1\" style:parent-style-name=\"Normal\" style:family=\"paragraph\">\
              <style:text-properties style:font-name=\"Arial\" fo:font-size=\"14pt\"/></style:style>\
            </office:automatic-styles><office:body><office:text>\
            <text:p text:style-name=\"P1\">Header</text:p>\
            <text:p text:style-name=\"P1\"/>\
            <text:p>unstyled</text:p>\
            </office:text></office:body></office:document-content>";
        let styles = "<office:document-styles><office:font-face-decls>\
            <style:font-face style:name=\"Arial\" svg:font-family=\"Arial\"/></office:font-face-decls><office:styles>\
            <style:default-style style:family=\"paragraph\"><style:paragraph-properties fo:margin-bottom=\"0.1111in\" fo:line-height=\"107%\"/>\
              <style:text-properties fo:font-size=\"11pt\"/></style:default-style>\
            <style:style style:name=\"Normal\" style:family=\"paragraph\"><style:text-properties fo:hyphenate=\"false\"/></style:style>\
            </office:styles></office:document-styles>";
        let d = read_package(content, styles);
        assert_eq!(d.paragraphs.len(), 3);
        for p in &d.paragraphs {
            assert!((p.style.space_after_pt - 8.0).abs() < 0.01, "{:?}", p.style.space_after_pt);
            assert!((p.style.line_spacing - 1.07).abs() < 1e-4, "{:?}", p.style.line_spacing);
        }
        let blank = &d.paragraphs[1];
        assert_eq!(blank.text(), "");
        assert_eq!(blank.runs.first().map(|r| r.style.font_size_hp), Some(Some(28)), "the blank line is 14pt");
    }

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
            let name = f.name().unwrap().into_owned();
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
        d.paragraphs[0].style.tab_stops_pt = vec![TabStop::left(36.0), TabStop { pos_pt: 108.0, align: TabAlign::Right, leader: TabLeader::Dot }];
        let rt = round_trip(&d);
        assert_eq!(rt.paragraphs[0].style.tab_stops_pt.len(), 2, "stops lost");
        for (got, want) in rt.paragraphs[0].style.tab_stops_pt.iter().zip([36.0, 108.0]) {
            assert!((got.pos_pt - want).abs() < 0.01, "stop {} != {want}", got.pos_pt);
        }
        assert_eq!(rt.paragraphs[0].style.tab_stops_pt[1].align, TabAlign::Right);
        assert_eq!(rt.paragraphs[0].style.tab_stops_pt[1].leader, TabLeader::Dot);
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
        d.paragraphs[0].style.tab_stops_pt = vec![TabStop::left(36.0)];
        d.paragraphs[1].style.tab_stops_pt = vec![TabStop::left(144.0)];
        let rt = round_trip(&d);
        assert_eq!(rt.paragraphs[0].style.tab_stops_pt.len(), 1);
        assert_eq!(rt.paragraphs[1].style.tab_stops_pt.len(), 1);
        assert!((rt.paragraphs[0].style.tab_stops_pt[0].pos_pt - 36.0).abs() < 0.01);
        assert!(
            (rt.paragraphs[1].style.tab_stops_pt[0].pos_pt - 144.0).abs() < 0.01,
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

    /// A page layout's header and footer areas take room only on a master
    /// page that shows a header or footer: the social housing dictionary's
    /// layout declares a 0.5in header area its master never uses, and its
    /// body starts at the 1in margin, as Writer draws it.
    #[test]
    fn a_header_area_takes_room_only_where_the_master_shows_a_header() {
        let styles = |header: &str| {
            format!(
                "<office:document-styles><office:automatic-styles><style:page-layout style:name=\"PL\">\
                 <style:page-layout-properties fo:page-width=\"8.27in\" fo:page-height=\"11.69in\" fo:margin-top=\"1in\" fo:margin-bottom=\"1in\"/>\
                 <style:header-style><style:header-footer-properties fo:min-height=\"0.5in\"/></style:header-style>\
                 <style:footer-style><style:header-footer-properties fo:min-height=\"0.25in\"/></style:footer-style>\
                 </style:page-layout></office:automatic-styles><office:master-styles>\
                 <style:master-page style:name=\"Standard\" style:page-layout-name=\"PL\">{header}</style:master-page>\
                 </office:master-styles></office:document-styles>"
            )
        };
        let content = "<office:document-content><office:body><office:text><text:p>body</text:p></office:text></office:body></office:document-content>";
        let margins = |header: &str| {
            let page = read_package(content, &styles(header)).page.unwrap();
            (page.margin_top_pt, page.margin_bottom_pt)
        };
        assert_eq!(margins("<style:footer><text:p>1</text:p></style:footer>"), (72.0, 90.0), "no header: the footer's area alone");
        assert_eq!(margins("<style:header><text:p>Report</text:p></style:header>"), (108.0, 72.0), "the header's area alone");
        assert_eq!(margins("<style:header style:display=\"false\"><text:p>Report</text:p></style:header>"), (72.0, 72.0), "a header turned off takes none");
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

    /// A heading paragraph whose style says `fo:font-weight="normal"` is
    /// drawn regular, as Writer draws it: the vessel declaration's table
    /// cells are Heading 3 paragraphs set back to normal weight, and came
    /// out bold. Elsewhere "normal" is the default and is not kept. The
    /// weight survives a save.
    #[test]
    fn a_normal_weight_undoes_a_headings_bold() {
        let styles = "<office:document-styles><office:styles>\
             <style:style style:name=\"Heading3\" style:family=\"paragraph\"><style:text-properties fo:font-weight=\"bold\"/></style:style>\
             </office:styles></office:document-styles>";
        let content = "<office:document-content><office:automatic-styles>\
             <style:style style:name=\"P1\" style:family=\"paragraph\" style:parent-style-name=\"Heading3\"><style:text-properties fo:font-weight=\"normal\"/></style:style>\
             <style:style style:name=\"P2\" style:family=\"paragraph\"><style:text-properties fo:font-weight=\"normal\"/></style:style>\
             </office:automatic-styles><office:body><office:text>\
             <text:h text:style-name=\"P1\" text:outline-level=\"3\">Towing Operations</text:h><text:h text:style-name=\"Heading3\" text:outline-level=\"3\">Bold</text:h><text:p text:style-name=\"P2\">Body</text:p>\
             </office:text></office:body></office:document-content>";
        let d = read_package(content, styles);
        let regular: Vec<(bool, bool)> = d.paragraphs.iter().map(|p| (p.runs[0].style.bold, p.runs[0].style.regular)).collect();
        assert_eq!(regular, [(false, true), (false, false), (false, false)], "Heading 3 itself stays bold by its look");
        assert!(round_trip(&d).paragraphs[0].runs[0].style.regular);
    }

    /// The automatic font colour undoes a parent style's: a heading whose
    /// style says `use-window-font-color` over a blue Heading 1 is black.
    #[test]
    fn the_window_font_colour_undoes_a_parents() {
        let styles = "<office:document-styles><office:styles>\
             <style:style style:name=\"Heading1\" style:family=\"paragraph\"><style:text-properties fo:color=\"#0F4761\"/></style:style>\
             </office:styles></office:document-styles>";
        let content = "<office:document-content><office:automatic-styles>\
             <style:style style:name=\"P1\" style:family=\"paragraph\" style:parent-style-name=\"Heading1\"><style:text-properties style:use-window-font-color=\"true\"/></style:style>\
             </office:automatic-styles><office:body><office:text>\
             <text:h text:style-name=\"P1\" text:outline-level=\"1\">Notes</text:h><text:h text:style-name=\"Heading1\" text:outline-level=\"1\">Blue</text:h>\
             </office:text></office:body></office:document-content>";
        let d = read_package(content, styles);
        assert_eq!(d.paragraphs[0].runs[0].style.color.as_deref(), Some("000000"));
        assert_ne!(d.paragraphs[1].runs[0].style.color.as_deref(), Some("000000"), "Heading 1 itself is not undone");
    }

    /// A paragraph takes the indents its parent style sets and its own
    /// style leaves alone: the vessel declaration's cells are "Table
    /// Paragraph"s, 0.0395in in from the cell's edge, whose automatic
    /// styles only change the size. A list item keeps its list's indent.
    #[test]
    fn a_paragraph_takes_its_parent_styles_indents() {
        let styles = "<office:document-styles><office:styles>\
             <style:style style:name=\"TableParagraph\" style:family=\"paragraph\"><style:paragraph-properties fo:margin-left=\"0.5in\" fo:text-indent=\"0.25in\"/></style:style>\
             </office:styles></office:document-styles>";
        let content = "<office:document-content><office:automatic-styles>\
             <style:style style:name=\"P1\" style:family=\"paragraph\" style:parent-style-name=\"TableParagraph\"><style:text-properties fo:font-size=\"11pt\"/></style:style>\
             <style:style style:name=\"P2\" style:family=\"paragraph\" style:parent-style-name=\"TableParagraph\"><style:paragraph-properties fo:margin-left=\"0in\"/></style:style>\
             </office:automatic-styles><office:body><office:text>\
             <text:p text:style-name=\"P1\">inherits</text:p><text:p text:style-name=\"P2\">own</text:p><text:p>plain</text:p>\
             </office:text></office:body></office:document-content>";
        let d = read_package(content, styles);
        let indents: Vec<(f64, f64)> = d.paragraphs.iter().map(|p| (p.style.left_indent_pt, p.style.first_line_indent_pt)).collect();
        assert_eq!(indents, [(36.0, 18.0), (0.0, 18.0), (0.0, 0.0)]);
    }

    /// A header's paragraphs are its lines, the blank ones too, and a
    /// table row's cells one line: the explanatory note's header is a blank
    /// line, a row whose last cell says "FINANCE BILL", and a blank line,
    /// three lines tall in Writer. Read as one line, the body sat a third
    /// of an inch high on every page. The export writes the lines back.
    #[test]
    fn a_headers_paragraphs_are_its_lines() {
        let styles = "<office:document-styles><office:automatic-styles>\
             <style:style style:name=\"R\" style:family=\"paragraph\"><style:paragraph-properties fo:text-align=\"end\"/></style:style>\
             </office:automatic-styles><office:master-styles><style:master-page style:name=\"MP0\"><style:header><text:p/>\
             <table:table><table:table-row><table:table-cell><text:p/></table:table-cell><table:table-cell><text:p>FINANCE</text:p></table:table-cell>\
             <table:table-cell><text:p text:style-name=\"R\">BILL</text:p></table:table-cell></table:table-row></table:table><text:p/></style:header>\
             <style:footer><text:p/></style:footer></style:master-page></office:master-styles></office:document-styles>";
        let content = "<office:document-content><office:body><office:text><text:p>body</text:p></office:text></office:body></office:document-content>";
        let d = read_package(content, styles);
        assert_eq!(d.header.as_deref(), Some("\nFINANCE BILL\n"));
        assert_eq!((d.footer.as_deref(), d.footer_lines.len()), (Some(""), 1), "a footer of a blank paragraph shows nothing but takes its line's room");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h.odt");
        let path = path.to_str().unwrap();
        write(&d, path).unwrap();
        assert_eq!(read(path).unwrap().header.as_deref(), Some("\nFINANCE BILL\n"));
    }

    /// A header of a blank paragraph takes its line's room, as Writer
    /// gives it: the statistical release's empty header puts its body a
    /// 14pt line under the page margin, not the header area's 7pt minimum.
    /// (One whose paragraph holds a picture is the picture's row:
    /// `header_and_footer_pictures_round_trip`.)
    #[test]
    fn a_blank_header_takes_its_lines_room() {
        let layout = "<office:automatic-styles><style:page-layout style:name=\"PL0\"><style:page-layout-properties fo:margin-top=\"0.5in\"/>\
             <style:header-style><style:header-footer-properties style:dynamic-spacing=\"true\" fo:min-height=\"0.1in\"/></style:header-style></style:page-layout>\
             <style:style style:name=\"H\" style:family=\"paragraph\"><style:paragraph-properties fo:line-height=\"14pt\"/></style:style></office:automatic-styles>";
        let styles = |header: &str| format!(
            "<office:document-styles>{layout}<office:master-styles><style:master-page style:name=\"MP0\" style:page-layout-name=\"PL0\"><style:header>{header}</style:header></style:master-page></office:master-styles></office:document-styles>"
        );
        let content = "<office:document-content><office:body><office:text><text:p>body</text:p></office:text></office:body></office:document-content>";
        let d = read_package(content, &styles("<text:p text:style-name=\"H\"/>"));
        assert_eq!((d.header.as_deref(), d.header_lines.len()), (Some(""), 1));
        let page = crate::layout::layout(&d, &crate::layout::LayoutOptions::default(), &mut crate::layout::MonoShaper);
        let body = page.pages[0].items.iter().find_map(|i| match i { crate::layout::Item::Line { source: crate::layout::Source::Paragraph(0), top_pt, .. } => Some(*top_pt), _ => None });
        assert!(body.is_some_and(|y| y > 36.0 + 7.2), "the body under the header's line, not its 7.2pt minimum: {body:?}");
    }

    /// An inline picture's room beside it survives a save and reopen, as
    /// its frame's margins.
    #[test]
    fn an_inline_pictures_room_beside_it_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("p.png");
        std::fs::write(&src, PNG).unwrap();
        let mut d = Document::from_plain_text("caption");
        d.paragraphs[0].runs.push(Run {
            text: String::new(),
            style: RunStyle { image: Some(src.to_string_lossy().into_owned()), image_extent_emu: Some((914_400, 914_400)), image_sides_emu: Some((114_300, 50_800)), ..Default::default() },
        });
        let rt = round_trip(&d);
        let pic = rt.paragraphs.iter().flat_map(|p| &p.runs).find(|r| r.style.image.is_some()).expect("the picture");
        assert_eq!(pic.style.image_sides_emu, Some((114_300, 50_800)));
    }

    /// An ODF document's paragraph spacing adds up between paragraphs, as
    /// Writer spaces every one (measured: 30pt after and 40pt before stand
    /// 70pt apart, whatever its settings say); a Word file's does not.
    #[test]
    fn an_odf_documents_spacing_adds_up() {
        let content = "<office:document-content><office:body><office:text><text:p>body</text:p></office:text></office:body></office:document-content>";
        assert!(read_package(content, "<office:document-styles/>").space_adds);
        assert!(round_trip(&Document::from_plain_text("body")).space_adds, "a document saved as ODF is spaced as one");
    }

    /// The first page's own header: the screening committee's form has
    /// its logo over three blank lines on the first page and a running
    /// title on the others (`style:header-first`). Read as the running
    /// header, the logo never showed and the title sat a line high.
    #[test]
    fn the_first_pages_own_header_is_read_apart() {
        let styles = "<office:document-styles><office:styles>\
             <style:style style:name=\"Normal\" style:family=\"paragraph\"><style:paragraph-properties fo:margin-bottom=\"12pt\" fo:line-height=\"16pt\"/></style:style>\
             <style:style style:name=\"Header\" style:family=\"paragraph\" style:parent-style-name=\"Normal\"><style:paragraph-properties fo:line-height=\"10pt\"/></style:style>\
             </office:styles><office:master-styles><style:master-page style:name=\"MP0\">\
             <style:header><text:p text:style-name=\"Header\">Running title</text:p></style:header>\
             <style:footer><text:p>{page}</text:p></style:footer>\
             <style:header-first><text:p text:style-name=\"Header\">Letterhead</text:p><text:p text:style-name=\"Header\"/><text:p text:style-name=\"Header\"/></style:header-first>\
             <style:footer-first/></style:master-page></office:master-styles></office:document-styles>";
        let content = "<office:document-content><office:body><office:text><text:p>body</text:p></office:text></office:body></office:document-content>";
        let d = read_package(content, styles);
        assert_eq!(d.header.as_deref(), Some("Running title"), "the running header keeps its own text");
        let first = d.first_header.as_ref().expect("the first page's own header");
        assert_eq!(first.text, "Letterhead\n\n");
        // Each line has Normal's space after and its own exact height.
        assert_eq!(first.lines.len(), 3);
        assert!(first.lines.iter().all(|l| (l.space_after_pt - 12.0).abs() < 0.01 && l.line_height == Some(LineHeight::Exact(10.0))), "{:?}", first.lines);
        assert_eq!(d.first_footer.as_ref().map(|s| s.text.as_str()), Some(""), "an empty first footer is a first page without one");
    }

    /// The first page's own header and footer, their lines' spacing too,
    /// survive the writer and the reader.
    #[test]
    fn the_first_pages_own_header_round_trips() {
        use crate::model::{HfLine, HfStory};
        let mut d = Document::from_plain_text("body");
        d.header = Some("Running title".into());
        d.first_header = Some(HfStory {
            text: "Letterhead\n".into(),
            alignment: Alignment::Center,
            lines: vec![
                HfLine { alignment: Alignment::Center, space_after_pt: 12.0, line_height: Some(LineHeight::Exact(10.0)), ..Default::default() },
                HfLine { space_before_pt: 3.0, line_height: Some(LineHeight::Minimum(14.0)), ..Default::default() },
            ],
            ..Default::default()
        });
        d.first_footer = Some(HfStory::default());
        let rt = round_trip(&d);
        assert_eq!(rt.header.as_deref(), Some("Running title"));
        assert_eq!((&rt.first_header, &rt.first_footer), (&d.first_header, &d.first_footer));
    }

    /// Writer keeps a paragraph's space above and below as one setting: a
    /// style that sets either has none of its parent's other, and the
    /// nearest style that sets a line height decides it. The committee
    /// form's headings set their space above and a 100% line: under them
    /// Normal's 12pt after and exact 16pt line must not show.
    #[test]
    fn a_style_that_sets_its_space_above_has_none_below_from_its_parent() {
        let styles = "<office:document-styles><office:styles>\
             <style:style style:name=\"Normal\" style:family=\"paragraph\"><style:paragraph-properties fo:margin-bottom=\"12pt\" fo:line-height=\"16pt\"/></style:style>\
             <style:style style:name=\"Heading1\" style:family=\"paragraph\" style:parent-style-name=\"Normal\"><style:paragraph-properties fo:margin-top=\"24pt\" fo:line-height=\"100%\"/></style:style>\
             </office:styles></office:document-styles>";
        let content = "<office:document-content><office:automatic-styles>\
             <style:style style:name=\"P1\" style:family=\"paragraph\" style:parent-style-name=\"Heading1\"><style:paragraph-properties fo:break-before=\"page\"/></style:style>\
             </office:automatic-styles><office:body><office:text>\
             <text:h text:style-name=\"P1\" text:outline-level=\"1\">Title</text:h><text:p text:style-name=\"Normal\">body</text:p>\
             </office:text></office:body></office:document-content>";
        let d = read_package(content, styles);
        let st = |i: usize| &d.paragraphs[i].style;
        assert_eq!((st(0).space_before_pt, st(0).space_after_pt, st(0).line_height), (24.0, 0.0, None), "the heading's own");
        assert_eq!((st(1).space_after_pt, st(1).line_height), (12.0, Some(LineHeight::Exact(16.0))), "Normal's own");
    }

    /// A span named by a style of styles.xml takes its look, with what its
    /// parent gives it: the expression-of-interest form's "Responses"
    /// placeholders are blue, and an automatic style over it keeps that.
    #[test]
    fn a_named_text_style_colours_its_spans() {
        let styles = "<office:document-styles><office:styles>\
             <style:style style:name=\"Base\" style:family=\"text\"><style:text-properties fo:font-weight=\"bold\"/></style:style>\
             <style:style style:name=\"Responses\" style:family=\"text\" style:parent-style-name=\"Base\"><style:text-properties fo:color=\"#114F75\"/></style:style>\
             </office:styles></office:document-styles>";
        let content = "<office:document-content><office:automatic-styles>\
             <style:style style:name=\"T1\" style:family=\"text\" style:parent-style-name=\"Responses\"><style:text-properties fo:font-style=\"italic\"/></style:style>\
             </office:automatic-styles><office:body><office:text><text:p>Title: <text:span text:style-name=\"Responses\">Project title</text:span> \
             <text:span text:style-name=\"T1\">Deadline</text:span></text:p></office:text></office:body></office:document-content>";
        let d = read_package(content, styles);
        let run = |t: &str| d.paragraphs[0].runs.iter().find(|r| r.text == t).unwrap_or_else(|| panic!("{t}: {:?}", d.paragraphs[0].runs)).style.clone();
        let named = run("Project title");
        assert_eq!((named.color.as_deref(), named.bold), (Some("114f75"), true), "the named style and its parent");
        let auto = run("Deadline");
        assert_eq!((auto.color.as_deref(), auto.bold, auto.italic), (Some("114f75"), true, true), "an automatic style over it");
    }

    /// A paragraph named after a style in styles.xml takes its text
    /// properties and the ones its parents set: the form's "Coloured Box
    /// Headline" is bold and 14pt. Normal, the base, is the document's base
    /// font, not its runs'; a heading keeps its level's look.
    #[test]
    fn a_named_paragraph_style_styles_its_text() {
        let styles = "<office:document-styles><office:styles>\
             <style:style style:name=\"Normal\" style:family=\"paragraph\"><style:text-properties fo:font-size=\"10pt\" fo:color=\"#222222\"/></style:style>\
             <style:style style:name=\"Box\" style:family=\"paragraph\" style:parent-style-name=\"Normal\"><style:text-properties fo:color=\"#114F75\"/></style:style>\
             <style:style style:name=\"BoxHeadline\" style:family=\"paragraph\" style:parent-style-name=\"Box\"><style:text-properties fo:font-weight=\"bold\" fo:font-size=\"14pt\"/></style:style>\
             <style:style style:name=\"H\" style:family=\"paragraph\" style:parent-style-name=\"Box\"><style:text-properties fo:font-weight=\"bold\"/></style:style>\
             </office:styles></office:document-styles>";
        let content = "<office:document-content><office:body><office:text>\
             <text:p text:style-name=\"BoxHeadline\">Important</text:p>\
             <text:p text:style-name=\"Normal\">Body</text:p>\
             <text:h text:style-name=\"H\" text:outline-level=\"2\">Heading</text:h>\
             </office:text></office:body></office:document-content>";
        let d = read_package(content, styles);
        let first = |i: usize| d.paragraphs[i].runs[0].style.clone();
        let head = first(0);
        assert_eq!((head.bold, head.font_size_hp, head.color.as_deref()), (true, Some(28), Some("114f75")), "its own and its parent's");
        let body = first(1);
        assert_eq!((body.bold, body.font_size_hp, body.color.as_deref()), (false, None, None), "the base style is the base font");
        assert_eq!(first(2).color, None, "a heading looks as its level says");
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
            ..PageGeometry::default()
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

    /// A page border survives a save and reopen, the margins still
    /// running to the text: ODF's run to the border, its rule and padding
    /// inside them.
    #[test]
    fn a_page_border_round_trips() {
        let mut d = Document::from_plain_text("body");
        let border = PageBorder { width_pt: 0.5, color: 0x00_00_80, padding_pt: [1.0, 4.0, 1.0, 4.0] };
        d.page = Some(PageGeometry { margin_top_pt: 30.0, margin_left_pt: 40.0, border: Some(border), ..PageGeometry::default() });
        let pg = round_trip(&d).page.expect("page geometry lost");
        assert!(pg.approx_eq(&d.page.unwrap()), "geometry drifted: {pg:?}");
        assert_eq!(pg.border, Some(border));
    }

    /// Writer's page border, read from the declaration of vessel
    /// operation: the text starts past the margin, the rule and the
    /// padding.
    #[test]
    fn a_page_borders_rule_and_padding_move_the_text_in() {
        let styles = "<office:document-styles><office:automatic-styles><style:page-layout style:name=\"PL0\">\
             <style:page-layout-properties fo:page-width=\"8.268in\" fo:page-height=\"11.693in\" fo:margin-top=\"0.2944in\" fo:margin-left=\"0.4375in\" fo:margin-bottom=\"0.2944in\" fo:margin-right=\"0.4375in\" \
             fo:border=\"0.0069in solid #000000\" fo:padding-top=\"0.0138in\" fo:padding-left=\"0.0555in\" fo:padding-bottom=\"0.0138in\" fo:padding-right=\"0.0555in\"/>\
             </style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"PL0\"/></office:master-styles></office:document-styles>";
        let content = "<office:document-content><office:body><office:text><text:p>body</text:p></office:text></office:body></office:document-content>";
        let pg = read_package(content, styles).page.expect("the page");
        let b = pg.border.expect("the page's border");
        let close = |a: f64, b: f64| (a - b).abs() < 0.01;
        assert!(close(b.width_pt, 0.4968) && b.color == 0, "{b:?}");
        assert!(close(b.padding_pt[0], 0.9936) && close(b.padding_pt[3], 3.996), "{b:?}");
        assert!(close(pg.margin_top_pt, 0.2944 * 72.0 + 0.4968 + 0.9936), "{pg:?}");
        assert!(close(pg.margin_left_pt, 0.4375 * 72.0 + 0.4968 + 3.996), "{pg:?}");
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

    /// A span's face named only by its font declaration
    /// (`style:font-name`), as Writer saves a Word file's: the statistical
    /// release's Verdana body came out in the paragraph's Arial.
    #[test]
    fn a_spans_face_named_by_its_declaration_is_read() {
        let content = "<office:document-content><office:automatic-styles>\
             <style:style style:name=\"T1\" style:parent-style-name=\"DefaultParagraphFont\" style:family=\"text\"><style:text-properties style:font-name=\"Verdana\" style:font-name-complex=\"Arial\" fo:font-weight=\"bold\"/></style:style>\
             </office:automatic-styles><office:body><office:text>\
             <text:p text:style-name=\"Normal\"><text:span text:style-name=\"T1\">Impact</text:span></text:p>\
             </office:text></office:body></office:document-content>";
        let styles = "<office:document-styles><office:styles>\
             <style:style style:name=\"Normal\" style:family=\"paragraph\"><style:text-properties style:font-name=\"Arial\"/></style:style>\
             <style:style style:name=\"DefaultParagraphFont\" style:family=\"text\"/>\
             </office:styles></office:document-styles>";
        let run = &read_package(content, styles).paragraphs[0].runs[0];
        assert_eq!((run.text.as_str(), run.style.font_family.as_deref(), run.style.bold), ("Impact", Some("Verdana"), true));
    }

    /// Contextual spacing: two paragraphs of one style that asks for it
    /// (`style:contextual-spacing`) have no space between them, though
    /// each names its own automatic style; a paragraph of another style
    /// keeps its own. The screening committee form's list items stood a
    /// blank line and Normal's 12pt apart. A style that sets contextual
    /// spacing and no spacing of its own takes the document default's
    /// (8pt after here), not its parent's, as Writer keeps the two in one
    /// setting.
    #[test]
    fn contextual_spacing_drops_the_space_between_paragraphs_of_one_style() {
        let styles = "<office:document-styles><office:styles>\
             <style:default-style style:family=\"paragraph\"><style:paragraph-properties fo:margin-bottom=\"8pt\"/></style:default-style>\
             <style:style style:name=\"Normal\" style:family=\"paragraph\"><style:paragraph-properties fo:margin-bottom=\"12pt\"/></style:style>\
             <style:style style:name=\"ListParagraph\" style:family=\"paragraph\" style:parent-style-name=\"Normal\"><style:paragraph-properties style:contextual-spacing=\"true\"/></style:style>\
             </office:styles></office:document-styles>";
        let content = "<office:document-content><office:automatic-styles>\
             <style:style style:name=\"P1\" style:parent-style-name=\"ListParagraph\" style:family=\"paragraph\"><style:paragraph-properties fo:margin-left=\"36pt\"/></style:style>\
             <style:style style:name=\"P2\" style:parent-style-name=\"ListParagraph\" style:family=\"paragraph\"><style:paragraph-properties fo:margin-left=\"36pt\"/></style:style>\
             </office:automatic-styles><office:body><office:text>\
             <text:p text:style-name=\"P1\">one</text:p><text:p text:style-name=\"P2\">two</text:p><text:p text:style-name=\"Normal\">after</text:p>\
             </office:text></office:body></office:document-content>";
        let d = read_package(content, styles);
        let after: Vec<f64> = d.paragraphs.iter().map(|p| p.style.space_after_pt).collect();
        assert_eq!(after, [0.0, 8.0, 12.0], "only between the two list paragraphs, the last with the default's");
    }

    /// A line break within a paragraph (`text:line-break`, Shift+Enter)
    /// is read and written: the statistical release's title is two lines,
    /// and ran together as one.
    #[test]
    fn a_line_break_within_a_paragraph_survives() {
        let content = "<office:document-content><office:body><office:text>\
             <text:p>Statistical Release<text:line-break/>October<text:s/>2020</text:p>\
             </office:text></office:body></office:document-content>";
        let d = read_package(content, "<office:document-styles/>");
        assert_eq!(d.paragraphs.len(), 1);
        assert_eq!(d.paragraphs[0].runs[0].text, "Statistical Release\nOctober 2020");
        assert_eq!(round_trip(&d).paragraphs[0].runs, d.paragraphs[0].runs);
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

    /// A document of three sections (LET-005S) saves each one's page as a
    /// page layout and master page, and reopens with the same sections:
    /// a landscape two-column middle with its own header, then a part that
    /// starts with a table and has no footer.
    #[test]
    fn multi_section_odt_round_trips_geometry_and_headers() {
        let cell = |col, text: &str| Paragraph {
            style: ParaStyle { table_cell: Some(crate::model::TableCell { table: 0, row: 0, col }), ..Default::default() },
            runs: vec![Run::plain(text)],
        };
        let mut d = Document::from_plain_text("Intro\nBody\nWide one\nWide two");
        d.paragraphs.extend([cell(0, "a1"), cell(1, "b1"), Paragraph { style: ParaStyle::default(), runs: vec![Run::plain("after")] }]);
        d.header = Some("Report".into());
        d.footer = Some("Page {page}".into());
        let portrait = PageGeometry::default();
        let landscape = PageGeometry {
            width_pt: 842.0,
            height_pt: 595.0,
            margin_top_pt: 36.0,
            margin_bottom_pt: 36.0,
            margin_left_pt: 36.0,
            margin_right_pt: 36.0,
            columns: 2,
            column_gap_pt: 24.0,
            ..PageGeometry::default()
        };
        let last = PageGeometry { margin_left_pt: 50.0, margin_right_pt: 50.0, ..PageGeometry::default() };
        let section = |id: u32, start, end, geometry, header: Option<&str>, footer: Option<&str>| DocSection {
            id,
            range: ParagraphRange::new(start, end),
            geometry,
            header: header.map(str::to_string),
            footer: footer.map(str::to_string),
        };
        d.sections = vec![
            section(0, 0, 2, portrait, None, None),
            section(1, 2, 4, landscape, Some("Appendix {page}"), None),
            section(2, 4, 7, last, None, Some("")),
        ];
        d.next_section_id = 3;
        d.page = Some(portrait);
        assert!(crate::sections::is_valid(&d, &d.sections));

        let rt = round_trip(&d);
        let ranges: Vec<(usize, usize)> = rt.sections.iter().map(|s| (s.range.start, s.range.end)).collect();
        assert_eq!(ranges, vec![(0, 2), (2, 4), (4, 7)], "{:?}", rt.sections);
        for (got, want) in rt.sections.iter().zip(&d.sections) {
            assert!(got.geometry.approx_eq(&want.geometry), "section {}: {:?}", want.id, got.geometry);
        }
        assert_eq!(rt.sections.iter().map(|s| s.id).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(rt.next_section_id, 3);
        // The first section's header and footer are the document's; the
        // middle's header is its own and its footer continues; the last's
        // header continues the middle's and it has no footer.
        assert_eq!(rt.header.as_deref(), Some("Report"));
        assert_eq!(rt.footer.as_deref(), Some("Page {page}"));
        let stories: Vec<(Option<&str>, Option<&str>)> = rt.sections.iter().map(|s| (s.header.as_deref(), s.footer.as_deref())).collect();
        assert_eq!(stories, vec![(None, None), (Some("Appendix {page}"), None), (None, Some(""))]);
        assert!(rt.page.expect("the first section's page").approx_eq(&portrait));
        assert_eq!(rt.paragraphs[4].style.table_cell.map(|c| c.col), Some(0));
        assert_eq!(rt.paragraphs.iter().map(|p| p.text()).collect::<Vec<_>>(), d.paragraphs.iter().map(|p| p.text()).collect::<Vec<_>>());
    }

    /// A body paragraph whose style names another master page starts a
    /// section on that master's page layout (LET-005S), with the master's
    /// header ("" for none); one naming the master already in use, here
    /// through its parent style, is a page break.
    #[test]
    fn a_master_page_switch_starts_a_section() {
        let styles = "<office:document-styles><office:styles>\
             <style:style style:name=\"Wide\" style:family=\"paragraph\" style:master-page-name=\"Landscape\"/>\
             </office:styles><office:automatic-styles>\
             <style:page-layout style:name=\"PL0\"><style:page-layout-properties fo:page-width=\"8.27in\" fo:page-height=\"11.69in\" fo:margin-left=\"1in\"/></style:page-layout>\
             <style:page-layout style:name=\"PL1\"><style:page-layout-properties fo:page-width=\"11.69in\" fo:page-height=\"8.27in\" fo:margin-left=\"0.5in\">\
             <style:columns fo:column-count=\"2\" fo:column-gap=\"0.5in\"/></style:page-layout-properties></style:page-layout>\
             </office:automatic-styles><office:master-styles>\
             <style:master-page style:name=\"Standard\" style:page-layout-name=\"PL0\"><style:header><text:p>Main</text:p></style:header></style:master-page>\
             <style:master-page style:name=\"Landscape\" style:page-layout-name=\"PL1\"/>\
             </office:master-styles></office:document-styles>";
        let content = "<office:document-content><office:automatic-styles>\
             <style:style style:name=\"P2\" style:family=\"paragraph\" style:master-page-name=\"Landscape\"/>\
             <style:style style:name=\"P3\" style:family=\"paragraph\" style:parent-style-name=\"Wide\"/>\
             </office:automatic-styles><office:body><office:text>\
             <text:p>one</text:p><text:p text:style-name=\"P2\">two</text:p><text:p>three</text:p><text:p text:style-name=\"P3\">four</text:p>\
             </office:text></office:body></office:document-content>";
        let d = read_package(content, styles);
        assert_eq!(d.sections.len(), 2, "{:?}", d.sections);
        assert_eq!((d.sections[0].range.start, d.sections[0].range.end), (0, 1));
        assert_eq!((d.sections[1].range.start, d.sections[1].range.end), (1, 4));
        let wide = d.sections[1].geometry;
        assert!((wide.width_pt - 11.69 * 72.0).abs() < 1e-6 && (wide.height_pt - 8.27 * 72.0).abs() < 1e-6, "{wide:?}");
        assert!((wide.margin_left_pt - 36.0).abs() < 1e-6);
        assert_eq!((wide.columns, (wide.column_gap_pt - 36.0).abs() < 1e-6), (2, true));
        assert!((d.sections[0].geometry.margin_left_pt - 72.0).abs() < 1e-6);
        assert_eq!(d.page, Some(d.sections[0].geometry));
        assert_eq!(d.header.as_deref(), Some("Main"));
        assert_eq!((d.sections[0].header.as_deref(), d.sections[1].header.as_deref()), (None, Some("")));
        assert_eq!((d.sections[0].footer.as_deref(), d.sections[1].footer.as_deref()), (None, None));
        assert_eq!(d.next_section_id, 2);
        let breaks: Vec<bool> = d.paragraphs.iter().map(|p| p.style.page_break_before).collect();
        assert_eq!(breaks, vec![false, false, false, true]);
    }

    /// LibreOffice converting a Word section with columns writes them as a
    /// `text:section` around the section's paragraphs, its page style one
    /// column: the section (LET-005S) takes the text:section's columns;
    /// the first section, which no text:section wraps, keeps one.
    #[test]
    fn writer_section_columns_become_the_sections_columns() {
        let styles = "<office:document-styles><office:automatic-styles>\
             <style:page-layout style:name=\"PL0\"><style:page-layout-properties fo:page-width=\"8.27in\" fo:page-height=\"11.69in\"/></style:page-layout>\
             <style:page-layout style:name=\"PL1\"><style:page-layout-properties fo:page-width=\"11.69in\" fo:page-height=\"8.27in\">\
             <style:columns fo:column-count=\"1\" fo:column-gap=\"0in\"/></style:page-layout-properties></style:page-layout>\
             </office:automatic-styles><office:master-styles>\
             <style:master-page style:name=\"Standard\" style:page-layout-name=\"PL0\"/>\
             <style:master-page style:name=\"Converted1\" style:page-layout-name=\"PL1\"/>\
             </office:master-styles></office:document-styles>";
        let content = "<office:document-content><office:automatic-styles>\
             <style:style style:name=\"P2\" style:family=\"paragraph\" style:master-page-name=\"Converted1\"/>\
             <style:style style:name=\"Sect1\" style:family=\"section\"><style:section-properties>\
             <style:columns fo:column-count=\"2\" fo:column-gap=\"24pt\"><style:column style:rel-width=\"1*\"/><style:column style:rel-width=\"1*\"/></style:columns>\
             </style:section-properties></style:style>\
             </office:automatic-styles><office:body><office:text>\
             <text:p>one</text:p>\
             <text:section text:style-name=\"Sect1\" text:name=\"Section1\"><text:p text:style-name=\"P2\">two</text:p><text:p>three</text:p></text:section>\
             <text:p/>\
             </office:text></office:body></office:document-content>";
        let d = read_package(content, styles);
        assert_eq!(d.sections.len(), 2, "{:?}", d.sections);
        assert_eq!((d.sections[1].range.start, d.sections[1].range.end), (1, 4));
        let wide = d.sections[1].geometry;
        assert_eq!(wide.columns, 2, "{wide:?}");
        assert!((wide.column_gap_pt - 24.0).abs() < 1e-6, "{wide:?}");
        assert!((wide.width_pt - 11.69 * 72.0).abs() < 1e-6);
        assert_eq!(d.sections[0].geometry.columns, 1);
        assert_eq!(d.page, Some(d.sections[0].geometry));
    }

    /// A file of one master page reads as one section, even where a
    /// paragraph names that master again (a page break), and a document of
    /// one section saves one page layout and one master page.
    #[test]
    fn single_master_files_keep_one_page() {
        let styles = "<office:document-styles><office:automatic-styles>\
             <style:page-layout style:name=\"PL0\"><style:page-layout-properties fo:page-width=\"8.5in\" fo:page-height=\"11in\"/></style:page-layout>\
             </office:automatic-styles><office:master-styles>\
             <style:master-page style:name=\"Standard\" style:page-layout-name=\"PL0\"/>\
             </office:master-styles></office:document-styles>";
        let content = "<office:document-content><office:automatic-styles>\
             <style:style style:name=\"P1\" style:family=\"paragraph\" style:master-page-name=\"Standard\"/>\
             </office:automatic-styles><office:body><office:text>\
             <text:p text:style-name=\"P1\">one</text:p><text:p>two</text:p><text:p text:style-name=\"P1\">three</text:p>\
             </office:text></office:body></office:document-content>";
        let d = read_package(content, styles);
        assert!(d.sections.is_empty(), "{:?}", d.sections);
        assert_eq!(d.next_section_id, 0);
        assert!((d.page.expect("the master's page").width_pt - 612.0).abs() < 1e-6);
        assert!(d.paragraphs[2].style.page_break_before);

        let mut one = Document::from_plain_text("a\nb");
        one.header = Some("H".into());
        let xml = styles_xml(&one);
        assert_eq!(xml.matches("<style:page-layout ").count(), 1);
        assert_eq!(xml.matches("<style:master-page ").count(), 1);
        assert!(xml.contains("<style:master-page style:name=\"Standard\" style:page-layout-name=\"pm1\"><style:header><text:p>H</text:p></style:header></style:master-page>"));
        assert!(!content_xml(&one).contains("master-page-name"));
        assert!(round_trip(&one).sections.is_empty());
    }

    /// A text box (a frame holding a `draw:text-box`) is read as one
    /// floating object at its anchoring paragraph, with its size, rule,
    /// fill, padding and text, as the opt-out form's answer boxes are
    /// drawn in Writer. Its paragraphs used to be read as the body's: they
    /// replaced the paragraph that anchors the box.
    #[test]
    fn a_text_box_is_read_as_one_floating_box() {
        use crate::model::{AnchorAlign, AnchorFrame, BoxRule};
        let content = "<office:document-content><office:automatic-styles>\
            <style:style style:name=\"a1\" style:family=\"graphic\"><style:graphic-properties fo:border=\"0.75pt solid #000000\" \
              fo:padding-top=\"0.05in\" fo:padding-bottom=\"0.05in\" fo:padding-left=\"0.1in\" fo:padding-right=\"0.1in\" \
              draw:fill=\"solid\" draw:fill-color=\"#ffff00\" style:wrap=\"parallel\" \
              style:horizontal-pos=\"right\" style:horizontal-rel=\"page-content\" style:vertical-pos=\"from-top\" style:vertical-rel=\"paragraph\"/></style:style>\
            </office:automatic-styles><office:body><office:text>\
            <text:p>Your name<draw:frame draw:style-name=\"a1\" text:anchor-type=\"paragraph\" svg:y=\"0.1in\" svg:width=\"2in\" svg:height=\"0.5in\">\
              <draw:text-box><text:p>first<text:s text:c=\"2\"/>line</text:p><text:p>second</text:p></draw:text-box></draw:frame> here</text:p>\
            <text:p>after</text:p>\
            </office:text></office:body></office:document-content>";
        let d = read_package(content, "<office:document-styles/>");
        let texts: Vec<String> = d.paragraphs.iter().map(|p| p.runs.iter().map(|r| r.text.as_str()).collect()).collect();
        assert_eq!(texts, ["Your name here", "after"], "the box's text is not the body's");
        let boxes: Vec<&Run> = d.paragraphs[0].runs.iter().filter(|r| r.style.text_box.is_some()).collect();
        assert_eq!(boxes.len(), 1);
        let tb = boxes[0].style.text_box.as_deref().unwrap();
        assert_eq!(tb.extent_emu, (1_828_800, 457_200));
        assert_eq!(tb.text, "first  line\nsecond");
        assert_eq!(tb.border, Some(BoxRule { width_emu: 9525, color: "000000".into() }));
        assert_eq!(tb.fill.as_deref(), Some("FFFF00"));
        assert_eq!(tb.padding_emu, [45_720, 91_440, 45_720, 91_440]);
        let a = boxes[0].style.image_anchor.unwrap();
        assert_eq!((a.h_from, a.h_align, a.v_from, a.y_emu, a.wrap), (AnchorFrame::Margin, Some(AnchorAlign::End), AnchorFrame::Text, 91_440, crate::model::Wrap::Parallel));
    }

    /// A frame whose style fills it only with `fo:background-color` is
    /// unfilled, as Writer draws it.
    #[test]
    fn a_text_box_without_a_solid_fill_is_unfilled() {
        let content = "<office:document-content><office:automatic-styles>\
            <style:style style:name=\"a1\" style:family=\"graphic\"><style:graphic-properties fo:background-color=\"#ff0000\" fo:border=\"none\"/></style:style>\
            </office:automatic-styles><office:body><office:text>\
            <text:p><draw:frame draw:style-name=\"a1\" text:anchor-type=\"paragraph\" svg:width=\"1in\" svg:height=\"1in\"><draw:text-box/></draw:frame></text:p>\
            </office:text></office:body></office:document-content>";
        let d = read_package(content, "<office:document-styles/>");
        let tb = d.paragraphs[0].runs.iter().find_map(|r| r.style.text_box.as_deref()).expect("the empty box");
        assert_eq!((tb.border.as_ref(), tb.fill.as_ref(), tb.text.as_str()), (None, None, ""));
    }

    /// Writer's form of a Word text box, a rectangle drawing shape with its
    /// paragraphs inside and a stroke for its rule, is read as a text box; a
    /// line shape with no text is not one.
    #[test]
    fn a_shape_holding_text_is_read_as_a_text_box() {
        use crate::model::BoxRule;
        let content = "<office:document-content><office:automatic-styles>\
            <style:style style:name=\"gr1\" style:family=\"graphic\"><style:graphic-properties draw:stroke=\"solid\" svg:stroke-width=\"0.0138in\" \
              svg:stroke-color=\"#1f3864\" draw:fill=\"solid\" draw:fill-color=\"#deeaf6\" fo:padding-top=\"0.05in\" fo:padding-bottom=\"0.05in\" \
              fo:padding-left=\"0.1in\" fo:padding-right=\"0.1in\" style:horizontal-pos=\"right\" style:horizontal-rel=\"page-content\"/></style:style>\
            </office:automatic-styles><office:body><office:text>\
            <text:p>Sign here<draw:custom-shape text:anchor-type=\"char\" draw:style-name=\"gr1\" svg:width=\"2in\" svg:height=\"0.5in\" svg:y=\"0.1in\">\
              <text:p>Name</text:p><text:p>signed</text:p><draw:enhanced-geometry draw:type=\"ooxml-rect\"><draw:equation draw:name=\"f0\" draw:formula=\"logwidth/2\"/></draw:enhanced-geometry></draw:custom-shape>\
              <draw:custom-shape text:anchor-type=\"char\" svg:width=\"2in\" svg:height=\"0in\"><draw:enhanced-geometry draw:type=\"line\"/></draw:custom-shape></text:p>\
            <text:p>after</text:p>\
            </office:text></office:body></office:document-content>";
        let d = read_package(content, "<office:document-styles/>");
        let texts: Vec<String> = d.paragraphs.iter().map(|p| p.text()).collect();
        assert_eq!(texts, ["Sign here", "after"]);
        let boxes: Vec<&crate::model::TextBox> = d.paragraphs[0].runs.iter().filter_map(|r| r.style.text_box.as_deref()).collect();
        assert_eq!(boxes.len(), 1, "the line is not a box");
        assert_eq!((boxes[0].extent_emu, boxes[0].text.as_str()), ((1_828_800, 457_200), "Name\nsigned"));
        assert_eq!(boxes[0].border, Some(BoxRule { width_emu: 12_619, color: "1F3864".into() }));
        assert_eq!((boxes[0].fill.as_deref(), boxes[0].padding_emu), (Some("DEEAF6"), [45_720, 91_440, 45_720, 91_440]));
    }

    /// A text box keeps its place, size, look and text through a save.
    #[test]
    fn text_boxes_round_trip() {
        use crate::model::{AnchorAlign, AnchorFrame, BoxRule, ImageAnchor, TextBox, Wrap};
        let anchor = ImageAnchor { h_from: AnchorFrame::Margin, h_align: Some(AnchorAlign::End), v_from: AnchorFrame::Text, y_emu: 91_440, wrap: Wrap::TopBottom, ..Default::default() };
        let tb = TextBox {
            extent_emu: (1_828_800, 457_200),
            border: Some(BoxRule { width_emu: 12_700, color: "1F3864".into() }),
            fill: Some("DEEAF6".into()),
            padding_emu: [45_720, 91_440, 45_720, 91_440],
            text: "Name & date\n<signed>".into(),
        };
        let mut d = Document::from_plain_text("");
        d.paragraphs = vec![Paragraph {
            style: ParaStyle::default(),
            runs: vec![Run::plain("Sign "), Run { text: String::new(), style: RunStyle { text_box: Some(Box::new(tb.clone())), image_anchor: Some(anchor), ..Default::default() } }, Run::plain("here")],
        }];
        let rt = round_trip(&d);
        let runs = &rt.paragraphs[0].runs;
        let got = runs.iter().find(|r| r.style.text_box.is_some()).expect("the box");
        assert_eq!(got.style.text_box.as_deref(), Some(&tb));
        assert_eq!(got.style.image_anchor, Some(anchor));
        assert_eq!(runs.iter().map(|r| r.text.as_str()).collect::<String>(), "Sign here");
    }
}
