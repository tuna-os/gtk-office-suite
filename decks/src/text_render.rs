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
use decks_core::engine::{ParaAlign, Run, TextBody};
use gtk4::{cairo, pango};

/// One paragraph, laid out and placed relative to the box's inner origin.
struct Placed {
    layout: pango::Layout,
    marker: Option<(pango::Layout, f64)>,
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

    // A shrunk box (a:normAutofit fontScale/lnSpcReduction) draws every
    // size smaller and its lines closer, as the file records.
    let (font_k, line_factor) = match body.autofit {
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
        // The marker takes the first run's size and colour.
        let marker = marks[i].as_ref().zip(g.marker_x).map(|(m, mx)| {
            let ml = pangocairo::functions::create_layout(cr);
            ml.set_font_description(Some(desc));
            let style = para.first().map(|r| r.style.clone()).unwrap_or_default();
            let style = decks_core::engine::RunStyle { bold: false, italic: false, underline: false, ..style };
            set_styled_text(&ml, m, &[Run { text: m.clone(), style }], text_scale);
            (ml, mx)
        });
        let h = layout.pixel_size().1 as f64;
        placed.push(Placed { layout, marker, x: g.text_x, y });
        // Pango tightens the lines after a paragraph's first; the first
        // line's share of the reduction comes off the advance.
        let first_cut = body.autofit.map_or(0.0, |a| a.line_reduction) * line_h;
        y += h - first_cut + st.space_after.resolve(line_h / scale) * scale;
    }
    let dy = body.anchor.offset(inner_h, y);
    for p in &mut placed {
        p.x += bx + l;
        p.y += by + t + dy;
        if let Some((_, mx)) = p.marker.as_mut() {
            *mx += bx + l;
        }
    }
    placed
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
        if let Some((ml, mx)) = &p.marker {
            cr.move_to(*mx, p.y);
            pangocairo::functions::show_layout(cr, ml);
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
        let (ml, mx) = placed[0].marker.as_ref().expect("a marker");
        assert_eq!(*mx, 10.0, "marker at margin + indent");
        assert_eq!(ml.text().as_str(), "•");
    }

    #[test]
    fn a_shrunk_box_draws_its_text_smaller_and_tighter() {
        use decks_core::engine::Autofit;
        let big = vec![Run { text: "a\nb".into(), style: RunStyle { font_size_hp: Some(48), ..Default::default() } }];
        let plain = TextBody { insets: Some(Insets { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 }), ..Default::default() };
        let shrunk = TextBody { autofit: Some(Autofit { font_scale: 0.5, line_reduction: 0.2 }), ..plain.clone() };
        let rect = (0.0, 0.0, 300.0, 300.0);
        let a = place(&ctx(), "a\nb", &big, &plain, rect, 1.0, &desc());
        let b = place(&ctx(), "a\nb", &big, &shrunk, rect, 1.0, &desc());
        let h = |p: &[Placed]| p[1].y - p[0].y;
        assert!(h(&b) < h(&a) * 0.5, "half the size and 20% tighter: {} vs {}", h(&b), h(&a));
        let w = |p: &[Placed]| p[0].layout.pixel_size().0;
        assert!(w(&b) < w(&a));
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
