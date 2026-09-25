// magic_move.rs — Keynote's Magic Move (PowerPoint's Morph), in the core.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// docs/DESIGN-UI.md, "Magic Move": objects two consecutive slides share
// glide from where they were to where they are; everything else fades.
// This module decides which objects are "the same" (`match_objects`) and
// what the slide looks like part-way through (`frame`), as plain model
// objects with an opacity, so the canvas only draws them. GTK-free and
// unit-tested; a frame costs a few clones, no layout.

use crate::engine::shape::{Color, ShapeKind};
use crate::engine::SlideObject;
use crate::undo::{obj_bounds, obj_rotation, set_obj_bounds, set_obj_rotation};

/// What makes two objects on different slides the same object: its kind
/// and content. Text boxes match on their text, pictures on the picture,
/// shapes on their preset, tables on their size and text.
fn identity(o: &SlideObject) -> String {
    match o {
        SlideObject::TextBox { text, .. } => format!("text:{}", text.trim()),
        SlideObject::Image { path, .. } => format!("image:{}", picture_key(path)),
        SlideObject::Shape { kind, .. } => format!("shape:{}", kind_key(kind)),
        SlideObject::Rect { .. } => "shape:rect".into(),
        SlideObject::Circle { .. } => "shape:ellipse".into(),
        SlideObject::Table { table, .. } => {
            let cells: Vec<String> = table.rows.iter().flatten().map(|c| c.text()).collect();
            format!("table:{}x{}:{}", table.rows.len(), table.rows.first().map_or(0, |r| r.len()), cells.join("|"))
        }
    }
}

fn kind_key(k: &ShapeKind) -> &str {
    match k {
        ShapeKind::Rect => "rect",
        ShapeKind::RoundRect { .. } => "roundRect",
        ShapeKind::Ellipse => "ellipse",
        ShapeKind::Triangle => "triangle",
        ShapeKind::Diamond => "diamond",
        ShapeKind::Other(p) => p,
    }
}

/// A picture is the same picture if the bytes are: a reader extracts each
/// slide's copy to its own temporary file, so the paths differ.
fn picture_key(path: &str) -> String {
    match std::fs::read(path) {
        Ok(bytes) => {
            // FNV-1a: a cheap content fingerprint, not a security hash.
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for b in bytes {
                h ^= b as u64;
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
            format!("{h:016x}")
        }
        Err(_) => path.to_string(),
    }
}

fn centre(o: &SlideObject) -> (f64, f64) {
    let (x, y, w, h) = obj_bounds(o);
    (x + w / 2.0, y + h / 2.0)
}

/// Pairs `(from index, to index)` of the objects the two slides share.
/// Each object is used once; among several candidates the nearest one
/// wins, so two identical shapes keep their places rather than cross.
pub fn match_objects(from: &[SlideObject], to: &[SlideObject]) -> Vec<(usize, usize)> {
    let from_ids: Vec<String> = from.iter().map(identity).collect();
    let mut used = vec![false; from.len()];
    let mut pairs = Vec::new();
    for (ti, t) in to.iter().enumerate() {
        let id = identity(t);
        let (tx, ty) = centre(t);
        let best = from
            .iter()
            .enumerate()
            .filter(|(fi, _)| !used[*fi] && from_ids[*fi] == id)
            .min_by(|(_, a), (_, b)| {
                let d = |o: &SlideObject| {
                    let (x, y) = centre(o);
                    (x - tx).powi(2) + (y - ty).powi(2)
                };
                d(a).total_cmp(&d(b))
            })
            .map(|(fi, _)| fi);
        if let Some(fi) = best {
            used[fi] = true;
            pairs.push((fi, ti));
        }
    }
    pairs
}

/// Keynote's pacing: slow in, slow out.
pub fn ease(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn lerp_color(a: Color, b: Color, t: f64) -> Color {
    let c = |x: u8, y: u8| lerp(x as f64, y as f64, t).round().clamp(0.0, 255.0) as u8;
    Color(c(a.0, b.0), c(a.1, b.1), c(a.2, b.2))
}

/// The shorter way round from `a` to `b` degrees.
fn lerp_angle(a: f64, b: f64, t: f64) -> f64 {
    let d = (b - a + 540.0).rem_euclid(360.0) - 180.0;
    (a + d * t).rem_euclid(360.0)
}

/// `to` as it is at `t` on its way from `from`.
fn between(from: &SlideObject, to: &SlideObject, t: f64) -> SlideObject {
    let mut o = to.clone();
    let (a, b) = (obj_bounds(from), obj_bounds(to));
    set_obj_bounds(&mut o, lerp(a.0, b.0, t), lerp(a.1, b.1, t), lerp(a.2, b.2, t), lerp(a.3, b.3, t));
    set_obj_rotation(&mut o, lerp_angle(obj_rotation(from), obj_rotation(to), t));
    match (from, &mut o) {
        (SlideObject::Shape { style: sa, .. }, SlideObject::Shape { style: sb, .. }) => {
            if let (Some(fa), Some(fb)) = (sa.fill, sb.fill) {
                sb.fill = Some(lerp_color(fa, fb, t));
            }
            if let (Some(la), Some(lb)) = (sa.stroke, sb.stroke.as_mut()) {
                lb.color = lerp_color(la.color, lb.color, t);
                lb.width = lerp(la.width, lb.width, t);
            }
        }
        (SlideObject::TextBox { runs: ra, .. }, SlideObject::TextBox { runs: rb, .. }) if ra.len() == rb.len() => {
            for (x, y) in ra.iter().zip(rb.iter_mut()) {
                let sa = x.style.font_size_hp.unwrap_or(36) as f64;
                let sb = y.style.font_size_hp.unwrap_or(36) as f64;
                if sa != sb {
                    y.style.font_size_hp = Some(lerp(sa, sb, t).round().max(1.0) as u16);
                }
            }
        }
        _ => {}
    }
    o
}

/// One object of a Magic Move frame and how opaque it is drawn.
#[derive(Clone, Debug)]
pub struct FrameObject {
    pub object: SlideObject,
    pub opacity: f64,
}

/// The slide at `t` (0 → `from`, 1 → `to`, eased here). Objects only on
/// `from` fade out underneath; then `to`'s objects in `to`'s stacking
/// order, the shared ones moved part-way, the new ones fading in.
pub fn frame(from: &[SlideObject], to: &[SlideObject], pairs: &[(usize, usize)], t: f64) -> Vec<FrameObject> {
    let e = ease(t);
    let mut out = Vec::new();
    for (fi, o) in from.iter().enumerate() {
        if !pairs.iter().any(|(f, _)| *f == fi) {
            out.push(FrameObject { object: o.clone(), opacity: 1.0 - e });
        }
    }
    for (ti, o) in to.iter().enumerate() {
        match pairs.iter().find(|(_, t2)| *t2 == ti) {
            Some((fi, _)) => out.push(FrameObject { object: between(&from[*fi], o, e), opacity: 1.0 }),
            None => out.push(FrameObject { object: o.clone(), opacity: e }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::shape::ShapeStyle;
    use crate::engine::TextBody;

    fn text(t: &str, x: f64) -> SlideObject {
        SlideObject::TextBox { text: t.into(), x, y: 10.0, w: 100.0, h: 40.0, rotation: 0.0, runs: vec![], body: TextBody::default() }
    }

    fn shape(kind: ShapeKind, x: f64, fill: Color) -> SlideObject {
        SlideObject::Shape {
            kind,
            x,
            y: 100.0,
            w: 50.0,
            h: 50.0,
            rotation: 0.0,
            style: ShapeStyle { fill: Some(fill), gradient: None, stroke: None },
        }
    }

    #[test]
    fn objects_match_on_kind_and_content_and_the_nearest_wins() {
        let from = vec![text("Title", 0.0), shape(ShapeKind::Ellipse, 0.0, Color(0, 0, 0)), shape(ShapeKind::Ellipse, 500.0, Color(0, 0, 0))];
        let to = vec![shape(ShapeKind::Ellipse, 480.0, Color(9, 9, 9)), text("Title", 300.0), text("New", 0.0), shape(ShapeKind::Rect, 0.0, Color(0, 0, 0))];
        let pairs = match_objects(&from, &to);
        // The ellipse at 480 is the one that was at 500, not the one at 0.
        assert_eq!(pairs, vec![(2, 0), (0, 1)]);
    }

    #[test]
    fn a_shared_object_glides_and_the_rest_fade() {
        let from = vec![shape(ShapeKind::Rect, 0.0, Color(0, 0, 0)), text("Gone", 0.0)];
        let to = vec![shape(ShapeKind::Rect, 200.0, Color(200, 100, 0)), text("New", 0.0)];
        let pairs = match_objects(&from, &to);
        assert_eq!(pairs, vec![(0, 0)]);
        let mid = frame(&from, &to, &pairs, 0.5);
        assert_eq!(mid.len(), 3, "the leaving text, the shape, the arriving text");
        assert!(matches!(&mid[0].object, SlideObject::TextBox { text, .. } if text == "Gone"));
        assert!((mid[0].opacity - 0.5).abs() < 1e-9 && (mid[2].opacity - 0.5).abs() < 1e-9);
        assert_eq!(obj_bounds(&mid[1].object).0, 100.0, "half way");
        let SlideObject::Shape { style, .. } = &mid[1].object else { panic!() };
        assert_eq!(style.fill, Some(Color(100, 50, 0)), "the colour turns as it goes");
        // The ends are the slides themselves.
        let end = frame(&from, &to, &pairs, 1.0);
        assert_eq!(obj_bounds(&end[1].object).0, 200.0);
        assert_eq!(end[0].opacity, 0.0);
        let start = frame(&from, &to, &pairs, 0.0);
        assert_eq!(obj_bounds(&start[1].object).0, 0.0);
        assert_eq!(start[2].opacity, 0.0);
    }

    #[test]
    fn rotation_takes_the_short_way_round() {
        assert_eq!(lerp_angle(350.0, 10.0, 0.5), 0.0);
        assert_eq!(lerp_angle(10.0, 350.0, 0.25), 5.0);
        assert_eq!(lerp_angle(0.0, 90.0, 0.5), 45.0);
    }

    #[test]
    fn easing_is_slow_at_both_ends() {
        assert_eq!((ease(0.0), ease(0.5), ease(1.0)), (0.0, 0.5, 1.0));
        assert!(ease(0.1) < 0.1 && ease(0.9) > 0.9);
        assert_eq!(ease(2.0), 1.0);
    }

    #[test]
    fn the_same_picture_in_two_files_is_one_picture() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, c) = (dir.path().join("a"), dir.path().join("b"), dir.path().join("c"));
        std::fs::write(&a, b"png bytes").unwrap();
        std::fs::write(&b, b"png bytes").unwrap();
        std::fs::write(&c, b"other").unwrap();
        let img = |p: &std::path::Path, x| SlideObject::Image { path: p.to_string_lossy().into(), x, y: 0.0, w: 10.0, h: 10.0, rotation: 0.0 };
        assert_eq!(match_objects(&[img(&a, 0.0)], &[img(&c, 0.0), img(&b, 50.0)]), vec![(0, 1)]);
    }
}
