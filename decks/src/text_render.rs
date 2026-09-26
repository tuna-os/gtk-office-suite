//! text_render.rs — draw a text box's paragraphs: alignment, indents,
//! bullets and numbers, spacing, and the vertical anchor.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! The geometry is decided in decks-core (`ParaStyle::geometry`,
//! `Anchor::offset`, `markers`); this lays it out with Pango, one layout per
//! paragraph, because each paragraph has its own alignment, indent and
//! marker, and the anchor needs the block's height before anything is drawn.

use crate::canvas::set_styled_text;
use decks_core::engine::text_body::{markers, paragraphs};
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
    let (l, t, r, b) = match body.insets {
        Some(i) => (i.left * scale, i.top * scale, i.right * scale, i.bottom * scale),
        None => (PLAIN_INSET, PLAIN_INSET, PLAIN_INSET, PLAIN_INSET),
    };
    ((bw - l - r).max(8.0), bh - t - b)
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
    let (l, t, r, b) = match body.insets {
        Some(i) => (i.left * scale, i.top * scale, i.right * scale, i.bottom * scale),
        None => (PLAIN_INSET, PLAIN_INSET, PLAIN_INSET, PLAIN_INSET),
    };
    let inner_w = (bw - l - r).max(8.0);
    let inner_h = bh - t - b;
    let paras: Vec<Vec<Run>> = if runs.is_empty() {
        text.split('\n').map(|p| vec![Run { text: p.to_string(), style: Default::default() }]).collect()
    } else {
        paragraphs(runs)
    };
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
        // An empty paragraph still has its runs' height: give it a space.
        if para_text.is_empty() {
            let style = para.first().map(|r| r.style.clone()).unwrap_or_default();
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
            let style = decks_core::engine::RunStyle {
                bold: false,
                italic: false,
                underline: false,
                font_family: ms.font.clone().or(first.font_family.clone()),
                font_size_hp: ms.size.map(|s| (s.points(text_pt) * 2.0).round().max(1.0) as u16).or(first.font_size_hp),
                color: ms.color.clone().or(first.color.clone()),
                ..first.clone()
            };
            set_styled_text(&ml, label, &[Run { text: label.clone(), style }], text_scale);
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
}
