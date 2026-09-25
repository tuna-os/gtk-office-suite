// ops.rs — deck edits as ops with exact inverses (RFC-0001 Phase 0).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Every change to a deck's slides is an [`Op`]; applying one returns the
// ops that undo it exactly, so undo is "apply the inverses" rather than a
// stored copy, and the stream of ops is what a CRDT replicates. The API
// has the shape of `letters_core::edit` and `tables_core::controller::ops`
// ([`apply`] returns the inverse ops of one op, [`apply_all`] applies a
// group all or nothing and returns the group's inverse in undo order), so
// the three can share one history in suite-common-core.
//
// The model follows RFC-0001's decisions for Decks:
//
// 1. **Slides and objects have stable ids** ([`SlideIds`]), not positions:
//    an op names the slide and the object it changes, so an edit recorded
//    before other slides or objects were added, moved or deleted still
//    finds its own. Positions appear only where an op places something
//    (`at`, `to`), as in Loro's MovableTree.
// 2. **Delete is a tombstone, and a delete beats a concurrent move.** A
//    deleted object's id stays on its slide's tombstone list; an op on a
//    tombstoned (or unknown) id changes nothing and has an empty inverse,
//    rather than failing. `InsertObject` of a tombstoned id (the inverse of
//    a delete) takes the tombstone away. Slides are the same: an op on a
//    deleted slide's id is a no-op, and ids are never reused.
// 3. **An object is one register.** `SetObject` replaces an object
//    whole (last writer wins per object); slide properties are one
//    register (`SetSlide`). Finer registers (per field, like Tables' cell
//    fields) are a Phase 3 refinement where concurrency needs them.
// 4. **Builds follow their objects.** Deleting an object drops its builds
//    and renumbers the rest, moving one renumbers them; the inverses put
//    them back.

use crate::builds::{after_delete, after_reorder, Build};
use crate::engine::{Slide, SlideObject, Transition};

/// A slide's and its objects' stable ids.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SlideIds {
    /// 0: not assigned yet.
    pub slide: u64,
    /// One per object, in the slide's order.
    pub objects: Vec<u64>,
    /// Deleted objects' ids (tombstones).
    pub deleted: Vec<u64>,
}

/// A slide's properties other than its objects: one register.
#[derive(Clone, Debug, PartialEq)]
pub struct SlideProps {
    pub title: String,
    pub background: String,
    pub notes: String,
    pub master_idx: Option<usize>,
    pub transition: Transition,
    pub builds: Vec<Build>,
}

impl SlideProps {
    pub fn of(s: &Slide) -> SlideProps {
        SlideProps {
            title: s.title.clone(),
            background: s.background.clone(),
            notes: s.notes.clone(),
            master_idx: s.master_idx,
            transition: s.transition,
            builds: s.builds.clone(),
        }
    }

    fn put(self, s: &mut Slide) {
        s.title = self.title;
        s.background = self.background;
        s.notes = self.notes;
        s.master_idx = self.master_idx;
        s.transition = self.transition;
        s.builds = self.builds;
    }
}

/// One change to a deck's slides.
#[derive(Clone, Debug)]
pub enum Op {
    /// Put `slide` (with its ids) at position `at`.
    InsertSlide { at: usize, slide: Box<Slide> },
    DeleteSlide { slide: u64 },
    /// Move a slide to position `to` (counted after it is taken out).
    MoveSlide { slide: u64, to: usize },
    SetSlide { slide: u64, props: Box<SlideProps> },
    /// Put `object` with id `id` at z-position `at` on a slide.
    InsertObject { slide: u64, at: usize, id: u64, object: Box<SlideObject> },
    DeleteObject { slide: u64, id: u64 },
    /// Move an object to z-position `to` (counted after it is taken out).
    MoveObject { slide: u64, id: u64, to: usize },
    SetObject { slide: u64, id: u64, object: Box<SlideObject> },
}

/// Why an op could not be applied. The deck is unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpError {
    /// A position past the end.
    OutOfRange,
    /// An insert of an id that is already there.
    Duplicate,
    /// Deleting the last slide (a deck always has one).
    LastSlide,
}

/// The largest id in use or tombstoned.
fn max_id(slides: &[Slide]) -> u64 {
    slides
        .iter()
        .flat_map(|s| std::iter::once(s.ids.slide).chain(s.ids.objects.iter().copied()).chain(s.ids.deleted.iter().copied()))
        .max()
        .unwrap_or(0)
}

/// Give every slide and object without an id a fresh one. Readers and
/// direct edits append objects without ids; ops need them all. Idempotent.
pub fn ensure_ids(slides: &mut [Slide]) {
    let mut next = max_id(slides) + 1;
    for s in slides.iter_mut() {
        if s.ids.slide == 0 {
            s.ids.slide = next;
            next += 1;
        }
        s.ids.objects.truncate(s.objects.len());
        while s.ids.objects.len() < s.objects.len() {
            s.ids.objects.push(next);
            next += 1;
        }
    }
}

/// A fresh id, after [`ensure_ids`].
pub fn next_id(slides: &[Slide]) -> u64 {
    max_id(slides) + 1
}

fn slide_pos(slides: &[Slide], id: u64) -> Option<usize> {
    slides.iter().position(|s| s.ids.slide == id)
}

fn object_pos(s: &Slide, id: u64) -> Option<usize> {
    s.ids.objects.iter().position(|o| *o == id)
}

/// Apply `op` to `slides`. Returns the ops that undo it, in order. An op
/// on a deleted or unknown slide or object changes nothing and returns no
/// inverse (a delete beats a concurrent move).
pub fn apply(slides: &mut Vec<Slide>, op: &Op) -> Result<Vec<Op>, OpError> {
    ensure_ids(slides);
    match op {
        Op::InsertSlide { at, slide } => {
            if *at > slides.len() {
                return Err(OutOfRange);
            }
            if slide.ids.slide != 0 && slide_pos(slides, slide.ids.slide).is_some() {
                return Err(OpError::Duplicate);
            }
            let mut s = (**slide).clone();
            if s.ids.slide == 0 {
                s.ids.slide = next_id(slides);
            }
            let id = s.ids.slide;
            slides.insert(*at, s);
            ensure_ids(slides);
            Ok(vec![Op::DeleteSlide { slide: id }])
        }
        Op::DeleteSlide { slide } => {
            let Some(i) = slide_pos(slides, *slide) else { return Ok(vec![]) };
            if slides.len() == 1 {
                return Err(OpError::LastSlide);
            }
            let removed = slides.remove(i);
            Ok(vec![Op::InsertSlide { at: i, slide: Box::new(removed) }])
        }
        Op::MoveSlide { slide, to } => {
            let Some(i) = slide_pos(slides, *slide) else { return Ok(vec![]) };
            if *to >= slides.len() {
                return Err(OutOfRange);
            }
            let s = slides.remove(i);
            slides.insert(*to, s);
            Ok(vec![Op::MoveSlide { slide: *slide, to: i }])
        }
        Op::SetSlide { slide, props } => {
            let Some(i) = slide_pos(slides, *slide) else { return Ok(vec![]) };
            let old = SlideProps::of(&slides[i]);
            (**props).clone().put(&mut slides[i]);
            Ok(vec![Op::SetSlide { slide: *slide, props: Box::new(old) }])
        }
        Op::InsertObject { slide, at, id, object } => {
            let Some(i) = slide_pos(slides, *slide) else { return Ok(vec![]) };
            let s = &mut slides[i];
            if *at > s.objects.len() {
                return Err(OutOfRange);
            }
            if object_pos(s, *id).is_some() {
                return Err(OpError::Duplicate);
            }
            s.objects.insert(*at, (**object).clone());
            s.ids.objects.insert(*at, *id);
            s.ids.deleted.retain(|d| d != id);
            // Builds point at positions: the ones at or after `at` move up.
            for b in &mut s.builds {
                if b.object >= *at {
                    b.object += 1;
                }
            }
            Ok(vec![Op::DeleteObject { slide: *slide, id: *id }])
        }
        Op::DeleteObject { slide, id } => {
            let Some(i) = slide_pos(slides, *slide) else { return Ok(vec![]) };
            let s = &mut slides[i];
            let Some(at) = object_pos(s, *id) else { return Ok(vec![]) };
            let old_props = SlideProps::of(s);
            let object = s.objects.remove(at);
            s.ids.objects.remove(at);
            s.ids.deleted.push(*id);
            let had_builds = s.builds.iter().any(|b| b.object == at);
            s.builds = after_delete(&s.builds, at);
            let mut undo = vec![Op::InsertObject { slide: *slide, at, id: *id, object: Box::new(object) }];
            if had_builds {
                // Its own builds went with it: put the slide's builds back.
                undo.push(Op::SetSlide { slide: *slide, props: Box::new(old_props) });
            }
            Ok(undo)
        }
        Op::MoveObject { slide, id, to } => {
            let Some(i) = slide_pos(slides, *slide) else { return Ok(vec![]) };
            let s = &mut slides[i];
            let Some(from) = object_pos(s, *id) else { return Ok(vec![]) };
            if *to >= s.objects.len() {
                return Err(OutOfRange);
            }
            let mut order: Vec<usize> = (0..s.objects.len()).collect();
            let o = order.remove(from);
            order.insert(*to, o);
            let obj = s.objects.remove(from);
            s.objects.insert(*to, obj);
            let oid = s.ids.objects.remove(from);
            s.ids.objects.insert(*to, oid);
            s.builds = after_reorder(&s.builds, &order);
            Ok(vec![Op::MoveObject { slide: *slide, id: *id, to: from }])
        }
        Op::SetObject { slide, id, object } => {
            let Some(i) = slide_pos(slides, *slide) else { return Ok(vec![]) };
            let s = &mut slides[i];
            let Some(at) = object_pos(s, *id) else { return Ok(vec![]) };
            let old = std::mem::replace(&mut s.objects[at], (**object).clone());
            Ok(vec![Op::SetObject { slide: *slide, id: *id, object: Box::new(old) }])
        }
    }
}

use OpError::OutOfRange;

/// Apply `ops` in order, all or nothing (the shared
/// `suite_common_core::ops::apply_all`): on success, the ops that undo the
/// whole group, in the order to apply them.
pub fn apply_all(slides: &mut Vec<Slide>, ops: &[Op]) -> Result<Vec<Op>, OpError> {
    suite_common_core::ops::apply_all(slides, ops)
}

/// The ops that turn the objects of slide `si` into `objects`, where the
/// change is edits in place (same count): one `SetObject` per object that
/// differs. How a whole-slide edit (align, distribute, a format over the
/// selection) becomes ops.
pub fn set_objects(slides: &[Slide], si: usize, objects: &[SlideObject]) -> Vec<Op> {
    let Some(s) = slides.get(si) else { return vec![] };
    s.objects
        .iter()
        .zip(objects)
        .zip(&s.ids.objects)
        .filter(|((a, b), _)| format!("{a:?}") != format!("{b:?}"))
        .map(|((_, b), id)| Op::SetObject { slide: s.ids.slide, id: *id, object: Box::new(b.clone()) })
        .collect()
}

/// Decks' ops in the suite's one op/history shape (ADR 0011): undo and
/// redo are `suite_common_core::ops::History<Op>`.
impl suite_common_core::ops::Op for Op {
    type Doc = Vec<Slide>;
    type Error = OpError;
    fn apply(&self, doc: &mut Vec<Slide>) -> Result<Vec<Op>, OpError> {
        apply(doc, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slide(title: &str, n: usize) -> Slide {
        Slide {
            title: title.into(),
            background: "#ffffff".into(),
            objects: (0..n).map(|i| SlideObject::Rect { x: i as f64 * 10.0, y: 0.0, w: 5.0, h: 5.0, rotation: 0.0 }).collect(),
            notes: String::new(),
            master_idx: None,
            transition: Default::default(),
            builds: vec![],
            ids: Default::default(),
        }
    }

    fn dbg(s: &[Slide]) -> String {
        format!("{s:?}")
    }

    #[test]
    fn ids_are_assigned_once_and_never_reused() {
        let mut d = vec![slide("a", 2), slide("b", 1)];
        ensure_ids(&mut d);
        let before = dbg(&d);
        ensure_ids(&mut d);
        assert_eq!(dbg(&d), before, "idempotent");
        let (s, o) = (d[0].ids.slide, d[0].ids.objects[1]);
        apply(&mut d, &Op::DeleteObject { slide: s, id: o }).unwrap();
        d[0].objects.push(SlideObject::Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0, rotation: 0.0 });
        ensure_ids(&mut d);
        assert_ne!(*d[0].ids.objects.last().unwrap(), o, "a tombstoned id is not reused");
    }

    #[test]
    fn a_delete_beats_a_later_move_or_edit_of_the_same_object() {
        let mut d = vec![slide("a", 3)];
        ensure_ids(&mut d);
        let (s, o) = (d[0].ids.slide, d[0].ids.objects[0]);
        apply(&mut d, &Op::DeleteObject { slide: s, id: o }).unwrap();
        assert_eq!(d[0].ids.deleted, vec![o]);
        let before = dbg(&d);
        // A concurrent move and edit arriving after the delete: no-ops.
        assert_eq!(apply(&mut d, &Op::MoveObject { slide: s, id: o, to: 1 }).unwrap().len(), 0);
        let obj = Box::new(SlideObject::Rect { x: 9.0, y: 9.0, w: 9.0, h: 9.0, rotation: 0.0 });
        assert_eq!(apply(&mut d, &Op::SetObject { slide: s, id: o, object: obj }).unwrap().len(), 0);
        assert_eq!(dbg(&d), before);
    }

    #[test]
    fn deleting_an_object_takes_its_builds_and_undo_brings_them_back() {
        use crate::builds::BuildEffect;
        let mut d = vec![slide("a", 3)];
        d[0].builds = vec![
            Build { object: 1, effect: BuildEffect::Appear, out: false },
            Build { object: 2, effect: BuildEffect::Dissolve, out: false },
        ];
        ensure_ids(&mut d);
        let before = dbg(&d);
        let (s, o) = (d[0].ids.slide, d[0].ids.objects[1]);
        let undo = apply(&mut d, &Op::DeleteObject { slide: s, id: o }).unwrap();
        assert_eq!(d[0].builds, vec![Build { object: 1, effect: BuildEffect::Dissolve, out: false }]);
        apply_all(&mut d, &undo).unwrap();
        // The tombstone goes when the object comes back.
        assert_eq!(dbg(&d), before);
    }

    #[test]
    fn the_shared_history_undoes_and_redoes_from_inverses() {
        use suite_common_core::ops::History;
        let mut d = vec![slide("a", 1)];
        ensure_ids(&mut d);
        let before = dbg(&d);
        let (s, o) = (d[0].ids.slide, d[0].ids.objects[0]);
        let mut h: History<Op> = History::default();
        h.record(apply_all(&mut d, &[Op::DeleteObject { slide: s, id: o }]).unwrap());
        let after = dbg(&d);
        assert!(h.undo(&mut d).is_some());
        assert_eq!(dbg(&d), before);
        assert!(h.redo(&mut d).is_some());
        assert_eq!(dbg(&d), after);
        assert!(h.redo(&mut d).is_none());
    }

    #[test]
    fn a_group_is_all_or_nothing() {
        let mut d = vec![slide("a", 2)];
        ensure_ids(&mut d);
        let before = dbg(&d);
        let s = d[0].ids.slide;
        let o = d[0].ids.objects[0];
        let err = apply_all(&mut d, &[Op::MoveObject { slide: s, id: o, to: 1 }, Op::MoveSlide { slide: s, to: 5 }]);
        assert_eq!(err.unwrap_err(), OpError::OutOfRange);
        assert_eq!(dbg(&d), before);
        assert_eq!(apply(&mut d, &Op::DeleteSlide { slide: s }).unwrap_err(), OpError::LastSlide);
    }
}
