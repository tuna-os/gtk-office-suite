// builds.rs — object builds: objects that arrive or leave one click at a
// time during a show (Keynote's builds, PowerPoint's entrance and exit
// animations).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// docs/DESIGN-UI.md, "Object builds (animate in/out, one item at a time)",
// "driven by the same tween engine as Magic Move": a slide's builds are an
// ordered list, one per click; `frame` gives what the slide shows at a
// build step part-way through its animation as the same FrameObjects Magic
// Move draws (an object and an opacity), with magic_move's easing. GTK-free
// and unit-tested; the canvas only draws the frames.

use crate::engine::{Slide, SlideObject};
use crate::magic_move::{ease, FrameObject};
use crate::undo::{obj_bounds, set_obj_origin};

/// Where a moving build comes from (in) or goes to (out).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// How an object builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BuildEffect {
    /// At once.
    Appear,
    /// Fading.
    Dissolve,
    /// Sliding in from (or out to) beyond the slide's edge.
    Move(Edge),
}

impl BuildEffect {
    pub const ALL: [BuildEffect; 6] = [
        BuildEffect::Appear,
        BuildEffect::Dissolve,
        BuildEffect::Move(Edge::Left),
        BuildEffect::Move(Edge::Right),
        BuildEffect::Move(Edge::Top),
        BuildEffect::Move(Edge::Bottom),
    ];

    pub fn label(self) -> &'static str {
        match self {
            BuildEffect::Appear => "Appear",
            BuildEffect::Dissolve => "Dissolve",
            BuildEffect::Move(Edge::Left) => "Move from Left",
            BuildEffect::Move(Edge::Right) => "Move from Right",
            BuildEffect::Move(Edge::Top) => "Move from Top",
            BuildEffect::Move(Edge::Bottom) => "Move from Bottom",
        }
    }
}

/// One build: object `object` (an index into the slide's objects) arrives
/// (`out` false) or leaves (`out` true) with `effect`, on its own click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Build {
    pub object: usize,
    pub effect: BuildEffect,
    pub out: bool,
}

/// How many clicks a slide's builds take before the next slide.
pub fn steps(slide: &Slide) -> usize {
    slide.builds.len()
}

/// Whether `object` is on the slide once `done` builds have played: an
/// object that builds in is hidden until its build, one that builds out
/// is gone after its build.
fn shown_after(slide: &Slide, object: usize, done: usize) -> bool {
    let mut shown = !slide.builds.iter().any(|b| b.object == object && !b.out)
        || slide.builds.iter().position(|b| b.object == object && !b.out).is_some_and(|i| i < done);
    if slide.builds.iter().enumerate().any(|(i, b)| b.object == object && b.out && i < done) {
        shown = false;
    }
    shown
}

/// `o` moved to beyond the slide's `edge`.
fn offstage(o: &SlideObject, edge: Edge) -> SlideObject {
    let (x, y, w, h) = obj_bounds(o);
    let (nx, ny) = match edge {
        Edge::Left => (-w, y),
        Edge::Right => (crate::guides::SLIDE.0, y),
        Edge::Top => (x, -h),
        Edge::Bottom => (x, crate::guides::SLIDE.1),
    };
    let mut moved = o.clone();
    set_obj_origin(&mut moved, nx, ny);
    moved
}

/// An object `amount` of the way between offstage (0) and in place (1).
fn slid(o: &SlideObject, edge: Edge, amount: f64) -> SlideObject {
    let (x, y, _, _) = obj_bounds(o);
    let (ox, oy, _, _) = obj_bounds(&offstage(o, edge));
    let mut moved = o.clone();
    set_obj_origin(&mut moved, ox + (x - ox) * amount, oy + (y - oy) * amount);
    moved
}

/// What `slide` shows while build `step` (0-based) plays, at `t` (0 → just
/// before it, 1 → done). `step == steps(slide)` with any `t` is the slide
/// with every build played; step 0 at t=0 is how the slide first appears.
pub fn frame(slide: &Slide, step: usize, t: f64) -> Vec<FrameObject> {
    let e = ease(t);
    // Before it starts, a build is just the slide as it stands.
    let current = if t > 0.0 { slide.builds.get(step) } else { None };
    slide
        .objects
        .iter()
        .enumerate()
        .filter_map(|(i, o)| {
            if let Some(b) = current.filter(|b| b.object == i) {
                // This object's build is playing.
                let amount = if b.out { 1.0 - e } else { e };
                return Some(match b.effect {
                    BuildEffect::Appear => {
                        FrameObject { object: o.clone(), opacity: if (b.out && t < 1.0) || (!b.out && t > 0.0) { 1.0 } else { 0.0 } }
                    }
                    BuildEffect::Dissolve => FrameObject { object: o.clone(), opacity: amount },
                    BuildEffect::Move(edge) => FrameObject { object: slid(o, edge, amount), opacity: 1.0 },
                });
            }
            shown_after(slide, i, step).then(|| FrameObject { object: o.clone(), opacity: 1.0 })
        })
        .filter(|f| f.opacity > 0.0)
        .collect()
}

/// A slide's builds after object `index` is removed: its own go, later
/// indices shift down.
pub fn after_delete(builds: &[Build], index: usize) -> Vec<Build> {
    builds
        .iter()
        .filter(|b| b.object != index)
        .map(|b| Build { object: if b.object > index { b.object - 1 } else { b.object }, ..*b })
        .collect()
}

/// A slide's builds after its objects are reordered: `old_of_new[n]` is
/// the old index of the object now at `n`.
pub fn after_reorder(builds: &[Build], old_of_new: &[usize]) -> Vec<Build> {
    builds
        .iter()
        .filter_map(|b| old_of_new.iter().position(|&old| old == b.object).map(|n| Build { object: n, ..*b }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::shape::{ShapeKind, ShapeStyle};

    fn sq(x: f64) -> SlideObject {
        SlideObject::Shape { kind: ShapeKind::Rect, x, y: 100.0, w: 50.0, h: 50.0, rotation: 0.0, style: ShapeStyle::default() }
    }

    fn slide(builds: Vec<Build>) -> Slide {
        Slide {
            title: String::new(),
            background: "#ffffff".into(),
            objects: vec![sq(0.0), sq(100.0), sq(200.0)],
            notes: String::new(),
            master_idx: None,
            transition: Default::default(),
            builds,
            ids: Default::default(),
        }
    }

    fn xs(frame: &[FrameObject]) -> Vec<(f64, f64)> {
        frame.iter().map(|f| (obj_bounds(&f.object).0, f.opacity)).collect()
    }

    #[test]
    fn objects_that_build_in_wait_for_their_click() {
        let s = slide(vec![
            Build { object: 1, effect: BuildEffect::Dissolve, out: false },
            Build { object: 2, effect: BuildEffect::Appear, out: false },
        ]);
        assert_eq!(steps(&s), 2);
        assert_eq!(xs(&frame(&s, 0, 0.0)), [(0.0, 1.0)], "only the object without a build");
        assert_eq!(xs(&frame(&s, 0, 0.5)), [(0.0, 1.0), (100.0, 0.5)], "the first fading in");
        assert_eq!(xs(&frame(&s, 1, 0.0)), [(0.0, 1.0), (100.0, 1.0)]);
        assert_eq!(xs(&frame(&s, 1, 0.5)), [(0.0, 1.0), (100.0, 1.0), (200.0, 1.0)], "appear is at once");
        assert_eq!(xs(&frame(&s, 2, 0.0)).len(), 3, "all built");
    }

    #[test]
    fn a_build_out_leaves_and_stays_gone() {
        let s = slide(vec![Build { object: 0, effect: BuildEffect::Move(Edge::Left), out: true }]);
        assert_eq!(xs(&frame(&s, 0, 0.0))[0], (0.0, 1.0));
        let mid = xs(&frame(&s, 0, 0.5));
        assert_eq!(mid[0], (-25.0, 1.0), "half way to beyond the left edge (x = -50)");
        assert_eq!(xs(&frame(&s, 1, 0.0)).len(), 2, "gone");
    }

    #[test]
    fn a_move_in_slides_from_beyond_the_edge() {
        let s = slide(vec![Build { object: 2, effect: BuildEffect::Move(Edge::Right), out: false }]);
        let at = |t| xs(&frame(&s, 0, t)).last().copied().unwrap();
        assert_eq!(xs(&frame(&s, 0, 0.0)).len(), 2, "not started: still hidden");
        assert!(at(0.001).0 > 959.0, "starts beyond the right edge");
        assert_eq!(at(0.5).0, 580.0);
        assert_eq!(at(1.0).0, 200.0);
    }

    #[test]
    fn builds_follow_their_objects_through_deletes_and_reorders() {
        let b = |object| Build { object, effect: BuildEffect::Appear, out: false };
        assert_eq!(after_delete(&[b(0), b(1), b(2)], 1), vec![b(0), b(1)]);
        // Object 0 sent to the back of three: new order is old [1, 2, 0].
        assert_eq!(after_reorder(&[b(0), b(2)], &[1, 2, 0]), vec![b(2), b(1)]);
    }
}
