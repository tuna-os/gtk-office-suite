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
    /// The document's heading looks (levels 1–6), if it has its own.
    #[serde(default)]
    pub heading_styles: Vec<crate::model::RunStyle>,
}

impl LayoutOptions {
    /// The document's own look for heading `level`, if it has one.
    pub fn heading_style(&self, level: u8) -> Option<&crate::model::RunStyle> {
        self.heading_styles.get(usize::from(level.clamp(1, 6)) - 1)
    }

    /// Size in points of heading `level`'s text.
    pub fn heading_size_pt(&self, level: u8) -> f64 {
        match self.heading_style(level).and_then(|h| h.font_size_hp) {
            Some(hp) => f64::from(hp) / 2.0,
            None if self.heading_style(level).is_some() => self.font_size_pt,
            None => self.font_size_pt * heading_scale(level),
        }
    }

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
        o.heading_styles.clone_from(&doc.heading_styles);
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
            heading_styles: Vec::new(),
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
    /// Title, Subtitle or block quote, which have looks of their own.
    pub look: Look,
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

/// A key identifying everything a shaper's result depends on: the runs and
/// their styles, the paragraph's formatting and the width. Two requests
/// with one key shape identically.
pub fn request_key(req: &ShapeRequest<'_>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{:?}", req.runs).hash(&mut h);
    (req.heading, req.code, req.look, format!("{:?}", req.alignment)).hash(&mut h);
    (req.width_pt.to_bits(), req.first_line_indent_pt.to_bits()).hash(&mut h);
    req.tab_stops_pt.iter().map(|t| t.to_bits()).collect::<Vec<_>>().hash(&mut h);
    (&req.defaults.font_family, req.defaults.font_size_pt.to_bits()).hash(&mut h);
    if let Some(level) = req.heading {
        format!("{:?}", req.defaults.heading_style(level)).hash(&mut h);
    }
    h.finish()
}

/// Shaped paragraphs kept between layouts: an edit re-shapes only the
/// paragraphs it changed, and the rest of the relayout is arithmetic.
/// Entries not used by the latest layout are dropped (`prune`), so the cache
/// holds one document's worth.
#[derive(Default)]
pub struct ShapeCache {
    entries: std::collections::HashMap<u64, Vec<LineBox>>,
    used: std::collections::HashSet<u64>,
    /// Requests shaped (not found in the cache) since the last `prune`.
    pub misses: usize,
}

impl ShapeCache {
    /// Forget entries the last layout did not use; reset the miss count.
    pub fn prune(&mut self) {
        let used = std::mem::take(&mut self.used);
        self.entries.retain(|k, _| used.contains(k));
        self.misses = 0;
    }

    /// Keys of the entries kept.
    pub fn keys(&self) -> impl Iterator<Item = &u64> {
        self.entries.keys()
    }
}

/// A shaper that answers from `cache` when it can.
pub struct Cached<'a> {
    pub shaper: &'a mut dyn Shaper,
    pub cache: &'a mut ShapeCache,
}

impl Shaper for Cached<'_> {
    fn shape(&mut self, req: &ShapeRequest<'_>) -> Vec<LineBox> {
        let key = request_key(req);
        self.cache.used.insert(key);
        if let Some(lines) = self.cache.entries.get(&key) {
            return lines.clone();
        }
        self.cache.misses += 1;
        let lines = self.shaper.shape(req);
        self.cache.entries.insert(key, lines.clone());
        lines
    }
}

/// `layout`, re-shaping only what `cache` does not already hold. After an
/// edit to one paragraph, that is the one paragraph (and its neighbour when
/// a heading looks ahead).
pub fn relayout(doc: &Document, opts: &LayoutOptions, shaper: &mut dyn Shaper, cache: &mut ShapeCache) -> RenderTree {
    layout(doc, opts, &mut Cached { shaper, cache })
}

/// Where a line's text comes from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Source {
    /// `Document::paragraphs[i]`, body text or a table cell.
    Paragraph(usize),
    Header,
    Footer,
    /// Footnote `Document::footnotes[i]`, at the foot of its page
    /// (`note_paragraph`).
    Footnote(usize),
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
    /// A smart chip: its run's object char `ch` in paragraph `para`, drawn
    /// as a pill from `x_pt` on the line's baseline (the painter shapes its
    /// label, as the shaper measured it).
    Chip { para: usize, ch: usize, x_pt: f64, baseline_pt: f64 },
    /// A footnote reference: its run's object char `ch` in paragraph
    /// `para`, drawn as the note's superscript number from `x_pt`.
    NoteRef { para: usize, ch: usize, x_pt: f64, baseline_pt: f64 },
    /// The short rule above a page's footnotes.
    Rule { x_pt: f64, y_pt: f64, width_pt: f64 },
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

/// Whether a run is an inline object (an image, a footnote reference or a
/// smart chip): one `OBJECT` char in the layout and edit sequence, whatever
/// its text.
pub fn is_object(run: &Run) -> bool {
    run.style.image.is_some() || run.style.footnote.is_some() || run.style.chip.is_some()
}

/// Space around a smart chip's label, in points: horizontal padding inside
/// the pill, and the gap kept outside it on each side.
pub const CHIP_PAD_PT: f64 = 4.0;
pub const CHIP_GAP_PT: f64 = 1.5;

/// A paragraph's text as the layout sees it: every inline object (image,
/// footnote reference) is one `OBJECT` char (the model keeps an image's alt
/// text as the run's text, and a footnote reference has none). Line char
/// ranges, `pango::TextPos` and `edit`'s sequence offsets index this text.
pub fn layout_text(runs: &[Run]) -> String {
    runs.iter().map(|r| if is_object(r) { OBJECT.to_string() } else { r.text.clone() }).collect()
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
        let n = if is_object(r) { 1 } else { r.text.chars().count() };
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
/// Footnote text size relative to body text: Word's and LibreOffice's
/// default, 10 pt notes under 12 pt text.
pub const NOTE_SCALE: f64 = 10.0 / 12.0;

/// A footnote reference's number: the superscript size and rise relative
/// to the text it is in (as `VertAlign::Superscript` draws).
pub const NOTE_REF_SCALE: f64 = 0.58;
pub const NOTE_REF_RISE: f64 = 0.33;

/// Room between a page's body text and its footnotes, holding the
/// separator rule `NOTE_RULE_GAP_PT` above the first note (LibreOffice's
/// footnote area: a thin line and a small gap).
pub const NOTE_SEPARATOR_PT: f64 = 6.0;
pub const NOTE_RULE_GAP_PT: f64 = 2.0;

/// Length of the separator rule, relative to the text width (LibreOffice's
/// default footnote line).
pub const NOTE_RULE_FRACTION: f64 = 0.25;

/// Footnote `i` as the paragraph drawn at the foot of its page, as
/// LibreOffice draws a Word footnote: its number at body size, a space, and
/// its text at `NOTE_SCALE` of it, spaced like the document's body
/// paragraphs (a note is a Normal paragraph in Word). The engine and the
/// painter both build it here.
pub fn note_paragraph(doc: &Document, i: usize, opts: &LayoutOptions) -> Paragraph {
    let hp = (opts.font_size_pt * NOTE_SCALE * 2.0).round() as u16;
    let size = crate::model::RunStyle { font_size_hp: Some(hp), ..Default::default() };
    let text = doc.footnotes.get(i).cloned().unwrap_or_default();
    let body = doc
        .paragraphs
        .iter()
        .find(|p| p.style.heading.is_none() && p.style.table_cell.is_none() && p.style.list == ListKind::None && p.style.named_style.is_none())
        .map(|p| &p.style);
    Paragraph {
        style: crate::model::ParaStyle {
            line_spacing: body.map_or(1.0, |s| s.line_spacing),
            space_after_pt: body.map_or(0.0, |s| s.space_after_pt),
            ..Default::default()
        },
        runs: vec![Run::plain((i + 1).to_string()), Run { text: format!(" {text}"), style: size }],
    }
}

/// A footnote's height at the foot of a page, space after included.
fn note_height(p: &Paragraph, shaped: &Shaped) -> f64 {
    shaped.lines.iter().map(|l| l.natural_height() * spacing(p)).sum::<f64>() + p.style.space_after_pt.max(0.0)
}

/// The footnotes referenced on line `lb` of `para`.
fn line_notes(para: &Paragraph, lb: &LineBox) -> Vec<usize> {
    lb.objects.iter().filter_map(|&(ch, _)| run_at(&para.runs, ch)?.style.footnote).collect()
}

pub fn layout(doc: &Document, opts: &LayoutOptions, shaper: &mut dyn Shaper) -> RenderTree {
    let opts = &opts.for_document(doc);
    let geometry = doc.page.unwrap_or(opts.page);
    let mut flow = Flow::new(geometry, opts);
    // Each footnote's height at the foot of a page, for the flow to keep
    // room for the notes of the lines it places.
    let text_w = flow.content_width();
    flow.note_heights = (0..doc.footnotes.len())
        .map(|i| {
            let p = note_paragraph(doc, i, opts);
            note_height(&p, &shape_paragraph(&p, text_w, opts, shaper))
        })
        .collect();
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
    let quote_indent = if st.block_quote { QUOTE_INDENT_PT } else { 0.0 };
    st.left_indent_pt.max(0.0) + list_indent + quote_indent
}

/// How far a block quote is indented from the left, in points (the Draft
/// editor's `blockquote` tag: 24 px).
pub const QUOTE_INDENT_PT: f64 = 18.0;

/// A paragraph's look beyond body text, headings and code: the named
/// Title and Subtitle styles, and block quotes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Look {
    Body,
    /// Large and bold (Google Docs' and the old Letters stylesheet's 26 pt
    /// over 11 pt body text).
    Title,
    /// Larger than body text, grey (15 pt over 11 pt).
    Subtitle,
    /// Italic, indented by `QUOTE_INDENT_PT`.
    Quote,
}

impl Look {
    /// Text size relative to body text.
    pub fn scale(self) -> f64 {
        match self {
            Look::Title => 26.0 / 11.0,
            Look::Subtitle => 15.0 / 11.0,
            Look::Body | Look::Quote => 1.0,
        }
    }
}

/// The look of a paragraph styled `st`. A heading level wins over a named
/// style; a named style is matched as Word and LibreOffice spell it.
pub fn paragraph_look(st: &crate::model::ParaStyle) -> Look {
    if st.heading.is_some() {
        return Look::Body;
    }
    match st.named_style.as_deref().map(str::to_ascii_lowercase).as_deref() {
        Some("title") => Look::Title,
        Some("subtitle") => Look::Subtitle,
        _ if st.block_quote => Look::Quote,
        _ => Look::Body,
    }
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
        look: paragraph_look(st),
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
        // How many lines fit here, each with room for its footnotes?
        let mut fit = 0;
        let mut y = flow.y;
        let mut notes: Vec<usize> = Vec::new();
        while line + fit < n {
            let mut with = notes.clone();
            with.extend(line_notes(para, &shaped.lines[line + fit]));
            if y + heights[line + fit] > flow.bottom_with(&with) + 1e-6 {
                break;
            }
            notes = with;
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
            flow.add_notes(line_notes(para, lb));
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
        if run.style.chip.is_some() {
            flow.push(Item::Chip { para: idx, ch, x_pt: box_x + x, baseline_pt: baseline });
            continue;
        }
        if run.style.footnote.is_some() {
            flow.push(Item::NoteRef { para: idx, ch, x_pt: box_x + x, baseline_pt: baseline });
            continue;
        }
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
    /// Height of each footnote at the foot of a page.
    note_heights: Vec<f64>,
    /// The footnotes referenced on each page, in order.
    page_notes: Vec<Vec<usize>>,
}

impl<'o> Flow<'o> {
    fn new(geometry: PageGeometry, opts: &'o LayoutOptions) -> Self {
        let mut f = Flow {
            geometry,
            opts,
            pages: Vec::new(),
            column: 0,
            y: 0.0,
            column_items: 0,
            pending_after: 0.0,
            note_heights: Vec::new(),
            page_notes: Vec::new(),
        };
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

    /// The foot of the text area, above the bottom margin.
    fn foot(&self) -> f64 {
        (self.geometry.height_pt - self.geometry.margin_bottom_pt).max(self.top() + 1.0)
    }

    /// Room the footnotes `notes` take at the foot of a page: their heights
    /// and the separator above them.
    fn notes_height(&self, notes: &[usize]) -> f64 {
        if notes.is_empty() {
            return 0.0;
        }
        NOTE_SEPARATOR_PT + notes.iter().map(|&i| self.note_heights.get(i).copied().unwrap_or(0.0)).sum::<f64>()
    }

    /// Where body text must end on this page, above its footnotes.
    fn bottom(&self) -> f64 {
        self.bottom_with(&[])
    }

    /// Where body text must end if the lines being placed add the notes
    /// `more` to this page's.
    fn bottom_with(&self, more: &[usize]) -> f64 {
        let mut notes = self.page_notes.last().cloned().unwrap_or_default();
        notes.extend(more.iter().filter(|i| !notes.contains(i)).collect::<Vec<_>>());
        (self.foot() - self.notes_height(&notes)).max(self.top() + 1.0)
    }

    fn add_notes(&mut self, notes: Vec<usize>) {
        if let Some(page) = self.page_notes.last_mut() {
            for i in notes {
                if !page.contains(&i) {
                    page.push(i);
                }
            }
        }
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
        self.page_notes.push(Vec::new());
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
        // Each page's footnotes, ending at the foot of its text area under
        // a short rule.
        let foot = self.foot();
        for (page, notes) in self.pages.iter_mut().zip(&self.page_notes) {
            if notes.is_empty() {
                continue;
            }
            // The last note ends at the foot; its space after is not drawn.
            let after = notes.last().map_or(0.0, |&i| note_paragraph(doc, i, opts).style.space_after_pt.max(0.0));
            let mut y = foot + after - notes.iter().map(|&i| self.note_heights.get(i).copied().unwrap_or(0.0)).sum::<f64>();
            page.items.push(Item::Rule { x_pt: left, y_pt: y - NOTE_RULE_GAP_PT, width_pt: width * NOTE_RULE_FRACTION });
            for &i in notes {
                let p = note_paragraph(doc, i, opts);
                let shaped = shape_paragraph(&p, width, opts, shaper);
                let chars: Vec<char> = layout_text(&p.runs).chars().collect();
                let ls = spacing(&p);
                for (k, lb) in shaped.lines.iter().enumerate() {
                    page.items.push(Item::Line {
                        source: Source::Footnote(i),
                        line: k,
                        start: lb.start,
                        end: lb.end,
                        text: chars[lb.start.min(chars.len())..lb.end.min(chars.len())].iter().collect(),
                        box_x_pt: left,
                        box_width_pt: width,
                        x_pt: left + lb.x_pt,
                        top_pt: y,
                        baseline_pt: y + lb.ascent_pt,
                        height_pt: lb.natural_height() * ls,
                    });
                    y += lb.natural_height() * ls;
                }
                y += p.style.space_after_pt.max(0.0);
            }
        }
        for page in &mut self.pages {
            for (source, template) in [(Source::Header, &doc.header), (Source::Footer, &doc.footer)] {
                let Some(template) = template.as_deref().filter(|t| !t.is_empty()) else { continue };
                let text = page_field_text(template, page.index + 1, total);
                let runs = [Run::plain(text.clone())];
                let lines = shaper.shape(&ShapeRequest {
                    runs: &runs,
                    heading: None,
                    code: false,
                    look: Look::Body,
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
            Some(l) => req.defaults.heading_size_pt(l),
            None => req.defaults.font_size_pt * req.look.scale(),
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
                } else if let Some(n) = r.style.footnote {
                    let digits = (n + 1).to_string().len() as f64;
                    vec![(OBJECT, digits * s * NOTE_REF_SCALE * 0.5, 0.0)]
                } else if r.style.chip.is_some() {
                    let w = r.text.chars().count() as f64 * s * 0.5 + 2.0 * (CHIP_PAD_PT + CHIP_GAP_PT);
                    vec![(OBJECT, w, s)]
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
