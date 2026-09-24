// layout — the Letters page layout engine (ADR 0010).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Input: a `Document`. Output: a `RenderTree` — pages, and on each page
// positioned lines, list markers, table cell boxes, header and footer —
// in points from the page's top-left corner. Screen, print, PDF and
// thumbnails all draw this one tree, so they cannot disagree about where
// a line or a page break is.
//
// Text shaping (fonts, line breaking, glyph metrics) is behind the
// `Shaper` trait. The engine itself is pure arithmetic over line boxes:
// it decides where each paragraph goes, never how its glyphs look. The
// production shaper is Pango (`layout::pango`, feature `render`); tests use
// `MonoShaper`, whose metrics are exact and font-free.

use serde::{Deserialize, Serialize};

use crate::lists;
use crate::model::{Document, ListKind, PageGeometry, Paragraph, Run};

#[cfg(feature = "render")]
pub mod pango;

/// Text defaults a document does not spell out itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutOptions {
    /// Page size and margins when the document has none.
    pub page: PageGeometry,
    /// Body font family when a run names none.
    pub font_family: String,
    /// Body font size in points when a run names none.
    pub font_size_pt: f64,
    /// Lines kept together at the foot (orphans) and head (widows) of a
    /// paragraph split across pages. 2 is Word's and LibreOffice's default.
    pub orphans: usize,
    pub widows: usize,
    /// Distance of the header's top from the page's top edge, and of the
    /// footer's bottom from the bottom edge (Word's default: 0.5in).
    pub header_distance_pt: f64,
    pub footer_distance_pt: f64,
}

impl LayoutOptions {
    /// These options with the document's own body font where it names one.
    /// `layout` applies it; a renderer re-shaping paragraphs must use the
    /// same result.
    pub fn for_document(&self, doc: &Document) -> LayoutOptions {
        let mut o = self.clone();
        if let Some(family) = &doc.base_font.family {
            o.font_family = family.clone();
        }
        if let Some(hp) = doc.base_font.size_hp.filter(|hp| *hp > 0) {
            o.font_size_pt = f64::from(hp) / 2.0;
        }
        o
    }
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            page: PageGeometry::default(),
            font_family: "Liberation Serif".into(),
            font_size_pt: 12.0,
            orphans: 2,
            widows: 2,
            header_distance_pt: 36.0,
            footer_distance_pt: 36.0,
        }
    }
}

/// What a shaper is asked to break into lines: one paragraph's runs at a
/// given width.
#[derive(Clone, Debug)]
pub struct ShapeRequest<'a> {
    pub runs: &'a [Run],
    /// Heading level 1–6, which scales and weights the text.
    pub heading: Option<u8>,
    /// Code block: monospace.
    pub code: bool,
    pub alignment: crate::model::Alignment,
    /// Width available to the lines, in points.
    pub width_pt: f64,
    /// Extra indent of the first line (negative = hanging), in points.
    pub first_line_indent_pt: f64,
    /// Tab stops in points from the text box's left edge.
    pub tab_stops_pt: Vec<f64>,
    pub defaults: &'a LayoutOptions,
}

/// One line of a shaped paragraph.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineBox {
    /// Char range of the paragraph's text on this line.
    pub start: usize,
    pub end: usize,
    /// Left edge of the ink box relative to the text box (alignment and
    /// first-line indent included).
    pub x_pt: f64,
    pub width_pt: f64,
    /// Natural (single-spaced) extents above and below the baseline.
    pub ascent_pt: f64,
    pub descent_pt: f64,
    /// Inline objects (images) on this line: their char index in the
    /// layout text and left edge relative to the text box.
    #[serde(default)]
    pub objects: Vec<(usize, f64)>,
}

impl LineBox {
    pub fn natural_height(&self) -> f64 {
        self.ascent_pt + self.descent_pt
    }
}

/// Breaks paragraphs into lines. Must return at least one line, also for an
/// empty paragraph (whose line has the height of its font).
pub trait Shaper {
    fn shape(&mut self, req: &ShapeRequest<'_>) -> Vec<LineBox>;
}

/// Where a line's text comes from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Source {
    /// `Document::paragraphs[i]`, body text or a table cell.
    Paragraph(usize),
    Header,
    Footer,
}

/// A positioned element of a page. Coordinates are points from the page's
/// top-left corner.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Item {
    /// Line `line` of the shaped `source`, drawn with its text box's left
    /// edge at `box_x_pt` (the line's own `x_pt` offset is inside it) and
    /// its baseline at `baseline_pt`. `box_width_pt` is the width it was
    /// shaped at, which a renderer needs to reproduce the same breaks.
    Line {
        source: Source,
        line: usize,
        start: usize,
        end: usize,
        text: String,
        box_x_pt: f64,
        box_width_pt: f64,
        x_pt: f64,
        top_pt: f64,
        baseline_pt: f64,
        height_pt: f64,
    },
    /// A list item's marker ("•", "3."), in the item's first-line font.
    Marker { para: usize, text: String, x_pt: f64, baseline_pt: f64 },
    /// An inline image, `src` as the model names it, in its box.
    Image { para: usize, src: String, x_pt: f64, y_pt: f64, width_pt: f64, height_pt: f64 },
    /// A table cell's border box.
    Cell { table: u32, row: u32, col: u32, x_pt: f64, y_pt: f64, width_pt: f64, height_pt: f64 },
}

/// One page of the render tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub index: usize,
    pub width_pt: f64,
    pub height_pt: f64,
    pub geometry: PageGeometry,
    pub items: Vec<Item>,
}

impl Page {
    /// The body lines on this page, in order: what a reader sees.
    pub fn lines(&self) -> impl Iterator<Item = &Item> {
        self.items.iter().filter(|i| matches!(i, Item::Line { source: Source::Paragraph(_), .. }))
    }
}

/// The laid-out document.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RenderTree {
    pub pages: Vec<Page>,
}

impl RenderTree {
    /// The page holding the first line of paragraph `para`, if any.
    pub fn page_of_paragraph(&self, para: usize) -> Option<usize> {
        self.pages.iter().position(|p| {
            p.items.iter().any(|i| matches!(i, Item::Line { source: Source::Paragraph(q), .. } if *q == para))
        })
    }
}

/// The object replacement character: an inline image's one char in the
/// layout text.
pub const OBJECT: char = '\u{FFFC}';

/// A paragraph's text as the layout sees it: every image is one
/// `OBJECT` char (the model keeps its alt text as the run's text).
/// Line char ranges index this text.
pub fn layout_text(runs: &[Run]) -> String {
    runs.iter()
        .map(|r| if r.style.image.is_some() { OBJECT.to_string() } else { r.text.clone() })
        .collect()
}

/// EMU per point (914400 per inch).
const EMU_PER_PT: f64 = 12700.0;

/// Displayed size of an image run in points, shrunk to fit `max_width_pt`
/// with its aspect kept: the document's extent if it has one, else the
/// image's own pixels at 96 dpi (PNG), else one inch square.
pub fn image_size_pt(run: &Run, max_width_pt: f64) -> (f64, f64) {
    let (w, h) = match run.style.image_extent_emu {
        Some((w, h)) => (w as f64 / EMU_PER_PT, h as f64 / EMU_PER_PT),
        None => run.style.image.as_deref().and_then(png_size_px).map_or((72.0, 72.0), |(w, h)| {
            (f64::from(w) * 0.75, f64::from(h) * 0.75)
        }),
    };
    if w > max_width_pt && w > 0.0 {
        (max_width_pt, h * max_width_pt / w)
    } else {
        (w, h)
    }
}

/// Pixel size from a PNG file's IHDR chunk.
fn png_size_px(path: &str) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut head = [0u8; 24];
    std::fs::File::open(path).ok()?.read_exact(&mut head).ok()?;
    if &head[..8] != b"\x89PNG\r\n\x1a\n" || &head[12..16] != b"IHDR" {
        return None;
    }
    let be = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    Some((be(&head[16..20]), be(&head[20..24])))
}

/// The run holding char `index` of a paragraph's layout text.
fn run_at(runs: &[Run], index: usize) -> Option<&Run> {
    let mut at = 0;
    for r in runs {
        let n = if r.style.image.is_some() { 1 } else { r.text.chars().count() };
        if index < at + n {
            return Some(r);
        }
        at += n;
    }
    None
}

/// Cell padding left and right, in points (Word's and LibreOffice's
/// default for a table: 0.08in).
pub const CELL_PADDING_PT: f64 = 5.4;

/// Width of the cell rules the painter draws, in points. A row is its
/// content plus one rule, as Word and LibreOffice measure it.
pub const CELL_RULE_PT: f64 = 0.5;

/// Lay out `doc` into pages.
pub fn layout(doc: &Document, opts: &LayoutOptions, shaper: &mut dyn Shaper) -> RenderTree {
    let opts = &opts.for_document(doc);
    let geometry = doc.page.unwrap_or(opts.page);
    let mut flow = Flow::new(geometry, opts);
    let ordinals = lists::ordinals(doc.paragraphs.iter().map(|p| &p.style));
    let col_w = flow.column_width();

    let mut i = 0;
    while i < doc.paragraphs.len() {
        let para = &doc.paragraphs[i];
        if let Some(cell) = para.style.table_cell {
            let end = doc.paragraphs[i..]
                .iter()
                .position(|p| p.style.table_cell.is_none_or(|c| c.table != cell.table))
                .map_or(doc.paragraphs.len(), |n| i + n);
            layout_table(&mut flow, doc, i..end, shaper);
            i = end;
            continue;
        }
        let shaped = shape_paragraph(para, col_w, opts, shaper);
        // Keep a heading with the first lines of what follows it.
        let keep_next = if para.style.heading.is_some() {
            doc.paragraphs.get(i + 1).filter(|n| n.style.table_cell.is_none()).map(|n| {
                let s = shape_paragraph(n, col_w, opts, shaper);
                let lines = s.lines.len().min(opts.orphans.max(1));
                n.style.space_before_pt
                    + s.lines[..lines].iter().map(|l| l.natural_height() * spacing(n)).sum::<f64>()
            })
        } else {
            None
        };
        place_paragraph(&mut flow, i, para, &shaped, ordinals[i], keep_next);
        i += 1;
    }
    flow.finish(doc, opts, shaper)
}

/// Line-height multiplier of a paragraph (1.0 when unset or nonsensical).
fn spacing(p: &Paragraph) -> f64 {
    let s = f64::from(p.style.line_spacing);
    if s.is_finite() && s > 0.0 { s } else { 1.0 }
}

/// A paragraph broken into lines, with the horizontal box it was shaped in.
struct Shaped {
    lines: Vec<LineBox>,
    /// Text box left edge, relative to the column's left edge.
    box_x: f64,
    box_w: f64,
    /// List marker x, relative to the column's left edge.
    marker_x: f64,
}

/// Left edge of a paragraph's text box, in points from its column's (or
/// table cell's) left edge: the left indent plus a list item's text indent.
pub fn paragraph_box_x(p: &Paragraph) -> f64 {
    let st = &p.style;
    let list_indent = if st.list == ListKind::None { 0.0 } else { lists::text_indent_pt(st.list_level) };
    st.left_indent_pt.max(0.0) + list_indent
}

/// The shaping request for paragraph `p` in a text box `box_w` wide. The
/// engine and every renderer build it here, so a renderer re-shaping a
/// paragraph to draw it gets exactly the lines the engine placed.
pub fn paragraph_request<'a>(p: &'a Paragraph, box_w: f64, opts: &'a LayoutOptions) -> ShapeRequest<'a> {
    let st = &p.style;
    let box_x = paragraph_box_x(p);
    ShapeRequest {
        runs: &p.runs,
        heading: st.heading,
        code: st.code_block.is_some(),
        alignment: st.alignment,
        width_pt: box_w,
        // A list item's first line starts at its text indent; the marker
        // hangs in front of it.
        first_line_indent_pt: if st.list == ListKind::None { st.first_line_indent_pt } else { 0.0 },
        tab_stops_pt: st.tab_stops_pt.iter().map(|t| t - box_x).filter(|t| *t > 0.0).collect(),
        defaults: opts,
    }
}

/// Size multiplier of heading level 1–6 relative to body text (the
/// editor's h1–h6 tags use the same scale).
pub fn heading_scale(level: u8) -> f64 {
    [1.6, 1.4, 1.2, 1.1, 1.0, 0.9][usize::from(level.clamp(1, 6) - 1)]
}

fn shape_paragraph(p: &Paragraph, avail_w: f64, opts: &LayoutOptions, shaper: &mut dyn Shaper) -> Shaped {
    let box_x = paragraph_box_x(p);
    let box_w = (avail_w - box_x - p.style.right_indent_pt.max(0.0)).max(1.0);
    let mut lines = shaper.shape(&paragraph_request(p, box_w, opts));
    if lines.is_empty() {
        let size = opts.font_size_pt;
        lines.push(LineBox { start: 0, end: 0, x_pt: 0.0, width_pt: 0.0, ascent_pt: size * 0.9, descent_pt: size * 0.25, objects: Vec::new() });
    }
    Shaped { lines, box_x, box_w, marker_x: box_x - lists::HANGING_PT.min(box_x) }
}

/// Place a shaped paragraph in the flow, splitting it across columns and
/// pages with orphan and widow control. `keep_next` is the height that must
/// fit below the paragraph on the same page (a heading's follower).
fn place_paragraph(flow: &mut Flow, idx: usize, para: &Paragraph, shaped: &Shaped, ordinal: u32, keep_next: Option<f64>) {
    let st = &para.style;
    if st.page_break_before && !flow.page_is_empty() {
        flow.new_page();
    }
    let ls = spacing(para);
    let heights: Vec<f64> = shaped.lines.iter().map(|l| l.natural_height() * ls).collect();
    let total: f64 = heights.iter().sum();

    // Between two paragraphs the larger of the space after the first and
    // the space before the second applies, not their sum: LibreOffice
    // draws Word documents that way (render-lab letters/paragraph-spacing:
    // 10pt after, then 24pt before, is a 24pt gap).
    flow.y += st.space_before_pt.max(0.0).max(std::mem::take(&mut flow.pending_after));
    // Everything that must stay together with the first line: the whole
    // paragraph if short, the orphan lines otherwise, plus a heading's
    // follower.
    let n = heights.len();
    let head: f64 = if n < flow.opts.orphans + flow.opts.widows {
        total
    } else {
        heights[..flow.opts.orphans.min(n)].iter().sum()
    };
    let needed = head + keep_next.map(|k| st.space_after_pt.max(0.0) + k).unwrap_or(0.0);
    if flow.y + needed > flow.bottom() && !flow.column_is_empty() {
        flow.next_column();
        flow.y += st.space_before_pt.max(0.0);
    }

    let text: Vec<char> = layout_text(&para.runs).chars().collect();
    let mut line = 0;
    while line < n {
        // How many lines fit here?
        let mut fit = 0;
        let mut y = flow.y;
        while line + fit < n && y + heights[line + fit] <= flow.bottom() + 1e-6 {
            y += heights[line + fit];
            fit += 1;
        }
        if line + fit < n {
            // Widows: leave at least `widows` lines for the next column.
            let rest = n - line - fit;
            if rest < flow.opts.widows {
                fit = fit.saturating_sub(flow.opts.widows - rest);
            }
            // Orphans: don't strand fewer than `orphans` lines (only matters
            // for the paragraph's first chunk).
            if line == 0 && fit < flow.opts.orphans.min(n) {
                fit = 0;
            }
            // An empty column takes at least one line, or nothing would fit
            // anywhere.
            if fit == 0 && flow.column_is_empty() {
                fit = 1;
            }
        }
        for (k, (lb, &height)) in shaped.lines.iter().zip(&heights).enumerate().skip(line).take(fit) {
            let top = flow.y;
            let baseline = top + lb.ascent_pt;
            let x0 = flow.column_x();
            if k == 0 && st.list != ListKind::None {
                if let Some(text) = lists::marker(st.list, ordinal) {
                    flow.push(Item::Marker { para: idx, text, x_pt: x0 + shaped.marker_x, baseline_pt: baseline });
                }
            }
            emit_line(flow, idx, para, &text, k, lb, x0 + shaped.box_x, shaped.box_w, top, height);
            flow.y += height;
        }
        line += fit;
        if line < n {
            flow.next_column();
        }
    }
    // Applied by whatever comes next, or dropped at the foot of a page.
    flow.pending_after = st.space_after_pt.max(0.0);
}

/// Emit line `k` of paragraph `idx` with its text box's left edge at
/// `box_x` and top at `top`, and the images on it.
#[allow(clippy::too_many_arguments)]
fn emit_line(flow: &mut Flow, idx: usize, para: &Paragraph, text: &[char], k: usize, lb: &LineBox, box_x: f64, box_w: f64, top: f64, height: f64) {
    let baseline = top + lb.ascent_pt;
    flow.push(Item::Line {
        source: Source::Paragraph(idx),
        line: k,
        start: lb.start,
        end: lb.end,
        text: text[lb.start.min(text.len())..lb.end.min(text.len())].iter().collect(),
        box_x_pt: box_x,
        box_width_pt: box_w,
        x_pt: box_x + lb.x_pt,
        top_pt: top,
        baseline_pt: baseline,
        height_pt: height,
    });
    for &(ch, x) in &lb.objects {
        let Some(run) = run_at(&para.runs, ch) else { continue };
        let Some(src) = run.style.image.clone() else { continue };
        let (w, h) = image_size_pt(run, box_w);
        flow.push(Item::Image { para: idx, src, x_pt: box_x + x, y_pt: baseline - h, width_pt: w, height_pt: h });
    }
}

/// A shaped paragraph of a table cell: its index, lines and height.
type CellParagraph = (usize, Shaped, f64);

/// Lay out the table made of cell paragraphs `range`: equal-width columns
/// across the column box, rows as tall as their tallest cell, and a row
/// never split across pages.
fn layout_table(flow: &mut Flow, doc: &Document, range: std::ops::Range<usize>, shaper: &mut dyn Shaper) {
    let cells: Vec<(usize, crate::model::TableCell)> =
        range.clone().filter_map(|i| doc.paragraphs[i].style.table_cell.map(|c| (i, c))).collect();
    let Some(&(_, first)) = cells.first() else { return };
    let cols = cells.iter().map(|(_, c)| c.col).max().unwrap_or(0) + 1;
    let rows = cells.iter().map(|(_, c)| c.row).max().unwrap_or(0) + 1;
    let table_w = flow.column_width();
    let col_w = table_w / f64::from(cols);
    let inner_w = (col_w - 2.0 * CELL_PADDING_PT).max(1.0);
    let opts = flow.opts.clone();
    flow.y += std::mem::take(&mut flow.pending_after);

    for row in 0..rows {
        // Shape each cell of the row (a cell may hold several paragraphs).
        let mut row_cells: Vec<(u32, Vec<CellParagraph>)> = Vec::new();
        let mut row_h: f64 = 0.0;
        for col in 0..cols {
            let mut h = 0.0;
            let mut paras = Vec::new();
            for &(i, _) in cells.iter().filter(|(_, c)| c.row == row && c.col == col) {
                let p = &doc.paragraphs[i];
                let s = shape_paragraph(p, inner_w, &opts, shaper);
                let ph: f64 = s.lines.iter().map(|l| l.natural_height() * spacing(p)).sum::<f64>()
                    + p.style.space_before_pt.max(0.0)
                    + p.style.space_after_pt.max(0.0);
                paras.push((i, s, ph));
                h += ph;
            }
            row_h = row_h.max(h + CELL_RULE_PT);
            row_cells.push((col, paras));
        }
        if flow.y + row_h > flow.bottom() && !flow.column_is_empty() {
            flow.next_column();
        }
        let top = flow.y;
        let x0 = flow.column_x();
        for (col, paras) in row_cells {
            // The table hangs one cell padding into the left margin, so
            // cell text lines up with the body text (Word's and
            // LibreOffice's default table indent).
            let cx = x0 - CELL_PADDING_PT + f64::from(col) * col_w;
            flow.push(Item::Cell { table: first.table, row, col, x_pt: cx, y_pt: top, width_pt: col_w, height_pt: row_h });
            let mut y = top;
            for (i, s, _) in paras {
                let p = &doc.paragraphs[i];
                y += p.style.space_before_pt.max(0.0);
                let text: Vec<char> = layout_text(&p.runs).chars().collect();
                for (k, lb) in s.lines.iter().enumerate() {
                    let h = lb.natural_height() * spacing(p);
                    emit_line(flow, i, p, &text, k, lb, cx + CELL_PADDING_PT + s.box_x, s.box_w, y, h);
                    y += h;
                }
                y += p.style.space_after_pt.max(0.0);
            }
        }
        flow.y = top + row_h;
    }
}

/// The cursor that fills columns and pages.
struct Flow<'o> {
    geometry: PageGeometry,
    opts: &'o LayoutOptions,
    pages: Vec<Page>,
    column: usize,
    /// Current vertical position, points from the page top.
    y: f64,
    /// Items placed in the current column.
    column_items: usize,
    /// Space after the last paragraph, not yet added (see place_paragraph).
    pending_after: f64,
}

impl<'o> Flow<'o> {
    fn new(geometry: PageGeometry, opts: &'o LayoutOptions) -> Self {
        let mut f = Flow { geometry, opts, pages: Vec::new(), column: 0, y: 0.0, column_items: 0, pending_after: 0.0 };
        f.new_page();
        f
    }

    fn columns(&self) -> usize {
        usize::from(self.geometry.columns.max(1))
    }

    fn content_width(&self) -> f64 {
        (self.geometry.width_pt - self.geometry.margin_left_pt - self.geometry.margin_right_pt).max(1.0)
    }

    fn column_width(&self) -> f64 {
        let n = self.columns() as f64;
        ((self.content_width() - self.geometry.column_gap_pt * (n - 1.0)) / n).max(1.0)
    }

    fn column_x(&self) -> f64 {
        self.geometry.margin_left_pt + self.column as f64 * (self.column_width() + self.geometry.column_gap_pt)
    }

    fn top(&self) -> f64 {
        self.geometry.margin_top_pt
    }

    fn bottom(&self) -> f64 {
        (self.geometry.height_pt - self.geometry.margin_bottom_pt).max(self.top() + 1.0)
    }

    fn page_is_empty(&self) -> bool {
        self.pages.last().is_none_or(|p| p.items.is_empty())
    }

    fn column_is_empty(&self) -> bool {
        self.column_items == 0
    }

    fn new_page(&mut self) {
        let index = self.pages.len();
        self.pages.push(Page {
            index,
            width_pt: self.geometry.width_pt,
            height_pt: self.geometry.height_pt,
            geometry: self.geometry,
            items: Vec::new(),
        });
        self.column = 0;
        self.y = self.top();
        self.column_items = 0;
        self.pending_after = 0.0;
    }

    fn next_column(&mut self) {
        if self.column + 1 < self.columns() {
            self.column += 1;
            self.y = self.top();
            self.column_items = 0;
        self.pending_after = 0.0;
        } else {
            self.new_page();
        }
    }

    fn push(&mut self, item: Item) {
        self.column_items += 1;
        self.pages.last_mut().expect("a page").items.push(item);
    }

    /// Add headers and footers now that the page count is known.
    fn finish(mut self, doc: &Document, opts: &LayoutOptions, shaper: &mut dyn Shaper) -> RenderTree {
        let total = self.pages.len();
        let width = self.content_width();
        let (left, height) = (self.geometry.margin_left_pt, self.geometry.height_pt);
        for page in &mut self.pages {
            for (source, template) in [(Source::Header, &doc.header), (Source::Footer, &doc.footer)] {
                let Some(template) = template.as_deref().filter(|t| !t.is_empty()) else { continue };
                let text = page_field_text(template, page.index + 1, total);
                let runs = [Run::plain(text.clone())];
                let lines = shaper.shape(&ShapeRequest {
                    runs: &runs,
                    heading: None,
                    code: false,
                    // Left, as a Word or LibreOffice header paragraph is by default.
                    alignment: crate::model::Alignment::Left,
                    width_pt: width,
                    first_line_indent_pt: 0.0,
                    tab_stops_pt: Vec::new(),
                    defaults: opts,
                });
                let block: f64 = lines.iter().map(LineBox::natural_height).sum();
                let mut y = match source {
                    Source::Header => opts.header_distance_pt,
                    _ => height - opts.footer_distance_pt - block,
                };
                let chars: Vec<char> = text.chars().collect();
                for (k, lb) in lines.iter().enumerate() {
                    page.items.push(Item::Line {
                        source: source.clone(),
                        line: k,
                        start: lb.start,
                        end: lb.end,
                        text: chars[lb.start.min(chars.len())..lb.end.min(chars.len())].iter().collect(),
                        box_x_pt: left,
                        box_width_pt: width,
                        x_pt: left + lb.x_pt,
                        top_pt: y,
                        baseline_pt: y + lb.ascent_pt,
                        height_pt: lb.natural_height(),
                    });
                    y += lb.natural_height();
                }
            }
        }
        RenderTree { pages: self.pages }
    }
}

/// Header/footer text for one page: "{page}" and "{total}" substituted.
pub fn page_field_text(template: &str, page: usize, total: usize) -> String {
    template.replace("{page}", &page.to_string()).replace("{total}", &total.to_string())
}

/// A font-free shaper with exact, predictable metrics: every char is
/// `0.5 × size` wide, a line is `size` ascent plus `0.25 × size` descent,
/// breaks fall at spaces (greedy), headings are scaled by `heading_scale`.
/// For tests of the engine, and for layout without fonts.
#[derive(Clone, Debug, Default)]
pub struct MonoShaper;

impl MonoShaper {
    fn size(req: &ShapeRequest<'_>, run: Option<&Run>) -> f64 {
        let base = match req.heading {
            Some(l) => req.defaults.font_size_pt * heading_scale(l),
            None => req.defaults.font_size_pt,
        };
        run.and_then(|r| r.style.font_size_hp).map_or(base, |hp| f64::from(hp) / 2.0)
    }
}

impl Shaper for MonoShaper {
    fn shape(&mut self, req: &ShapeRequest<'_>) -> Vec<LineBox> {
        // Flatten to (char, advance, ascent); an image is one char.
        let chars: Vec<(char, f64, f64)> = req
            .runs
            .iter()
            .flat_map(|r| -> Vec<(char, f64, f64)> {
                let s = Self::size(req, Some(r));
                if r.style.image.is_some() {
                    let (w, h) = image_size_pt(r, req.width_pt);
                    vec![(OBJECT, w, h)]
                } else {
                    r.text.chars().map(|c| (c, s * 0.5, s)).collect()
                }
            })
            .collect();
        let empty_size = Self::size(req, req.runs.first());
        let line = |start: usize, end: usize, width: f64, indent: f64| {
            let ascent = chars[start..end].iter().map(|c| c.2).fold(0.0, f64::max);
            let ascent = if ascent > 0.0 { ascent } else { empty_size };
            let text_size = chars[start..end].iter().filter(|c| c.0 != OBJECT).map(|c| c.2).fold(0.0, f64::max);
            let size = if text_size > 0.0 { text_size } else { empty_size };
            let free = (req.width_pt - indent - width).max(0.0);
            let align = match req.alignment {
                crate::model::Alignment::Center => free / 2.0,
                crate::model::Alignment::Right => free,
                _ => 0.0,
            };
            let mut x = indent + align;
            let mut objects = Vec::new();
            for (k, c) in chars.iter().enumerate().take(end).skip(start) {
                if c.0 == OBJECT {
                    objects.push((k, x));
                }
                x += c.1;
            }
            LineBox { start, end, x_pt: indent + align, width_pt: width, ascent_pt: ascent, descent_pt: size * 0.25, objects }
        };
        let mut out = Vec::new();
        let mut start = 0;
        while start < chars.len() || out.is_empty() {
            let indent = if out.is_empty() { req.first_line_indent_pt } else { 0.0 };
            let avail = req.width_pt - indent;
            let mut w = 0.0;
            let mut end = start;
            let mut last_break: Option<usize> = None;
            while end < chars.len() {
                let cw = chars[end].1;
                if w + cw > avail + 1e-9 && end > start {
                    break;
                }
                w += cw;
                if chars[end].0 == ' ' {
                    last_break = Some(end + 1);
                }
                end += 1;
            }
            if end < chars.len() {
                if let Some(b) = last_break {
                    end = b;
                }
            }
            // Trailing spaces hang past the edge and take no width.
            let ink_end = (start..end).rev().find(|&k| chars[k].0 != ' ').map_or(start, |k| k + 1);
            let width: f64 = chars[start..ink_end].iter().map(|c| c.1).sum();
            out.push(line(start, end, width, indent));
            if end == start {
                break;
            }
            start = end;
        }
        out
    }
}

#[cfg(test)]
mod tests;
