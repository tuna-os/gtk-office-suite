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
use std::collections::HashMap;

use pango::prelude::*;

use super::{paragraph_request, request_key, Item, LayoutOptions, LineBox, Look, RenderTree, ShapeCache, ShapeRequest, Shaper, Source};
use crate::model::{Alignment, Document, VertAlign};

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

    /// The Pango layout of one paragraph, exactly as the engine shapes it.
    pub fn layout(&self, req: &ShapeRequest<'_>) -> pango::Layout {
        let layout = pango::Layout::new(&self.context);
        let mut base = pango::FontDescription::new();
        base.set_family(if req.code { "Liberation Mono" } else { &req.defaults.font_family });
        let mut size = req.defaults.font_size_pt;
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
                } else {
                    let (w, h) = if run.style.image.is_some() { super::image_size_pt(run, req.width_pt) } else { (0.0, 0.0) };
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
            }
            if st.italic {
                add(pango::AttrInt::new_style(pango::Style::Italic).into());
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
        layout.set_width(to_units(req.width_pt.max(1.0)));
        layout.set_wrap(pango::WrapMode::WordChar);
        layout.set_indent(to_units(req.first_line_indent_pt));
        layout.set_alignment(match req.alignment {
            Alignment::Center => pango::Alignment::Center,
            Alignment::Right => pango::Alignment::Right,
            _ => pango::Alignment::Left,
        });
        layout.set_justify(req.alignment == Alignment::Justify);
        if !req.tab_stops_pt.is_empty() {
            let mut tabs = pango::TabArray::new(req.tab_stops_pt.len() as i32, false);
            for (i, t) in req.tab_stops_pt.iter().enumerate() {
                tabs.set_tab(i as i32, pango::TabAlign::Left, to_units(*t));
            }
            layout.set_tabs(Some(&tabs));
        }
        layout
    }
}

/// "RRGGBB" to 16-bit channels.
fn parse_hex(hex: &str) -> Option<(u16, u16, u16)> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok().map(|v| u16::from(v) * 257);
    Some((c(0)?, c(2)?, c(4)?))
}

/// Lines of a layout as `LineBox`es, with char (not byte) offsets.
fn line_boxes(layout: &pango::Layout) -> Vec<LineBox> {
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
                    .map(|&(ci, bi)| (ci, to_pt(layout.index_to_pos(bi).x())))
                    .collect(),
                start: cs,
                end: ce,
                x_pt: to_pt(logical.x()),
                width_pt: to_pt(logical.width()),
                ascent_pt: to_pt(baseline - logical.y()),
                descent_pt: to_pt(logical.y() + logical.height() - baseline),
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
        let mut lines = line_boxes(&layout);
        // A line is as tall as the font's ascent, descent *and* line gap,
        // as LibreOffice and Word space lines (Liberation Serif 12pt:
        // 13.8pt); Pango's logical extents leave the gap out.
        if let Some(desc) = layout.font_description() {
            let m = self.context.metrics(Some(&desc), None);
            let gap = to_pt(m.height() - m.ascent() - m.descent()).max(0.0);
            for l in &mut lines {
                l.descent_pt += gap;
            }
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
        }
    }

    pub fn tree(&self) -> &RenderTree {
        &self.tree
    }

    /// Lay out `doc` in place of the current document, re-shaping only the
    /// paragraphs that changed (ADR 0010 stage 3c). Returns how many were
    /// shaped.
    pub fn update(&mut self, doc: Document, opts: LayoutOptions) -> usize {
        self.opts = opts.for_document(&doc);
        self.tree = super::relayout(&doc, &self.opts, &mut self.shaper, &mut self.shapes);
        self.doc = doc;
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
                    }
                }
                Item::Line { source: Source::Header | Source::Footer, text, box_x_pt, box_width_pt, top_pt, .. } => {
                    let runs = [crate::model::Run::plain(text.clone())];
                    let req = ShapeRequest {
                        runs: &runs,
                        heading: None,
                        code: false,
                        look: Look::Body,
                        alignment: Alignment::Left,
                        width_pt: *box_width_pt,
                        first_line_indent_pt: 0.0,
                        tab_stops_pt: Vec::new(),
                        defaults: &self.opts,
                    };
                    let layout = self.shaper.layout(&req);
                    cr.move_to(*box_x_pt, *top_pt);
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
                        let r = size * 0.155;
                        let (r_, g, b) = run.style.color.as_deref().and_then(parse_hex).unwrap_or((0, 0, 0));
                        let _ = cr.save();
                        cr.set_source_rgb(f64::from(r_) / 65535.0, f64::from(g) / 65535.0, f64::from(b) / 65535.0);
                        cr.arc(*x_pt + r, *baseline_pt - size * 0.28, r, 0.0, std::f64::consts::TAU);
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
                Item::Rule { x_pt, y_pt, width_pt } => {
                    let _ = cr.save();
                    cr.set_line_width(0.5);
                    cr.move_to(*x_pt, *y_pt);
                    cr.line_to(*x_pt + *width_pt, *y_pt);
                    let _ = cr.stroke();
                    let _ = cr.restore();
                }
                Item::Cell { x_pt, y_pt, width_pt, height_pt, .. } => {
                    // A 0.5 pt rule, at least one device pixel, on whole
                    // pixels: on screen a hairline between two pixel rows
                    // was drawn as two grey rows, which blurred into the
                    // cell text. (On paper and PDF a device unit is tiny,
                    // so this changes nothing there.)
                    let (px, _) = cr.device_to_user_distance(1.0, 0.0).unwrap_or((0.5, 0.0));
                    let lw = 0.5f64.max(px.abs());
                    let snap = |x: f64, y: f64| {
                        let (dx, dy) = cr.user_to_device(x, y);
                        let (dw, _) = cr.user_to_device_distance(lw, 0.0).unwrap_or((lw, 0.0));
                        let half = (dw.round().max(1.0) % 2.0) / 2.0;
                        cr.device_to_user(dx.round() + half, dy.round() + half).unwrap_or((x, y))
                    };
                    let (x0, y0) = snap(*x_pt, *y_pt);
                    let (x1, y1) = snap(*x_pt + *width_pt, *y_pt + *height_pt);
                    let _ = cr.save();
                    cr.set_source_rgb(0.0, 0.0, 0.0);
                    cr.set_line_width(lw);
                    cr.rectangle(x0, y0, x1 - x0, y1 - y0);
                    let _ = cr.stroke();
                    let _ = cr.restore();
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
                tab_stops_pt: Vec::new(),
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
}
