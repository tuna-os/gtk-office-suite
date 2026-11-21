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
#[derive(serde::Serialize, serde::Deserialize)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// How an object builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[derive(serde::Serialize, serde::Deserialize)]
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

/// One build: object `object` (a stable object id from the slide's
/// [`crate::ops::SlideIds::objects`]) arrives (`out` false) or leaves
/// (`out` true) with `effect`, on its own click. Identity, not z-position:
/// reordering never retargets a build, and deleting an object drops its
/// builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Build {
    pub object: u64,
    pub effect: BuildEffect,
    pub out: bool,
}

impl Build {
    /// The z-position of this build's object on `slide`, if still there.
    pub fn index_on(&self, slide: &Slide) -> Option<usize> {
        slide.object_index(self.object)
    }
}

/// How many clicks a slide's builds take before the next slide.
pub fn steps(slide: &Slide) -> usize {
    slide.builds.len()
}

/// Whether the object with stable id `object` is on the slide once `done`
/// builds have played: an object that builds in is hidden until its build,
/// one that builds out is gone after its build. A build whose object is
/// gone hides nothing.
fn shown_after(slide: &Slide, object: u64, done: usize) -> bool {
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
            let id = slide.ids.objects.get(i).copied().unwrap_or(u64::MAX);
            if let Some(b) = current.filter(|b| b.object == id) {
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
            shown_after(slide, id, step).then(|| FrameObject { object: o.clone(), opacity: 1.0 })
        })
        .filter(|f| f.opacity > 0.0)
        .collect()
}

/// A slide's builds after the object with stable id `id` is removed: its
/// own builds go; every other build keeps its target, whatever z-position
/// that object now has. Reordering needs no rewrite at all: identity is
/// not position.
pub fn without_object(builds: &[Build], id: u64) -> Vec<Build> {
    builds.iter().filter(|b| b.object != id).copied().collect()
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::shape::{ShapeKind, ShapeStyle};

    fn sq(x: f64) -> SlideObject {
        SlideObject::Shape { kind: ShapeKind::Rect, x, y: 100.0, w: 50.0, h: 50.0, rotation: 0.0, style: ShapeStyle::default() }
    }

    fn slide(builds: Vec<Build>) -> Slide {
        // Builds name objects by these stable ids, not z-positions.
        Slide { hidden: false,
            title: String::new(),
            background: "#ffffff".into(),
            background_picture: None,
            objects: vec![sq(0.0), sq(100.0), sq(200.0)],
            notes: String::new(),
            master_id: None,
            transition: Default::default(),
            builds,
            ids: crate::ops::SlideIds { slide: 1, objects: vec![11, 12, 13], deleted: vec![] },
            layout_id: None,
        }
    }

    fn xs(frame: &[FrameObject]) -> Vec<(f64, f64)> {
        frame.iter().map(|f| (obj_bounds(&f.object).0, f.opacity)).collect()
    }

    #[test]
    fn objects_that_build_in_wait_for_their_click() {
        let s = slide(vec![
            Build { object: 12, effect: BuildEffect::Dissolve, out: false },
            Build { object: 13, effect: BuildEffect::Appear, out: false },
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
        let s = slide(vec![Build { object: 11, effect: BuildEffect::Move(Edge::Left), out: true }]);
        assert_eq!(xs(&frame(&s, 0, 0.0))[0], (0.0, 1.0));
        let mid = xs(&frame(&s, 0, 0.5));
        assert_eq!(mid[0], (-25.0, 1.0), "half way to beyond the left edge (x = -50)");
        assert_eq!(xs(&frame(&s, 1, 0.0)).len(), 2, "gone");
    }

    #[test]
    fn a_move_in_slides_from_beyond_the_edge() {
        let s = slide(vec![Build { object: 13, effect: BuildEffect::Move(Edge::Right), out: false }]);
        let at = |t| xs(&frame(&s, 0, t)).last().copied().unwrap();
        assert_eq!(xs(&frame(&s, 0, 0.0)).len(), 2, "not started: still hidden");
        assert!(at(0.001).0 > 959.0, "starts beyond the right edge");
        assert_eq!(at(0.5).0, 580.0);
        assert_eq!(at(1.0).0, 200.0);
    }

    #[test]
    fn deleting_an_object_drops_only_its_builds_and_reorder_keeps_them() {
        let b = |object| Build { object, effect: BuildEffect::Appear, out: false };
        assert_eq!(without_object(&[b(11), b(12), b(13)], 12), vec![b(11), b(13)]);
        // Reordering needs no rewrite: the same builds still name the same
        // objects whatever z-position they now hold.
        assert_eq!(without_object(&[b(11), b(13)], u64::MAX), vec![b(11), b(13)]);
        let s = slide(vec![b(11), b(13)]);
        assert_eq!(s.builds[0].index_on(&s), Some(0));
        assert_eq!(s.builds[1].index_on(&s), Some(2));
        assert_eq!(Build { object: 99, ..b(99) }.index_on(&s), None);
    }
}
