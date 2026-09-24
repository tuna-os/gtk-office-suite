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

use super::{heading_scale, paragraph_request, Item, LayoutOptions, LineBox, RenderTree, ShapeRequest, Shaper, Source};
use crate::model::{Alignment, Document, VertAlign};

/// Pango's fixed-point scale.
const SCALE: f64 = pango::SCALE as f64;

fn to_pt(units: i32) -> f64 {
    f64::from(units) / SCALE
}

fn to_units(pt: f64) -> i32 {
    (pt * SCALE).round() as i32
}

/// A shaper backed by Pango through a Cairo font map, at 72 dpi.
pub struct PangoShaper {
    context: pango::Context,
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
        Self { context }
    }

    /// The Pango layout of one paragraph, exactly as the engine shapes it.
    pub fn layout(&self, req: &ShapeRequest<'_>) -> pango::Layout {
        let layout = pango::Layout::new(&self.context);
        let mut base = pango::FontDescription::new();
        base.set_family(if req.code { "Liberation Mono" } else { &req.defaults.font_family });
        let mut size = req.defaults.font_size_pt;
        if let Some(level) = req.heading {
            size *= heading_scale(level);
            base.set_weight(pango::Weight::Bold);
        }
        base.set_size(to_units(size));
        layout.set_font_description(Some(&base));

        let mut text = String::new();
        let attrs = pango::AttrList::new();
        for run in req.runs {
            let start = text.len() as u32;
            if run.style.image.is_some() {
                // An image is one object char holding its box open; the
                // painter draws the picture into it (layout_text).
                text.push(super::OBJECT);
                let (w, h) = super::image_size_pt(run, req.width_pt);
                let rect = pango::Rectangle::new(0, -to_units(h), to_units(w), to_units(h));
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
    /// Paragraph layouts by (paragraph, box width bits).
    cache: RefCell<HashMap<(usize, u64), pango::Layout>>,
    images: RefCell<HashMap<String, Option<cairo::ImageSurface>>>,
    loader: Box<ImageLoader>,
}

/// Decodes the image at a path for drawing.
type ImageLoader = dyn Fn(&str) -> Option<cairo::ImageSurface>;

/// The built-in image loader: PNG through Cairo.
fn load_png(path: &str) -> Option<cairo::ImageSurface> {
    let mut f = std::fs::File::open(path).ok()?;
    cairo::ImageSurface::create_from_png(&mut f).ok()
}

impl Typeset {
    /// Lay `doc` out into pages.
    pub fn new(doc: Document, opts: LayoutOptions) -> Self {
        let mut shaper = PangoShaper::new();
        let opts = opts.for_document(&doc);
        let tree = super::layout(&doc, &opts, &mut shaper);
        Self {
            doc,
            opts,
            tree,
            shaper,
            cache: RefCell::new(HashMap::new()),
            images: RefCell::new(HashMap::new()),
            loader: Box::new(load_png),
        }
    }

    pub fn tree(&self) -> &RenderTree {
        &self.tree
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
        let key = (para, box_w.to_bits());
        let mut cache = self.cache.borrow_mut();
        Some(cache.entry(key).or_insert_with(|| self.shaper.layout(&paragraph_request(p, box_w, &self.opts))).clone())
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
                Item::Cell { x_pt, y_pt, width_pt, height_pt, .. } => {
                    cr.set_line_width(0.5);
                    cr.rectangle(*x_pt, *y_pt, *width_pt, *height_pt);
                    let _ = cr.stroke();
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
}
