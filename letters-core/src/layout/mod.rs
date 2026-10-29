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
use crate::model::{Document, HfFloat, HfLine, ListKind, PageGeometry, Paragraph, Run};

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
    /// Whether paragraph spacing adds up between paragraphs
    /// (`Document::space_adds`) rather than the larger applying.
    #[serde(default)]
    pub space_adds: bool,
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
        o.space_adds = doc.space_adds;
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
            space_adds: false,
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
    /// Tab stops from the text box's left edge: the paragraph's own,
    /// then Word's default grid (see `paragraph_request`).
    pub tab_stops: Vec<crate::model::TabStop>,
    /// A table of contents entry: its tab is one right-aligned stop at the
    /// box's right edge, so the page number sits at the margin (the
    /// painter draws the dot leader).
    pub right_tab: bool,
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
    /// The height of the line's text alone, when an inline picture makes
    /// the line taller than its font: what proportional line spacing
    /// scales (`text_height`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_pt: Option<f64>,
}

impl LineBox {
    pub fn natural_height(&self) -> f64 {
        self.ascent_pt + self.descent_pt
    }

    /// The part of the line proportional line spacing scales: its text's
    /// height, not a picture's. Writer adds half a line of text below a
    /// chart at 150%, not half the chart.
    pub fn text_height(&self) -> f64 {
        self.text_pt.map_or(self.natural_height(), |t| t.min(self.natural_height()))
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
    req.tab_stops.iter().map(|t| (t.pos_pt.to_bits(), t.align)).collect::<Vec<_>>().hash(&mut h);
    req.right_tab.hash(&mut h);
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
        /// A header's or footer's line: the look, alignment and tab stops
        /// of the line of its text it is on (`Document::header_lines`),
        /// which a renderer draws it with.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        story_line: Option<Box<crate::model::HfLine>>,
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
    /// A table cell's box and its background colour (six hex digits), if
    /// the file shades it.
    Cell { table: u32, row: u32, col: u32, x_pt: f64, y_pt: f64, width_pt: f64, height_pt: f64, fill: Option<String>, ruled: bool, open: crate::model::OpenEdges },
    /// A paragraph's shading and borders: the text box of one run of its
    /// lines on this page (`x_pt`, `y_pt`, `width_pt`, `height_pt`), drawn
    /// under the text. A paragraph split across pages gets one box per
    /// page: `top_edge` only on its first box, `bottom_edge` only on its
    /// last, so a border opens where the paragraph continues.
    ParaBox { para: usize, x_pt: f64, y_pt: f64, width_pt: f64, height_pt: f64, fill: Option<String>, borders: crate::model::ParaBorders, top_edge: bool, bottom_edge: bool },
    /// The page's border (`PageGeometry::border`): a rule `line_pt` wide
    /// in `color` (0xRRGGBB) whose outer edge is this box, around the
    /// page's text and the header and footer it shows.
    PageBorder { x_pt: f64, y_pt: f64, width_pt: f64, height_pt: f64, line_pt: f64, color: u32 },
    /// A text box of paragraph `para`: its outer box, drawn with its fill,
    /// its rule and its text inside its padding (`crate::model::TextBox`).
    TextBox { para: usize, x_pt: f64, y_pt: f64, width_pt: f64, height_pt: f64, text_box: Box<crate::model::TextBox> },
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

/// Where each page sits when the pages are stacked top to bottom with a
/// gap before each and after the last (the Print Layout view), at a scale
/// in pixels per point (#1282).
///
/// The tops are prefix sums, so finding the pages in a band, or the page
/// nearest a point, is a binary search: the view once re-summed every page
/// above each page it placed, which cost a 500-page document a quarter of a
/// million additions per frame and per pointer motion.
#[derive(Clone, Debug, Default)]
pub struct PageStack {
    /// `tops[i]`: the top of page `i`.
    tops: Vec<f64>,
    sizes: Vec<(f64, f64)>,
    gap: f64,
}

impl PageStack {
    pub fn new(sizes: Vec<(f64, f64)>, gap: f64) -> Self {
        let mut tops = Vec::with_capacity(sizes.len());
        let mut at = gap;
        for &(_, h) in &sizes {
            tops.push(at);
            at += h + gap;
        }
        PageStack { tops, sizes, gap }
    }

    pub fn len(&self) -> usize {
        self.sizes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sizes.is_empty()
    }

    /// Top of page `index` (the end of the stack past the last page).
    pub fn top(&self, index: usize) -> f64 {
        self.tops.get(index).copied().unwrap_or_else(|| self.height())
    }

    /// Width and height of page `index`, or zero past the last.
    pub fn size(&self, index: usize) -> (f64, f64) {
        self.sizes.get(index).copied().unwrap_or((0.0, 0.0))
    }

    /// The whole stack's height, gaps included.
    pub fn height(&self) -> f64 {
        self.tops.last().map_or(self.gap, |&t| t + self.sizes.last().map_or(0.0, |s| s.1) + self.gap)
    }

    /// The pages any part of which is in `top..=bottom`, as placed by a
    /// view that floors each page's top to a whole pixel.
    pub fn visible(&self, top: f64, bottom: f64) -> std::ops::Range<usize> {
        let first = self.first(|i| self.bottom(i) >= top);
        let end = self.first(|i| self.tops[i].floor() > bottom);
        first..end.max(first)
    }

    /// Bottom edge of page `i` as placed (its top floored).
    fn bottom(&self, i: usize) -> f64 {
        self.tops[i].floor() + self.sizes[i].1
    }

    /// The first page for which `past` holds, `past` being false for every
    /// page before some point and true from it on; `len()` if none.
    fn first(&self, past: impl Fn(usize) -> bool) -> usize {
        let (mut lo, mut hi) = (0, self.len());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if past(mid) { hi = mid } else { lo = mid + 1 }
        }
        lo
    }

    /// The page nearest widget `y` (a point between pages belongs to the
    /// nearer one, ties to the earlier), or `None` with no pages.
    pub fn nearest(&self, y: f64) -> Option<usize> {
        if self.is_empty() {
            return None;
        }
        let distance = |i: usize| {
            let (top, h) = (self.tops[i].floor(), self.sizes[i].1);
            if y < top { top - y } else if y > top + h { y - top - h } else { 0.0 }
        };
        // The first page whose bottom reaches y; y is in it, in the gap
        // above it, or past the last page.
        let i = self.first(|i| self.bottom(i) >= y).min(self.len() - 1);
        if i > 0 && distance(i - 1) <= distance(i) { Some(i - 1) } else { Some(i) }
    }
}

impl RenderTree {
    /// Where each page starts, as a character offset in `doc`'s text
    /// (paragraphs joined by one newline, `Document::paragraph_offset`):
    /// the first body line on the page (#1280). Print Layout, printing and
    /// PDF export all draw this tree, so these are their page breaks; a
    /// page with no body line of its own starts where the previous ended.
    pub fn page_breaks(&self, doc: &crate::model::Document) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::with_capacity(self.pages.len());
        for page in &self.pages {
            let first = page.items.iter().find_map(|i| match i {
                Item::Line { source: Source::Paragraph(p), start, .. } => Some(doc.paragraph_offset(*p) + start),
                _ => None,
            });
            let previous = out.last().copied().unwrap_or(0);
            out.push(first.unwrap_or(previous).max(previous));
        }
        out
    }

    /// The pages stacked at `scale` px per point with `gap` px between.
    pub fn page_stack(&self, scale: f64, gap: f64) -> PageStack {
        PageStack::new(self.pages.iter().map(|p| (p.width_pt * scale, p.height_pt * scale)).collect(), gap)
    }

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

/// Whether a run is an inline object (an image, a text box, a footnote
/// reference or a smart chip): one `OBJECT` char in the layout and edit
/// sequence, whatever its text.
pub fn is_object(run: &Run) -> bool {
    run.style.image.is_some() || run.style.text_box.is_some() || run.style.footnote.is_some() || run.style.chip.is_some()
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
pub(crate) const EMU_PER_PT: f64 = 12700.0;

/// Displayed size of an image run in points: the document's extent if it
/// has one, else the image's own pixels at 96 dpi (PNG), else one inch
/// square. A picture wider than its column keeps its size and runs into
/// the margin, as Writer and Word draw it; shrunk to fit, a chart a little
/// wider than the text was drawn shorter and pulled everything after it up
/// (render-lab `real-statistical-release-october-to-december-2020`).
pub fn image_size_pt(run: &Run) -> (f64, f64) {
    let (w, h) = match run.style.image_extent_emu {
        Some((w, h)) => (w as f64 / EMU_PER_PT, h as f64 / EMU_PER_PT),
        None => run.style.image.as_deref().and_then(png_size_px).map_or((72.0, 72.0), |(w, h)| {
            (f64::from(w) * 0.75, f64::from(h) * 0.75)
        }),
    };
    (w, h)
}

/// The room an image run takes in its line: its size and the room kept
/// beside it, or none for a floating image, which is drawn where its
/// anchor puts it.
pub fn inline_size_pt(run: &Run) -> (f64, f64) {
    if run.style.image_anchor.is_some() {
        (0.0, 0.0)
    } else {
        let (left, right) = image_sides_pt(run);
        let (w, h) = image_size_pt(run);
        (w + left + right, h)
    }
}

/// The room kept left and right of an inline image, in points.
pub fn image_sides_pt(run: &Run) -> (f64, f64) {
    run.style.image_sides_emu.map_or((0.0, 0.0), |(l, r)| (l as f64 / EMU_PER_PT, r as f64 / EMU_PER_PT))
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
/// LibreOffice's default spacing between the rule and the notes: 0.1 cm.
pub const NOTE_RULE_GAP_PT: f64 = 72.0 * 0.1 / 2.54;

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
        // A Word note's text carries its own leading space after the
        // reference mark; one without gets one.
        runs: vec![
            Run::plain((i + 1).to_string()),
            Run { text: if text.starts_with(char::is_whitespace) { text } else { format!(" {text}") }, style: size },
        ],
    }
}

/// Part of a footnote placed at the foot of a page: lines `from..to` of
/// note `note`. A note that does not fit on its reference's page continues
/// on the next, as LibreOffice and Word split one.
#[derive(Clone, Copy, Debug, PartialEq)]
struct NotePiece {
    note: usize,
    from: usize,
    to: usize,
}

/// The footnotes referenced on line `lb` of `para`.
fn line_notes(para: &Paragraph, lb: &LineBox) -> Vec<usize> {
    lb.objects.iter().filter_map(|&(ch, _)| run_at(&para.runs, ch)?.style.footnote).collect()
}

pub fn layout(doc: &Document, opts: &LayoutOptions, shaper: &mut dyn Shaper) -> RenderTree {
    let opts = &opts.for_document(doc);
    // Each section's paragraphs and page (LET-005S); a document of one
    // section is one run on its own page or the application's.
    let runs = crate::sections::runs(doc, opts.page);
    let geometries: Vec<PageGeometry> = runs.iter().enumerate().map(|(k, (_, g))| section_geometry(doc, k, *g, false, opts, shaper)).collect();
    // The first page makes room for its own header and footer.
    let first = if doc.first_header.is_some() || doc.first_footer.is_some() { section_geometry(doc, 0, runs[0].1, true, opts, shaper) } else { geometries[0] };
    let mut flow = Flow::new(first, opts);
    flow.geometry = geometries[0];
    // Each footnote's height at the foot of a page, for the flow to keep
    // room for the notes of the lines it places.
    flow.shape_notes(doc, shaper);
    let ordinals = lists::ordinals(doc.paragraphs.iter().map(|p| &p.style));
    let mut col_w = flow.column_width();
    let mut section = 0;

    let mut i = 0;
    while i < doc.paragraphs.len() {
        // A section starts on a page of its own, laid out on its page.
        while section + 1 < runs.len() && runs[section + 1].0.start <= i {
            section += 1;
            flow.start_section(geometries[section], section, doc, shaper);
            col_w = flow.column_width();
        }
        let section_end = runs[section].0.end;
        let para = &doc.paragraphs[i];
        if let Some(cell) = para.style.table_cell {
            let end = doc.paragraphs[i..]
                .iter()
                .position(|p| p.style.table_cell.is_none_or(|c| c.table != cell.table))
                .map_or(doc.paragraphs.len(), |n| i + n);
            layout_table(&mut flow, doc, i..end, &ordinals, shaper);
            i = end;
            continue;
        }
        let shaped = shape_beside_floats(&mut flow, para, col_w, opts, shaper);
        // Keep a heading, and any paragraph set to keep with the next (a
        // Title, a Word style's keepNext), with the first lines of what
        // follows it — in its own section: the next section starts a page
        // anyway.
        let keep_next = if keeps_with_next(para) && i + 1 < section_end { keep_chain(doc, i + 1, section_end, col_w, opts, shaper) } else { None };
        let group = border_group(doc, i, section_end);
        place_paragraph(&mut flow, i, para, &shaped, ordinals[i], keep_next, group);
        i += 1;
    }
    flow.finish(doc, opts, shaper)
}

/// Whether paragraph `p` stays on the page of what follows it: a heading,
/// or a paragraph set to keep with the next (a Title, a Word style's
/// keepNext).
fn keeps_with_next(p: &Paragraph) -> bool {
    p.style.heading.is_some() || p.style.keep_with_next
}

/// How much of what follows must share a page with the paragraph before
/// paragraph `n`: its first lines, or, when it keeps with the next too,
/// all of it and what that keeps in turn. Word and Writer move the whole
/// chain: a "Proposal" heading over a "Question" heading goes to the next
/// page with it, not left behind alone (questionnaire-2). None at a table.
fn keep_chain(doc: &Document, n: usize, section_end: usize, col_w: f64, opts: &LayoutOptions, shaper: &mut dyn Shaper) -> Option<f64> {
    let mut total = 0.0;
    for (k, i) in (n..section_end).enumerate() {
        let p = doc.paragraphs.get(i).filter(|p| p.style.table_cell.is_none())?;
        let s = shape_paragraph(p, col_w, opts, shaper);
        let chained = keeps_with_next(p) && i + 1 < section_end && k < 10 && doc.paragraphs.get(i + 1).is_some_and(|n| n.style.table_cell.is_none());
        // Its first lines, or all of it when it is too short to leave its
        // orphans here and its widows over, as `place_paragraph` breaks
        // it: a three-line paragraph after a heading moves whole, the
        // heading with it (Writer's W1 notes, "Warehouse name").
        let (orphans, widows) = orphan_widow(&p.style, opts);
        let whole = chained || s.lines.len() < orphans.max(1) + widows.max(1);
        let lines = if whole { s.lines.len() } else { orphans.max(1) };
        total += p.style.space_before_pt + s.lines[..lines].iter().map(|l| line_height(p, l)).sum::<f64>();
        if !chained {
            return Some(total);
        }
        total += p.style.space_after_pt.max(0.0);
    }
    Some(total)
}

/// Section `k`'s page as the body sees it: a header taller than the room
/// above the top margin pushes the body down, and a tall footer pushes its
/// foot up, as Word and LibreOffice lay a page out: a letterhead's logo is
/// not drawn over the text.
fn section_geometry(doc: &Document, k: usize, page: PageGeometry, first: bool, opts: &LayoutOptions, shaper: &mut dyn Shaper) -> PageGeometry {
    let mut geometry = page;
    let width = content_width(&geometry);
    let [header_h, footer_h] = [true, false].map(|header| {
        let s = page_story(doc, k, first, header);
        header_footer_height(s.pictures, s.text, s.alignment, s.lines, width, opts, shaper)
    });
    if header_h > 0.0 {
        geometry.margin_top_pt = geometry.margin_top_pt.max(page.header_pt.unwrap_or(opts.header_distance_pt) + header_h);
    }
    if footer_h > 0.0 {
        geometry.margin_bottom_pt = geometry.margin_bottom_pt.max(page.footer_pt.unwrap_or(opts.footer_distance_pt) + footer_h);
    }
    geometry
}

/// The space between a paragraph with `after` points after it and one
/// with `before` before it: their sum where the document's spacing adds
/// up (`Document::space_adds`), else the larger.
fn between(doc: &Document, after: f64, before: f64) -> f64 {
    let (after, before) = (after.max(0.0), before.max(0.0));
    if doc.space_adds { after + before } else { after.max(before) }
}

/// A page's header or footer: its text, the pictures above it, its
/// alignment and its lines.
struct PageStory<'d> {
    text: Option<&'d str>,
    pictures: &'d [Run],
    alignment: crate::model::Alignment,
    lines: &'d [HfLine],
}

/// The header (`header`) or footer on section `k`'s pages, or on the
/// document's first page (`first`) when that has its own. A section's
/// own header or footer has its text alone: the document's lines and
/// pictures go with the document's.
fn page_story(doc: &Document, k: usize, first: bool, header: bool) -> PageStory<'_> {
    let own = if header { &doc.first_header } else { &doc.first_footer };
    if let Some(s) = own.as_ref().filter(|_| first && k == 0) {
        return PageStory { text: Some(&s.text), pictures: &s.pictures, alignment: s.alignment, lines: &s.lines };
    }
    let ((text, doc_own), pictures, alignment, lines) = if header {
        (crate::sections::header(doc, k), &doc.header_pictures, doc.header_alignment, &doc.header_lines)
    } else {
        (crate::sections::footer(doc, k), &doc.footer_pictures, doc.footer_alignment, &doc.footer_lines)
    };
    PageStory { text, pictures: if doc_own { pictures } else { &[] }, alignment, lines: if doc_own { lines } else { &[] } }
}

/// The width between a page's side margins.
fn content_width(g: &PageGeometry) -> f64 {
    (g.width_pt - g.margin_left_pt - g.margin_right_pt).max(1.0)
}

/// The foot of a page's text area, above its bottom margin.
fn foot_of(g: &PageGeometry) -> f64 {
    (g.height_pt - g.margin_bottom_pt).max(g.margin_top_pt + 1.0)
}

/// Line-height multiplier of a paragraph (1.0 when unset or nonsensical).
fn spacing(p: &Paragraph) -> f64 {
    let s = f64::from(p.style.line_spacing);
    if s.is_finite() && s > 0.0 { s } else { 1.0 }
}

/// How tall one line of `natural` points stands in paragraph `p`: the
/// proportional multiple, unless the paragraph fixes its height
/// (`ParaStyle::line_height`: OOXML exact/atLeast).
fn line_height(p: &Paragraph, lb: &LineBox) -> f64 {
    let multiple = lb.natural_height() + lb.text_height() * (spacing(p) - 1.0);
    match p.style.line_height {
        Some(crate::model::LineHeight::Exact(pt)) if pt.is_finite() && pt > 0.0 => pt,
        Some(crate::model::LineHeight::Minimum(pt)) if pt.is_finite() && pt > 0.0 => multiple.max(pt),
        _ => multiple,
    }
}

/// How far below its top a line of paragraph `p`, `height` points tall
/// (`line_height`), puts its baseline. Writer's rule
/// (`SwTextFormatter::CalcRealHeight`): an exact line's ascent is four
/// fifths of its height whatever the font; a line an at-least rule grows
/// takes the growth above its text; proportional spacing adds below.
fn line_ascent(p: &Paragraph, lb: &LineBox, height: f64) -> f64 {
    match p.style.line_height {
        Some(crate::model::LineHeight::Exact(pt)) if pt.is_finite() && pt > 0.0 => 0.8 * height,
        Some(crate::model::LineHeight::Minimum(pt)) if pt.is_finite() && pt > 0.0 => lb.ascent_pt + (height - lb.natural_height()).max(0.0),
        _ => lb.ascent_pt,
    }
}

/// The orphan/widow counts that govern paragraph `p`: its own
/// `widow_control` when it switches control off (single lines may
/// strand), else the document default.
fn orphan_widow(st: &crate::model::ParaStyle, opts: &LayoutOptions) -> (usize, usize) {
    match st.widow_control {
        Some(false) => (1, 1),
        _ => (opts.orphans, opts.widows),
    }
}

/// Word's default tab-stop grid, in points: a tab with no stop of its
/// own advances to the next half inch, as Writer lays it out. Pango's
/// own default is narrower, so stops are spelled out past the
/// paragraph's own (which keep their places on the grid).
pub const DEFAULT_TAB_PT: f64 = 36.0;

/// A paragraph broken into lines, with the horizontal box it was shaped in.
struct Shaped {
    lines: Vec<LineBox>,
    /// Text box left edge, relative to the column's left edge.
    box_x: f64,
    box_w: f64,
    /// List marker x, relative to the column's left edge.
    marker_x: f64,
    /// A list item's number in its list, for its marker: a table sets its
    /// cells' (the body passes the paragraph's own).
    ordinal: u32,
}

/// Left edge of a paragraph's text box, in points from its column's (or
/// table cell's) left edge: the left indent plus a list item's text indent.
pub fn paragraph_box_x(p: &Paragraph) -> f64 {
    let st = &p.style;
    let list_indent = if st.list == ListKind::None { 0.0 } else { lists::text_indent_pt(st.list_level) };
    // A file's own quote style gives its indent in `left_indent_pt`.
    let quote_indent = if st.block_quote && !st.own_look { QUOTE_INDENT_PT } else { 0.0 };
    (st.left_indent_pt + list_indent).max(0.0) + quote_indent
}

/// Where a list item's first line starts, from its text box's left edge.
/// The marker hangs in front of the text; a marker wider than its hanging
/// is followed by a tab, as Word and Writer draw it, to the next tab stop
/// past it: the business appointment rules' bullets hang 2 twips at the
/// margin, and their text starts at the 0.5in default stop, with the
/// lines below back at the margin.
fn list_first_line_pt(p: &Paragraph, box_x: f64, opts: &LayoutOptions) -> f64 {
    let st = &p.style;
    let Some(marker) = lists::marker_for(st, 1) else { return 0.0 };
    let hanging = st.list_hanging_pt.unwrap_or(lists::HANGING_PT).max(0.0).min(box_x);
    // Measured without a shaper, as every renderer rebuilds this request:
    // a body-text glyph is about 0.6em wide.
    let size = p.runs.first().and_then(|r| r.style.font_size_hp).map_or(opts.font_size_pt, |hp| f64::from(hp) / 2.0);
    let label_end = box_x - hanging + marker.chars().count() as f64 * size * 0.6;
    if label_end <= box_x {
        return 0.0;
    }
    let stop = st
        .tab_stops_pt
        .iter()
        .map(|t| t.pos_pt)
        .filter(|&x| x > label_end)
        .fold(f64::INFINITY, f64::min)
        .min(((label_end / DEFAULT_TAB_PT).floor() + 1.0) * DEFAULT_TAB_PT);
    stop - box_x
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

/// The tab stops a shaping request carries: the paragraph's own stops,
/// relative to its text box, then Word's default grid past the last of
/// them. An explicit stop keeps its alignment and leader; a grid stop is
/// plain left-aligned.
fn request_tabs(
    st: &crate::model::ParaStyle,
    box_x: f64,
    box_w: f64,
) -> Vec<crate::model::TabStop> {
    use crate::model::TabStop;
    let mut stops: Vec<TabStop> = st
        .tab_stops_pt
        .iter()
        .map(|t| TabStop { pos_pt: t.pos_pt - box_x, ..*t })
        .filter(|t| t.pos_pt > 0.0)
        .collect();
    stops.sort_by(|a, b| a.pos_pt.partial_cmp(&b.pos_pt).unwrap_or(std::cmp::Ordering::Equal));
    let last = stops.iter().map(|t| t.pos_pt).fold(0.0f64, f64::max);
    let mut grid = (last / DEFAULT_TAB_PT).floor() * DEFAULT_TAB_PT + DEFAULT_TAB_PT;
    while grid < box_w {
        stops.push(TabStop::left(grid));
        grid += DEFAULT_TAB_PT;
    }
    stops
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
        // A file's own Title is drawn as its runs say, not ours.
        look: if st.own_look { Look::Body } else { paragraph_look(st) },
        alignment: st.alignment,
        width_pt: box_w,
        // A list item's first line starts at its text indent; the marker
        // hangs in front of it.
        first_line_indent_pt: if st.list == ListKind::None { st.first_line_indent_pt } else { list_first_line_pt(p, box_x, opts) },
        tab_stops: if st.toc.is_some() { Vec::new() } else { request_tabs(st, box_x, box_w) },
        right_tab: st.toc.is_some(),
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
        lines.push(LineBox { start: 0, end: 0, x_pt: 0.0, width_pt: 0.0, ascent_pt: size * 0.9, descent_pt: size * 0.25, objects: Vec::new(), text_pt: None });
    }
    Shaped { lines, box_x, box_w, marker_x: box_x - p.style.list_hanging_pt.unwrap_or(lists::HANGING_PT).max(0.0).min(box_x), ordinal: 1 }
}

/// Place a shaped paragraph in the flow, splitting it across columns and
/// pages with orphan and widow control. `keep_next` is the height that must
/// fit below the paragraph on the same page (a heading's follower).
/// How paragraph `i` joins the bordered group around it: whether the
/// paragraph before it has the same borders and indents (so no top edge or
/// top gap here), and, when the one after does, the space between them,
/// which the group's box spans. Writer and Word draw a run of paragraphs
/// with identical borders as one box, its gaps at its top and bottom only.
fn border_group(doc: &Document, i: usize, section_end: usize) -> (bool, Option<f64>) {
    let p = &doc.paragraphs[i];
    if p.style.borders.is_empty() {
        return (false, None);
    }
    let same = |o: &Paragraph| {
        o.style.table_cell.is_none()
            && o.style.borders == p.style.borders
            && (o.style.left_indent_pt - p.style.left_indent_pt).abs() < 0.01
            && (o.style.right_indent_pt - p.style.right_indent_pt).abs() < 0.01
    };
    let joins_prev = i > 0 && same(&doc.paragraphs[i - 1]) && !p.style.page_break_before;
    let next_gap = doc
        .paragraphs
        .get(i + 1)
        .filter(|n| i + 1 < section_end && same(n) && !n.style.page_break_before)
        .map(|n| between(doc, p.style.space_after_pt, n.style.space_before_pt));
    (joins_prev, next_gap)
}

fn place_paragraph(flow: &mut Flow, idx: usize, para: &Paragraph, shaped: &Shaped, ordinal: u32, keep_next: Option<f64>, group: (bool, Option<f64>)) {
    let (joins_prev, next_gap) = group;
    let st = &para.style;
    if st.page_break_before && !flow.page_is_empty() {
        flow.new_page();
    }
    let heights: Vec<f64> = shaped.lines.iter().map(|l| line_height(para, l)).collect();
    let total: f64 = heights.iter().sum();
    let (orphans, widows) = orphan_widow(st, flow.opts);

    // Between two paragraphs the larger of the space after the first and
    // the space before the second applies, not their sum: LibreOffice
    // draws Word documents that way (render-lab letters/paragraph-spacing:
    // 10pt after, then 24pt before, is a 24pt gap). Writer's own
    // documents add the two (`Document::space_adds`). Past the first
    // paragraph neither applies at the top of a page or column: Word and
    // LibreOffice start the text at the margin there, so a heading's
    // space before must not ride a page break onto the next page
    // (render-lab letters/toc). The document's own first paragraph keeps
    // its space before (render-lab letters/headings).
    let document_top = flow.pages.len() == 1 && flow.column == 0 && flow.column_is_empty();
    if !flow.column_is_empty() || document_top {
        let (before, after) = (st.space_before_pt.max(0.0), std::mem::take(&mut flow.pending_after));
        flow.y += if flow.opts.space_adds { before + after } else { before.max(after) };
    }
    // Below a floating object it may not sit beside (`shape_beside_floats`).
    if let Some(below) = flow.wrap_below.take() {
        flow.y = flow.y.max(below);
    }
    // Everything that must stay together with the first line: the whole
    // paragraph if short, the orphan lines otherwise, plus a heading's
    // follower. A paragraph that keeps its lines together moves whole
    // when it fits a column; one taller than a column still splits, as
    // Word splits it.
    let n = heights.len();
    let head: f64 = if n < orphans + widows {
        total
    } else {
        heights[..orphans.min(n)].iter().sum()
    };
    let tail = keep_next.map(|k| st.space_after_pt.max(0.0) + k).unwrap_or(0.0);
    if st.keep_lines && !flow.column_is_empty() {
        let room = flow.foot() - flow.top();
        if total <= room && flow.y + total + tail > flow.bottom() {
            flow.next_column();
        }
    }
    let needed = head + tail;
    if flow.y + needed > flow.bottom() && !flow.column_is_empty() {
        flow.next_column();
        if !flow.column_is_empty() {
            flow.y += st.space_before_pt.max(0.0);
        }
    }

    let text: Vec<char> = layout_text(&para.runs).chars().collect();
    let mut line = 0;
    while line < n {
        // How many lines fit here, each with room for its footnotes? A line
        // whose note fits only in part ends the chunk: the note continues
        // on the next page (`split`: that line, and what carries over).
        let mut fit = 0;
        let mut y = flow.y;
        let mut notes: Vec<usize> = Vec::new();
        let mut split: Option<(usize, Vec<NotePiece>)> = None;
        while line + fit < n {
            let mut with = notes.clone();
            with.extend(line_notes(para, &shaped.lines[line + fit]));
            let Some(carry) = flow.plan(y + heights[line + fit], &with) else { break };
            y += heights[line + fit];
            fit += 1;
            if !carry.is_empty() {
                split = Some((line + fit - 1, carry));
                break;
            }
            notes = with;
        }
        if line + fit < n {
            // Widows: leave at least `widows` lines for the next column.
            let rest = n - line - fit;
            if rest < widows {
                fit = fit.saturating_sub(widows - rest);
            }
            // Orphans: don't strand fewer than `orphans` lines (only matters
            // for the paragraph's first chunk).
            if line == 0 && fit < orphans.min(n) {
                fit = 0;
            }
            // An empty column takes at least one line, or nothing would fit
            // anywhere.
            if fit == 0 && flow.column_is_empty() {
                fit = 1;
            }
        }
        if split.as_ref().is_some_and(|(k, _)| *k >= line + fit) {
            split = None;
        }
        let chunk_top = flow.y;
        let chunk_x = flow.column_x();
        let chunk_at = flow.page_len();
        let (opens, closes) = (line == 0 && !joins_prev, line + fit == n && next_gap.is_none());
        // Writer's border box: a top border stands its width and gap above
        // the first line.
        if let Some(b) = st.borders.top.as_ref().filter(|_| opens && fit > 0) {
            flow.y += b.inset_pt();
        }
        for (k, (lb, &height)) in shaped.lines.iter().zip(&heights).enumerate().skip(line).take(fit) {
            let top = flow.y;
            let baseline = top + line_ascent(para, lb, height);
            let x0 = flow.column_x();
            if k == 0 && st.list != ListKind::None {
                if let Some(text) = lists::marker_for(st, ordinal) {
                    flow.push(Item::Marker { para: idx, text, x_pt: x0 + shaped.marker_x, baseline_pt: baseline });
                }
            }
            emit_line(flow, idx, para, &text, k, lb, x0 + shaped.box_x, shaped.box_w, top, height);
            match split.take_if(|(at, _)| *at == k) {
                // What fits of the line's notes here, the rest next page.
                Some((_, carry)) => flow.add_notes_split(line_notes(para, lb), carry),
                None => flow.add_notes(line_notes(para, lb)),
            }
            flow.y += height;
        }
        // A bottom border stands its gap below the last line's natural
        // foot; that line's proportional extra goes below the border
        // (Writer: measured 13.8pt lines at 1.15 and 2.0 spacing).
        let mut below = 0.0;
        if let Some(b) = st.borders.bottom.as_ref().filter(|_| closes && fit > 0) {
            if para.style.line_height.is_none() {
                let last = &shaped.lines[line + fit - 1];
                below = (heights[line + fit - 1] - last.natural_height()).max(0.0);
            }
            flow.y += b.inset_pt() - below;
        }
        // A shaded or bordered paragraph draws its box under this chunk's
        // lines: one box per page, opening where the paragraph continues.
        // Side borders stand off the text by their width and gap.
        if (st.shading.is_some() || !st.borders.is_empty()) && fit > 0 {
            let left = st.borders.left.as_ref().map_or(0.0, crate::model::ParaBorder::inset_pt);
            let right = st.borders.right.as_ref().map_or(0.0, crate::model::ParaBorder::inset_pt);
            flow.insert(
                chunk_at,
                Item::ParaBox {
                    para: idx,
                    x_pt: chunk_x + shaped.box_x - left,
                    y_pt: chunk_top,
                    width_pt: shaped.box_w + left + right,
                    // A group's box runs on over the gap to its next
                    // paragraph, so its sides do not break there.
                    height_pt: flow.y - chunk_top + if line + fit == n { next_gap.unwrap_or(0.0) } else { 0.0 },
                    fill: st.shading.clone(),
                    borders: st.borders.clone(),
                    top_edge: opens,
                    bottom_edge: closes,
                },
            );
        }
        flow.y += below;
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
    let baseline = top + line_ascent(para, lb, height);
    // A paragraph's floating images are placed from its first line's top:
    // those behind the text before the line, the rest after it.
    // Where its floating objects hang from: the top it was given, when a
    // floating object moved its text below (`shape_beside_floats`).
    let anchor_top = flow.anchor_top.unwrap_or(top);
    if k == 0 {
        place_floating(flow, idx, para, anchor_top, true);
    }
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
        story_line: None,
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
        if run.style.image_anchor.is_some() {
            continue;
        }
        // Drawn inside the room its line keeps beside it.
        let (left, _) = image_sides_pt(run);
        let (w, h) = image_size_pt(run);
        flow.push(Item::Image { para: idx, src, x_pt: box_x + x + left, y_pt: baseline - h, width_pt: w, height_pt: h });
    }
    if k == 0 {
        place_floating(flow, idx, para, anchor_top, false);
        flow.anchor_top = None;
    }
}

/// Place paragraph `idx`'s floating images (those `behind` the text, or
/// the others) on the current page: each axis offset from, or aligned in,
/// its frame (the page, the page inside its margins, or the paragraph's
/// column and first line's `top`).
fn place_floating(flow: &mut Flow, idx: usize, para: &Paragraph, top: f64, behind: bool) {
    for (run, x, y, w, h) in floating_boxes(flow, para, top) {
        if run.style.image_anchor.is_some_and(|a| a.behind != behind) {
            continue;
        }
        match (&run.style.image, &run.style.text_box) {
            (Some(src), _) => flow.push(Item::Image { para: idx, src: src.clone(), x_pt: x, y_pt: y, width_pt: w, height_pt: h }),
            (None, Some(tb)) => flow.push(Item::TextBox { para: idx, x_pt: x, y_pt: y, width_pt: w, height_pt: h, text_box: tb.clone() }),
            (None, None) => {}
        }
    }
}

/// Paragraph `para`'s floating pictures and text boxes, each with its box
/// on the current page (x, y, width, height) when its first line's top is
/// `top`.
fn floating_boxes<'p>(flow: &Flow, para: &'p Paragraph, top: f64) -> Vec<(&'p Run, f64, f64, f64, f64)> {
    use crate::model::{AnchorAlign, AnchorFrame};
    let g = flow.geometry;
    let mut out = Vec::new();
    for run in &para.runs {
        let Some(anchor) = run.style.image_anchor else { continue };
        if run.style.image.is_none() && run.style.text_box.is_none() {
            continue;
        }
        let (w, h) = match &run.style.text_box {
            Some(tb) => (tb.extent_emu.0 as f64 / EMU_PER_PT, tb.extent_emu.1 as f64 / EMU_PER_PT),
            None => image_size_pt(run),
        };
        let (left, width) = match anchor.h_from {
            AnchorFrame::Page => (0.0, g.width_pt),
            AnchorFrame::Margin => (g.margin_left_pt, flow.content_width()),
            AnchorFrame::Text => (flow.column_x(), flow.column_width()),
        };
        let (upper, height) = match anchor.v_from {
            AnchorFrame::Page => (0.0, g.height_pt),
            AnchorFrame::Margin => (flow.top(), flow.foot() - flow.top()),
            AnchorFrame::Text => (top, 0.0),
        };
        let place = |start: f64, room: f64, size: f64, offset: i64, align: Option<AnchorAlign>| match align {
            Some(AnchorAlign::Start) => start,
            Some(AnchorAlign::Center) => start + (room - size) / 2.0,
            Some(AnchorAlign::End) => start + room - size,
            None => start + offset as f64 / EMU_PER_PT,
        };
        let x = place(left, width, w, anchor.x_emu, anchor.h_align);
        // Kept on the page: Word moves a picture that would hang off it.
        let y = place(upper, height, h, anchor.y_emu, anchor.v_align).clamp(0.0, (g.height_pt - h).max(0.0));
        out.push((run, x, y, w, h));
    }
    out
}

/// Shape paragraph `para` for the column, beside the floating objects on
/// the page that text flows around (its own among them, placed where its
/// first line will be). One its first line would sit beside is set in the
/// room on the object's wider side, as Writer and Word set a short
/// paragraph beside a picture; the whole paragraph is set that wide (a long
/// one that runs on past the object is not widened again below it). One
/// with too little room there, or beside an object it may not sit beside,
/// starts below the object.
fn shape_beside_floats(flow: &mut Flow, para: &Paragraph, col_w: f64, opts: &LayoutOptions, shaper: &mut dyn Shaper) -> Shaped {
    use crate::model::Wrap;
    let shaped = shape_paragraph(para, col_w, opts, shaper);
    // A paragraph that starts a page of its own is past this page's.
    if para.style.page_break_before && !flow.page_is_empty() {
        return shaped;
    }
    let top = flow.next_top(&para.style);
    let own: Vec<WrapBox> = floating_boxes(flow, para, top)
        .into_iter()
        .filter_map(|(run, x, y, w, h)| run.style.image_anchor.filter(|a| a.wrap != Wrap::None && !a.behind).map(|a| WrapBox { x0: x, y0: y, x1: x + w, y1: y + h, wrap: a.wrap }))
        .collect();
    flow.wraps.extend(own);
    if flow.wraps.is_empty() {
        return shaped;
    }
    let first_h = shaped.lines.first().map_or(0.0, |l| line_height(para, l));
    let cx = flow.column_x();
    let (mut l, mut r) = (cx, cx + col_w);
    let mut below: Option<f64> = None;
    let beside: Vec<WrapBox> = flow.wraps.iter().copied().filter(|b| b.y0 < top + first_h && b.y1 > top && b.x1 > cx && b.x0 < cx + col_w).collect();
    for b in &beside {
        match b.wrap {
            Wrap::TopBottom => below = Some(below.map_or(b.y1, |y: f64| y.max(b.y1))),
            Wrap::Parallel => {
                if b.x0 - l >= r - b.x1 {
                    r = r.min(b.x0 - WRAP_GAP_PT);
                } else {
                    l = l.max(b.x1 + WRAP_GAP_PT);
                }
            }
            Wrap::None => {}
        }
    }
    if below.is_none() && r - l < WRAP_MIN_PT {
        below = beside.iter().filter(|b| b.wrap != Wrap::None).map(|b| b.y1).reduce(f64::max);
    }
    if let Some(y) = below {
        flow.wrap_below = Some(y);
        flow.anchor_top = Some(top);
        return shaped;
    }
    if l == cx && r == cx + col_w {
        return shaped;
    }
    let mut narrow = shape_paragraph(para, r - l, opts, shaper);
    narrow.box_x += l - cx;
    narrow.marker_x += l - cx;
    narrow
}

/// A shaped paragraph of a table cell: its index, lines and height.
type CellParagraph = (usize, Shaped, f64);

/// Lay out the table made of cell paragraphs `range`: the file's column
/// widths (scaled down to the column box if they are wider), or equal
/// columns across it without them; rows as tall as their tallest cell,
/// split across pages as Word and LibreOffice split them.
///
/// A merged cell (`Document::table_spans`) is as wide as the columns it
/// spans. One that spans rows starts its content in its first row, adds
/// nothing to that row's height, and makes its last row tall enough for
/// what is left of it; its box is drawn once that row is placed. The grid
/// positions a merged cell covers are not drawn.
fn layout_table(flow: &mut Flow, doc: &Document, range: std::ops::Range<usize>, ordinals: &[u32], shaper: &mut dyn Shaper) {
    let cells: Vec<(usize, crate::model::TableCell)> =
        range.clone().filter_map(|i| doc.paragraphs[i].style.table_cell.map(|c| (i, c))).collect();
    let Some(&(_, first)) = cells.first() else { return };
    // A table that starts a page (a page break the file put before it).
    if doc.paragraphs[range.start].style.page_break_before && !flow.page_is_empty() {
        flow.new_page();
    }
    let cols = cells.iter().map(|(_, c)| c.col).max().unwrap_or(0) + 1;
    let rows = cells.iter().map(|(_, c)| c.row).max().unwrap_or(0) + 1;
    let table_w = flow.column_width();
    // A table the file made wider than the text runs on into the right
    // margin, as Word and Writer draw it; only one wider than the page's
    // edge is narrowed. One sized as a share of the text's width fits it,
    // and between columns a table keeps to its column.
    let fits = flow.geometry.columns > 1 || doc.table_relative.contains(&first.table);
    let room = if fits { table_w } else { table_w + flow.geometry.margin_right_pt.max(0.0) };
    let widths = column_widths(doc.table_columns.get(&first.table), cols as usize, table_w, room);
    let indent = doc.table_indent.get(&first.table).copied().unwrap_or(0.0);
    let col_x: Vec<f64> = widths.iter().scan(indent, |x, w| { let at = *x; *x += w; Some(at) }).collect();
    let spans: &[crate::model::CellSpan] = doc.table_spans.get(&first.table).map_or(&[], Vec::as_slice);
    // The cell at a grid position, as (columns, rows) it spans; `None`
    // where a merged cell covers the position.
    let cell_at = |row: u32, col: u32| -> Option<(u32, u32)> {
        match spans.iter().find(|s| s.contains(row, col)) {
            Some(s) if s.row == row && s.col == col => Some((s.cols.min(cols - col).max(1), s.rows.min(rows - row).max(1))),
            Some(_) => None,
            None => Some((1, 1)),
        }
    };
    let span_w = |col: u32, n: u32| -> f64 { widths[col as usize..(col + n) as usize].iter().sum() };
    // The room inside each cell around its text. The table sits out to
    // the left by its left padding, so cell text lines up with the margin
    // (or its indent).
    let pad = doc.table_padding.get(&first.table).copied().unwrap_or_default();
    // A table without rules has no rule in its rows' height either.
    let ruled = !doc.table_unruled.contains(&first.table);
    let rule_pt = if ruled { doc.table_rule_widths.get(&first.table).copied().unwrap_or(CELL_RULE_PT) } else { 0.0 };
    let opts = flow.opts.clone();
    flow.y += std::mem::take(&mut flow.pending_after);

    let mut tall: Vec<Tall> = Vec::new();

    for row in 0..rows {
        // Shape each cell of the row (a cell may hold several paragraphs).
        let mut row_cells: Vec<(u32, f64, Vec<CellParagraph>)> = Vec::new();
        let mut starting: Vec<(u32, f64, u32, Vec<CellParagraph>)> = Vec::new();
        for col in 0..cols {
            let Some((ncols, nrows)) = cell_at(row, col) else { continue };
            let width = span_w(col, ncols);
            let inner_w = (width - pad.left_pt - pad.right_pt).max(1.0);
            let mut paras = Vec::new();
            for &(i, _) in cells.iter().filter(|(_, c)| c.row == row && c.col == col) {
                let p = &doc.paragraphs[i];
                let mut s = shape_paragraph(p, inner_w, &opts, shaper);
                s.ordinal = ordinals.get(i).copied().unwrap_or(1);
                let ph: f64 = s.lines.iter().map(|l| line_height(p, l)).sum::<f64>()
                    + p.style.space_before_pt.max(0.0)
                    + p.style.space_after_pt.max(0.0);
                paras.push((i, s, ph));
            }
            if nrows > 1 {
                starting.push((col, width, row + nrows - 1, paras));
            } else {
                row_cells.push((col, width, paras));
            }
        }
        let open = |col: u32| open_edges(doc, first.table, row, col);
        let valign = |col: u32| doc.table_valign.get(&first.table).and_then(|v| v.iter().find(|v| (v.row, v.col) == (row, col))).map(|v| v.align);
        let fill = |col: u32| doc.table_fills.get(&first.table).and_then(|f| f.iter().find(|f| f.row == row && f.col == col)).map(|f| f.color.clone());
        // Lay the merged cells that start in this row down from its top.
        let mut start_tall = |flow: &mut Flow, tall: &mut Vec<Tall>, top: f64| {
            for (col, width, last_row, paras) in starting.drain(..) {
                let cx = flow.column_x() - pad.left_pt + col_x[col as usize];
                let slices = cell_slices(doc, &paras, &pad);
                place_slices(flow, doc, &paras, &slices, cx + pad.left_pt, top);
                let bottom = top + slices.iter().map(Slice::height).sum::<f64>() + rule_pt;
                tall.push(Tall { row, col, width, last_row, top, page: flow.pages.len(), column: flow.column, bottom, fill: fill(col) });
            }
        };
        let file_h = doc.table_rows.get(&first.table).and_then(|r| r.get(row as usize).copied().flatten()).filter(|h| h.pt.is_finite() && h.pt > 0.0);
        // A fixed height: the row is that tall whatever it holds (taller
        // content runs past it, as Word clips it), and moves whole.
        if let Some(h) = file_h.filter(|h| h.exact) {
            if flow.y + h.pt > flow.bottom() && !flow.column_is_empty() {
                flow.next_column();
            }
            let top = flow.y;
            let x0 = flow.column_x();
            start_tall(flow, &mut tall, top);
            for (col, width, paras) in &row_cells {
                let cx = x0 - pad.left_pt + col_x[*col as usize];
                flow.push(Item::Cell { table: first.table, row, col: *col, x_pt: cx, y_pt: top, width_pt: *width, height_pt: h.pt, fill: fill(*col), ruled, open: open(*col) });
                let slices = cell_slices(doc, paras, &pad);
                let content: f64 = slices.iter().map(Slice::height).sum();
                let drop = valign(*col).map_or(0.0, |v| (h.pt - rule_pt - content).max(0.0) * v.share());
                place_slices(flow, doc, paras, &slices, cx + pad.left_pt, top + drop);
            }
            flow.y = top + h.pt;
            close_tall(flow, doc, &mut tall, first.table, row, &col_x, pad.left_pt);
            continue;
        }
        // Otherwise the row splits across pages, as Word and LibreOffice
        // split it: each piece takes the lines of every cell that fit
        // above the foot of the page, and the rest continue at the top of
        // the next. A minimum height (an answer box) is made up by the
        // last piece. Moved whole, a tall row left a page of blank space
        // before it and a row taller than a page ran off its foot.
        let slices: Vec<Vec<Slice>> = row_cells.iter().map(|(_, _, paras)| cell_slices(doc, paras, &pad)).collect();
        // A row kept on one page that does not fit what is left of this
        // one starts the next, unless it is taller than a page.
        if doc.table_whole_rows.get(&first.table).is_some_and(|w| w.contains(&row)) && !flow.column_is_empty() {
            let need = slices.iter().map(|c| c.iter().map(Slice::height).sum::<f64>()).fold(0.0, f64::max).max(file_h.map_or(0.0, |h| h.pt)) + rule_pt;
            if flow.y + need > flow.bottom() + 1e-6 && need <= flow.bottom() - flow.top() {
                flow.next_column();
            }
        }
        let mut next = vec![0usize; slices.len()];
        // A merged cell that ends here needs the row to reach the end of
        // its content, when the two are in the same column of one page.
        let tall_need = tall
            .iter()
            .filter(|t| t.last_row == row && t.page == flow.pages.len() && t.column == flow.column)
            .map(|t| t.bottom - flow.y)
            .fold(0.0, f64::max);
        let mut min_left = file_h.map_or(0.0, |h| h.pt).max(tall_need);
        let mut first_piece = true;
        loop {
            // A cell continued on a new page starts with its next line: the
            // space between paragraphs at the break stays on the page
            // before, as Writer lays it out. Its top padding opens the new
            // piece again, as Writer opens a continued cell.
            let pad_top = if first_piece { 0.0 } else { pad.top_pt.max(0.0) };
            if !first_piece {
                for (c, n) in slices.iter().zip(next.iter_mut()) {
                    while c.get(*n).is_some_and(|s| matches!(s, Slice::Gap(_))) {
                        *n += 1;
                    }
                }
            }
            // How many of each cell's remaining slices fit in `room`.
            let fit = |cell: &[Slice], from: usize, room: f64| -> (usize, f64) {
                let mut h = 0.0;
                let mut n = from;
                while n < cell.len() && h + cell[n].height() <= room + 1e-6 {
                    h += cell[n].height();
                    n += 1;
                }
                (n, h)
            };
            // The lines a piece takes leave room at the foot of the page for
            // the footnotes they reference: take less until they fit.
            let mut notes_h = 0.0;
            let takes: Vec<(usize, f64)> = loop {
                let room = flow.bottom() - flow.y - rule_pt - notes_h - pad_top;
                let takes: Vec<(usize, f64)> = slices.iter().zip(&next).map(|(c, &from)| fit(c, from, room)).collect();
                let notes: Vec<usize> = row_cells
                    .iter()
                    .zip(&slices)
                    .zip(next.iter().zip(&takes))
                    .flat_map(|(((_, _, paras), cell), (&from, &(to, _)))| slice_notes(doc, paras, &cell[from..to]))
                    .collect();
                let need = flow.notes_room(&notes);
                if need <= notes_h + 1e-6 {
                    break takes;
                }
                notes_h = need;
            };
            let done = takes.iter().zip(&slices).all(|((n, _), c)| *n == c.len());
            let placed_any = takes.iter().zip(&next).any(|((n, _), from)| n > from);
            if !done && !placed_any && !flow.column_is_empty() {
                flow.next_column();
                continue;
            }
            // A line taller than a whole page goes where it is anyway.
            let takes: Vec<(usize, f64)> = if !done && !placed_any {
                slices.iter().zip(&next).map(|(c, &from)| if from < c.len() { (from + 1, c[from].height()) } else { (from, 0.0) }).collect()
            } else {
                takes
            };
            let done = takes.iter().zip(&slices).all(|((n, _), c)| *n == c.len());
            let content = takes.iter().map(|(_, h)| *h).fold(0.0, f64::max) + rule_pt + pad_top;
            let height = if done {
                content.max(min_left.min((flow.bottom() - flow.y).max(content)))
            } else {
                (flow.bottom() - flow.y).max(content)
            };
            let top = flow.y;
            let x0 = flow.column_x();
            let whole = std::mem::take(&mut first_piece);
            if whole {
                start_tall(flow, &mut tall, top);
            }
            for (((col, width, paras), cell), (&from, &(to, used))) in row_cells.iter().zip(&slices).zip(next.iter().zip(&takes)) {
                let cx = x0 - pad.left_pt + col_x[*col as usize];
                flow.push(Item::Cell { table: first.table, row, col: *col, x_pt: cx, y_pt: top, width_pt: *width, height_pt: height, fill: fill(*col), ruled, open: open(*col) });
                // A row placed in one piece sets each cell's content where
                // the cell says; a row split across pages fills from the top.
                let drop = valign(*col).filter(|_| whole && done).map_or(0.0, |v| (height - rule_pt - used).max(0.0) * v.share());
                place_slices(flow, doc, paras, &cell[from..to], cx + pad.left_pt, top + pad_top + drop);
            }
            for (n, (to, _)) in next.iter_mut().zip(&takes) {
                *n = *to;
            }
            min_left -= height;
            flow.y = top + height;
            if done && min_left <= 0.5 {
                break;
            }
            flow.next_column();
        }
        close_tall(flow, doc, &mut tall, first.table, row, &col_x, pad.left_pt);
    }
}

/// A merged cell running down the rows: its first row, column and width,
/// its last row, and where its box starts (top, page, column) and its
/// content ends.
struct Tall {
    row: u32,
    col: u32,
    width: f64,
    last_row: u32,
    top: f64,
    page: usize,
    column: usize,
    bottom: f64,
    /// Its shading, if the file gives it one.
    fill: Option<String>,
}

/// A cell's sides the file draws no rule on.
fn open_edges(doc: &Document, table: u32, row: u32, col: u32) -> crate::model::OpenEdges {
    doc.table_open_edges.get(&table).and_then(|e| e.iter().find(|e| (e.row, e.col) == (row, col))).map(|e| e.open).unwrap_or_default()
}

/// Draw the box of each merged cell whose last row is `row`, now placed:
/// from its first row's top to here, when both are in this column of this
/// page. One that crossed a page keeps its content and loses its box.
fn close_tall(flow: &mut Flow, doc: &Document, tall: &mut Vec<Tall>, table: u32, row: u32, col_x: &[f64], left_pad: f64) {
    let ruled = !doc.table_unruled.contains(&table);
    let (page, column, y, x0) = (flow.pages.len(), flow.column, flow.y, flow.column_x());
    let (ending, rest): (Vec<Tall>, Vec<Tall>) = std::mem::take(tall).into_iter().partition(|t| t.last_row == row);
    *tall = rest;
    for t in ending.into_iter().filter(|t| (t.page, t.column) == (page, column)) {
        let cx = x0 - left_pad + col_x[t.col as usize];
        flow.push(Item::Cell { table, row: t.row, col: t.col, x_pt: cx, y_pt: t.top, width_pt: t.width, height_pt: y - t.top, fill: t.fill.clone(), ruled, open: open_edges(doc, table, t.row, t.col) });
    }
}

/// One step down a cell: a paragraph's space before or after, or one of
/// its lines.
#[derive(Clone, Copy)]
enum Slice {
    Gap(f64),
    /// The cell's paragraph (by position in the cell), its line, height.
    Line(usize, usize, f64),
}

impl Slice {
    fn height(&self) -> f64 {
        match *self {
            Slice::Gap(h) | Slice::Line(_, _, h) => h,
        }
    }
}

/// A cell's content as the steps it is laid down in, inside the cell's
/// top and bottom padding.
fn cell_slices(doc: &Document, paras: &[CellParagraph], pad: &crate::model::CellPadding) -> Vec<Slice> {
    let mut out = vec![Slice::Gap(pad.top_pt)];
    // Between two paragraphs of a cell the space is as in the body
    // (measured in Writer: in a Word file 4.5pt before and after stand
    // 4.5pt apart in a cell too, not 9pt).
    let mut after = 0.0;
    for (k, (i, s, _)) in paras.iter().enumerate() {
        let p = &doc.paragraphs[*i];
        out.push(Slice::Gap(between(doc, after, p.style.space_before_pt)));
        out.extend(s.lines.iter().enumerate().map(|(l, lb)| Slice::Line(k, l, line_height(p, lb))));
        after = p.style.space_after_pt.max(0.0);
    }
    out.push(Slice::Gap(after));
    out.push(Slice::Gap(pad.bottom_pt));
    out
}

/// Draw a run of a cell's slices from `top` down, its text's left edge at
/// `text_x` (the cell's, inside its padding).
/// A shaded or bordered cell paragraph gets its box under its lines
/// here too, opening where the cell continues on another page.
fn place_slices(flow: &mut Flow, doc: &Document, paras: &[CellParagraph], slices: &[Slice], text_x: f64, top: f64) {
    // One box per paragraph run of lines, under the text: collect the
    // runs first, since the box precedes its lines. A run is (paragraph,
    // top, bottom, first line, last line); gaps always end one.
    let mut runs: Vec<(usize, f64, f64, usize, usize)> = Vec::new();
    let mut y = top;
    let mut run: Option<(usize, f64, usize, usize)> = None;
    let close = |run: &mut Option<(usize, f64, usize, usize)>, y: f64, runs: &mut Vec<(usize, f64, f64, usize, usize)>| {
        if let Some((rk, rtop, first, last)) = run.take() {
            runs.push((rk, rtop, y, first, last));
        }
    };
    for slice in slices {
        match *slice {
            Slice::Line(k, l, h) => {
                if matches!(run, Some((rk, _, _, _)) if rk == k) {
                    if let Some((_, _, _, last)) = run.as_mut() {
                        *last = l;
                    }
                } else {
                    close(&mut run, y, &mut runs);
                    run = Some((k, y, l, l));
                }
                y += h;
            }
            Slice::Gap(h) => {
                close(&mut run, y, &mut runs);
                y += h;
            }
        }
    }
    close(&mut run, y, &mut runs);
    for (k, rtop, rbottom, first, last) in runs {
        let (i, s, _) = &paras[k];
        let p = &doc.paragraphs[*i];
        if p.style.shading.is_none() && p.style.borders.is_empty() {
            continue;
        }
        flow.push(Item::ParaBox {
            para: *i,
            x_pt: text_x + s.box_x,
            y_pt: rtop,
            width_pt: s.box_w,
            height_pt: rbottom - rtop,
            fill: p.style.shading.clone(),
            borders: p.style.borders.clone(),
            top_edge: first == 0,
            bottom_edge: last == s.lines.len().saturating_sub(1),
        });
    }
    let mut y = top;
    for slice in slices {
        if let Slice::Line(k, l, h) = *slice {
            let (i, s, _) = &paras[k];
            let p = &doc.paragraphs[*i];
            let text: Vec<char> = layout_text(&p.runs).chars().collect();
            // A list item in a cell has its marker, as in the body.
            if l == 0 && p.style.list != ListKind::None {
                if let Some(marker) = lists::marker_for(&p.style, s.ordinal) {
                    flow.push(Item::Marker { para: *i, text: marker, x_pt: text_x + s.marker_x, baseline_pt: y + line_ascent(p, &s.lines[l], h) });
                }
            }
            emit_line(flow, *i, p, &text, l, &s.lines[l], text_x + s.box_x, s.box_w, y, h);
            // A footnote referenced in a cell goes to the foot of the page
            // the line is on, as one in the body does.
            flow.add_notes(line_notes(p, &s.lines[l]));
        }
        y += slice.height();
    }
}

/// The footnotes referenced by a cell's slices.
fn slice_notes(doc: &Document, paras: &[CellParagraph], slices: &[Slice]) -> Vec<usize> {
    slices
        .iter()
        .filter_map(|slice| match *slice {
            Slice::Line(k, l, _) => {
                let (i, s, _) = &paras[k];
                Some(line_notes(&doc.paragraphs[*i], &s.lines[l]))
            }
            Slice::Gap(_) => None,
        })
        .flatten()
        .collect()
}

/// A table's `cols` column widths: the file's, scaled down to `room` if
/// they add up to more, or equal columns across `table_w` without them (or
/// when the table has gained or lost a column since).
fn column_widths(file: Option<&Vec<f64>>, cols: usize, table_w: f64, room: f64) -> Vec<f64> {
    match file.filter(|w| w.len() == cols && w.iter().all(|x| x.is_finite() && *x > 0.0)) {
        Some(w) => {
            let total: f64 = w.iter().sum();
            let scale = if total > room { room / total } else { 1.0 };
            w.iter().map(|x| x * scale).collect()
        }
        None => vec![table_w / cols as f64; cols],
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
    /// Each footnote's line heights at the foot of a page, and its space
    /// after.
    note_lines: Vec<Vec<f64>>,
    note_after: Vec<f64>,
    /// The footnote pieces at the foot of each page, in order.
    page_notes: Vec<Vec<NotePiece>>,
    /// What continues at the foot of the next page.
    carry: Vec<NotePiece>,
    /// The section (index into the document's sections, 0 for a document
    /// of one) being laid out, and the one each page belongs to: whose
    /// header and footer it shows.
    section: usize,
    page_sections: Vec<usize>,
    /// The floating objects on this page that text flows around.
    wraps: Vec<WrapBox>,
    /// Where the next paragraph starts at the earliest: below an object
    /// it may not sit beside.
    wrap_below: Option<f64>,
    /// Where that paragraph's own floating objects hang from when it is
    /// moved below one: the top it had.
    anchor_top: Option<f64>,
}

/// A floating object text flows around: its box on the page and how.
#[derive(Clone, Copy, Debug)]
struct WrapBox {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    wrap: crate::model::Wrap,
}

/// The room kept between a floating object and the text beside it:
/// Word's own 0.125in.
const WRAP_GAP_PT: f64 = 9.0;
/// The narrowest a paragraph is set beside a floating object; one with
/// less room than this goes below it, as Writer and Word move it.
const WRAP_MIN_PT: f64 = 72.0;

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
            note_lines: Vec::new(),
            note_after: Vec::new(),
            page_notes: Vec::new(),
            carry: Vec::new(),
            section: 0,
            page_sections: Vec::new(),
            wraps: Vec::new(),
            wrap_below: None,
            anchor_top: None,
        };
        f.new_page();
        f
    }

    /// Shape the document's footnotes at this page's text width: the
    /// heights the flow keeps room for.
    fn shape_notes(&mut self, doc: &Document, shaper: &mut dyn Shaper) {
        let text_w = self.content_width();
        self.note_lines.clear();
        self.note_after.clear();
        for i in 0..doc.footnotes.len() {
            let p = note_paragraph(doc, i, self.opts);
            let shaped = shape_paragraph(&p, text_w, self.opts, shaper);
            self.note_lines.push(shaped.lines.iter().map(|l| line_height(&p, l)).collect());
            self.note_after.push(p.style.space_after_pt.max(0.0));
        }
    }

    /// Start section `section` on `geometry`: on a page of its own, or on
    /// the current one if nothing is on it yet (no blank page between).
    /// Footnotes are reshaped when the text is a different width; a note
    /// continued across the break keeps the lines that are left of it.
    fn start_section(&mut self, geometry: PageGeometry, section: usize, doc: &Document, shaper: &mut dyn Shaper) {
        let width = self.content_width();
        self.geometry = geometry;
        self.section = section;
        if self.page_is_empty() {
            if let Some(page) = self.pages.last_mut() {
                page.width_pt = geometry.width_pt;
                page.height_pt = geometry.height_pt;
                page.geometry = geometry;
            }
            if let Some(s) = self.page_sections.last_mut() {
                *s = section;
            }
            self.column = 0;
            self.y = self.top();
            self.column_items = 0;
            self.pending_after = 0.0;
        } else {
            self.new_page();
        }
        if (self.content_width() - width).abs() > 1e-6 && !doc.footnotes.is_empty() {
            self.shape_notes(doc, shaper);
            let lens: Vec<usize> = self.note_lines.iter().map(Vec::len).collect();
            let clamp = |p: &mut NotePiece| {
                let len = lens.get(p.note).copied().unwrap_or(0);
                p.to = p.to.min(len);
                p.from = p.from.min(p.to);
            };
            self.carry.iter_mut().for_each(clamp);
            if let Some(pieces) = self.page_notes.last_mut() {
                pieces.iter_mut().for_each(clamp);
                pieces.retain(|p| p.to > p.from);
            }
        }
    }

    fn columns(&self) -> usize {
        usize::from(self.geometry.columns.max(1))
    }

    fn content_width(&self) -> f64 {
        content_width(&self.geometry)
    }

    fn column_width(&self) -> f64 {
        let n = self.columns() as f64;
        ((self.content_width() - self.geometry.column_gap_pt * (n - 1.0)) / n).max(1.0)
    }

    fn column_x(&self) -> f64 {
        self.geometry.margin_left_pt + self.column as f64 * (self.column_width() + self.geometry.column_gap_pt)
    }

    /// The page being filled's own geometry: the section's, or the first
    /// page's when its header or footer is its own (`Document::first_header`).
    fn page_geometry(&self) -> &PageGeometry {
        self.pages.last().map_or(&self.geometry, |p| &p.geometry)
    }

    fn top(&self) -> f64 {
        self.page_geometry().margin_top_pt
    }

    /// The foot of the text area, above the bottom margin.
    fn foot(&self) -> f64 {
        foot_of(self.page_geometry())
    }

    fn note_len(&self, note: usize) -> usize {
        self.note_lines.get(note).map_or(0, Vec::len)
    }

    fn whole(&self, note: usize) -> NotePiece {
        NotePiece { note, from: 0, to: self.note_len(note) }
    }

    /// A piece's height: its lines, and the note's space after if it ends
    /// the note.
    fn piece_height(&self, p: &NotePiece) -> f64 {
        let lines = self.note_lines.get(p.note).map_or(0.0, |l| l[p.from.min(l.len())..p.to.min(l.len())].iter().sum());
        lines + if p.to >= self.note_len(p.note) { self.note_after.get(p.note).copied().unwrap_or(0.0) } else { 0.0 }
    }

    /// Room footnote pieces take at the foot of a page, with the separator.
    fn reserved(&self, pieces: &[NotePiece]) -> f64 {
        if pieces.is_empty() {
            return 0.0;
        }
        NOTE_SEPARATOR_PT + pieces.iter().map(|p| self.piece_height(p)).sum::<f64>()
    }

    fn current_notes(&self) -> &[NotePiece] {
        self.page_notes.last().map_or(&[], Vec::as_slice)
    }

    /// Where body text must end on this page, above its footnotes.
    fn bottom(&self) -> f64 {
        (self.foot() - self.reserved(self.current_notes())).max(self.top() + 1.0)
    }

    /// Whether body text can reach `y_end` on this page with the notes
    /// `notes` referenced by it: `None` if not, else the note lines that
    /// have to continue on the next page (empty when all fit). A note
    /// splits only when its first line fits with its reference.
    fn plan(&self, y_end: f64, notes: &[usize]) -> Option<Vec<NotePiece>> {
        let here = self.current_notes();
        let new: Vec<usize> = notes.iter().copied().filter(|n| !here.iter().any(|p| p.note == *n)).collect();
        let sep = if here.is_empty() && !new.is_empty() { NOTE_SEPARATOR_PT } else { 0.0 };
        let mut avail = self.foot() - self.reserved(here) - sep - y_end;
        if avail < -1e-6 {
            return None;
        }
        let mut carry = Vec::new();
        let mut placed_any = false;
        for n in new {
            let whole = self.whole(n);
            if !carry.is_empty() {
                carry.push(whole);
                continue;
            }
            let full = self.piece_height(&whole);
            if full <= avail + 1e-6 {
                avail -= full;
                placed_any = true;
                continue;
            }
            let lines = &self.note_lines[n];
            let mut k = 0;
            let mut used = 0.0;
            while k < lines.len() && used + lines[k] <= avail + 1e-6 {
                used += lines[k];
                k += 1;
            }
            if k == 0 && !placed_any {
                return None;
            }
            carry.push(NotePiece { note: n, from: k, to: whole.to });
            avail -= used;
            placed_any |= k > 0;
        }
        Some(carry)
    }

    /// Room the notes `notes` would add at this page's foot: those not
    /// already on it, whole, and the separator if they are its first.
    fn notes_room(&self, notes: &[usize]) -> f64 {
        let here = self.current_notes();
        let mut new: Vec<usize> = notes.iter().copied().filter(|n| !here.iter().any(|p| p.note == *n)).collect();
        new.sort_unstable();
        new.dedup();
        if new.is_empty() {
            return 0.0;
        }
        let sep = if here.is_empty() { NOTE_SEPARATOR_PT } else { 0.0 };
        sep + new.iter().map(|&n| self.piece_height(&self.whole(n))).sum::<f64>()
    }

    /// Put the notes referenced by a placed line at this page's foot.
    fn add_notes(&mut self, notes: Vec<usize>) {
        self.add_notes_split(notes, Vec::new());
    }

    /// As `add_notes`, with `carry` (from `plan`) continuing next page.
    fn add_notes_split(&mut self, notes: Vec<usize>, carry: Vec<NotePiece>) {
        let pieces: Vec<NotePiece> = notes
            .into_iter()
            .filter(|n| !self.current_notes().iter().any(|p| p.note == *n))
            .map(|n| {
                let whole = self.whole(n);
                match carry.iter().find(|c| c.note == n) {
                    Some(c) => NotePiece { to: c.from, ..whole },
                    None => whole,
                }
            })
            .filter(|p| p.to > p.from)
            .collect();
        if let Some(page) = self.page_notes.last_mut() {
            page.extend(pieces);
        }
        self.carry.extend(carry);
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
        self.page_sections.push(self.section);
        // A footnote continued from the page before comes first.
        let carried = std::mem::take(&mut self.carry);
        self.page_notes.push(carried);
        self.column = 0;
        self.y = self.top();
        self.column_items = 0;
        self.pending_after = 0.0;
        self.wraps.clear();
        self.wrap_below = None;
        self.anchor_top = None;
    }

    /// Where a paragraph of style `st` placed next has its first line's
    /// top, if it stays in this column: past the space between it and the
    /// one before (as `place_paragraph` adds it).
    fn next_top(&self, st: &crate::model::ParaStyle) -> f64 {
        let document_top = self.pages.len() == 1 && self.column == 0 && self.column_is_empty();
        if !self.column_is_empty() || document_top {
            let (before, after) = (st.space_before_pt.max(0.0), self.pending_after);
            self.y + if self.opts.space_adds { before + after } else { before.max(after) }
        } else {
            self.y
        }
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

    /// Items on the current page: where a chunk's box goes to sit under
    /// the lines just placed.
    fn page_len(&self) -> usize {
        self.pages.last().map_or(0, |p| p.items.len())
    }

    /// Insert `item` at `at` on the current page, under the lines placed
    /// since `at` was read.
    fn insert(&mut self, at: usize, item: Item) {
        self.column_items += 1;
        let items = &mut self.pages.last_mut().expect("a page").items;
        items.insert(at.min(items.len()), item);
    }

    /// Add footnotes, headers and footers now that the page count is
    /// known: each page on its own section's page, with its section's
    /// header and footer.
    fn finish(mut self, doc: &Document, opts: &LayoutOptions, shaper: &mut dyn Shaper) -> RenderTree {
        let total = self.pages.len();
        // Each page's footnotes, ending at the foot of its text area under
        // a short rule.
        let page_notes = std::mem::take(&mut self.page_notes);
        for (page, pieces) in self.pages.iter_mut().zip(&page_notes) {
            if pieces.is_empty() {
                continue;
            }
            let g = page.geometry;
            let (left, width, foot) = (g.margin_left_pt, content_width(&g), foot_of(&g));
            let notes: Vec<(Paragraph, Vec<LineBox>)> = pieces
                .iter()
                .map(|piece| {
                    let p = note_paragraph(doc, piece.note, opts);
                    let lines = shape_paragraph(&p, width, opts, shaper).lines;
                    (p, lines)
                })
                .collect();
            let ends = |piece: &NotePiece, lines: &[LineBox]| piece.to >= lines.len();
            // The last piece ends at the foot; a note's space after it is
            // not drawn.
            let after = pieces.last().zip(notes.last()).filter(|(piece, (_, lines))| ends(piece, lines)).map_or(0.0, |(_, (p, _))| p.style.space_after_pt.max(0.0));
            let total: f64 = pieces
                .iter()
                .zip(&notes)
                .map(|(piece, (p, lines))| {
                    lines[piece.from.min(lines.len())..piece.to.min(lines.len())].iter().map(|lb| line_height(p, lb)).sum::<f64>()
                        + if ends(piece, lines) { p.style.space_after_pt.max(0.0) } else { 0.0 }
                })
                .sum();
            // As at a bordered paragraph's foot, the last line's
            // proportional extra hangs below the foot: its natural foot
            // sits on it (Writer, render-lab letters/footnotes).
            // A note split at the page's foot hangs the same way.
            let hang = pieces
                .last()
                .zip(notes.last())
                .filter(|(_, (p, _))| p.style.line_height.is_none())
                .and_then(|(piece, (p, lines))| {
                    let lb = lines.get(piece.to.min(lines.len()).checked_sub(1)?)?;
                    Some((line_height(p, lb) - lb.natural_height()).max(0.0))
                })
                .unwrap_or(0.0);
            let mut y = foot + after + hang - total;
            page.items.push(Item::Rule { x_pt: left, y_pt: y - NOTE_RULE_GAP_PT, width_pt: width * NOTE_RULE_FRACTION });
            for (piece, (p, lines)) in pieces.iter().zip(&notes) {
                let i = piece.note;
                let chars: Vec<char> = layout_text(&p.runs).chars().collect();
                for (k, lb) in lines.iter().enumerate().take(piece.to).skip(piece.from) {
                    let h = line_height(p, lb);
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
                        baseline_pt: y + line_ascent(p, lb, h),
                        height_pt: h,
                        story_line: None,
                    });
                    y += h;
                }
                if ends(piece, lines) {
                    y += p.style.space_after_pt.max(0.0);
                }
            }
        }
        let page_sections = std::mem::take(&mut self.page_sections);
        for page in &mut self.pages {
            let g = page.geometry;
            let (left, width, height) = (g.margin_left_pt, content_width(&g), page.height_pt);
            let section = page_sections.get(page.index).copied().unwrap_or(0);
            // How far up the page's header reaches and down its footer:
            // the border runs around them as well as the body.
            let (mut text_top, mut text_bottom) = (g.margin_top_pt, height - g.margin_bottom_pt);
            for source in [Source::Header, Source::Footer] {
                let PageStory { text: template, pictures, alignment, lines: story } = page_story(doc, section, page.index == 0, source == Source::Header);
                let row_h = picture_row_height(pictures);
                let text_h = flowing_text(template, story).map_or(0.0, |t| {
                    let text = page_field_text(t, page.index + 1, total);
                    flow_height(&text, &header_footer_lines(&text, alignment, story, pictures, width, opts, shaper), story)
                });
                let top = match source {
                    Source::Header => g.header_pt.unwrap_or(opts.header_distance_pt),
                    _ => height - g.footer_pt.unwrap_or(opts.footer_distance_pt) - row_h - text_h,
                };
                place_header_footer_pictures(page, pictures, left, width, top, height);
                if !pictures.is_empty() || template.is_some_and(|t| !t.is_empty()) {
                    match source {
                        Source::Header => text_top = text_top.min(top),
                        _ => text_bottom = text_bottom.max(top + row_h + text_h),
                    }
                }
                let Some(template) = template.filter(|t| !t.is_empty()) else { continue };
                let text = page_field_text(template, page.index + 1, total);
                let lines = header_footer_lines(&text, alignment, story, pictures, width, opts, shaper);
                // Under the pictures' row.
                let (boxes, _) = hf_line_boxes(&text, &lines, story);
                let chars: Vec<char> = text.chars().collect();
                for (k, (lb, &(dy, h, ascent))) in lines.iter().zip(&boxes).enumerate() {
                    // The line of the text this one is on: as many breaks
                    // before it.
                    let story_line = (!story.is_empty()).then(|| {
                        let n = chars[..lb.start.min(chars.len())].iter().filter(|&&c| c == '\n').count();
                        Box::new(story.get(n).cloned().unwrap_or(HfLine { alignment, ..Default::default() }))
                    });
                    // A floating line is set against the page's edge and
                    // across the page, out of the flow.
                    let float = story_line.as_ref().and_then(|l| l.float);
                    let (x_pt, top_pt) = match float {
                        None => (left + lb.x_pt, top + row_h + dy),
                        Some(f) => {
                            let across = match story_line.as_ref().map_or(alignment, |l| l.alignment) {
                                crate::model::Alignment::Center => page.width_pt / 2.0 - (left + width / 2.0),
                                crate::model::Alignment::Right => page.width_pt - (left + width),
                                _ => -left,
                            };
                            let top = match f {
                                HfFloat::Top(d) => d,
                                HfFloat::Bottom(d) => height - d - lb.natural_height(),
                            };
                            (left + lb.x_pt + across, top)
                        }
                    };
                    page.items.push(Item::Line {
                        source: source.clone(),
                        line: k,
                        start: lb.start,
                        end: lb.end,
                        text: chars[lb.start.min(chars.len())..lb.end.min(chars.len())].iter().collect(),
                        box_x_pt: left,
                        box_width_pt: width,
                        x_pt,
                        top_pt,
                        baseline_pt: top_pt + ascent,
                        height_pt: h,
                        story_line,
                    });
                }
            }
            if let Some(b) = g.border {
                let [pad_top, pad_right, pad_bottom, pad_left] = b.padding_pt;
                let x = left - pad_left - b.width_pt;
                let y = text_top - pad_top - b.width_pt;
                // Under the other items, as Writer paints it.
                page.items.insert(0, Item::PageBorder {
                    x_pt: x.max(0.0),
                    y_pt: y.max(0.0),
                    width_pt: (left + width + pad_right + b.width_pt).min(page.width_pt) - x.max(0.0),
                    height_pt: (text_bottom + pad_bottom + b.width_pt).min(height) - y.max(0.0),
                    line_pt: b.width_pt,
                    color: b.color,
                });
            }
        }
        RenderTree { pages: self.pages }
    }
}

/// A header's or footer's text broken into lines across `width`, aligned
/// as the document has it. With lines of its own (`story`), each line of
/// the text is shaped alone in its look, alignment and tab stops, its
/// char ranges counted in the whole text; a line past their end is
/// `alignment`'s, in the body's look.
fn shape_header_footer(text: &str, alignment: crate::model::Alignment, story: &[HfLine], width: f64, opts: &LayoutOptions, shaper: &mut dyn Shaper) -> Vec<LineBox> {
    let mut request = |runs: &[Run], alignment, tab_stops| {
        shaper.shape(&ShapeRequest {
            runs,
            heading: None,
            code: false,
            look: Look::Body,
            alignment,
            width_pt: width,
            first_line_indent_pt: 0.0,
            tab_stops,
            right_tab: false,
            defaults: opts,
        })
    };
    if story.is_empty() {
        return request(&[Run::plain(text)], alignment, Vec::new());
    }
    let mut out = Vec::new();
    let mut offset = 0;
    for (k, piece) in text.split('\n').enumerate() {
        let line = story.get(k);
        let run = Run { text: piece.to_string(), style: line.map(|l| l.look.clone()).unwrap_or_default() };
        let shaped = request(&[run], line.map_or(alignment, |l| l.alignment), line.map(|l| l.tab_stops.clone()).unwrap_or_default());
        out.extend(shaped.into_iter().map(|mut lb| {
            lb.start += offset;
            lb.end += offset;
            lb
        }));
        offset += piece.chars().count() + 1;
    }
    out
}

/// The height of the row a header's or footer's pictures in line make:
/// its tallest picture (floating ones take no room).
fn picture_row_height(pictures: &[Run]) -> f64 {
    pictures.iter().filter(|r| r.style.image.is_some()).map(|r| inline_size_pt(r).1).fold(0.0, f64::max)
}

/// How tall a header or footer is: its pictures' row and its text, laid
/// out for page 1.
/// A header's or footer's text as it takes room on the page: none without
/// one, and an empty paragraph's line where the file has one. The business
/// appointment rules' header is one empty paragraph, which Writer gives a
/// line's height: from 35.4pt down it ends below the 45pt top margin and
/// moves the body down.
fn flowing_text<'a>(template: Option<&'a str>, story: &[HfLine]) -> Option<&'a str> {
    template.filter(|t| !t.is_empty() || !story.is_empty())
}

fn header_footer_height(pictures: &[Run], template: Option<&str>, alignment: crate::model::Alignment, story: &[HfLine], width: f64, opts: &LayoutOptions, shaper: &mut dyn Shaper) -> f64 {
    let text = flowing_text(template, story).map_or(0.0, |t| {
        let text = page_field_text(t, 1, 1);
        flow_height(&text, &header_footer_lines(&text, alignment, story, pictures, width, opts, shaper), story)
    });
    picture_row_height(pictures) + text
}

/// The room a header's or footer's `lines` of `text` take: a floating
/// line's (`HfLine::float`) none.
fn flow_height(text: &str, lines: &[LineBox], story: &[HfLine]) -> f64 {
    hf_line_boxes(text, lines, story).1
}

/// Where each of a header's or footer's lines sits below the text's top:
/// its top, height and baseline, each paragraph with the room above and
/// below it and its line height rule (`HfLine`); and the text's height,
/// to the foot of its last line.
/// A floating line takes no room.
fn hf_line_boxes(text: &str, lines: &[LineBox], story: &[HfLine]) -> (Vec<(f64, f64, f64)>, f64) {
    let chars: Vec<char> = text.chars().collect();
    let (mut y, mut out, mut prev) = (0.0, Vec::with_capacity(lines.len()), None::<usize>);
    for lb in lines {
        let n = chars[..lb.start.min(chars.len())].iter().filter(|&&c| c == '\n').count();
        let line = story.get(n);
        let natural = lb.natural_height();
        if line.is_some_and(|l| l.float.is_some()) {
            out.push((y, natural, lb.ascent_pt));
            continue;
        }
        if prev != Some(n) {
            y += prev.and_then(|p| story.get(p)).map_or(0.0, |p| p.space_after_pt.max(0.0));
            y += line.map_or(0.0, |l| l.space_before_pt.max(0.0));
            prev = Some(n);
        }
        let (h, ascent) = match line.and_then(|l| l.line_height) {
            Some(crate::model::LineHeight::Exact(pt)) if pt.is_finite() && pt > 0.0 => (pt, 0.8 * pt),
            Some(crate::model::LineHeight::Minimum(pt)) if pt.is_finite() && pt > natural => (pt, lb.ascent_pt + pt - natural),
            _ => (natural, lb.ascent_pt),
        };
        out.push((y, h, ascent));
        y += h;
    }
    // The last paragraph's space after is outside the part, as Writer
    // sizes a header to its text.
    (out, y)
}

/// The lines of a header's or footer's text under its pictures. The
/// paragraph a letterhead's logo sits in reads as an empty first line;
/// the pictures' row is that line.
fn header_footer_lines(text: &str, alignment: crate::model::Alignment, story: &[HfLine], pictures: &[Run], width: f64, opts: &LayoutOptions, shaper: &mut dyn Shaper) -> Vec<LineBox> {
    let mut lines = shape_header_footer(text, alignment, story, width, opts, shaper);
    if picture_row_height(pictures) > 0.0 && lines.len() > 1 && lines[0].start == lines[0].end {
        lines.remove(0);
    }
    lines
}

/// Draw a header's or footer's pictures on `page`: those in line side by
/// side from the left at `top`, bottoms aligned, and floating ones where
/// their anchors put them (the header or footer's top for their text
/// frame).
fn place_header_footer_pictures(page: &mut Page, pictures: &[Run], left: f64, width: f64, top: f64, page_h: f64) {
    use crate::model::{AnchorAlign, AnchorFrame};
    let row_h = picture_row_height(pictures);
    let mut x = left;
    for run in pictures {
        let Some(src) = run.style.image.clone() else { continue };
        let (w, h) = image_size_pt(run);
        let (x_pt, y_pt) = match run.style.image_anchor {
            None => {
                let at = x;
                x += w;
                (at, top + row_h - h)
            }
            Some(a) => {
                let (from, room) = match a.h_from {
                    AnchorFrame::Page => (0.0, page.width_pt),
                    _ => (left, width),
                };
                let (from_y, room_y) = match a.v_from {
                    AnchorFrame::Page => (0.0, page_h),
                    _ => (top, 0.0),
                };
                let place = |start: f64, room: f64, size: f64, offset: i64, align: Option<AnchorAlign>| match align {
                    Some(AnchorAlign::Start) => start,
                    Some(AnchorAlign::Center) => start + (room - size) / 2.0,
                    Some(AnchorAlign::End) => start + room - size,
                    None => start + offset as f64 / EMU_PER_PT,
                };
                (place(from, room, w, a.x_emu, a.h_align), place(from_y, room_y, h, a.y_emu, a.v_align).max(0.0))
            }
        };
        // Not a body paragraph's: no paragraph index.
        page.items.push(Item::Image { para: usize::MAX, src, x_pt, y_pt, width_pt: w, height_pt: h });
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
                    let (w, h) = inline_size_pt(r);
                    vec![(OBJECT, w, h)]
                } else if r.style.text_box.is_some() {
                    vec![(OBJECT, 0.0, 0.0)]
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
            LineBox { start, end, x_pt: indent + align, width_pt: width, ascent_pt: ascent, descent_pt: size * 0.25, objects, text_pt: None }
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
