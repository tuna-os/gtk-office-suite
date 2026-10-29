// layout::pango — the production shaper, and the one Cairo painter for a
// `RenderTree` (ADR 0010). Screen, print and PDF all draw through
// `Painter::draw_page`, so they cannot disagree.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pango and Cairo are not GTK: this runs headless, in tests, in a worker
// thread, or into a `cairo::PdfSurface`.
//
// Units: the Pango context is set to 72 dpi, so one Pango device unit is
// one point and the tree needs no conversion. Metrics hinting is off, so
// line breaks and line heights do not change with zoom — the page is laid
// out once, in points, and only scaled for display.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use pango::prelude::*;

use super::{paragraph_request, request_key, Item, LayoutOptions, LineBox, Look, RenderTree, ShapeCache, ShapeRequest, Shaper, Source};
use crate::model::{Alignment, Document, Paragraph, VertAlign};

/// Pango's fixed-point scale.
const SCALE: f64 = pango::SCALE as f64;

/// A smart chip's label size relative to the text around it.
const CHIP_TEXT_SCALE: f64 = 0.92;

fn to_pt(units: i32) -> f64 {
    f64::from(units) / SCALE
}

fn to_units(pt: f64) -> i32 {
    (pt * SCALE).round() as i32
}

/// A shaper backed by Pango through a Cairo font map, at 72 dpi. It keeps
/// the Pango layout of each paragraph it shapes (by `request_key`), so
/// drawing a page reuses the layout its lines came from.
pub struct PangoShaper {
    context: pango::Context,
    layouts: HashMap<u64, pango::Layout>,
}

impl Default for PangoShaper {
    fn default() -> Self {
        Self::new()
    }
}

impl PangoShaper {
    pub fn new() -> Self {
        let font_map = pangocairo::FontMap::new();
        let context = font_map.create_context();
        pangocairo::functions::context_set_resolution(&context, 72.0);
        let mut options = cairo::FontOptions::new().expect("cairo font options");
        options.set_hint_metrics(cairo::HintMetrics::Off);
        options.set_hint_style(cairo::HintStyle::None);
        // Greyscale, like print and like LibreOffice: subpixel colour
        // fringes belong to one screen, not to the page.
        options.set_antialias(cairo::Antialias::Gray);
        pangocairo::functions::context_set_font_options(&context, Some(&options));
        // Positions in fractional points, not rounded to whole units.
        context.set_round_glyph_positions(false);
        Self { context, layouts: HashMap::new() }
    }

    /// A small label (a smart chip's, a footnote reference's number) in
    /// the paragraph's font at `size_pt`: what the shaper measures and the
    /// painter draws.
    fn label(&self, label: &str, font: &pango::FontDescription, size_pt: f64) -> pango::Layout {
        let layout = pango::Layout::new(&self.context);
        let mut font = font.clone();
        font.set_size(to_units(size_pt));
        layout.set_font_description(Some(&font));
        layout.set_text(label);
        layout
    }

    /// Installed family names, lowercased: what `Typeset` checks the
    /// document's families against for missing-font reporting.
    fn family_names(&self) -> HashSet<String> {
        self.context.list_families().iter().map(|f| f.name().to_string().to_lowercase()).collect()
    }

    /// The Pango layout of one paragraph, exactly as the engine shapes it.
    pub fn layout(&self, req: &ShapeRequest<'_>) -> pango::Layout {
        let layout = pango::Layout::new(&self.context);
        let mut base = pango::FontDescription::new();
        base.set_family(if req.code { "Liberation Mono" } else { &req.defaults.font_family });
        let mut size = req.defaults.font_size_pt;
        // A blank paragraph is as tall as its own (empty) run's font.
        if req.runs.iter().all(|r| r.text.is_empty() && !super::is_object(r)) {
            if let Some(st) = req.runs.first().map(|r| &r.style) {
                if let Some(hp) = st.font_size_hp {
                    size = f64::from(hp) / 2.0;
                }
                if let Some(f) = &st.font_family {
                    base.set_family(f);
                }
            }
        }
        // A heading's look: the document's own heading style if it has
        // one (family, size, weight, slant, colour), else bold and scaled.
        let mut heading_color = None;
        if let Some(level) = req.heading {
            size = req.defaults.heading_size_pt(level);
            match req.defaults.heading_style(level) {
                Some(h) => {
                    if let Some(f) = &h.font_family {
                        base.set_family(f);
                    }
                    if h.bold {
                        base.set_weight(pango::Weight::Bold);
                    }
                    if h.italic {
                        base.set_style(pango::Style::Italic);
                    }
                    heading_color = h.color.as_deref().and_then(parse_hex);
                }
                None => base.set_weight(pango::Weight::Bold),
            }
        }
        // Title, Subtitle and block quotes (a heading level wins, above).
        if req.heading.is_none() {
            size *= req.look.scale();
            match req.look {
                Look::Title => base.set_weight(pango::Weight::Bold),
                Look::Subtitle => heading_color = Some((0x6666, 0x6666, 0x6666)),
                Look::Quote => base.set_style(pango::Style::Italic),
                Look::Body => {}
            }
        }
        base.set_size(to_units(size));
        layout.set_font_description(Some(&base));

        let mut text = String::new();
        let attrs = pango::AttrList::new();
        if let Some((r, g, b)) = heading_color {
            attrs.insert(pango::AttrColor::new_foreground(r, g, b));
        }
        for run in req.runs {
            let start = text.len() as u32;
            if super::is_object(run) {
                // An image is one object char holding its box open; the
                // painter draws the picture into it (layout_text). A
                // footnote reference is one too, with no size yet.
                text.push(super::OBJECT);
                let rect = if run.style.chip.is_some() {
                    // A chip holds its pill open: the label's width plus
                    // padding and gaps, the label's own ascent and descent.
                    let run_size = run.style.font_size_hp.map_or(size, |hp| f64::from(hp) / 2.0);
                    let label = self.label(&run.text, &base, run_size * CHIP_TEXT_SCALE);
                    let (_, logical) = label.extents();
                    let w = to_pt(logical.width()) + 2.0 * (super::CHIP_PAD_PT + super::CHIP_GAP_PT);
                    pango::Rectangle::new(0, -label.baseline(), to_units(w), logical.height())
                } else if let Some(n) = run.style.footnote {
                    // A footnote reference holds its superscript number's
                    // width; it is drawn raised, so it adds no height.
                    let run_size = run.style.font_size_hp.map_or(size, |hp| f64::from(hp) / 2.0);
                    let label = self.label(&(n + 1).to_string(), &base, run_size * super::NOTE_REF_SCALE);
                    let (_, logical) = label.extents();
                    pango::Rectangle::new(0, 0, logical.width(), 0)
                } else if run.style.image_anchor.is_some() {
                    // A floating picture or text box takes no room in its
                    // line, but holds the line open as an empty one is: a
                    // paragraph holding only one is a line tall in Writer
                    // and Word, not none (the opt-out form's answer boxes
                    // each sit in a paragraph of their own).
                    let run_size = run.style.font_size_hp.map_or(size, |hp| f64::from(hp) / 2.0);
                    let label = self.label(" ", &base, run_size);
                    let (_, logical) = label.extents();
                    pango::Rectangle::new(0, -label.baseline(), 0, logical.height())
                } else {
                    let (w, h) = if run.style.image.is_some() { super::inline_size_pt(run) } else { (0.0, 0.0) };
                    pango::Rectangle::new(0, -to_units(h), to_units(w), to_units(h))
                };
                let mut a: pango::Attribute = pango::AttrShape::new(&rect, &rect).into();
                a.set_start_index(start);
                a.set_end_index(text.len() as u32);
                attrs.insert(a);
                continue;
            }
            text.push_str(&run.text);
            let end = text.len() as u32;
            if start == end {
                continue;
            }
            let add = |mut a: pango::Attribute| {
                a.set_start_index(start);
                a.set_end_index(end);
                attrs.insert(a);
            };
            let st = &run.style;
            if st.bold {
                add(pango::AttrInt::new_weight(pango::Weight::Bold).into());
            } else if st.regular {
                add(pango::AttrInt::new_weight(pango::Weight::Normal).into());
            }
            if st.italic {
                add(pango::AttrInt::new_style(pango::Style::Italic).into());
            }
            if st.caps {
                add(pango::AttrInt::new_text_transform(pango::TextTransform::Uppercase).into());
            }
            if st.underline || st.link.is_some() {
                add(pango::AttrInt::new_underline(pango::Underline::Single).into());
            }
            if st.strikethrough {
                add(pango::AttrInt::new_strikethrough(true).into());
            }
            if let Some(family) = &st.font_family {
                add(pango::AttrString::new_family(family).into());
            }
            if st.code {
                add(pango::AttrString::new_family("Liberation Mono").into());
            }
            let run_size = st.font_size_hp.map_or(size, |hp| f64::from(hp) / 2.0);
            if st.font_size_hp.is_some() {
                add(pango::AttrSize::new(to_units(run_size)).into());
            }
            if let Some(tw) = st.letter_spacing_tw {
                add(pango::AttrInt::new_letter_spacing(to_units(f64::from(tw) / 20.0)).into());
            }
            // Pango grows the line by the rise, as LibreOffice does.
            if let Some(hp) = st.position_hp {
                add(pango::AttrInt::new_rise(to_units(f64::from(hp) / 2.0)).into());
            }
            if let Some((r, g, b)) = st.color.as_deref().and_then(parse_hex) {
                add(pango::AttrColor::new_foreground(r, g, b).into());
            } else if st.link.is_some() {
                add(pango::AttrColor::new_foreground(0x1a1a, 0x5f5f, 0xb4b4).into());
            }
            if st.highlight {
                add(pango::AttrColor::new_background(0xffff, 0xffff, 0x0000).into());
            }
            match st.vert_align {
                // 58% size, raised a third / lowered a sixth of the line,
                // as LibreOffice and Word draw them.
                Some(VertAlign::Superscript) => {
                    add(pango::AttrFloat::new_scale(0.58).into());
                    add(pango::AttrInt::new_rise(to_units(run_size * 0.33)).into());
                }
                Some(VertAlign::Subscript) => {
                    add(pango::AttrFloat::new_scale(0.58).into());
                    add(pango::AttrInt::new_rise(-to_units(run_size * 0.14)).into());
                }
                None => {}
            }
            // A tracked change, as Word and LibreOffice show one: in its
            // author's colour, an insertion underlined, a deletion struck
            // through. Last, so it wins over the run's own colour.
            if let Some(rev) = &st.revision {
                let (r, g, b) = crate::track::author_color(&rev.author);
                add(pango::AttrColor::new_foreground(r, g, b).into());
                match rev.kind {
                    crate::model::RevisionKind::Insert => add(pango::AttrInt::new_underline(pango::Underline::Single).into()),
                    crate::model::RevisionKind::Delete => add(pango::AttrInt::new_strikethrough(true).into()),
                }
            }
        }
        // Spaces ending a paragraph take no width. Pango lets spaces hang
        // past the margin where it breaks a line, but not at the end of the
        // text, so a last line that fits to the margin without its trailing
        // space was wrapped a word early (render-lab letters/indents: the
        // fixture's paragraphs end in a space, as typed text often does).
        // Word and LibreOffice let them hang. A zero-size shape keeps the
        // chars, so offsets are unchanged.
        let body = text.trim_end_matches([' ', '\t']);
        if !body.trim().is_empty() && body.len() < text.len() {
            let zero = pango::Rectangle::new(0, 0, 0, 0);
            let mut a: pango::Attribute = pango::AttrShape::new(&zero, &zero).into();
            a.set_start_index(body.len() as u32);
            a.set_end_index(text.len() as u32);
            attrs.insert(a);
        }
        layout.set_text(&text);
        layout.set_attributes(Some(&attrs));
        // A negative first-line indent is a hanging one: the first line
        // starts left of the text box (Word's w:hanging). Pango reads a
        // negative indent differently, leaving the first line in place and
        // indenting the others, so the layout is widened by the hang and
        // line_boxes shifts every line back by it. Without that, a
        // paragraph indented 72pt with a 72pt hang drew its first line at
        // 72pt and the rest at 144pt instead of at 0pt and 72pt.
        let hang = hang_pt(req);
        layout.set_width(to_units(req.width_pt.max(1.0) + hang));
        layout.set_wrap(pango::WrapMode::WordChar);
        layout.set_indent(to_units(req.first_line_indent_pt));
        layout.set_alignment(match req.alignment {
            Alignment::Center => pango::Alignment::Center,
            Alignment::Right => pango::Alignment::Right,
            _ => pango::Alignment::Left,
        });
        layout.set_justify(req.alignment == Alignment::Justify);
        if req.right_tab {
            let mut tabs = pango::TabArray::new(1, false);
            tabs.set_tab(0, pango::TabAlign::Right, to_units(req.width_pt));
            layout.set_tabs(Some(&tabs));
        } else if !req.tab_stops.is_empty() {
            let mut tabs = pango::TabArray::new(req.tab_stops.len() as i32, false);
            for (i, t) in req.tab_stops.iter().enumerate() {
                let align = match t.align {
                    crate::model::TabAlign::Center => pango::TabAlign::Center,
                    crate::model::TabAlign::Right => pango::TabAlign::Right,
                    crate::model::TabAlign::Decimal => pango::TabAlign::Decimal,
                    crate::model::TabAlign::Left => pango::TabAlign::Left,
                };
                tabs.set_tab(i as i32, align, to_units(t.pos_pt));
            }
            layout.set_tabs(Some(&tabs));
        }
        layout
    }
}

/// "RRGGBB" to 16-bit channels, via the canonical shared colour
/// (`suite_common_core::color::Color`). Behaviour on the inputs
/// Letters produces (6-digit RRGGBB, optional '#') is unchanged; the
/// shared parser additionally trims whitespace and accepts AARRGGBB,
/// which Letters' model never emits.
fn parse_hex(hex: &str) -> Option<(u16, u16, u16)> {
    suite_common_core::color::Color::from_hex(hex).map(|c| c.to_u16())
}

/// Lines of a layout as `LineBox`es, with char (not byte) offsets.
/// How far a paragraph's first line hangs left of its text box (a
/// negative first-line indent), in points.
fn hang_pt(req: &ShapeRequest<'_>) -> f64 {
    (-req.first_line_indent_pt).max(0.0)
}

/// The lines of `layout`, placed relative to the text box: `hang` is what
/// `layout` widened it by for a hanging first line (`hang_pt`).
fn line_boxes(layout: &pango::Layout, hang: f64) -> Vec<LineBox> {
    let text = layout.text();
    let char_at = |byte: i32| text[..(byte.max(0) as usize).min(text.len())].chars().count();
    // Inline objects: (char index, byte index).
    let objects: Vec<(usize, i32)> = text
        .char_indices()
        .enumerate()
        .filter(|(_, (_, c))| *c == super::OBJECT)
        .map(|(ci, (bi, _))| (ci, bi as i32))
        .collect();
    let mut out = Vec::new();
    let mut iter = layout.iter();
    loop {
        let (_, logical) = iter.line_extents();
        let baseline = iter.baseline();
        if let Some(line) = iter.line_readonly() {
            let start = line.start_index();
            let (cs, ce) = (char_at(start), char_at(start + line.length()));
            out.push(LineBox {
                objects: objects
                    .iter()
                    .filter(|(ci, _)| (cs..ce).contains(ci))
                    .map(|&(ci, bi)| (ci, to_pt(layout.index_to_pos(bi).x()) - hang))
                    .collect(),
                start: cs,
                end: ce,
                x_pt: to_pt(logical.x()) - hang,
                width_pt: to_pt(logical.width()),
                ascent_pt: to_pt(baseline - logical.y()),
                descent_pt: to_pt(logical.y() + logical.height() - baseline),
                text_pt: None,
            });
        }
        if !iter.next_line() {
            break;
        }
    }
    out
}

impl Shaper for PangoShaper {
    fn shape(&mut self, req: &ShapeRequest<'_>) -> Vec<LineBox> {
        let layout = self.layout(req);
        self.layouts.insert(request_key(req), layout.clone());
        let mut lines = line_boxes(&layout, hang_pt(req));
        // A line is as tall as the font's ascent, descent *and* line gap,
        // as LibreOffice and Word space lines (Liberation Serif 12pt:
        // 13.8pt); Pango's logical extents leave the gap out. The gap is
        // the text's own font's: a paragraph of 14pt Arial in a Calibri
        // document has Arial's (render-lab `real-salary-template`).
        if let Some(mut desc) = layout.font_description() {
            // A blank paragraph's is its empty run's, as its height is.
            let text = req.runs.iter().find(|r| !r.text.is_empty() && !super::is_object(r));
            if let Some(run) = text.or_else(|| req.runs.iter().find(|r| !super::is_object(r))) {
                if let Some(f) = &run.style.font_family {
                    desc.set_family(f);
                }
                if let Some(hp) = run.style.font_size_hp {
                    desc.set_size(to_units(f64::from(hp) / 2.0));
                }
            }
            let m = self.context.metrics(Some(&desc), None);
            let gap = to_pt(m.height() - m.ascent() - m.descent()).max(0.0);
            for l in &mut lines {
                l.descent_pt += gap;
                // A line holding an inline picture: its text's own height,
                // which proportional spacing scales (`LineBox::text_height`).
                if l.objects.iter().any(|&(ch, _)| super::run_at(req.runs, ch).is_some_and(|r| r.style.image.is_some() && r.style.image_anchor.is_none())) {
                    l.text_pt = Some(to_pt(m.height()));
                }
            }
        }
        // Raised or lowered text (`position_hp`) grows its line by as much,
        // as LibreOffice draws it; Pango (since 1.50) sizes a line by its
        // fonts alone.
        let mut at = 0;
        let shifts: Vec<(usize, usize, f64)> = req
            .runs
            .iter()
            .map(|r| {
                let n = r.text.chars().count();
                at += n;
                (at - n, at, r.style.position_hp.map_or(0.0, |hp| f64::from(hp) / 2.0))
            })
            .filter(|&(_, _, pt)| pt != 0.0)
            .collect();
        for l in &mut lines {
            let on = shifts.iter().filter(|&&(s, e, _)| s < l.end.max(l.start + 1) && e > l.start);
            let (up, down) = on.fold((0.0f64, 0.0f64), |(u, d), &(_, _, pt)| (u.max(pt), d.max(-pt)));
            l.ascent_pt += up;
            l.descent_pt += down;
        }
        lines
    }
}

/// A document laid out with Pango, ready to draw: the `RenderTree` plus
/// what drawing it needs. `draw_page` is the one routine that paints a
/// page, onto any Cairo context — a widget snapshot, a print context or a
/// PDF surface. The context's user space must be the page in points (scale
/// it for zoom before calling).
pub struct Typeset {
    doc: Document,
    opts: LayoutOptions,
    tree: RenderTree,
    shaper: PangoShaper,
    /// Shaped paragraphs kept for the next `update`.
    shapes: ShapeCache,
    /// Layouts made for drawing that the shaper did not keep (a paragraph's
    /// lines came from the shape cache), by request key.
    cache: RefCell<HashMap<u64, pango::Layout>>,
    images: RefCell<HashMap<String, Option<cairo::ImageSurface>>>,
    loader: Box<ImageLoader>,
    /// The `LiveModel` revision this was laid out from (0 = unpinned, as in
    /// tests that build documents by hand). Print, thumbnails, PDF and
    /// headless export carry the revision they drew.
    revision: u64,
}

/// Decodes the image at a path for drawing.
type ImageLoader = dyn Fn(&str) -> Option<cairo::ImageSurface>;

/// A place in the document's text: paragraph index and char offset in its
/// layout text (`layout::layout_text`: an image is one char).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextPos {
    pub para: usize,
    pub offset: usize,
}

/// A piece of a selection: (page, x, top, width, height) in points.
pub type SelectionRect = (usize, f64, f64, f64, f64);

/// A comment's margin mark: width and height in points.
pub const COMMENT_MARK_PT: (f64, f64) = (12.0, 10.0);

/// An open comment thread on the pages, as a view shows it (print and PDF
/// do not, as LibreOffice's do not): the rectangles its text covers, and
/// a spot in the right margin beside its first line.
#[derive(Clone, Debug, PartialEq)]
pub struct CommentMark {
    pub id: u32,
    pub author: String,
    /// Comments in the thread, the first included.
    pub count: usize,
    /// Where its text starts.
    pub from: TextPos,
    pub rects: Vec<SelectionRect>,
    pub page: usize,
    /// The margin spot's left and top, in points on `page`.
    pub x_pt: f64,
    pub y_pt: f64,
}

/// A caret's box, in points on page `page`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caret {
    pub page: usize,
    pub x_pt: f64,
    pub top_pt: f64,
    pub height_pt: f64,
}

/// One body line on a page, as hit-testing needs it.
struct LineRef {
    para: usize,
    line: usize,
    box_x: f64,
    box_w: f64,
    x: f64,
    top: f64,
    height: f64,
    start: usize,
    end: usize,
}

/// The path of a bullet at `x` on `baseline` for text of `size` points:
/// a disc 0.31 em across, its centre 0.28 em above the baseline. A path of
/// its own: after text, the current point is where that text was drawn
/// from, and an arc from there is joined to it by a line.
fn bullet_path(cr: &cairo::Context, x: f64, baseline: f64, size: f64) {
    let r = size * 0.155;
    cr.new_path();
    cr.arc(x + r, baseline - size * 0.28, r, 0.0, std::f64::consts::TAU);
}

/// How far a double border's inner hairline sits from its outer one: one
/// full edge width toward the box's centre.
fn edge_offset((x0, y0): (f64, f64), (x1, y1): (f64, f64), bounds: (f64, f64, f64, f64), by: f64) -> (f64, f64) {
    let (left, top, right, bottom) = bounds;
    let (dx, dy) = ((left + right) / 2.0 - (x0 + x1) / 2.0, (top + bottom) / 2.0 - (y0 + y1) / 2.0);
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    (dx / len * by, dy / len * by)
}

/// Fill a tab's gap `from..to` with leader glyph `glyph` (`w` points
/// wide) as Writer does (`SwTabPortion::Paint`): as many whole glyphs as
/// fit, packed at their own advance from the tab's start, the remainder
/// left before the text.
fn fill_leader(cr: &cairo::Context, glyph: &pango::LayoutLine, from: f64, to: f64, w: f64, baseline_pt: f64) {
    let n = ((to - from) / w + 1e-6).floor().max(0.0) as usize;
    for k in 0..n {
        cr.move_to(from + k as f64 * w, baseline_pt);
        pangocairo::functions::show_layout_line(cr, glyph);
    }
}

/// The built-in image loader: PNG through Cairo.
fn load_png(path: &str) -> Option<cairo::ImageSurface> {
    let mut f = std::fs::File::open(path).ok()?;
    cairo::ImageSurface::create_from_png(&mut f).ok()
}

impl Typeset {
    /// Lay `doc` out into pages.
    pub fn new(doc: Document, opts: LayoutOptions) -> Self {
        let mut shaper = PangoShaper::new();
        let mut shapes = ShapeCache::default();
        let opts = opts.for_document(&doc);
        let tree = super::relayout(&doc, &opts, &mut shaper, &mut shapes);
        shapes.prune();
        Self {
            doc,
            opts,
            tree,
            shaper,
            shapes,
            cache: RefCell::new(HashMap::new()),
            images: RefCell::new(HashMap::new()),
            loader: Box::new(load_png),
            revision: 0,
        }
    }

    pub fn tree(&self) -> &RenderTree {
        &self.tree
    }

    /// Where each page starts, as a character offset in the document's
    /// text (`RenderTree::page_breaks`).
    pub fn page_breaks(&self) -> Vec<usize> {
        self.tree.page_breaks(&self.doc)
    }

    /// Lay out `doc` in place of the current document, re-shaping only the
    /// paragraphs that changed (ADR 0010 stage 3c). Returns how many were
    /// shaped. The revision is unpinned (0) until the caller pins the new
    /// document's with `set_revision`: an updated tree must never claim the
    /// old revision.
    pub fn update(&mut self, doc: Document, opts: LayoutOptions) -> usize {
        self.opts = opts.for_document(&doc);
        self.tree = super::relayout(&doc, &self.opts, &mut self.shaper, &mut self.shapes);
        self.doc = doc;
        self.revision = 0;
        let shaped = self.shapes.misses;
        self.shapes.prune();
        let keep: std::collections::HashSet<u64> = self.shapes.keys().copied().collect();
        self.shaper.layouts.retain(|k, _| keep.contains(k));
        self.cache.borrow_mut().clear();
        shaped
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// Pin the `LiveModel` revision this layout was drawn from (LET-004).
    pub fn set_revision(&mut self, revision: u64) {
        self.revision = revision;
    }

    /// The pinned revision (0 = unpinned).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Families the document asks for that Pango has no face for, one
    /// sorted list per page (LET-004): Pango substitutes silently, so the
    /// export names the substituting family per page in the log instead of
    /// failing — a test fails only where its fixture requires the missing
    /// face.
    pub fn missing_fonts_by_page(&self) -> Vec<Vec<String>> {
        // Pango's CSS generics always resolve through fontconfig; only a
        // named family with no installed face substitutes silently.
        const GENERIC: [&str; 3] = ["sans", "serif", "monospace"];
        let installed = self.shaper.family_names();
        let mut out: Vec<Vec<String>> = Vec::with_capacity(self.tree.pages.len());
        for page in &self.tree.pages {
            let mut fams: HashSet<String> = HashSet::new();
            let mut add = |p: &Paragraph| {
                fams.insert(self.opts.font_family.clone());
                if p.style.code_block.is_some() {
                    fams.insert("Liberation Mono".to_string());
                }
                if let Some(level) = p.style.heading {
                    if let Some(f) = self.opts.heading_style(level).and_then(|h| h.font_family.clone()) {
                        fams.insert(f);
                    }
                }
                fams.extend(p.runs.iter().filter_map(|r| r.style.font_family.clone()));
            };
            for item in &page.items {
                match item {
                    Item::Line { source: Source::Paragraph(p), .. }
                    | Item::Marker { para: p, .. }
                    | Item::Chip { para: p, .. }
                    | Item::NoteRef { para: p, .. } => {
                        if let Some(p) = self.doc.paragraphs.get(*p) {
                            add(p);
                        }
                    }
                    Item::Line { source: Source::Footnote(n), .. } => {
                        add(&super::note_paragraph(&self.doc, *n, &self.opts));
                    }
                    // Headers, footers, rules, images and cell boxes draw in
                    // the base family or no text at all: the base is pinned
                    // below.
                    _ => {}
                }
            }
            // The base family underlies every page (headers, footers, the
            // footnote area): report it missing wherever it is asked for.
            fams.insert(self.opts.font_family.clone());
            let mut missing: Vec<String> = fams
                .into_iter()
                .filter(|f| {
                    let lower = f.to_lowercase();
                    !GENERIC.contains(&lower.as_str()) && !installed.contains(&lower)
                })
                .collect();
            missing.sort();
            out.push(missing);
        }
        out
    }

    /// Decode images with `loader` (path to a Cairo surface) rather than
    /// the built-in PNG-only one. The app passes one backed by GdkTexture,
    /// which reads every format it can show.
    pub fn set_image_loader(&mut self, loader: impl Fn(&str) -> Option<cairo::ImageSurface> + 'static) {
        self.loader = Box::new(loader);
    }

    /// A smart chip: a rounded pill in the chip's colour with its label,
    /// shaped as `PangoShaper::layout` measured it.
    fn draw_chip(&self, cr: &cairo::Context, para: usize, ch: usize, x: f64, baseline: f64) {
        let Some(p) = self.doc.paragraphs.get(para) else { return };
        let Some(run) = super::run_at(&p.runs, ch) else { return };
        let Some(chip) = &run.style.chip else { return };
        // The paragraph's own font, as the shaper used it.
        let req = paragraph_request(p, 1000.0, &self.opts);
        let base = self.shaper.layout(&ShapeRequest { runs: &[], ..req });
        let font = base.font_description().unwrap_or_default();
        let size = run.style.font_size_hp.map_or(to_pt(font.size()), |hp| f64::from(hp) / 2.0);
        let label = self.shaper.label(&run.text, &font, size * CHIP_TEXT_SCALE);
        let (_, logical) = label.extents();
        let (w, h) = (to_pt(logical.width()) + 2.0 * super::CHIP_PAD_PT, to_pt(logical.height()));
        let (left, top) = (x + super::CHIP_GAP_PT, baseline - to_pt(label.baseline()));
        // Google's chip colours, light enough to print.
        let (fill, ink) = match chip.kind {
            crate::chips::ChipKind::Date => ((0.91, 0.94, 0.99), (0.10, 0.34, 0.71)),
            crate::chips::ChipKind::Person => ((0.93, 0.93, 0.93), (0.13, 0.13, 0.13)),
            crate::chips::ChipKind::Link => ((0.90, 0.96, 0.92), (0.09, 0.42, 0.22)),
        };
        let r = h / 2.0;
        let _ = cr.save();
        cr.new_sub_path();
        cr.arc(left + r, top + r, r, std::f64::consts::FRAC_PI_2, 3.0 * std::f64::consts::FRAC_PI_2);
        cr.arc(left + w - r, top + r, r, -std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
        cr.close_path();
        cr.set_source_rgb(fill.0, fill.1, fill.2);
        let _ = cr.fill();
        cr.set_source_rgb(ink.0, ink.1, ink.2);
        cr.move_to(left + super::CHIP_PAD_PT, top);
        pangocairo::functions::show_layout(cr, &label);
        let _ = cr.restore();
    }

    /// A footnote reference: the note's number, superscript, as the shaper
    /// measured it.
    fn draw_note_ref(&self, cr: &cairo::Context, para: usize, ch: usize, x: f64, baseline: f64) {
        let Some(p) = self.doc.paragraphs.get(para) else { return };
        let Some(run) = super::run_at(&p.runs, ch) else { return };
        let Some(n) = run.style.footnote else { return };
        let req = paragraph_request(p, 1000.0, &self.opts);
        let base = self.shaper.layout(&ShapeRequest { runs: &[], ..req });
        let font = base.font_description().unwrap_or_default();
        let size = run.style.font_size_hp.map_or(to_pt(font.size()), |hp| f64::from(hp) / 2.0);
        let label = self.shaper.label(&(n + 1).to_string(), &font, size * super::NOTE_REF_SCALE);
        let _ = cr.save();
        cr.move_to(x, baseline - size * super::NOTE_REF_RISE - to_pt(label.baseline()));
        pangocairo::functions::show_layout(cr, &label);
        let _ = cr.restore();
    }

    fn draw_image(&self, cr: &cairo::Context, src: &str, x: f64, y: f64, w: f64, h: f64) {
        let surface = self.images.borrow_mut().entry(src.to_string()).or_insert_with(|| (self.loader)(src)).clone();
        let _ = cr.save();
        match surface {
            Some(img) if img.width() > 0 && img.height() > 0 => {
                cr.rectangle(x, y, w, h);
                cr.clip();
                cr.translate(x, y);
                cr.scale(w / f64::from(img.width()), h / f64::from(img.height()));
                let _ = cr.set_source_surface(&img, 0.0, 0.0);
                let _ = cr.paint();
            }
            // Unreadable: a light frame where it would be, not nothing.
            _ => {
                cr.set_source_rgb(0.8, 0.8, 0.8);
                cr.set_line_width(0.5);
                cr.rectangle(x, y, w, h);
                let _ = cr.stroke();
            }
        }
        let _ = cr.restore();
    }

    fn paragraph_layout(&self, para: usize, box_w: f64) -> Option<pango::Layout> {
        let p = self.doc.paragraphs.get(para)?;
        let req = paragraph_request(p, box_w, &self.opts);
        let key = request_key(&req);
        if let Some(l) = self.shaper.layouts.get(&key) {
            return Some(l.clone());
        }
        let mut cache = self.cache.borrow_mut();
        Some(cache.entry(key).or_insert_with(|| self.shaper.layout(&req)).clone())
    }

    /// Write every page to a PDF at `path`, one PDF page per laid-out page
    /// at its own size, drawn by `draw_page` — the same routine the page
    /// view and printing use, so the PDF is the Print Layout.
    pub fn write_pdf(&self, path: impl AsRef<std::path::Path>) -> Result<(), String> {
        let first = self.tree.pages.first().ok_or("no pages to write")?;
        // Missing fonts substitute silently in Pango: name the substituting
        // family per page, so an export that fell back is explicit (LET-004).
        for (index, missing) in self.missing_fonts_by_page().iter().enumerate() {
            for family in missing {
                eprintln!("letters: PDF export substitutes for missing font family '{family}' on page {}", index + 1);
            }
        }
        let surface = cairo::PdfSurface::new(first.width_pt, first.height_pt, path.as_ref())
            .map_err(|e| format!("cannot create {}: {e}", path.as_ref().display()))?;
        // PDF 1.4: readable everywhere, and no compressed object streams, so
        // the page tree is plain text a test can count.
        surface.restrict(cairo::PdfVersion::_1_4).map_err(|e| e.to_string())?;
        let cr = cairo::Context::new(&surface).map_err(|e| e.to_string())?;
        for (index, page) in self.tree.pages.iter().enumerate() {
            // A section of another size (landscape) gets its own page size.
            surface.set_size(page.width_pt, page.height_pt).map_err(|e| e.to_string())?;
            self.draw_page(&cr, index);
            cr.show_page().map_err(|e| e.to_string())?;
        }
        drop(cr);
        surface.finish();
        match surface.status() {
            Ok(()) => Ok(()),
            Err(e) => Err(format!("cannot write {}: {e}", path.as_ref().display())),
        }
    }

    /// The body line items of `page`: (paragraph, line, box width, line x,
    /// top, height, char range).
    fn body_lines(&self, page: usize) -> Vec<LineRef> {
        let Some(page) = self.tree.pages.get(page) else { return Vec::new() };
        page.items
            .iter()
            .filter_map(|i| match i {
                Item::Line { source: Source::Paragraph(p), line, box_x_pt, box_width_pt, x_pt, top_pt, height_pt, start, end, .. } => {
                    Some(LineRef {
                        para: *p,
                        line: *line,
                        box_x: *box_x_pt,
                        box_w: *box_width_pt,
                        x: *x_pt,
                        top: *top_pt,
                        height: *height_pt,
                        start: *start,
                        end: *end,
                    })
                }
                _ => None,
            })
            .collect()
    }

    /// The text position nearest to (`x_pt`, `y_pt`) on page `page`, in
    /// points from the page's corner: the line under the point (or the
    /// nearest one), then the character boundary nearest to it on that line.
    pub fn hit_test(&self, page: usize, x_pt: f64, y_pt: f64) -> Option<TextPos> {
        let lines = self.body_lines(page);
        // Distance from the point to a line's box, vertical first: a click
        // beside a table cell belongs to that cell's row, then its column.
        let distance = |l: &LineRef| {
            let dy = if y_pt < l.top { l.top - y_pt } else if y_pt > l.top + l.height { y_pt - l.top - l.height } else { 0.0 };
            let dx = if x_pt < l.box_x { l.box_x - x_pt } else if x_pt > l.box_x + l.box_w { x_pt - l.box_x - l.box_w } else { 0.0 };
            (dy, dx)
        };
        let best = lines.iter().min_by(|a, b| distance(a).partial_cmp(&distance(b)).unwrap_or(std::cmp::Ordering::Equal))?;
        let layout = self.paragraph_layout(best.para, best.box_w)?;
        let line = layout.line_readonly(best.line as i32)?;
        let hit = line.x_to_index(to_units(x_pt - best.x));
        let text = layout.text();
        let byte = (hit.index().max(0) as usize).min(text.len());
        let mut offset = text[..byte].chars().count() + hit.trailing().max(0) as usize;
        // The end of a wrapped line is the start of the next one; keep the
        // caret on the line that was clicked (before its trailing space).
        let para_len = text.chars().count();
        if offset >= best.end && best.end < para_len && best.end > best.start {
            offset = best.end - 1;
        }
        Some(TextPos { para: best.para, offset: offset.min(para_len) })
    }

    /// Where the caret for `pos` is drawn: page, x, line top and height in
    /// points. The caret at a wrap point is on the later line.
    pub fn caret(&self, pos: TextPos) -> Option<Caret> {
        for (index, _) in self.tree.pages.iter().enumerate() {
            let lines = self.body_lines(index);
            let para_lines: Vec<&LineRef> = lines.iter().filter(|l| l.para == pos.para).collect();
            let Some(line) = para_lines
                .iter()
                .find(|l| pos.offset >= l.start && (pos.offset < l.end || l.start == l.end))
                .or_else(|| para_lines.iter().rev().find(|l| pos.offset == l.end))
            else {
                continue;
            };
            let layout = self.paragraph_layout(line.para, line.box_w)?;
            let pl = layout.line_readonly(line.line as i32)?;
            let text = layout.text();
            let byte = text.char_indices().nth(pos.offset).map_or(text.len(), |(b, _)| b);
            let x = to_pt(pl.index_to_x(byte as i32, false));
            return Some(Caret { page: index, x_pt: line.x + x, top_pt: line.top, height_pt: line.height });
        }
        None
    }

    /// Rectangles covering the text from `from` to `to` (in document order),
    /// one per line piece: (page, x, top, width, height) in points.
    pub fn selection_rects(&self, from: TextPos, to: TextPos) -> Vec<SelectionRect> {
        let (a, b) = if (to.para, to.offset) < (from.para, from.offset) { (to, from) } else { (from, to) };
        let mut out = Vec::new();
        for index in 0..self.tree.pages.len() {
            for l in self.body_lines(index) {
                if l.para < a.para || l.para > b.para {
                    continue;
                }
                let s = if l.para == a.para { a.offset.max(l.start) } else { l.start };
                let e = if l.para == b.para { b.offset.min(l.end) } else { l.end };
                let whole_rest = l.para < b.para && l.end >= self.layout_len(l.para);
                if s > e || (s == e && !whole_rest) {
                    continue;
                }
                let Some(layout) = self.paragraph_layout(l.para, l.box_w) else { continue };
                let Some(pl) = layout.line_readonly(l.line as i32) else { continue };
                let text = layout.text();
                let byte = |c: usize| text.char_indices().nth(c).map_or(text.len(), |(b, _)| b) as i32;
                let x0 = to_pt(pl.index_to_x(byte(s), false));
                let mut x1 = to_pt(pl.index_to_x(byte(e), false));
                // A selection running past the paragraph end shows its
                // newline as a space's width.
                if whole_rest {
                    x1 += 4.0;
                }
                out.push((index, l.x + x0.min(x1), l.top, (x1 - x0).abs(), l.height));
            }
        }
        out
    }

    /// The page (from 0) each paragraph starts on: a table of contents'
    /// page numbers (`crate::toc::settle`). None for one with no line.
    pub fn paragraph_pages(&self) -> Vec<Option<usize>> {
        let mut out = vec![None; self.doc.paragraphs.len()];
        for (i, page) in self.tree.pages.iter().enumerate() {
            for item in &page.items {
                if let Item::Line { source: Source::Paragraph(p), .. } = item {
                    if let Some(slot) = out.get_mut(*p).filter(|s| s.is_none()) {
                        *slot = Some(i);
                    }
                }
            }
        }
        out
    }

    /// The open (unresolved) comment threads with text, in document order
    /// (`crate::comments`). Spots on one line sit side by side.
    pub fn comment_marks(&self) -> Vec<CommentMark> {
        let mut out: Vec<CommentMark> = Vec::new();
        let pos = |at: usize| crate::edit::locate(&self.doc, at).map(|(para, offset)| TextPos { para, offset });
        for t in crate::comments::threads(&self.doc) {
            let Some(a) = t.anchor.filter(|_| !t.comment.resolved) else { continue };
            let (Some(from), Some(to)) = (pos(a.start), pos(a.end)) else { continue };
            let rects = self.selection_rects(from, to);
            let Some(&(page, _, top, _, _)) = rects.first() else { continue };
            let Some(p) = self.tree.pages.get(page) else { continue };
            let mut x = (p.width_pt - p.geometry.margin_right_pt + 8.0).min(p.width_pt - 20.0);
            // Beside the spots already on this line.
            x += 16.0 * out.iter().filter(|m| m.page == page && (m.y_pt - top).abs() < 1.0).count() as f64;
            out.push(CommentMark { id: t.comment.id, author: t.comment.author, count: 1 + t.replies.len(), from, rects, page, x_pt: x, y_pt: top });
        }
        out
    }

    /// A table of contents entry's dot leader: dots across its tab's gap on
    /// `line`, from the page number back, a dot and a half apart.
    fn draw_leader(&self, cr: &cairo::Context, para: usize, layout: &pango::Layout, line: &pango::LayoutLine, x_pt: f64, baseline_pt: f64) {
        let text = layout.text();
        let Some(tab) = text.find('\t') else { return };
        let start = line.start_index().max(0) as usize;
        if tab < start || tab >= start + line.length().max(0) as usize {
            return;
        }
        let from = x_pt + to_pt(line.index_to_x(tab as i32, false));
        let to = x_pt + to_pt(line.index_to_x(tab as i32, true));
        let Some(p) = self.doc.paragraphs.get(para) else { return };
        let dot_runs = [crate::model::Run::plain(".")];
        let mut req = paragraph_request(p, 1000.0, &self.opts);
        req.runs = &dot_runs;
        req.right_tab = false;
        let dot = self.shaper.layout(&req);
        let Some(dot_line) = dot.line_readonly(0) else { return };
        let w = to_pt(dot_line.index_to_x(1, false)).max(1.0);
        fill_leader(cr, &dot_line, from, to, w, baseline_pt);
    }

    /// A tab stop's leader across its tab's gap on `line`: the stop's
    /// leader glyph (dot, hyphen, underscore, middle dot) tiled from the
    /// tab to the text, as Writer draws it. Each line's tabs consume the
    /// paragraph's stops from the first, as Word numbers them.
    fn draw_tab_leaders(&self, cr: &cairo::Context, para: usize, layout: &pango::Layout, line: &pango::LayoutLine, x_pt: f64, baseline_pt: f64) {
        let Some(p) = self.doc.paragraphs.get(para) else { return };
        if p.style.toc.is_some() {
            return;
        }
        let text = layout.text();
        let start = line.start_index().max(0) as usize;
        let end = start + line.length().max(0) as usize;
        let mut stop_idx = 0usize;
        let mut at = start;
        while let Some(rel) = text.get(at..end).and_then(|t| t.find('\t')) {
            let tab = at + rel;
            let glyph = match p.style.tab_stops_pt.get(stop_idx).map(|s| s.leader) {
                Some(crate::model::TabLeader::Dot) => ".",
                Some(crate::model::TabLeader::Hyphen) => "-",
                Some(crate::model::TabLeader::Underscore) => "_",
                Some(crate::model::TabLeader::MiddleDot) => "·",
                _ => {
                    stop_idx += 1;
                    at = tab + 1;
                    continue;
                }
            };
            stop_idx += 1;
            let from = x_pt + to_pt(line.index_to_x(tab as i32, false));
            let to = x_pt + to_pt(line.index_to_x(tab as i32, true));
            let glyph_runs = [crate::model::Run::plain(glyph)];
            let mut req = paragraph_request(p, 1000.0, &self.opts);
            req.runs = &glyph_runs;
            req.right_tab = false;
            let glyph_layout = self.shaper.layout(&req);
            let Some(glyph_line) = glyph_layout.line_readonly(0) else { at = tab + 1; continue };
            let w = to_pt(glyph_line.index_to_x(glyph.len() as i32, false)).max(1.0);
            fill_leader(cr, &glyph_line, from, to, w, baseline_pt);
            at = tab + 1;
        }
    }

    /// A paragraph's shading and borders: the fill, then each set edge
    /// as its own segment, so a box split across pages opens where the
    /// paragraph continues. Under the text, like a table cell's fill.
    fn draw_para_box(
        &self,
        cr: &cairo::Context,
        rect: (f64, f64, f64, f64),
        fill: Option<&str>,
        borders: &crate::model::ParaBorders,
        top_edge: bool,
        bottom_edge: bool,
    ) {
        let (x, y, w, h) = rect;
        use crate::model::BorderStyle;
        if let Some((r, g, b)) = fill.and_then(parse_hex) {
            let _ = cr.save();
            cr.set_source_rgb(f64::from(r) / 65535.0, f64::from(g) / 65535.0, f64::from(b) / 65535.0);
            cr.rectangle(x, y, w, h);
            let _ = cr.fill();
            let _ = cr.restore();
        }
        // Each border lies inside the box, its outer edge on the box's.
        let half = |e: &Option<crate::model::ParaBorder>| e.as_ref().map_or(0.0, |e| e.width_pt.clamp(0.25, 12.0) / 2.0);
        let (t, b, l, r) = (half(&borders.top), half(&borders.bottom), half(&borders.left), half(&borders.right));
        for (edge, (x0, y0), (x1, y1)) in [
            (borders.top.clone().filter(|_| top_edge), (x, y + t), (x + w, y + t)),
            (borders.bottom.clone().filter(|_| bottom_edge), (x, y + h - b), (x + w, y + h - b)),
            (borders.left.clone(), (x + l, y), (x + l, y + h)),
            (borders.right.clone(), (x + w - r, y), (x + w - r, y + h)),
        ] {
            let Some(edge) = edge else { continue };
            let Some((r, g, b)) = parse_hex(&edge.color) else { continue };
            let lw = edge.width_pt.clamp(0.25, 12.0);
            let _ = cr.save();
            cr.set_source_rgb(f64::from(r) / 65535.0, f64::from(g) / 65535.0, f64::from(b) / 65535.0);
            match edge.style {
                // An invisible edge only keeps its gap.
                BorderStyle::None => {}
                BorderStyle::Single => {
                    cr.set_line_width(lw);
                    cr.move_to(x0, y0);
                    cr.line_to(x1, y1);
                    let _ = cr.stroke();
                }
                BorderStyle::Dashed => {
                    cr.set_line_width(lw);
                    cr.set_dash(&[3.0 * lw, 2.0 * lw], 0.0);
                    cr.move_to(x0, y0);
                    cr.line_to(x1, y1);
                    let _ = cr.stroke();
                }
                BorderStyle::Dotted => {
                    cr.set_line_width(lw);
                    cr.set_line_cap(cairo::LineCap::Round);
                    cr.set_dash(&[0.1, 1.5 * lw], 0.0);
                    cr.move_to(x0, y0);
                    cr.line_to(x1, y1);
                    let _ = cr.stroke();
                }
                BorderStyle::Double => {
                    // Two hairlines a full width apart, centred on the
                    // edge: the outer where Word puts it, the inner
                    // inside the box.
                    let inner = lw / 3.0;
                    cr.set_line_width(inner.max(0.25));
                    let (dx, dy) = edge_offset((x0, y0), (x1, y1), (x, y, x + w, y + h), lw);
                    cr.move_to(x0, y0);
                    cr.line_to(x1, y1);
                    let _ = cr.stroke();
                    cr.move_to(x0 + dx, y0 + dy);
                    cr.line_to(x1 + dx, y1 + dy);
                    let _ = cr.stroke();
                }
            }
            let _ = cr.restore();
        }
    }

    /// Length of paragraph `para`'s layout text, in chars.
    pub fn layout_len(&self, para: usize) -> usize {
        self.doc.paragraphs.get(para).map_or(0, |p| super::layout_text(&p.runs).chars().count())
    }

    /// Draw page `index`'s content (not the paper itself). An index out of
    /// range draws nothing.
    pub fn draw_page(&self, cr: &cairo::Context, index: usize) {
        let Some(page) = self.tree.pages.get(index) else { return };
        cr.set_source_rgb(0.0, 0.0, 0.0);
        for item in &page.items {
            match item {
                Item::Line { source: Source::Paragraph(p), line, box_width_pt, x_pt, baseline_pt, .. } => {
                    let Some(layout) = self.paragraph_layout(*p, *box_width_pt) else { continue };
                    if let Some(l) = layout.line_readonly(*line as i32) {
                        cr.move_to(*x_pt, *baseline_pt);
                        pangocairo::functions::show_layout_line(cr, &l);
                        if self.doc.paragraphs.get(*p).is_some_and(|p| p.style.toc.is_some()) {
                            self.draw_leader(cr, *p, &layout, &l, *x_pt, *baseline_pt);
                        } else {
                            self.draw_tab_leaders(cr, *p, &layout, &l, *x_pt, *baseline_pt);
                        }
                    }
                }
                // The line's own text from its own left edge (`x_pt`, which
                // carries the header's alignment), laid out left, in its
                // line's look and with its tab stops.
                Item::Line { source: Source::Header | Source::Footer, text, x_pt, box_width_pt, top_pt, story_line, .. } => {
                    let style = story_line.as_ref().map(|l| l.look.clone()).unwrap_or_default();
                    let runs = [crate::model::Run { text: text.clone(), style }];
                    let req = ShapeRequest {
                        runs: &runs,
                        heading: None,
                        code: false,
                        look: Look::Body,
                        alignment: Alignment::Left,
                        width_pt: *box_width_pt,
                        first_line_indent_pt: 0.0,
                        tab_stops: story_line.as_ref().map(|l| l.tab_stops.clone()).unwrap_or_default(),
                        right_tab: false,
                        defaults: &self.opts,
                    };
                    let layout = self.shaper.layout(&req);
                    cr.move_to(*x_pt, *top_pt);
                    pangocairo::functions::show_layout(cr, &layout);
                }
                Item::Marker { para, text, x_pt, baseline_pt } => {
                    // The marker takes the item's first-run font and size.
                    let Some(p) = self.doc.paragraphs.get(*para) else { continue };
                    let first = p.runs.iter().find(|r| !r.text.is_empty());
                    let mut run = crate::model::Run::plain(text.clone());
                    if let Some(f) = first {
                        run.style.font_family = f.style.font_family.clone();
                        run.style.font_size_hp = f.style.font_size_hp;
                        run.style.color = f.style.color.clone();
                    }
                    let size = run.style.font_size_hp.map_or_else(
                        || p.style.heading.map_or(self.opts.font_size_pt, |l| self.opts.heading_size_pt(l)),
                        |hp| f64::from(hp) / 2.0,
                    );
                    if text.as_str() == crate::lists::BULLET.to_string() {
                        // Word's and LibreOffice's default bullet (Symbol /
                        // OpenSymbol) is a disc about 0.31 em across, its
                        // centre 0.28 em above the baseline; the "•" of a
                        // text face is smaller and varies by font. Drawn,
                        // it looks the same whatever fonts are installed.
                        let (r_, g, b) = run.style.color.as_deref().and_then(parse_hex).unwrap_or((0, 0, 0));
                        let _ = cr.save();
                        cr.set_source_rgb(f64::from(r_) / 65535.0, f64::from(g) / 65535.0, f64::from(b) / 65535.0);
                        bullet_path(cr, *x_pt, *baseline_pt, size);
                        let _ = cr.fill();
                        let _ = cr.restore();
                        continue;
                    }
                    let runs = [run];
                    let mut req = paragraph_request(p, 1000.0, &self.opts);
                    req.runs = &runs;
                    req.first_line_indent_pt = 0.0;
                    req.alignment = Alignment::Left;
                    let layout = self.shaper.layout(&req);
                    cr.move_to(*x_pt, *baseline_pt - to_pt(layout.baseline()));
                    pangocairo::functions::show_layout(cr, &layout);
                }
                Item::Image { src, x_pt, y_pt, width_pt, height_pt, .. } => {
                    self.draw_image(cr, src, *x_pt, *y_pt, *width_pt, *height_pt);
                }
                Item::Chip { para, ch, x_pt, baseline_pt } => {
                    self.draw_chip(cr, *para, *ch, *x_pt, *baseline_pt);
                }
                Item::NoteRef { para, ch, x_pt, baseline_pt } => {
                    self.draw_note_ref(cr, *para, *ch, *x_pt, *baseline_pt);
                }
                Item::Line { source: Source::Footnote(n), line, box_width_pt, x_pt, baseline_pt, .. } => {
                    let p = super::note_paragraph(&self.doc, *n, &self.opts);
                    let layout = self.shaper.layout(&paragraph_request(&p, *box_width_pt, &self.opts));
                    if let Some(l) = layout.line_readonly(*line as i32) {
                        cr.move_to(*x_pt, *baseline_pt);
                        pangocairo::functions::show_layout_line(cr, &l);
                    }
                }
                Item::TextBox { x_pt, y_pt, width_pt, height_pt, text_box, .. } => {
                    let emu = |v: u32| f64::from(v) / super::EMU_PER_PT;
                    let _ = cr.save();
                    if let Some((r, g, b)) = text_box.fill.as_deref().and_then(parse_hex) {
                        cr.set_source_rgb(f64::from(r) / 65535.0, f64::from(g) / 65535.0, f64::from(b) / 65535.0);
                        cr.rectangle(*x_pt, *y_pt, *width_pt, *height_pt);
                        let _ = cr.fill();
                    }
                    // The rule's outer edge is the box's; a box thinner than
                    // its rule is a line (a form's rule), drawn along its
                    // middle.
                    if let Some(rule) = &text_box.border {
                        let (r, g, b) = parse_hex(&rule.color).unwrap_or((0, 0, 0));
                        let lw = emu(rule.width_emu).max(0.25);
                        cr.set_source_rgb(f64::from(r) / 65535.0, f64::from(g) / 65535.0, f64::from(b) / 65535.0);
                        cr.set_line_width(lw);
                        if *height_pt < lw {
                            cr.move_to(*x_pt, y_pt + height_pt / 2.0);
                            cr.line_to(x_pt + width_pt, y_pt + height_pt / 2.0);
                        } else if *width_pt < lw {
                            cr.move_to(x_pt + width_pt / 2.0, *y_pt);
                            cr.line_to(x_pt + width_pt / 2.0, y_pt + height_pt);
                        } else {
                            cr.rectangle(x_pt + lw / 2.0, y_pt + lw / 2.0, width_pt - lw, height_pt - lw);
                        }
                        let _ = cr.stroke();
                    }
                    // Its text inside its padding, a paragraph per line, in
                    // the body's look; what does not fit is clipped, as
                    // Writer clips a box that does not grow.
                    let [top, right, bottom, left] = text_box.padding_emu.map(emu);
                    let inner_w = (width_pt - left - right).max(1.0);
                    cr.rectangle(*x_pt, *y_pt, *width_pt, *height_pt);
                    cr.clip();
                    cr.set_source_rgb(0.0, 0.0, 0.0);
                    let mut y = y_pt + top;
                    for line in text_box.text.split('\n') {
                        let p = Paragraph { style: Default::default(), runs: vec![crate::model::Run::plain(line)] };
                        let layout = self.shaper.layout(&super::paragraph_request(&p, inner_w, &self.opts));
                        cr.move_to(x_pt + left, y);
                        pangocairo::functions::show_layout(cr, &layout);
                        y += f64::from(layout.extents().1.height()) / f64::from(pango::SCALE);
                        if y > y_pt + height_pt - bottom {
                            break;
                        }
                    }
                    let _ = cr.restore();
                }
                Item::PageBorder { x_pt, y_pt, width_pt, height_pt, line_pt, color } => {
                    let _ = cr.save();
                    let channel = |shift: u32| f64::from((color >> shift) & 0xFF) / 255.0;
                    cr.set_source_rgb(channel(16), channel(8), channel(0));
                    cr.set_line_width(*line_pt);
                    // The box is the rule's outer edge; cairo strokes
                    // centred on the path.
                    let half = line_pt / 2.0;
                    cr.rectangle(x_pt + half, y_pt + half, width_pt - line_pt, height_pt - line_pt);
                    let _ = cr.stroke();
                    let _ = cr.restore();
                }
                Item::Rule { x_pt, y_pt, width_pt } => {
                    let _ = cr.save();
                    cr.set_line_width(0.5);
                    cr.move_to(*x_pt, *y_pt);
                    cr.line_to(*x_pt + *width_pt, *y_pt);
                    let _ = cr.stroke();
                    let _ = cr.restore();
                }
                Item::Cell { table, x_pt, y_pt, width_pt, height_pt, fill, ruled, open, .. } => {
                    // The cell's shading first, under its rule and text.
                    if let Some((r, g, b)) = fill.as_deref().and_then(parse_hex) {
                        let _ = cr.save();
                        cr.set_source_rgb(f64::from(r) / 65535.0, f64::from(g) / 65535.0, f64::from(b) / 65535.0);
                        cr.rectangle(*x_pt, *y_pt, *width_pt, *height_pt);
                        let _ = cr.fill();
                        let _ = cr.restore();
                    }
                    // A 0.5 pt rule, at least one device pixel, on whole
                    // pixels: on screen a hairline between two pixel rows
                    // was drawn as two grey rows, which blurred into the
                    // cell text. (On paper and PDF a device unit is tiny,
                    // so this changes nothing there.)
                    let (px, _) = cr.device_to_user_distance(1.0, 0.0).unwrap_or((0.5, 0.0));
                    let lw = self.doc.table_rule_widths.get(table).copied().unwrap_or(super::CELL_RULE_PT).max(px.abs());
                    let snap = |x: f64, y: f64| {
                        let (dx, dy) = cr.user_to_device(x, y);
                        let (dw, _) = cr.user_to_device_distance(lw, 0.0).unwrap_or((lw, 0.0));
                        let half = (dw.round().max(1.0) % 2.0) / 2.0;
                        cr.device_to_user(dx.round() + half, dy.round() + half).unwrap_or((x, y))
                    };
                    if !*ruled {
                        continue;
                    }
                    let (x0, y0) = snap(*x_pt, *y_pt);
                    let (x1, y1) = snap(*x_pt + *width_pt, *y_pt + *height_pt);
                    let _ = cr.save();
                    // In the table's rule colour (a light grid's grey), else
                    // black.
                    let (r, g, b) = self.doc.table_rule_colors.get(table).and_then(|c| parse_hex(c)).unwrap_or((0, 0, 0));
                    cr.set_source_rgb(f64::from(r) / 65535.0, f64::from(g) / 65535.0, f64::from(b) / 65535.0);
                    cr.set_line_width(lw);
                    if open.any() {
                        // Only the sides the cell rules; a neighbour draws
                        // a shared side it rules itself.
                        for (shut, (ax, ay), (bx, by)) in [
                            (!open.top, (x0, y0), (x1, y0)),
                            (!open.right, (x1, y0), (x1, y1)),
                            (!open.bottom, (x0, y1), (x1, y1)),
                            (!open.left, (x0, y0), (x0, y1)),
                        ] {
                            if shut {
                                cr.move_to(ax, ay);
                                cr.line_to(bx, by);
                            }
                        }
                        cr.set_line_cap(cairo::LineCap::Square);
                    } else {
                        cr.rectangle(x0, y0, x1 - x0, y1 - y0);
                    }
                    let _ = cr.stroke();
                    let _ = cr.restore();
                }
                Item::ParaBox { x_pt, y_pt, width_pt, height_pt, fill, borders, top_edge, bottom_edge, .. } => {
                    let rect = (*x_pt, *y_pt, *width_pt, *height_pt);
                    self.draw_para_box(cr, rect, fill.as_deref(), borders, *top_edge, *bottom_edge);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout;
    use crate::model::{Paragraph, Run};

    fn doc(n: usize, text: &str) -> Document {
        let mut d = Document::new();
        d.paragraphs = (0..n).map(|_| Paragraph { style: Default::default(), runs: vec![Run::plain(text)] }).collect();
        d
    }

    /// A table of contents laid out: each page number ends at the right
    /// margin, a dot leader fills the gap, and the numbers are the pages
    /// the headings are really on.
    #[test]
    fn a_table_of_contents_has_page_numbers_at_the_margin() {
        let mut d = crate::toc::sample_document();
        let t = Typeset::new(d.clone(), LayoutOptions::default());
        let ops = crate::toc::update(&d, &t.paragraph_pages());
        crate::edit::apply_all(&mut d, &ops).unwrap();
        assert!(ops.is_empty(), "the sample's pages are the layout's: {:?}", d.paragraphs.iter().map(|p| p.text()).collect::<Vec<_>>());
        let t = Typeset::new(d.clone(), LayoutOptions::default());
        let page = &t.tree().pages[0];
        let right = page.width_pt - page.geometry.margin_right_pt;
        for p in 0..4 {
            let end = t.caret(TextPos { para: p, offset: t.layout_len(p) }).unwrap();
            assert!((end.x_pt - right).abs() < 1.0, "entry {p} ends at {} not {right}", end.x_pt);
        }
        // Dots drawn between title and number.
        let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, page.width_pt as i32, page.height_pt as i32).unwrap();
        {
            let cr = cairo::Context::new(&surface).unwrap();
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.paint().unwrap();
            t.draw_page(&cr, 0);
        }
        let start = t.caret(TextPos { para: 0, offset: "Introduction".len() }).unwrap();
        let data = surface.take_data().unwrap();
        let stride = page.width_pt as usize * 4;
        let dark = (start.x_pt as usize + 20..right as usize - 20)
            .filter(|&x| (start.top_pt as usize..(start.top_pt + start.height_pt) as usize).any(|y| data[y * stride + x * 4] < 128))
            .count();
        assert!(dark > 20, "a leader across the gap: {dark} dark columns");
    }

    /// A bullet is a disc and nothing else: its path starts at the disc,
    /// not with a line from wherever the text before it left the current
    /// point. The UK NSC form's bullets each trailed a hairline back to the
    /// line above in the PDF (an image draws the empty sliver as nothing).
    #[test]
    fn a_bullets_path_is_its_disc_alone() {
        let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, 100, 100).unwrap();
        let cr = cairo::Context::new(&surface).unwrap();
        // Where the text before it was drawn from.
        cr.move_to(5.0, 10.0);
        bullet_path(&cr, 40.0, 60.0, 12.0);
        let path = cr.copy_path().unwrap();
        let segments: Vec<cairo::PathSegment> = path.iter().collect();
        assert!(matches!(segments.first(), Some(cairo::PathSegment::MoveTo((x, _))) if *x > 40.0), "{segments:?}");
        assert!(!segments.iter().any(|s| matches!(s, cairo::PathSegment::LineTo(_))), "a line in the bullet's path: {segments:?}");
    }

    /// Space before does not ride a page break onto the next page: past
    /// the first paragraph, Word and LibreOffice start each page at the
    /// margin (render-lab letters/toc drew a heading with 24pt before
    /// 24pt too low on page 2). The document's own first paragraph keeps
    /// its space (render-lab letters/headings), and mid-page it applies.
    #[test]
    fn space_before_is_dropped_at_the_top_of_a_page() {
        use crate::layout::Source;
        let top = LayoutOptions::default().page.margin_top_pt;
        let first_top = |t: &Typeset, page: usize, para: usize| {
            t.tree().pages[page]
                .lines()
                .filter_map(|l| match l {
                    Item::Line { source: Source::Paragraph(p), top_pt, .. } if *p == para => Some(*top_pt),
                    _ => None,
                })
                .next()
                .unwrap()
        };
        let mut d = doc(3, "body");
        for p in d.paragraphs.iter_mut() {
            p.style.space_before_pt = 24.0;
        }
        d.paragraphs[2].style.page_break_before = true;
        let t = Typeset::new(d, LayoutOptions::default());
        assert_eq!(t.tree().pages.len(), 2);
        assert!((first_top(&t, 0, 0) - (top + 24.0)).abs() < 0.01, "the document's first paragraph keeps its space");
        assert!(first_top(&t, 0, 1) - top - 24.0 > 20.0, "mid-page the space applies after two lines: {}", first_top(&t, 0, 1));
        assert!((first_top(&t, 1, 2) - top).abs() < 0.01, "after the break it starts at the margin");
    }

    /// Open threads with text are marked where their text is, with a spot
    /// in the right margin; a resolved one is not.
    #[test]
    fn comment_marks_cover_open_threads_beside_their_text() {
        let d = crate::comments::sample_document();
        let t = Typeset::new(d, LayoutOptions::default());
        let marks = t.comment_marks();
        assert_eq!(marks.iter().map(|m| (m.id, m.count)).collect::<Vec<_>>(), [(1, 2), (2, 1)], "comment 4 is resolved");
        let page = &t.tree().pages[0];
        let body_right = page.width_pt - page.geometry.margin_right_pt;
        for m in &marks {
            assert!(m.x_pt >= body_right && m.x_pt + 12.0 <= page.width_pt, "in the margin: {m:?}");
            assert!(m.rects.iter().all(|r| r.0 == 0 && r.1 + r.3 <= body_right + 0.5));
        }
        assert_eq!(marks[0].y_pt, marks[1].y_pt, "both on the first line");
        assert!(marks[1].x_pt > marks[0].x_pt, "side by side");
    }

    /// A hanging indent (Word's `w:ind w:left="1440" w:hanging="1440"`)
    /// starts the first line at the margin and the wrapped lines at the
    /// left indent, as with the fallback shaper. Pango reads a negative
    /// indent as "indent the other lines", and drew the first line at 72pt
    /// and the rest at 144pt.
    #[test]
    fn a_hanging_indent_starts_the_first_line_left_of_the_others() {
        let opts = LayoutOptions::default();
        let mut d = doc(1, "Question 1: Do you agree with the proposal to introduce a three-month time limit for applications for permission to appeal by interested persons who are not the recipient?");
        d.paragraphs[0].style.left_indent_pt = 72.0;
        d.paragraphs[0].style.first_line_indent_pt = -72.0;
        let t = layout(&d, &opts, &mut PangoShaper::new());
        let xs: Vec<f64> = t.pages[0].lines().map(|l| match l { Item::Line { x_pt, .. } => *x_pt, _ => unreachable!() }).collect();
        assert!(xs.len() >= 2, "the paragraph wraps: {xs:?}");
        let margin = t.pages[0].lines().next().map(|l| match l { Item::Line { box_x_pt, .. } => *box_x_pt - 72.0, _ => 0.0 }).unwrap();
        assert!((xs[0] - margin).abs() < 0.5, "first line at the margin {margin}: {xs:?}");
        for x in &xs[1..] {
            assert!((x - (margin + 72.0)).abs() < 0.5, "wrapped lines at the 72pt indent: {xs:?}");
        }
    }

    /// Text lowered from its baseline (`w:position`) grows its line by as
    /// much, as LibreOffice draws it: the business appointment rules'
    /// Normal lowers every run half a point, and its lines stand 14.3pt
    /// apart where plain 12pt Arial's stand 13.8pt.
    #[test]
    fn lowered_text_grows_its_line() {
        let opts = LayoutOptions::default();
        let height = |position_hp: Option<i32>| {
            let mut d = doc(1, "Lowered");
            d.paragraphs[0].runs[0].style.position_hp = position_hp;
            let t = layout(&d, &opts, &mut PangoShaper::new());
            let line = t.pages[0].lines().next().map(|l| match l { Item::Line { height_pt, .. } => *height_pt, _ => 0.0 }).unwrap();
            line
        };
        let (plain, lowered) = (height(None), height(Some(-2)));
        assert!((lowered - plain - 1.0).abs() < 0.1, "{plain} {lowered}");
    }

    #[test]
    fn pango_lines_have_real_metrics_and_partition_the_text() {
        let opts = LayoutOptions::default();
        let text = "The quick brown fox jumps over the lazy dog. ".repeat(12);
        let d = doc(1, text.trim_end());
        let t = layout(&d, &opts, &mut PangoShaper::new());
        let lines: Vec<&Item> = t.pages[0].lines().collect();
        assert!(lines.len() >= 4, "a long paragraph wraps: {}", lines.len());
        let mut joined = String::new();
        for l in &lines {
            let Item::Line { text, height_pt, .. } = l else { unreachable!() };
            // Liberation Serif 12pt: about 13.8pt per line.
            assert!((12.0..16.0).contains(height_pt), "line height {height_pt}");
            joined.push_str(text);
        }
        assert_eq!(joined, text.trim_end());
    }

    #[test]
    fn the_painter_draws_ink_where_the_tree_puts_lines() {
        let opts = LayoutOptions::default();
        let typeset = Typeset::new(doc(3, "Hello, page."), opts);
        let page = &typeset.tree().pages[0];
        let (w, h) = (page.width_pt.ceil() as i32, page.height_pt.ceil() as i32);
        let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, w, h).unwrap();
        {
            let cr = cairo::Context::new(&surface).unwrap();
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.paint().unwrap();
            typeset.draw_page(&cr, 0);
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.take_data().unwrap();
        let dark = |x0: usize, y0: usize, x1: usize, y1: usize| {
            (y0..y1).any(|y| (x0..x1).any(|x| data[y * stride + x * 4] < 128))
        };
        // Ink inside the first line's box, none above the top margin.
        assert!(dark(72, 72, 160, 86), "no text drawn in the first line box");
        assert!(!dark(0, 0, w as usize, 70), "ink above the top margin");
    }

    /// Proportional line spacing scales a line's text, not an inline
    /// picture on it: at 150% a line holding a 100pt chart is the chart and
    /// half a line of text tall, as Writer lays it out (measured: 6.9pt more
    /// than at 100% for 12pt text), not half as tall again as the chart.
    #[test]
    fn proportional_spacing_scales_the_text_not_an_inline_picture() {
        use crate::model::{Run, RunStyle};
        let heights = |spacing: f32| {
            let mut d = doc(3, "x");
            d.paragraphs[1].style.line_spacing = spacing;
            d.paragraphs[1].runs = vec![Run { text: String::new(), style: RunStyle { image: Some("/nonexistent.png".into()), image_extent_emu: Some((914_400, 100 * 12_700)), ..Default::default() } }];
            let t = Typeset::new(d, LayoutOptions::default());
            let tops: Vec<f64> = t.tree().pages[0].lines().map(|l| match l { Item::Line { top_pt, .. } => *top_pt, _ => 0.0 }).collect();
            tops[2] - tops[1]
        };
        let text_line = {
            let t = Typeset::new(doc(2, "x"), LayoutOptions::default());
            let tops: Vec<f64> = t.tree().pages[0].lines().map(|l| match l { Item::Line { top_pt, .. } => *top_pt, _ => 0.0 }).collect();
            tops[1] - tops[0]
        };
        let (single, half_again) = (heights(1.0), heights(1.5));
        assert!(single >= 100.0, "the picture's line is at least the picture: {single}");
        assert!((half_again - single - text_line / 2.0).abs() < 0.01, "150% adds half a text line ({}), not half the picture: {single} -> {half_again}", text_line / 2.0);
    }

    /// A paragraph holding only a floating picture or text box is as tall
    /// as an empty one, as in Writer and Word: it used to be half a point
    /// tall, which drew the opt-out form's labels up against the answer
    /// boxes their paragraphs float.
    #[test]
    fn a_paragraph_holding_only_a_floating_object_is_a_line_tall() {
        use crate::model::{ImageAnchor, Run, RunStyle, TextBox};
        let heights = |runs: Vec<Run>| {
            let mut d = doc(3, "x");
            d.paragraphs[1].runs = runs;
            let t = Typeset::new(d, LayoutOptions::default());
            t.tree().pages[0].lines().map(|l| match l { Item::Line { height_pt, .. } => *height_pt, _ => 0.0 }).collect::<Vec<f64>>()
        };
        let empty = heights(vec![Run::plain("")]);
        let boxed = Run { text: String::new(), style: RunStyle { text_box: Some(Box::new(TextBox { extent_emu: (914_400, 914_400), ..Default::default() })), image_anchor: Some(ImageAnchor::default()), ..Default::default() } };
        let picture = Run { text: String::new(), style: RunStyle { image: Some("/nonexistent.png".into()), image_extent_emu: Some((914_400, 914_400)), image_anchor: Some(ImageAnchor::default()), ..Default::default() } };
        assert_eq!(heights(vec![boxed]), empty, "a text box's paragraph");
        assert_eq!(heights(vec![picture]), empty, "a floating picture's");
    }

    /// A text box is drawn as Writer draws one: its fill, its rule and its
    /// text inside its padding, where the tree puts it.
    #[test]
    fn the_painter_draws_a_text_box_filled_ruled_and_written() {
        use crate::model::{AnchorFrame, BoxRule, ImageAnchor, Run, RunStyle, TextBox};
        let mut d = doc(1, "x");
        d.paragraphs[0].runs.push(Run {
            text: String::new(),
            style: RunStyle {
                text_box: Some(Box::new(TextBox {
                    extent_emu: (200 * 12_700, 60 * 12_700),
                    border: Some(BoxRule { width_emu: 2 * 12_700, color: "000000".into() }),
                    fill: Some("FFFF00".into()),
                    padding_emu: [6 * 12_700; 4],
                    text: "Inside the box\nand a second line".into(),
                })),
                image_anchor: Some(ImageAnchor { h_from: AnchorFrame::Page, x_emu: 200 * 12_700, v_from: AnchorFrame::Page, y_emu: 300 * 12_700, ..Default::default() }),
                ..Default::default()
            },
        });
        let typeset = Typeset::new(d, LayoutOptions::default());
        let page = &typeset.tree().pages[0];
        let (w, h) = (page.width_pt.ceil() as i32, page.height_pt.ceil() as i32);
        let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, w, h).unwrap();
        {
            let cr = cairo::Context::new(&surface).unwrap();
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.paint().unwrap();
            typeset.draw_page(&cr, 0);
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.take_data().unwrap();
        // Rgb24 is B, G, R, X in memory.
        let px = |x: usize, y: usize| (data[y * stride + x * 4 + 2], data[y * stride + x * 4 + 1], data[y * stride + x * 4]);
        let dark = |x0: usize, y0: usize, x1: usize, y1: usize| (y0..y1).filter(|&y| (x0..x1).any(|x| px(x, y).0 < 128)).count();
        assert_eq!(px(201, 330), (0, 0, 0), "its rule on the left edge");
        assert_eq!(px(398, 330), (0, 0, 0), "and the right");
        assert_eq!(px(390, 355), (255, 255, 0), "filled inside");
        assert_eq!(px(195, 330), (255, 255, 255), "nothing outside it");
        assert!(dark(208, 306, 392, 320) > 4, "its first line inside the padding");
        assert!(dark(208, 320, 392, 340) > 4, "and its second below it");
        assert_eq!(dark(0, 0, w as usize, 70), 0, "no ink above the top margin");
    }

    /// Clicking a laid-out character finds it, and its caret is drawn where
    /// the click was: hit-testing and caret placement are inverse.
    #[test]
    fn hit_test_and_caret_are_inverse() {
        let text = "The quick brown fox jumps over the lazy dog. ".repeat(6);
        let mut d = doc(3, text.trim_end());
        d.paragraphs[1].style.alignment = crate::model::Alignment::Center;
        let t = Typeset::new(d, LayoutOptions::default());
        for (para, offset) in [(0, 0), (0, 10), (0, 100), (1, 57), (2, 200)] {
            let c = t.caret(TextPos { para, offset }).expect("a caret");
            let hit = t.hit_test(c.page, c.x_pt + 0.1, c.top_pt + c.height_pt / 2.0).expect("a hit");
            assert_eq!(hit, TextPos { para, offset }, "caret {c:?}");
        }
        // A click in the left margin lands at the start of that line; one
        // far below the text, on the last line.
        let first = t.caret(TextPos { para: 0, offset: 0 }).unwrap();
        assert_eq!(t.hit_test(0, 10.0, first.top_pt + 2.0), Some(TextPos { para: 0, offset: 0 }));
        let below = t.hit_test(0, 1000.0, 800.0).unwrap();
        assert_eq!(below.para, 2);
        // The caret at the end of a paragraph is after its last char.
        let len = t.layout_len(2);
        let end = t.caret(TextPos { para: 2, offset: len }).unwrap();
        let before = t.caret(TextPos { para: 2, offset: len - 1 }).unwrap();
        assert!(end.x_pt > before.x_pt);
    }

    #[test]
    fn a_selection_covers_whole_lines_between_its_ends() {
        let text = "The quick brown fox jumps over the lazy dog. ".repeat(6);
        let t = Typeset::new(doc(2, text.trim_end()), LayoutOptions::default());
        let rects = t.selection_rects(TextPos { para: 0, offset: 4 }, TextPos { para: 1, offset: 9 });
        let lines0 = t.tree().pages[0].lines().filter(|i| matches!(i, Item::Line { source: Source::Paragraph(0), .. })).count();
        assert_eq!(rects.len(), lines0 + 1, "every line of the first paragraph, and one of the second");
        assert!(rects[0].1 > 72.0, "starts after 'The '");
        assert!(rects.iter().all(|r| r.3 > 0.0));
        // Reversed ends select the same text.
        assert_eq!(rects, t.selection_rects(TextPos { para: 1, offset: 9 }, TextPos { para: 0, offset: 4 }));
    }

    /// A paragraph's trailing space hangs past the margin: a last line
    /// that fits without it is not wrapped a word early (render-lab
    /// letters/indents).
    #[test]
    fn a_trailing_space_does_not_wrap_the_last_line() {
        let opts = LayoutOptions::default();
        let line = "lazy dog. Pack my box with five dozen liquor jugs. How vexingly quick daft zebras jump.";
        let mut shaper = PangoShaper::new();
        let runs = [crate::model::Run::plain(line)];
        fn req<'a>(runs: &'a [crate::model::Run], w: f64, opts: &'a LayoutOptions) -> ShapeRequest<'a> {
            ShapeRequest {
                runs,
                heading: None,
                code: false,
                look: Look::Body,
                alignment: Alignment::Left,
                width_pt: w,
                first_line_indent_pt: 0.0,
                tab_stops: Vec::new(),
                right_tab: false,
                defaults: opts,
            }
        }
        let width = shaper.shape(&req(&runs, 1000.0, &opts))[0].width_pt;
        let spaced = [crate::model::Run::plain(format!("{line} "))];
        let lines = shaper.shape(&req(&spaced, width + 0.05, &opts));
        assert_eq!(lines.len(), 1, "the trailing space wrapped the line");
        assert!(lines[0].natural_height() > 10.0);
        // A paragraph of spaces only keeps its line height.
        let blank = [crate::model::Run::plain("   ")];
        assert!(shaper.shape(&req(&blank, 100.0, &opts))[0].natural_height() > 10.0);
    }

    /// Updating after an edit re-shapes the edited paragraph only, and
    /// draws and hit-tests the same as a fresh layout.
    #[test]
    fn update_reshapes_only_what_changed() {
        let text = "The quick brown fox jumps over the lazy dog. ".repeat(4);
        let mut d = doc(50, text.trim_end());
        let mut t = Typeset::new(d.clone(), LayoutOptions::default());
        let op = crate::edit::typing(&d, crate::edit::paragraph_start(&d, 20) + 3, "abc").unwrap();
        crate::edit::apply(&mut d, &op).unwrap();
        assert_eq!(t.update(d.clone(), LayoutOptions::default()), 1);
        let fresh = Typeset::new(d, LayoutOptions::default());
        assert_eq!(t.tree(), fresh.tree());
        let pos = TextPos { para: 20, offset: 5 };
        assert_eq!(t.caret(pos), fresh.caret(pos));
    }

    /// The PDF is the laid-out pages: as many PDF pages as the tree has,
    /// each at the page's size in points. (The render lab rasterises this
    /// same PDF and compares it with the on-screen page view pixel by
    /// pixel: `print_agreement`.)
    #[test]
    fn the_pdf_has_the_trees_pages_at_their_size() {
        let geometry = crate::model::PageGeometry { width_pt: 612.0, height_pt: 792.0, ..Default::default() };
        let mut d = doc(120, "One line of body text.");
        d.page = Some(geometry);
        let typeset = Typeset::new(d, LayoutOptions::default());
        let pages = typeset.tree().pages.len();
        assert!(pages >= 3, "120 lines take several pages: {pages}");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.pdf");
        typeset.write_pdf(&path).expect("write pdf");
        let bytes = std::fs::read(&path).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        let page_objects = text.matches("/Type /Page").count() - text.matches("/Type /Pages").count();
        assert_eq!(page_objects, pages, "one PDF page per laid-out page");
        assert!(text.contains("/MediaBox [ 0 0 612 792 ]"), "pages keep their size in points");
    }

    /// A family nothing provides is reported on the pages that ask for it,
    /// and only there: Pango substitutes silently, so the export names the
    /// substituting family per page instead of failing (LET-004). Only the
    /// fixture's own missing face is asserted — the runner's installed
    /// faces are its business.
    #[test]
    fn missing_fonts_name_the_substituting_family_per_page() {
        let text = "The quick brown fox jumps over the lazy dog. ".repeat(4);
        let mut d = doc(40, text.trim_end());
        d.paragraphs[0].runs[0].style.font_family = Some("No Such Family XYZ".into());
        let t = Typeset::new(d, LayoutOptions::default());
        assert!(t.tree().pages.len() >= 2, "the document runs to several pages");
        let missing = t.missing_fonts_by_page();
        assert_eq!(missing.len(), t.tree().pages.len());
        assert!(missing[0].contains(&"No Such Family XYZ".to_string()), "page 1 names its missing face: {missing:?}");
        for rest in &missing[1..] {
            assert!(!rest.contains(&"No Such Family XYZ".to_string()), "no other page asks for it: {missing:?}");
        }
    }

    /// Combining marks shape with their base character through the
    /// shared shaper: the caret steps over the cluster, never into it.
    #[test]
    fn combining_marks_shape_with_their_base_character() {
        // "e" plus a combining acute: two chars, one cluster.
        let d = doc(1, "cafe\u{301} au lait");
        let t = Typeset::new(d, LayoutOptions::default());
        assert_eq!(t.tree().pages[0].lines().count(), 1);
        let before = t.caret(TextPos { para: 0, offset: 3 }).expect("caret before the cluster");
        let after = t.caret(TextPos { para: 0, offset: 5 }).expect("caret after the cluster");
        assert!(after.x_pt > before.x_pt, "the cluster takes room: {} -> {}", before.x_pt, after.x_pt);
    }

    /// CJK text shapes through the shared shaper without panic — in a
    /// face Pango substitutes where none is installed — and the caret
    /// advances across it.
    #[test]
    fn cjk_text_shapes_and_takes_caret_steps() {
        let d = doc(1, "日本語のテスト");
        let t = Typeset::new(d, LayoutOptions::default());
        assert_eq!(t.tree().pages.len(), 1);
        assert_eq!(t.tree().pages[0].lines().count(), 1);
        let start = t.caret(TextPos { para: 0, offset: 0 }).expect("caret at the start");
        let end = t.caret(TextPos { para: 0, offset: 7 }).expect("caret at the end");
        assert!(end.x_pt > start.x_pt + 10.0, "seven CJK characters take room: {} -> {}", start.x_pt, end.x_pt);
    }

    /// A line's gap is its text's own font's: 14pt Liberation Sans in a
    /// document whose body font is 11pt Carlito stands as tall
    /// as it does in a Liberation Sans 14pt document (Writer measures the
    /// line by the text's font; `real-salary-template`'s Arial lines were
    /// a line gap short each).
    #[test]
    fn a_lines_gap_is_its_texts_own_fonts() {
        let height = |d: Document, o: LayoutOptions| -> f64 {
            let t = Typeset::new(d, o);
            t.tree().pages[0].items.iter().find_map(|i| match i { Item::Line { height_pt, .. } => Some(*height_pt), _ => None }).unwrap()
        };
        let mut styled = doc(1, "");
        styled.paragraphs[0].runs = vec![Run {
            text: "Arial text".into(),
            style: crate::model::RunStyle { font_family: Some("Liberation Sans".into()), font_size_hp: Some(28), ..Default::default() },
        }];
        let in_carlito = height(styled, LayoutOptions { font_family: "Carlito".into(), font_size_pt: 11.0, ..Default::default() });
        let native = height(doc(1, "Arial text"), LayoutOptions { font_family: "Liberation Sans".into(), font_size_pt: 14.0, ..Default::default() });
        assert!((in_carlito - native).abs() < 0.05, "{in_carlito} vs {native}");
    }

    /// A blank paragraph is as tall as its own empty run's font (an
    /// imported 14pt blank line), not the body size.
    #[test]
    fn a_blank_paragraph_is_as_tall_as_its_own_font() {
        let mut d = doc(2, "");
        d.paragraphs[0].runs = vec![];
        d.paragraphs[1].runs = vec![Run { text: String::new(), style: crate::model::RunStyle { font_size_hp: Some(56), ..Default::default() } }];
        let t = Typeset::new(d, LayoutOptions::default());
        let h: Vec<f64> = t.tree().pages[0]
            .items
            .iter()
            .filter_map(|i| match i { Item::Line { height_pt, .. } => Some(*height_pt), _ => None })
            .collect();
        assert!(h[1] > h[0] * 2.0, "a 28pt blank line under a 12pt one: {h:?}");
    }

    /// Tab leaders and paragraph boxes draw without panic: leaders
    /// across the tab's gap, the box under the text.
    #[test]
    fn tab_leaders_and_paragraph_boxes_draw() {
        let mut d = doc(1, "one\ttwo");
        d.paragraphs[0].style.tab_stops_pt = vec![crate::model::TabStop {
            pos_pt: 200.0,
            align: crate::model::TabAlign::Left,
            leader: crate::model::TabLeader::Dot,
        }];
        d.paragraphs[0].style.shading = Some("D9D9D9".into());
        let t = Typeset::new(d, LayoutOptions::default());
        let page = &t.tree().pages[0];
        assert!(page.items.iter().any(|i| matches!(i, Item::ParaBox { .. })), "a box under the shaded line");
        let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, page.width_pt as i32, page.height_pt as i32).unwrap();
        let cr = cairo::Context::new(&surface).unwrap();
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.paint().unwrap();
        t.draw_page(&cr, 0);
    }

    /// A dot leader as Writer fills it: whole dots packed at their own
    /// advance from the tab's start (render-lab `letters/tab-leaders`),
    /// not spaced out and pulled back from the text.
    #[test]
    fn dot_leaders_pack_from_the_tab_start() {
        let mut d = doc(1, "one\ttwo");
        d.paragraphs[0].style.tab_stops_pt = vec![crate::model::TabStop {
            pos_pt: 300.0,
            align: crate::model::TabAlign::Right,
            leader: crate::model::TabLeader::Dot,
        }];
        let t = Typeset::new(d, LayoutOptions::default());
        let page = &t.tree().pages[0];
        let tab = t.caret(TextPos { para: 0, offset: 3 }).unwrap();
        let two = t.caret(TextPos { para: 0, offset: 4 }).unwrap();
        // Drawn at 8 px per point, so a dot's advance is many pixels.
        let scale = 8.0;
        let (w, h) = ((page.width_pt * scale) as i32, ((tab.top_pt + tab.height_pt + 2.0) * scale) as i32);
        let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, w, h).unwrap();
        {
            let cr = cairo::Context::new(&surface).unwrap();
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.paint().unwrap();
            cr.scale(scale, scale);
            t.draw_page(&cr, 0);
        }
        let stride = surface.stride() as usize;
        let data = surface.take_data().unwrap();
        let rows = (tab.top_pt * scale) as usize..((tab.top_pt + tab.height_pt) * scale) as usize;
        let inked = |x: usize| rows.clone().any(|y| data[y * stride + x * 4] < 128);
        // Dot starts (ink after paper) between the tab and the next word.
        let (from, to) = ((tab.x_pt * scale) as usize + 1, (two.x_pt * scale) as usize - 1);
        let starts: Vec<usize> = (from..to).filter(|&x| inked(x) && !inked(x - 1)).collect();
        assert!(starts.len() > 20, "{starts:?}");
        let pitch = (starts[starts.len() - 1] - starts[0]) as f64 / (starts.len() - 1) as f64 / scale;
        let dot = {
            let runs = [Run::plain(".")];
            let mut req = super::super::paragraph_request(&t.doc.paragraphs[0], 1000.0, &t.opts);
            req.runs = &runs;
            to_pt(t.shaper.layout(&req).line_readonly(0).unwrap().index_to_x(1, false))
        };
        assert!((pitch - dot).abs() < 0.1, "dots every {pitch}pt, a dot is {dot}pt");
        assert!((starts[0] as f64 / scale - tab.x_pt) < dot, "the first dot opens the gap: {} vs {}", starts[0] as f64 / scale, tab.x_pt);
    }
}
