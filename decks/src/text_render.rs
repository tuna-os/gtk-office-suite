//! text_render.rs — draw a text box's paragraphs: alignment, indents,
//! bullets and numbers, spacing, and the vertical anchor.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! The geometry is decided in decks-core (`ParaStyle::geometry`,
//! `Anchor::offset`, `markers`); this lays it out with Pango, one layout per
//! paragraph, because each paragraph has its own alignment, indent and
//! marker, and the anchor needs the block's height before anything is drawn.

use crate::canvas::set_styled_text;
use decks_core::engine::text_body::{markers, paragraph_lead_styles, paragraphs};
use decks_core::engine::{shrink_font_to_fit, Autofit, ParaAlign, Run, TextBody};
use gtk4::{cairo, pango};

/// A bullet or number: its layout, left edge, and how far below the
/// paragraph's top it starts so that the two baselines line up.
struct Marker {
    layout: pango::Layout,
    x: f64,
    dy: f64,
}

/// One paragraph, laid out and placed relative to the box's inner origin.
struct Placed {
    layout: pango::Layout,
    marker: Option<Marker>,
    x: f64,
    y: f64,
}

/// The canvas's inset for a box that states none (canvas pixels), as the
/// plain path has always drawn it.
const PLAIN_INSET: f64 = 4.0;

/// Lay out a styled text box in the canvas rectangle `(x, y, w, h)` and
/// return the paragraph layouts with their positions, in canvas pixels.
/// `scale` is canvas pixels per model unit.
fn place(
    cr: &cairo::Context,
    text: &str,
    runs: &[Run],
    body: &TextBody,
    rect: (f64, f64, f64, f64),
    scale: f64,
    desc: &pango::FontDescription,
) -> Vec<Placed> {
    // A `normAutofit` box is measured, not trusted: the file's
    // `fontScale`/`lnSpcReduction` are the last editor's stale cache (see
    // `shrink_font_to_fit`), so the fit is recomputed from full size and
    // only ever shrinks.
    let fit = match body.autofit {
        Some(_) => {
            let inner_h = content_box(body, (rect.2, rect.3), scale).1;
            let s = shrink_font_to_fit(inner_h, &mut |s| {
                let trial_fit = Some(Autofit { font_scale: s, line_reduction: 0.0 });
                place_inner(cr, text, runs, body, rect, scale, desc, trial_fit).1
            });
            Some(Autofit { font_scale: s, line_reduction: 0.0 })
        }
        None => None,
    };
    place_inner(cr, text, runs, body, rect, scale, desc, fit).0
}

/// The box's inner size after its insets, in canvas pixels.
fn content_box(body: &TextBody, (bw, bh): (f64, f64), scale: f64) -> (f64, f64) {
    let (l, t, r, b) = insets(body, (bw, bh), scale);
    ((bw - l - r).max(8.0), bh - t - b)
}

/// The box's insets in canvas pixels: its own, inside the text rectangle
/// of the shape it is drawn on, as an oval's text wraps inside the oval.
fn insets(body: &TextBody, (bw, bh): (f64, f64), scale: f64) -> (f64, f64, f64, f64) {
    let (l, t, r, b) = match body.insets {
        Some(i) => (i.left * scale, i.top * scale, i.right * scale, i.bottom * scale),
        None => (PLAIN_INSET, PLAIN_INSET, PLAIN_INSET, PLAIN_INSET),
    };
    let (fl, ft, fr, fb) = body.frame.as_ref().map_or((0.0, 0.0, 0.0, 0.0), |f| f.kind.text_inset(bw, bh));
    (l + fl, t + ft, r + fr, b + fb)
}

#[allow(clippy::too_many_arguments)]
fn place_inner(
    cr: &cairo::Context,
    text: &str,
    runs: &[Run],
    body: &TextBody,
    rect: (f64, f64, f64, f64),
    scale: f64,
    desc: &pango::FontDescription,
    fit: Option<Autofit>,
) -> (Vec<Placed>, f64) {
    let (bx, by, bw, bh) = rect;
    let (l, t, r, b) = insets(body, (bw, bh), scale);
    let inner_w = (bw - l - r).max(8.0);
    let inner_h = bh - t - b;
    let paras: Vec<Vec<Run>> = if runs.is_empty() {
        text.split('\n').map(|p| vec![Run { text: p.to_string(), style: Default::default() }]).collect()
    } else {
        paragraphs(runs)
    };
    let lead = paragraph_lead_styles(runs);
    let styles: Vec<_> = (0..paras.len()).map(|i| body.para(i)).collect();
    let empty: Vec<bool> = paras.iter().map(|p| p.iter().all(|r| r.text.trim().is_empty())).collect();
    let marks = markers(&styles, &empty);

    // The recomputed fit draws every size smaller; a plain box draws at
    // full size with default line spacing.
    let (font_k, line_factor) = match fit {
        Some(a) => (a.font_scale, (1.0 - a.line_reduction) as f32),
        None => (1.0, 0.0),
    };
    let text_scale = scale * font_k;
    let mut desc = desc.clone();
    if font_k != 1.0 {
        desc.set_absolute_size(desc.size() as f64 * font_k);
    }
    let desc = &desc;

    let mut placed = Vec::new();
    let mut y = 0.0;
    for (i, (para, st)) in paras.iter().zip(&styles).enumerate() {
        let para_text: String = para.iter().map(|r| r.text.as_str()).collect();
        let g = st.geometry(inner_w, scale, marks[i].is_some());
        let layout = pangocairo::functions::create_layout(cr);
        layout.set_font_description(Some(desc));
        layout.set_width((g.text_width * pango::SCALE as f64) as i32);
        layout.set_wrap(pango::WrapMode::WordChar);
        layout.set_indent((g.first_indent * pango::SCALE as f64) as i32);
        layout.set_line_spacing(line_factor);
        match st.align {
            ParaAlign::Left => layout.set_alignment(pango::Alignment::Left),
            ParaAlign::Center => layout.set_alignment(pango::Alignment::Center),
            ParaAlign::Right => layout.set_alignment(pango::Alignment::Right),
            ParaAlign::Justify => layout.set_justify(true),
        }
        // An empty paragraph still has its runs' height: give it a space,
        // in the run its break was written in.
        if para_text.is_empty() {
            let style = para.first().map(|r| r.style.clone()).or_else(|| lead.get(i).cloned().flatten()).unwrap_or_default();
            set_styled_text(&layout, " ", &[Run { text: " ".into(), style }], text_scale);
        } else {
            set_styled_text(&layout, &para_text, para, text_scale);
        }
        let line_h = layout
            .line_readonly(0)
            .map(|line| line.pixel_extents().1.height() as f64)
            .unwrap_or(0.0);
        // Space above the first paragraph is not drawn: the inset is the
        // box's top margin, as PowerPoint lays it out.
        if i > 0 {
            y += st.space_before.resolve(line_h / scale) * scale;
        }
        // The marker takes the first run's font, size and colour unless it
        // has its own, and sits on the first line's baseline.
        let marker = marks[i].as_ref().zip(g.marker_x).map(|(label, mx)| {
            let ml = pangocairo::functions::create_layout(cr);
            ml.set_font_description(Some(desc));
            let first = para.first().map(|r| r.style.clone()).unwrap_or_default();
            let text_pt = first.font_size_hp.map_or(18.0, |hp| hp as f64 / 2.0);
            let ms = &st.marker;
            // A Wingdings or Symbol bullet draws as the Unicode glyph it
            // shows, in the text's font: those fonts aren't installed.
            let (label, marker_font) = match decks_core::engine::symbol_font::unicode_bullet(ms.font.as_deref(), label) {
                Some(glyph) => (glyph.to_string(), None),
                None => (label.to_string(), ms.font.clone()),
            };
            let label = &label;
            let style = decks_core::engine::RunStyle {
                bold: false,
                italic: false,
                underline: false,
                font_family: marker_font.or(first.font_family.clone()),
                font_size_hp: ms.size.map(|s| (s.points(text_pt) * 2.0).round().max(1.0) as u16).or(first.font_size_hp),
                color: ms.color.clone().or(first.color.clone()),
                ..first.clone()
            };
            set_styled_text(&ml, label, &[Run { text: label.clone(), style }], text_scale);
            // A marker wider than the room it hangs in pushes the first
            // line past it, as LibreOffice lays it out: a bullet with no
            // margin to hang in (marL 0) otherwise sits on the text.
            let overlap = mx + ml.pixel_extents().1.width() as f64 - g.text_x;
            if overlap > 0.0 {
                layout.set_indent(((g.first_indent + overlap) * pango::SCALE as f64) as i32);
            }
            let dy = (layout.baseline() - ml.baseline()) as f64 / pango::SCALE as f64;
            Marker { layout: ml, x: mx, dy }
        });
        let h = layout.pixel_size().1 as f64;
        placed.push(Placed { layout, marker, x: g.text_x, y });
        // Pango tightens the lines after a paragraph's first; the first
        // line's share of the reduction comes off the advance.
        let first_cut = fit.map_or(0.0, |a| a.line_reduction) * line_h;
        y += h - first_cut + st.space_after.resolve(line_h / scale) * scale;
    }
    let content_h = y;
    let dy = body.anchor.offset(inner_h, y);
    for p in &mut placed {
        p.x += bx + l;
        p.y += by + t + dy;
        if let Some(m) = p.marker.as_mut() {
            m.x += bx + l;
        }
    }
    (placed, content_h)
}

/// Draw a text box whose `body` carries paragraph styles, in the current
/// source colour for runs that name none.
#[allow(clippy::too_many_arguments)]
pub fn draw_text_body(
    cr: &cairo::Context,
    text: &str,
    runs: &[Run],
    body: &TextBody,
    rect: (f64, f64, f64, f64),
    scale: f64,
    desc: &pango::FontDescription,
) {
    for p in place(cr, text, runs, body, rect, scale, desc) {
        if let Some(m) = &p.marker {
            cr.move_to(m.x, p.y + m.dy);
            pangocairo::functions::show_layout(cr, &m.layout);
        }
        cr.move_to(p.x, p.y);
        pangocairo::functions::show_layout(cr, &p.layout);
    }
}

/// A plain box's one layout (no paragraph styles), and where it is drawn:
/// the canvas's small inset, wrapped to the box. The canvas draws it and
/// the text editor's caret and selection are measured on it, so the two
/// can never disagree.
pub fn plain_layout(
    cr: &cairo::Context,
    text: &str,
    runs: &[Run],
    (sx, sy, sw, _): (f64, f64, f64, f64),
    scale: f64,
    desc: &pango::FontDescription,
) -> (pango::Layout, (f64, f64)) {
    let layout = pangocairo::functions::create_layout(cr);
    layout.set_width(((sw - 2.0 * PLAIN_INSET).max(8.0) as i32) * pango::SCALE);
    layout.set_wrap(pango::WrapMode::WordChar);
    layout.set_font_description(Some(desc));
    set_styled_text(&layout, text, runs, scale);
    (layout, (sx + PLAIN_INSET, sy + PLAIN_INSET))
}

/// One laid-out stretch of a box's text: a layout, where it is drawn, and
/// which bytes of the box's text it shows.
struct Stretch {
    layout: pango::Layout,
    x: f64,
    y: f64,
    start: usize,
    len: usize,
}

/// The box's text as the canvas lays it out (DCK-007): one stretch for a
/// plain box, one per paragraph otherwise.
#[allow(clippy::too_many_arguments)]
fn stretches(
    cr: &cairo::Context,
    text: &str,
    runs: &[Run],
    body: &TextBody,
    rect: (f64, f64, f64, f64),
    scale: f64,
    desc: &pango::FontDescription,
) -> Vec<Stretch> {
    if body.is_plain() {
        let (layout, (x, y)) = plain_layout(cr, text, runs, rect, scale, desc);
        return vec![Stretch { layout, x, y, start: 0, len: text.len() }];
    }
    let mut start = 0;
    place(cr, text, runs, body, rect, scale, desc)
        .into_iter()
        .zip(text.split('\n'))
        .map(|(p, para)| {
            let s = Stretch { layout: p.layout, x: p.x, y: p.y, start, len: para.len() };
            start += para.len() + 1;
            s
        })
        .collect()
}

/// Rectangles (canvas pixels, in the box's unrotated frame) covering bytes
/// `start..end` of the box's text, line by line; for a caret, one thin
/// bar where the next character goes.
#[allow(clippy::too_many_arguments)]
pub fn range_rects(
    cr: &cairo::Context,
    text: &str,
    runs: &[Run],
    body: &TextBody,
    rect: (f64, f64, f64, f64),
    scale: f64,
    desc: &pango::FontDescription,
    (start, end): (usize, usize),
) -> Vec<(f64, f64, f64, f64)> {
    let k = pango::SCALE as f64;
    let mut out = Vec::new();
    for s in stretches(cr, text, runs, body, rect, scale, desc) {
        let (ps, pe) = (s.start, s.start + s.len);
        if start == end {
            if (ps..=pe).contains(&start) {
                let (strong, _) = s.layout.cursor_pos((start - ps) as i32);
                out.push((s.x + strong.x() as f64 / k, s.y + strong.y() as f64 / k, 1.5, (strong.height() as f64 / k).max(1.0)));
                break;
            }
            continue;
        }
        if end <= ps || start > pe {
            continue;
        }
        let (a, b) = ((start.max(ps) - ps) as i32, (end.min(pe) - ps) as i32);
        let mut iter = s.layout.iter();
        loop {
            if let Some(line) = iter.line_readonly() {
                let (_, logical) = iter.line_extents();
                let (ls, le) = (line.start_index(), line.start_index() + line.length());
                let top = s.y + logical.y() as f64 / k;
                let height = logical.height() as f64 / k;
                if a < le && b > ls {
                    for pair in line.x_ranges(a.max(ls), b.min(le)).chunks(2) {
                        if let [x0, x1] = pair {
                            out.push((s.x + *x0 as f64 / k, top, (*x1 - *x0) as f64 / k, height));
                        }
                    }
                } else if s.len == 0 {
                    // A selected empty paragraph: a sliver, so the
                    // selection visibly spans it.
                    out.push((s.x + logical.x() as f64 / k, top, 4.0, height));
                }
            }
            if !iter.next_line() {
                break;
            }
        }
    }
    out
}

/// The byte offset of the box's text nearest the canvas point `(px, py)`
/// in the box's unrotated frame: where a click puts the caret.
#[allow(clippy::too_many_arguments)]
pub fn offset_at(
    cr: &cairo::Context,
    text: &str,
    runs: &[Run],
    body: &TextBody,
    rect: (f64, f64, f64, f64),
    scale: f64,
    desc: &pango::FontDescription,
    (px, py): (f64, f64),
) -> usize {
    let all = stretches(cr, text, runs, body, rect, scale, desc);
    // The paragraph under the point, else the last one above it.
    let Some(s) = all
        .iter()
        .rev()
        .find(|s| py >= s.y)
        .or(all.first())
    else {
        return 0;
    };
    let k = pango::SCALE as f64;
    let (_, index, trailing) = s.layout.xy_to_index(((px - s.x) * k) as i32, ((py - s.y) * k) as i32);
    let para = &text[s.start..s.start + s.len];
    let mut at = (index.max(0) as usize).min(para.len());
    while !para.is_char_boundary(at) {
        at -= 1;
    }
    let at = para[at..].char_indices().nth(trailing.max(0) as usize).map_or(para.len(), |(i, _)| at + i);
    s.start + at
}

#[cfg(test)]
mod tests {
    use super::*;
    use decks_core::engine::{Anchor, Bullet, Insets, ParaStyle, RunStyle};

    fn ctx() -> cairo::Context {
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 400, 400).unwrap();
        cairo::Context::new(&surface).unwrap()
    }

    fn desc() -> pango::FontDescription {
        let mut d = pango::FontDescription::from_string("Sans");
        d.set_absolute_size(12.0 * pango::SCALE as f64);
        d
    }

    fn runs(text: &str) -> Vec<Run> {
        vec![Run { text: text.into(), style: RunStyle::default() }]
    }

    #[test]
    fn a_bulleted_paragraph_draws_its_marker_in_the_hanging_indent() {
        let body = TextBody {
            paras: vec![ParaStyle { margin_left: 20.0, indent: -20.0, bullet: Bullet::Char("•".into()), ..Default::default() }],
            insets: Some(Insets { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 }),
            ..Default::default()
        };
        let placed = place(&ctx(), "One", &runs("One"), &body, (10.0, 10.0, 300.0, 100.0), 1.0, &desc());
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].x, 30.0, "text at the margin");
        let m = placed[0].marker.as_ref().expect("a marker");
        assert_eq!(m.x, 10.0, "marker at margin + indent");
        assert_eq!(m.layout.text().as_str(), "•");
        assert_eq!(m.dy, 0.0, "same size, same baseline");
    }

    #[test]
    fn a_smaller_marker_in_its_own_font_sits_on_the_texts_baseline() {
        use decks_core::engine::{MarkerSize, MarkerStyle};
        let body = TextBody {
            paras: vec![ParaStyle {
                bullet: Bullet::Char("•".into()),
                marker: MarkerStyle { font: Some("Serif".into()), size: Some(MarkerSize::Relative(0.5)), color: None },
                ..Default::default()
            }],
            insets: Some(Insets { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 }),
            ..Default::default()
        };
        let big = vec![Run { text: "One".into(), style: RunStyle { font_size_hp: Some(64), ..Default::default() } }];
        let placed = place(&ctx(), "One", &big, &body, (0.0, 0.0, 300.0, 100.0), 1.0, &desc());
        let m = placed[0].marker.as_ref().expect("a marker");
        let base = |l: &pango::Layout| l.baseline() as f64 / pango::SCALE as f64;
        assert!(base(&m.layout) < base(&placed[0].layout), "the marker is smaller");
        assert!((m.dy + base(&m.layout) - base(&placed[0].layout)).abs() < 1e-9, "baselines line up");
        let attrs = m.layout.attributes().expect("styled");
        assert!(attrs.attributes().iter().any(|a| a.type_() == pango::AttrType::Family), "its own font");
    }

    #[test]
    fn an_overflowing_autofit_box_shrinks_its_text_to_fit() {
        let big = vec![Run { text: "a\nb".into(), style: RunStyle { font_size_hp: Some(96), ..Default::default() } }];
        let plain = TextBody { insets: Some(Insets { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 }), ..Default::default() };
        let fit = TextBody { autofit: Some(Autofit { font_scale: 0.5, line_reduction: 0.2 }), ..plain.clone() };
        let rect = (0.0, 0.0, 300.0, 40.0);
        let block = |p: &[Placed]| p.last().map(|l| l.y + l.layout.pixel_size().1 as f64).unwrap_or(0.0);
        let a = place(&ctx(), "a\nb", &big, &plain, rect, 1.0, &desc());
        assert!(block(&a) > 40.0, "the unshrunk block overflows: {}", block(&a));
        let b = place(&ctx(), "a\nb", &big, &fit, rect, 1.0, &desc());
        assert!(block(&b) <= 40.0, "the recomputed fit fits: {}", block(&b));
        assert!(b[1].y - b[0].y < a[1].y - a[0].y, "its lines are closer");
    }

    #[test]
    fn a_stale_autofit_cache_is_recomputed_not_honoured() {
        let big = vec![Run { text: "a\nb".into(), style: RunStyle { font_size_hp: Some(48), ..Default::default() } }];
        let plain = TextBody { insets: Some(Insets { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 }), ..Default::default() };
        let stale = TextBody { autofit: Some(Autofit { font_scale: 0.5, line_reduction: 0.2 }), ..plain.clone() };
        let rect = (0.0, 0.0, 300.0, 300.0);
        let a = place(&ctx(), "a\nb", &big, &plain, rect, 1.0, &desc());
        let b = place(&ctx(), "a\nb", &big, &stale, rect, 1.0, &desc());
        assert_eq!(b[0].y, a[0].y, "text that fits is drawn at full size, not at the cached half");
        assert_eq!(b[0].layout.pixel_size(), a[0].layout.pixel_size());
    }

    #[test]
    fn a_middle_anchored_box_centres_its_block() {
        let body = TextBody {
            anchor: Anchor::Middle,
            insets: Some(Insets { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 }),
            ..Default::default()
        };
        let placed = place(&ctx(), "a", &runs("a"), &body, (0.0, 0.0, 300.0, 200.0), 1.0, &desc());
        let h = placed[0].layout.pixel_size().1 as f64;
        assert!((placed[0].y - (200.0 - h) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn each_paragraph_takes_its_own_alignment_and_stacks_below_the_last() {
        let body = TextBody {
            paras: vec![
                ParaStyle { align: ParaAlign::Center, ..Default::default() },
                ParaStyle { align: ParaAlign::Right, ..Default::default() },
            ],
            ..Default::default()
        };
        let placed = place(&ctx(), "a\nb", &runs("a\nb"), &body, (0.0, 0.0, 300.0, 200.0), 1.0, &desc());
        assert_eq!(placed[0].layout.alignment(), pango::Alignment::Center);
        assert_eq!(placed[1].layout.alignment(), pango::Alignment::Right);
        assert!(placed[1].y > placed[0].y);
        assert_eq!(placed[0].x, PLAIN_INSET, "no insets stated: the plain inset");
    }

    /// A bullet with no margin to hang in (marL 0, a negative indent)
    /// sits at the box's edge and its first line starts after it, not under
    /// it (the pay-centre innovation deck's Wingdings squares).
    #[test]
    fn a_bullet_with_no_room_to_hang_pushes_its_first_line_over() {
        let zero = Insets { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };
        let body = TextBody {
            paras: vec![ParaStyle { margin_left: 0.0, indent: -30.0, bullet: Bullet::Char("•".into()), ..Default::default() }],
            insets: Some(zero),
            ..Default::default()
        };
        let placed = place(&ctx(), "text", &runs("text"), &body, (0.0, 0.0, 300.0, 200.0), 1.0, &desc());
        let m = placed[0].marker.as_ref().expect("a marker");
        assert_eq!(m.x, 0.0);
        let width = m.layout.pixel_extents().1.width();
        assert!(width > 0);
        assert_eq!(placed[0].layout.indent(), width * pango::SCALE, "the first line starts after the bullet");
    }

    /// An oval's text wraps inside the oval, in its inscribed rectangle,
    /// as PowerPoint and LibreOffice lay it out (the pay-centre innovation
    /// deck's call-outs ran over the oval's edges).
    #[test]
    fn an_ovals_text_sits_in_its_inscribed_rectangle() {
        use decks_core::engine::shape::{ShapeKind, ShapeStyle};
        use decks_core::engine::text_body::Frame;
        let zero = Insets { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };
        let oval = TextBody { insets: Some(zero), frame: Some(Frame { kind: ShapeKind::Ellipse, style: ShapeStyle::default() }), ..Default::default() };
        let placed = place(&ctx(), "a", &runs("a"), &oval, (0.0, 0.0, 400.0, 200.0), 1.0, &desc());
        let inset = 200.0 * (1.0 - std::f64::consts::FRAC_1_SQRT_2);
        assert!((placed[0].x - inset).abs() < 0.01, "{} vs {inset}", placed[0].x);
        assert_eq!(content_box(&oval, (400.0, 200.0), 1.0).0, 400.0 - 2.0 * inset);
        let square = TextBody { frame: Some(Frame { kind: ShapeKind::Rect, style: ShapeStyle::default() }), ..oval };
        assert_eq!(place(&ctx(), "a", &runs("a"), &square, (0.0, 0.0, 400.0, 200.0), 1.0, &desc())[0].x, 0.0, "a rectangle's is its box");
    }

    /// DCK-007: the editor's caret, selection and click placement are
    /// measured on the layouts the canvas draws, plain or styled.
    #[test]
    fn a_selection_and_a_click_measure_the_text_the_canvas_draws() {
        let zero = Insets { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };
        let body = TextBody { paras: vec![ParaStyle::default(), ParaStyle { margin_left: 30.0, ..Default::default() }], insets: Some(zero), ..Default::default() };
        let text = "Alpha Beta\nGamma";
        let runs = runs(text);
        let rect = (10.0, 10.0, 300.0, 200.0);
        let caret = range_rects(&ctx(), text, &runs, &body, rect, 1.0, &desc(), (11, 11));
        assert_eq!(caret.len(), 1, "{caret:?}");
        assert!((caret[0].0 - 40.0).abs() < 0.5, "the second paragraph starts at its margin: {caret:?}");
        let sel = range_rects(&ctx(), text, &runs, &body, rect, 1.0, &desc(), (6, 10));
        assert_eq!(sel.len(), 1, "{sel:?}");
        assert!(sel[0].0 > 10.0 && sel[0].2 > 0.0 && sel[0].1 < caret[0].1, "{sel:?}");
        assert!(range_rects(&ctx(), text, &runs, &body, rect, 1.0, &desc(), (6, 13)).len() >= 2, "a selection across the break covers both");
        let (x, y, w, h) = sel[0];
        let at = offset_at(&ctx(), text, &runs, &body, rect, 1.0, &desc(), (x + w / 2.0, y + h / 2.0));
        assert!((6..=10).contains(&at), "{at}");
        let at = offset_at(&ctx(), text, &runs, &body, rect, 1.0, &desc(), (caret[0].0 + 1.0, caret[0].1 + caret[0].3 / 2.0));
        assert!(at >= 11, "{at}");
        // The plain path: one layout at the canvas's own inset.
        let plain = TextBody::default();
        let r = range_rects(&ctx(), text, &runs, &plain, rect, 1.0, &desc(), (0, 5));
        assert_eq!(r.len(), 1, "{r:?}");
        assert!((r[0].0 - 14.0).abs() < 0.5, "{r:?}");
    }

    /// #1397's figure tiles: two blank paragraphs written at 14pt lead a
    /// 14pt figure in a box whose default is larger. The blank lines are
    /// the figure's height, so it sits where three 14pt lines put it.
    #[test]
    fn a_leading_blank_paragraph_is_as_tall_as_the_run_it_was_written_in() {
        let small = RunStyle { font_size_hp: Some(14), ..Default::default() };
        let mut big = desc();
        big.set_absolute_size(36.0 * pango::SCALE as f64);
        let body = TextBody { paras: vec![ParaStyle::default(); 3], ..Default::default() };
        let blank = place(&ctx(), "\n\n6", &[Run { text: "\n\n6".into(), style: small.clone() }], &body, (0.0, 0.0, 300.0, 300.0), 1.0, &big);
        let full = place(&ctx(), "4\n5\n6", &[Run { text: "4\n5\n6".into(), style: small }], &body, (0.0, 0.0, 300.0, 300.0), 1.0, &big);
        assert_eq!(blank.len(), 3);
        assert!((blank[2].y - full[2].y).abs() < 1e-9, "blank {} vs text {}", blank[2].y, full[2].y);
    }
}
