// guides.rs — smart guides: where a dragged object snaps, and the lines
// that say why.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// docs/DESIGN-UI.md, "Smart guides" (Keynote): while an object is dragged
// it snaps to the edges and centres of the other objects and of the slide,
// to the spacing already used between its neighbours, and while resizing
// to the other objects' widths and heights. Each snap comes with guides to
// draw. Everything here is plain rectangle arithmetic in model units, so
// the canvas only converts coordinates and strokes lines.

/// `(x, y, w, h)` in model units.
pub type Rect = (f64, f64, f64, f64);

/// The model's slide size.
pub const SLIDE: (f64, f64) = (960.0, 540.0);

/// Why a guide is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideKind {
    /// Edges line up.
    Edge,
    /// Centres line up.
    Centre,
    /// A gap equals another gap: drawn across each equal gap.
    Spacing,
    /// A width or height matches another object's: drawn along both.
    Size,
}

/// A guide line segment. `vertical` lines sit at x = `at` and run from y =
/// `from` to `to`; horizontal ones sit at y = `at` and run along x.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    pub vertical: bool,
    pub at: f64,
    pub from: f64,
    pub to: f64,
    pub kind: GuideKind,
}

/// The result of snapping: the adjusted rectangle and the guides to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapped {
    pub rect: Rect,
    pub guides: Vec<Guide>,
    /// Whether each axis (x, y) snapped to something.
    pub snapped: (bool, bool),
}

/// Which edges of a rectangle a resize moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

/// One axis of a rectangle: start and length.
#[derive(Clone, Copy, Debug)]
struct Span {
    start: f64,
    len: f64,
}

impl Span {
    fn end(self) -> f64 {
        self.start + self.len
    }
    fn mid(self) -> f64 {
        self.start + self.len / 2.0
    }
}

fn span(r: Rect, x_axis: bool) -> Span {
    if x_axis {
        Span { start: r.0, len: r.2 }
    } else {
        Span { start: r.1, len: r.3 }
    }
}

/// A candidate snap on one axis: the shift, and the guides it brings.
#[derive(Clone, Debug)]
struct Candidate {
    delta: f64,
    guides: Vec<Guide>,
}

/// Edge and centre alignment on one axis: `moving`'s start, middle and end
/// against every target's, and the slide's edges and middle.
fn align(moving: Rect, others: &[Rect], x_axis: bool, tol: f64) -> Vec<Candidate> {
    let m = span(moving, x_axis);
    let (slide_len, cross_len) = if x_axis { SLIDE } else { (SLIDE.1, SLIDE.0) };
    let mut targets: Vec<(Option<Rect>, f64, GuideKind)> = vec![
        (None, 0.0, GuideKind::Edge),
        (None, slide_len / 2.0, GuideKind::Centre),
        (None, slide_len, GuideKind::Edge),
    ];
    for &o in others {
        let s = span(o, x_axis);
        targets.push((Some(o), s.start, GuideKind::Edge));
        targets.push((Some(o), s.mid(), GuideKind::Centre));
        targets.push((Some(o), s.end(), GuideKind::Edge));
    }
    let mut out = Vec::new();
    for (i, own) in [m.start, m.mid(), m.end()].into_iter().enumerate() {
        for (rect, at, kind) in &targets {
            let delta = at - own;
            if delta.abs() > tol {
                continue;
            }
            // A centre lines up with a centre, an edge with an edge.
            let own_kind = if i == 1 { GuideKind::Centre } else { GuideKind::Edge };
            if own_kind != *kind {
                continue;
            }
            // The guide spans both objects across the other axis (or the
            // whole slide, for the slide's own lines).
            let mc = span(moving, !x_axis);
            let (from, to) = match rect {
                Some(o) => {
                    let oc = span(*o, !x_axis);
                    (mc.start.min(oc.start), mc.end().max(oc.end()))
                }
                None => (0.0, cross_len),
            };
            out.push(Candidate { delta, guides: vec![Guide { vertical: x_axis, at: *at, from, to, kind: *kind }] });
        }
    }
    out
}

/// Whether two rectangles overlap across the other axis (so they sit in
/// one row, for spacing along `x_axis`).
fn in_line(a: Rect, b: Rect, x_axis: bool) -> bool {
    let (sa, sb) = (span(a, !x_axis), span(b, !x_axis));
    sa.start < sb.end() && sb.start < sa.end()
}

/// A spacing guide across the gap from `a` (before) to `b` (after).
fn gap_guide(a: Rect, b: Rect, x_axis: bool) -> Guide {
    let (sa, sb) = (span(a, x_axis), span(b, x_axis));
    let (ca, cb) = (span(a, !x_axis), span(b, !x_axis));
    let at = (ca.start.max(cb.start) + ca.end().min(cb.end())) / 2.0;
    Guide { vertical: !x_axis, at, from: sa.end(), to: sb.start, kind: GuideKind::Spacing }
}

/// Equal spacing on one axis, among the objects in line with `moving`:
/// midway between its two neighbours, or the gap its neighbour already
/// keeps to the next one along.
fn spacing(moving: Rect, others: &[Rect], x_axis: bool, tol: f64) -> Vec<Candidate> {
    let m = span(moving, x_axis);
    let mut row: Vec<Rect> = others.iter().copied().filter(|o| in_line(*o, moving, x_axis)).collect();
    row.sort_by(|a, b| span(*a, x_axis).start.total_cmp(&span(*b, x_axis).start));
    let before: Vec<Rect> = row.iter().copied().filter(|o| span(*o, x_axis).end() <= m.start + tol).collect();
    let after: Vec<Rect> = row.iter().copied().filter(|o| span(*o, x_axis).start >= m.end() - tol).collect();
    let prev = before.iter().copied().max_by(|a, b| span(*a, x_axis).end().total_cmp(&span(*b, x_axis).end()));
    let next = after.iter().copied().min_by(|a, b| span(*a, x_axis).start.total_cmp(&span(*b, x_axis).start));
    let mut out = Vec::new();
    let shifted = |r: Rect, d: f64| if x_axis { (r.0 + d, r.1, r.2, r.3) } else { (r.0, r.1 + d, r.2, r.3) };
    // Midway between the two neighbours.
    if let (Some(p), Some(n)) = (prev, next) {
        let start = (span(p, x_axis).end() + span(n, x_axis).start - m.len) / 2.0;
        let delta = start - m.start;
        if delta.abs() <= tol && start >= span(p, x_axis).end() {
            let moved = shifted(moving, delta);
            out.push(Candidate { delta, guides: vec![gap_guide(p, moved, x_axis), gap_guide(moved, n, x_axis)] });
        }
    }
    // The gap the previous neighbour keeps to the one before it.
    if let Some(p) = prev {
        let pp = before
            .iter()
            .copied()
            .filter(|o| span(*o, x_axis).end() <= span(p, x_axis).start)
            .max_by(|a, b| span(*a, x_axis).end().total_cmp(&span(*b, x_axis).end()));
        if let Some(pp) = pp {
            let gap = span(p, x_axis).start - span(pp, x_axis).end();
            let delta = span(p, x_axis).end() + gap - m.start;
            if delta.abs() <= tol {
                let moved = shifted(moving, delta);
                out.push(Candidate { delta, guides: vec![gap_guide(pp, p, x_axis), gap_guide(p, moved, x_axis)] });
            }
        }
    }
    // The gap the next neighbour keeps to the one after it.
    if let Some(n) = next {
        let nn = after
            .iter()
            .copied()
            .filter(|o| span(*o, x_axis).start >= span(n, x_axis).end())
            .min_by(|a, b| span(*a, x_axis).start.total_cmp(&span(*b, x_axis).start));
        if let Some(nn) = nn {
            let gap = span(nn, x_axis).start - span(n, x_axis).end();
            let delta = span(n, x_axis).start - gap - m.end();
            if delta.abs() <= tol {
                let moved = shifted(moving, delta);
                out.push(Candidate { delta, guides: vec![gap_guide(moved, n, x_axis), gap_guide(n, nn, x_axis)] });
            }
        }
    }
    out
}

/// The closest candidate, and every guide of the candidates that agree
/// with it (two objects lined up at once show both lines).
fn best(cands: Vec<Candidate>) -> Option<(f64, Vec<Guide>)> {
    let d = cands.iter().map(|c| c.delta).min_by(|a, b| a.abs().total_cmp(&b.abs()))?;
    let mut guides: Vec<Guide> = cands.into_iter().filter(|c| (c.delta - d).abs() < 1e-6).flat_map(|c| c.guides).collect();
    guides.dedup();
    Some((d, guides))
}

/// Snap a rectangle being moved to `others` and the slide, within `tol`
/// model units on each axis.
pub fn snap_move(moving: Rect, others: &[Rect], tol: f64) -> Snapped {
    let mut rect = moving;
    let mut snapped = (false, false);
    for x_axis in [true, false] {
        let mut cands = align(moving, others, x_axis, tol);
        cands.extend(spacing(moving, others, x_axis, tol));
        if let Some((d, _)) = best(cands) {
            if x_axis {
                rect.0 += d;
                snapped.0 = true;
            } else {
                rect.1 += d;
                snapped.1 = true;
            }
        }
    }
    // The guides of the final position, so each one spans the objects
    // where they now are (the other axis may have moved too).
    let exact = 1e-6;
    let mut guides = Vec::new();
    for (x_axis, on) in [(true, snapped.0), (false, snapped.1)] {
        if on {
            let mut cands = align(rect, others, x_axis, exact);
            cands.extend(spacing(rect, others, x_axis, exact));
            guides.extend(cands.into_iter().flat_map(|c| c.guides));
        }
    }
    guides.dedup();
    Snapped { rect, guides, snapped }
}

/// Snap a rectangle being resized: the moving edges to the other objects'
/// and the slide's edges and centres, else its width or height to another
/// object's.
pub fn snap_resize(rect: Rect, edges: Edges, others: &[Rect], tol: f64) -> Snapped {
    let mut out = rect;
    let mut guides = Vec::new();
    let mut snapped = (false, false);
    for x_axis in [true, false] {
        let (lo, hi) = if x_axis { (edges.left, edges.right) } else { (edges.top, edges.bottom) };
        if !lo && !hi {
            continue;
        }
        let s = span(out, x_axis);
        let edge = if hi { s.end() } else { s.start };
        // Treat the moving edge as a zero-width rectangle and align it.
        let probe = if x_axis { (edge, out.1, 0.0, out.3) } else { (out.0, edge, out.2, 0.0) };
        let cands: Vec<Candidate> = align(probe, others, x_axis, tol)
            .into_iter()
            .filter(|c| c.guides.iter().all(|g| g.kind == GuideKind::Edge || g.kind == GuideKind::Centre))
            .collect();
        let mut delta = None;
        if let Some((d, g)) = best(cands) {
            delta = Some(d);
            guides.extend(g);
        } else {
            // Match another object's size.
            let len = s.len;
            let hit = others
                .iter()
                .map(|o| (o, span(*o, x_axis).len - len))
                .filter(|(_, d)| d.abs() <= tol)
                .min_by(|a, b| a.1.abs().total_cmp(&b.1.abs()));
            if let Some((o, d)) = hit {
                delta = Some(if hi { d } else { -d });
                let target_len = len + d;
                let (os, oc) = (span(*o, x_axis), span(*o, !x_axis));
                let new_start = if hi { s.start } else { s.start - d };
                let mc = span(out, !x_axis);
                // A line along each of the two equal sides, just outside.
                guides.push(Guide { vertical: !x_axis, at: oc.start - 4.0, from: os.start, to: os.start + target_len, kind: GuideKind::Size });
                guides.push(Guide { vertical: !x_axis, at: mc.start - 4.0, from: new_start, to: new_start + target_len, kind: GuideKind::Size });
            }
        }
        if let Some(d) = delta {
            if x_axis {
                snapped.0 = true;
                if hi {
                    out.2 += d;
                } else {
                    out.0 += d;
                    out.2 -= d;
                }
            } else {
                snapped.1 = true;
                if hi {
                    out.3 += d;
                } else {
                    out.1 += d;
                    out.3 -= d;
                }
            }
        }
    }
    Snapped { rect: out, guides, snapped }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 5.0;

    #[test]
    fn a_left_edge_snaps_to_another_objects_left_edge() {
        let other = (100.0, 100.0, 50.0, 50.0);
        let s = snap_move((103.0, 300.0, 80.0, 40.0), &[other], TOL);
        assert_eq!(s.rect.0, 100.0);
        assert!(s.snapped.0);
        let g = s.guides.iter().find(|g| g.vertical).expect("a vertical guide");
        assert_eq!((g.at, g.kind), (100.0, GuideKind::Edge));
        assert!(g.from <= 100.0 && g.to >= 340.0, "spans both objects: {g:?}");
    }

    #[test]
    fn centres_snap_to_centres_and_to_the_slides_middle() {
        let other = (100.0, 100.0, 100.0, 50.0); // centre x 150
        let s = snap_move((118.0, 300.0, 60.0, 40.0), &[other], TOL); // centre 148
        assert_eq!(s.rect.0, 120.0);
        assert_eq!(s.guides[0].kind, GuideKind::Centre);
        let s = snap_move((428.0, 247.0, 100.0, 50.0), &[], TOL);
        assert_eq!((s.rect.0, s.rect.1), (430.0, 245.0), "centred on the slide both ways");
        assert_eq!(s.guides.len(), 2);
        assert!(s.guides.iter().all(|g| g.kind == GuideKind::Centre));
    }

    #[test]
    fn nothing_near_is_no_snap() {
        let s = snap_move((300.0, 300.0, 50.0, 50.0), &[(100.0, 100.0, 50.0, 50.0)], TOL);
        assert_eq!(s.rect, (300.0, 300.0, 50.0, 50.0));
        assert_eq!(s.snapped, (false, false));
        assert!(s.guides.is_empty());
    }

    #[test]
    fn the_closest_candidate_wins() {
        // Left edge 2 away from one object's left, right edge 1 away from
        // another's right: the right edge wins.
        let a = (102.0, 0.0, 10.0, 10.0);
        let b = (0.0, 200.0, 181.0, 10.0);
        let s = snap_move((100.0, 100.0, 80.0, 10.0), &[a, b], TOL);
        assert_eq!(s.rect.0, 101.0);
    }

    #[test]
    fn midway_between_two_neighbours_is_equal_spacing() {
        let l = (100.0, 100.0, 50.0, 50.0); // ends at 150
        let r = (350.0, 100.0, 50.0, 50.0); // starts at 350
        let s = snap_move((223.0, 105.0, 50.0, 40.0), &[l, r], TOL); // midway start = 225
        assert_eq!(s.rect.0, 225.0);
        let gaps: Vec<&Guide> = s.guides.iter().filter(|g| g.kind == GuideKind::Spacing).collect();
        assert_eq!(gaps.len(), 2);
        assert!(gaps.iter().all(|g| (g.to - g.from - 75.0).abs() < 1e-9), "two 75-unit gaps: {gaps:?}");
    }

    #[test]
    fn the_gap_between_two_objects_repeats() {
        let a = (100.0, 100.0, 50.0, 50.0);
        let b = (170.0, 100.0, 50.0, 50.0); // gap 20
        let s = snap_move((243.0, 100.0, 50.0, 50.0), &[a, b], TOL);
        assert_eq!(s.rect.0, 240.0, "the same 20 after b");
        assert!(s.guides.iter().any(|g| g.kind == GuideKind::Spacing));
    }

    #[test]
    fn a_resized_edge_snaps_and_so_does_a_matching_width() {
        let other = (100.0, 100.0, 120.0, 50.0); // right edge 220
        let right = Edges { right: true, ..Default::default() };
        let s = snap_resize((100.0, 300.0, 118.0, 50.0), right, &[other], TOL);
        assert_eq!(s.rect, (100.0, 300.0, 120.0, 50.0));
        assert!(s.guides.iter().any(|g| g.vertical && g.at == 220.0));
        // Nowhere near an edge, but 3 short of the other's width.
        let s = snap_resize((600.0, 300.0, 117.0, 50.0), right, &[other], TOL);
        assert_eq!(s.rect.2, 120.0);
        assert_eq!(s.guides.iter().filter(|g| g.kind == GuideKind::Size).count(), 2);
        // Dragging the left edge keeps the right edge still.
        let left = Edges { left: true, ..Default::default() };
        let s = snap_resize((603.0, 300.0, 117.0, 50.0), left, &[other], TOL);
        assert_eq!((s.rect.0 + s.rect.2, s.rect.2), (720.0, 120.0));
        // An edge that isn't moving doesn't snap.
        let s = snap_resize((98.0, 300.0, 300.0, 50.0), right, &[other], TOL);
        assert_eq!(s.rect.0, 98.0);
    }
}
