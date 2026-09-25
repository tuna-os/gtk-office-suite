// controller.rs — canonical slide-list state and controller for Decks.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// First slice of issue #103 (GTK-free canonical controllers): owns the
// slide list, master list, undo history, and dirty flag as a single unit
// so `decks/src/window.rs` stops holding four separately-cloned Rcs in
// lockstep by convention. Object-level editing (add/move/delete object,
// text edits) still goes through decks_core::undo commands directly from
// window.rs for now — narrowing this to slide-list management only keeps
// the slice small enough to land fully wired in one pass rather than as
// a half-adopted abstraction.

use std::cell::{Cell, RefCell};
use std::rc::Rc;


use crate::engine::{MasterSlide, Slide, SlideObject};
use crate::ops::Op;
use suite_common_core::ops::History;

pub struct DecksController {
    pub slides: Rc<RefCell<Vec<Slide>>>,
    pub masters: Rc<RefCell<Vec<MasterSlide>>>,
    pub dirty: Rc<Cell<bool>>,
    /// The deck's on-disk path, or `None` for an unsaved new document.
    /// Canonical document identity (#103) — window.rs reads/writes this
    /// shared cell rather than tracking its own copy (mirrors
    /// tables_core::controller::WorkbookController::file_path).
    pub file_path: Rc<RefCell<Option<String>>>,
    /// Every edit's inverse ops (ops.rs). Applying, undoing and redoing
    /// through it are the only ways the controller changes the slides,
    /// and each marks the deck dirty.
    history: RefCell<History<Op>>,
}

impl DecksController {
    pub fn new(slides: Vec<Slide>, masters: Vec<MasterSlide>) -> Self {
        Self {
            slides: Rc::new(RefCell::new(slides)),
            masters: Rc::new(RefCell::new(masters)),
            dirty: Rc::new(Cell::new(false)),
            file_path: Rc::new(RefCell::new(None)),
            history: RefCell::new(History::default()),
        }
    }

    pub fn slide_count(&self) -> usize {
        self.slides.borrow().len()
    }

    /// Insert a new slide at `index` (typically the current slide count)
    /// and return the index it landed at.
    pub fn add_slide(&self, index: usize, slide: Slide) -> usize {
        let index = index.min(self.slides.borrow().len());
        let mut slide = slide;
        slide.ids = Default::default();
        self.apply_ops("Add Slide", vec![Op::InsertSlide { at: index, slide: Box::new(slide) }]);
        index
    }

    /// Delete the slide at `index`. Returns the index that should now be
    /// selected, or `None` if `index` was the only slide (deletion is a
    /// no-op — Decks always keeps at least one slide).
    pub fn delete_slide(&self, index: usize) -> Option<usize> {
        let (id, new_selected) = {
            let slides = self.ids_ready();
            if slides.len() <= 1 || index >= slides.len() {
                return None;
            }
            (slides[index].ids.slide, index.min(slides.len().saturating_sub(2)))
        };
        self.apply_ops("Delete Slide", vec![Op::DeleteSlide { slide: id }]);
        Some(new_selected)
    }

    /// Move the slide at `index` one place up (earlier). Returns its new
    /// index, or `None` if it was already first.
    pub fn move_slide_up(&self, index: usize) -> Option<usize> {
        if index == 0 {
            return None;
        }
        let id = self.ids_ready().get(index)?.ids.slide;
        self.apply_ops("Move Slide", vec![Op::MoveSlide { slide: id, to: index - 1 }]);
        Some(index - 1)
    }

    /// Move the slide at `index` one place down (later). Returns its new
    /// index, or `None` if it was already last.
    pub fn move_slide_down(&self, index: usize) -> Option<usize> {
        let id = {
            let slides = self.ids_ready();
            if index + 1 >= slides.len() {
                return None;
            }
            slides[index].ids.slide
        };
        self.apply_ops("Move Slide", vec![Op::MoveSlide { slide: id, to: index + 1 }]);
        Some(index + 1)
    }

    /// The slides, every slide and object given its id first.
    fn ids_ready(&self) -> std::cell::Ref<'_, Vec<Slide>> {
        crate::ops::ensure_ids(&mut self.slides.borrow_mut());
        self.slides.borrow()
    }

    /// Apply `ops` as one undo step named `description`, all or nothing.
    /// Returns false (and records nothing) when there is nothing to do or
    /// the group doesn't apply. The step's undo is the group's inverse ops.
    pub fn apply_ops(&self, description: &str, ops: Vec<Op>) -> bool {
        if ops.is_empty() {
            return false;
        }
        let inverses = crate::ops::apply_all(&mut self.slides.borrow_mut(), &ops);
        let Ok(inverses) = inverses else { return false };
        if inverses.is_empty() {
            // Every op addressed something deleted: nothing happened.
            return false;
        }
        // One user action, one step (`description` is for the caller's
        // messages; the shared history keeps only the inverse).
        let _ = description;
        self.history.borrow_mut().record(inverses);
        self.dirty.set(true);
        true
    }

    /// Edit the objects of slide `slide_idx` in place (same count) as one
    /// undo step: one `SetObject` per object `edit` changes.
    pub fn edit_objects(&self, slide_idx: usize, description: &str, edit: impl FnOnce(&mut Vec<SlideObject>)) -> bool {
        let ops = {
            let slides = self.ids_ready();
            let Some(slide) = slides.get(slide_idx) else { return false };
            let mut objects = slide.objects.clone();
            edit(&mut objects);
            if objects.len() != slide.objects.len() {
                return false;
            }
            crate::ops::set_objects(&slides, slide_idx, &objects)
        };
        self.apply_ops(description, ops)
    }

    pub fn add_object(&self, slide_idx: usize, object: SlideObject) {
        let op = {
            let slides = self.ids_ready();
            let Some(slide) = slides.get(slide_idx) else { return };
            Op::InsertObject {
                slide: slide.ids.slide,
                at: slide.objects.len(),
                id: crate::ops::next_id(&slides),
                object: Box::new(object),
            }
        };
        self.apply_ops("Add Object", vec![op]);
    }

    /// Delete object `index` of slide `slide_idx` (a tombstone: see ops.rs).
    /// `_object` is what the caller believes is there; the op takes the
    /// object from the slide itself.
    pub fn delete_object(&self, slide_idx: usize, index: usize, _object: SlideObject) {
        let op = {
            let slides = self.ids_ready();
            let Some(slide) = slides.get(slide_idx) else { return };
            let Some(id) = slide.ids.objects.get(index) else { return };
            Op::DeleteObject { slide: slide.ids.slide, id: *id }
        };
        self.apply_ops("Delete Object", vec![op]);
    }

    pub fn move_object(&self, slide_idx: usize, index: usize, dx: f64, dy: f64) {
        self.edit_objects(slide_idx, "Move Object", |o| {
            if let Some(obj) = o.get_mut(index) {
                crate::undo::offset_object(obj, dx, dy);
            }
        });
    }

    pub fn change_text(&self, slide_idx: usize, index: usize, _old_text: String, new_text: String) {
        self.edit_objects(slide_idx, "Edit text", |o| {
            if let Some(SlideObject::TextBox { text, .. }) = o.get_mut(index) {
                *text = new_text;
            }
        });
    }

    pub fn resize_object(&self, slide_idx: usize, index: usize, _old_bounds: (f64, f64, f64, f64), new_bounds: (f64, f64, f64, f64)) {
        self.edit_objects(slide_idx, "Resize Object", |o| {
            if let Some(obj) = o.get_mut(index) {
                let (x, y, w, h) = new_bounds;
                crate::undo::set_obj_bounds(obj, x, y, w, h);
            }
        });
    }

    pub fn rotate_object(&self, slide_idx: usize, index: usize, _old_angle: f64, new_angle: f64) {
        self.edit_objects(slide_idx, "Rotate Object", |o| {
            if let Some(obj) = o.get_mut(index) {
                crate::undo::set_obj_rotation(obj, new_angle);
            }
        });
    }

    pub fn align_objects(&self, slide_idx: usize, indices: &[usize], mode: crate::undo::AlignMode) {
        use crate::undo::{obj_bounds, AlignMode};
        let slides = self.slides.borrow();
        if slide_idx >= slides.len() || indices.len() < 2 { return; }
        let objects = &slides[slide_idx].objects;
        let mut bounds_list = Vec::new();
        let mut old_positions = Vec::new();
        for &idx in indices {
            if idx >= objects.len() { return; }
            bounds_list.push(obj_bounds(&objects[idx]));
            old_positions.push((objects[idx].x(), objects[idx].y()));
        }

        let target = match mode {
            AlignMode::Left => bounds_list.iter().map(|b| b.0).fold(f64::INFINITY, f64::min),
            AlignMode::Top => bounds_list.iter().map(|b| b.1).fold(f64::INFINITY, f64::min),
            AlignMode::Right => bounds_list.iter().map(|b| b.0 + b.2).fold(f64::NEG_INFINITY, f64::max),
            AlignMode::Bottom => bounds_list.iter().map(|b| b.1 + b.3).fold(f64::NEG_INFINITY, f64::max),
            AlignMode::Center => bounds_list.iter().map(|b| b.0 + b.2 / 2.0).sum::<f64>() / bounds_list.len() as f64,
            AlignMode::Middle => bounds_list.iter().map(|b| b.1 + b.3 / 2.0).sum::<f64>() / bounds_list.len() as f64,
        };

        let new_positions: Vec<(f64, f64)> = bounds_list.iter().map(|&(x, y, w, h)| {
            match mode {
                AlignMode::Left => (target, y),
                AlignMode::Right => (target - w, y),
                AlignMode::Center => (target - w / 2.0, y),
                AlignMode::Top => (x, target),
                AlignMode::Bottom => (x, target - h),
                AlignMode::Middle => (x, target - h / 2.0),
            }
        }).collect();
        drop(slides);
        let _ = old_positions;
        self.place_objects(slide_idx, "Align Objects", indices, &new_positions);
    }

    /// Move objects `indices` to `positions` (bounding-box origins) as one
    /// undo step.
    fn place_objects(&self, slide_idx: usize, description: &str, indices: &[usize], positions: &[(f64, f64)]) {
        self.edit_objects(slide_idx, description, |o| {
            for (&i, &(x, y)) in indices.iter().zip(positions) {
                if let Some(obj) = o.get_mut(i) {
                    crate::undo::set_obj_origin(obj, x, y);
                }
            }
        });
    }

    pub fn distribute_objects(&self, slide_idx: usize, indices: &[usize], mode: crate::undo::DistributeMode) {
        use crate::undo::{obj_bounds, DistributeMode};
        let slides = self.slides.borrow();
        if slide_idx >= slides.len() || indices.len() < 3 { return; }
        let objects = &slides[slide_idx].objects;
        let mut items: Vec<(usize, (f64, f64, f64, f64))> = indices.iter().filter_map(|&i| {
            objects.get(i).map(|o| (i, obj_bounds(o)))
        }).collect();

        if mode == DistributeMode::Horizontal {
            items.sort_by(|a, b| a.1.0.partial_cmp(&b.1.0).unwrap());
            let min_x = items.first().unwrap().1.0;
            let max_x = items.last().unwrap().1.0;
            let total_span = max_x - min_x;
            let step = total_span / (items.len() - 1) as f64;
            let old_pos: Vec<(f64, f64)> = items.iter().map(|i| (i.1.0, i.1.1)).collect();
            let new_pos: Vec<(f64, f64)> = items.iter().enumerate().map(|(idx, item)| {
                (min_x + idx as f64 * step, item.1.1)
            }).collect();
            let sorted_indices: Vec<usize> = items.iter().map(|i| i.0).collect();
            drop(slides);
            let _ = old_pos;
            self.place_objects(slide_idx, "Distribute Objects", &sorted_indices, &new_pos);
        } else {
            items.sort_by(|a, b| a.1.1.partial_cmp(&b.1.1).unwrap());
            let min_y = items.first().unwrap().1.1;
            let max_y = items.last().unwrap().1.1;
            let total_span = max_y - min_y;
            let step = total_span / (items.len() - 1) as f64;
            let old_pos: Vec<(f64, f64)> = items.iter().map(|i| (i.1.0, i.1.1)).collect();
            let new_pos: Vec<(f64, f64)> = items.iter().enumerate().map(|(idx, item)| {
                (item.1.0, min_y + idx as f64 * step)
            }).collect();
            let sorted_indices: Vec<usize> = items.iter().map(|i| i.0).collect();
            drop(slides);
            let _ = old_pos;
            self.place_objects(slide_idx, "Distribute Objects", &sorted_indices, &new_pos);
        }
    }

    pub fn z_order_object(&self, slide_idx: usize, index: usize, op: crate::undo::ZOrderOp) {
        let op = {
            let slides = self.ids_ready();
            let Some(slide) = slides.get(slide_idx) else { return };
            let Some(&id) = slide.ids.objects.get(index) else { return };
            let to = crate::undo::z_order_index(index, slide.objects.len(), op);
            if to == index {
                return;
            }
            Op::MoveObject { slide: slide.ids.slide, id, to }
        };
        self.apply_ops("Reorder Object", vec![op]);
    }

    /// Apply one Format inspector edit to the objects `indices` on slide
    /// `slide_idx`, as one undo step. Returns whether anything changed.
    pub fn format_objects(&self, slide_idx: usize, indices: &[usize], edit: &crate::format::FormatEdit) -> bool {
        self.edit_objects(slide_idx, edit.description(), |o| {
            let mut seen = Vec::new();
            for &i in indices {
                if seen.contains(&i) {
                    continue;
                }
                seen.push(i);
                if let Some(obj) = o.get_mut(i) {
                    crate::format::apply_edit(obj, edit);
                }
            }
        })
    }

    /// Set object `index`'s build in (`out` false) or out on slide
    /// `slide_idx`, or remove it (`effect` None), as one undo step. A new
    /// build goes last in the slide's build order. Returns whether
    /// anything changed.
    pub fn set_build(&self, slide_idx: usize, index: usize, out: bool, effect: Option<crate::builds::BuildEffect>) -> bool {
        use crate::builds::Build;
        let (old, new) = {
            let slides = self.slides.borrow();
            let Some(slide) = slides.get(slide_idx) else { return false };
            if index >= slide.objects.len() {
                return false;
            }
            let old = slide.builds.clone();
            let mut new = old.clone();
            match (new.iter_mut().find(|b| b.object == index && b.out == out), effect) {
                (Some(b), Some(e)) => b.effect = e,
                (Some(_), None) => new.retain(|b| !(b.object == index && b.out == out)),
                (None, Some(e)) => new.push(Build { object: index, effect: e, out }),
                (None, None) => {}
            }
            (old, new)
        };
        if old == new {
            return false;
        }
        self.set_slide_props(slide_idx, "Build", |p| p.builds = new)
    }

    /// Change slide `slide_idx`'s properties as one undo step.
    fn set_slide_props(&self, slide_idx: usize, description: &str, change: impl FnOnce(&mut crate::ops::SlideProps)) -> bool {
        let op = {
            let slides = self.ids_ready();
            let Some(slide) = slides.get(slide_idx) else { return false };
            let old = crate::ops::SlideProps::of(slide);
            let mut new = old.clone();
            change(&mut new);
            if new == old {
                return false;
            }
            Op::SetSlide { slide: slide.ids.slide, props: Box::new(new) }
        };
        self.apply_ops(description, vec![op])
    }

    /// Set how slide `slide_idx` arrives when presented, as one undo step.
    /// Returns whether anything changed.
    pub fn set_transition(&self, slide_idx: usize, transition: crate::engine::Transition) -> bool {
        self.set_slide_props(slide_idx, "Transition", |p| p.transition = transition)
    }

    /// What the inspector shows for object `index` on slide `slide_idx`.
    pub fn object_format(&self, slide_idx: usize, index: usize) -> Option<crate::format::ObjectFormat> {
        let slides = self.slides.borrow();
        slides.get(slide_idx)?.objects.get(index).map(crate::format::ObjectFormat::of)
    }

    pub fn undo(&self) -> bool {
        let done = self.history.borrow_mut().undo(&mut self.slides.borrow_mut()).is_some();
        if done {
            self.dirty.set(true);
        }
        done
    }

    pub fn redo(&self) -> bool {
        let done = self.history.borrow_mut().redo(&mut self.slides.borrow_mut()).is_some();
        if done {
            self.dirty.set(true);
        }
        done
    }

    pub fn can_undo(&self) -> bool {
        self.history.borrow().can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.borrow().can_redo()
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Slide;

    fn slide(title: &str) -> Slide {
        Slide { title: title.into(), background: "#fff".into(), objects: vec![], notes: String::new(), master_idx: Some(0), transition: Default::default(), builds: Vec::new(), ids: Default::default() }
    }

    fn rect(x: f64, y: f64) -> SlideObject {
        SlideObject::Rect { x, y, w: 10.0, h: 10.0, rotation: 0.0 }
    }

    #[test]
    fn add_object_appends_to_slide_and_marks_dirty() {
        let c = DecksController::new(vec![slide("S1")], vec![]);
        c.add_object(0, rect(1.0, 2.0));
        assert_eq!(c.slides.borrow()[0].objects.len(), 1);
        assert!(c.dirty.get());
    }

    #[test]
    fn move_object_offsets_position() {
        let c = DecksController::new(vec![slide("S1")], vec![]);
        c.add_object(0, rect(1.0, 2.0));
        c.move_object(0, 0, 5.0, -1.0);
        let slides = c.slides.borrow();
        match slides[0].objects[0] {
            SlideObject::Rect { x, y, .. } => {
                assert_eq!(x, 6.0);
                assert_eq!(y, 1.0);
            }
            _ => panic!("expected Rect"),
        }
    }

    #[test]
    fn delete_object_removes_it_and_undo_restores_it() {
        let c = DecksController::new(vec![slide("S1")], vec![]);
        c.add_object(0, rect(0.0, 0.0));
        let obj = c.slides.borrow()[0].objects[0].clone();
        c.delete_object(0, 0, obj);
        assert!(c.slides.borrow()[0].objects.is_empty());
        assert!(c.undo());
        assert_eq!(c.slides.borrow()[0].objects.len(), 1);
    }

    #[test]
    fn change_text_updates_textbox_and_undo_reverts() {
        let mut s = slide("S1");
        s.objects.push(SlideObject::TextBox { text: "old".into(), x: 0.0, y: 0.0, w: 10.0, h: 10.0, runs: vec![], rotation: 0.0, body: Default::default() });
        let c = DecksController::new(vec![s], vec![]);
        c.change_text(0, 0, "old".into(), "new".into());
        {
            let slides = c.slides.borrow();
            match &slides[0].objects[0] {
                SlideObject::TextBox { text, .. } => assert_eq!(text, "new"),
                _ => panic!("expected TextBox"),
            }
        }
        assert!(c.undo());
        let slides = c.slides.borrow();
        match &slides[0].objects[0] {
            SlideObject::TextBox { text, .. } => assert_eq!(text, "old"),
            _ => panic!("expected TextBox"),
        }
    }

    #[test]
    fn add_slide_appends_and_marks_dirty() {
        let c = DecksController::new(vec![slide("S1")], vec![]);
        assert!(!c.dirty.get());
        let idx = c.add_slide(1, slide("S2"));
        assert_eq!(idx, 1);
        assert_eq!(c.slide_count(), 2);
        assert_eq!(c.slides.borrow()[1].title, "S2");
        assert!(c.dirty.get());
    }

    #[test]
    fn delete_slide_removes_and_selects_prior() {
        let c = DecksController::new(vec![slide("S1"), slide("S2"), slide("S3")], vec![]);
        let new_idx = c.delete_slide(1).unwrap();
        assert_eq!(new_idx, 1);
        assert_eq!(c.slide_count(), 2);
        assert_eq!(c.slides.borrow()[1].title, "S3");
    }

    #[test]
    fn delete_slide_refuses_to_empty_deck() {
        let c = DecksController::new(vec![slide("Only")], vec![]);
        assert_eq!(c.delete_slide(0), None);
        assert_eq!(c.slide_count(), 1);
    }

    #[test]
    fn move_slide_up_and_down_swap_neighbors() {
        let c = DecksController::new(vec![slide("S1"), slide("S2")], vec![]);
        assert_eq!(c.move_slide_up(0), None);
        assert_eq!(c.move_slide_down(1), None);
        let idx = c.move_slide_down(0).unwrap();
        assert_eq!(idx, 1);
        assert_eq!(c.slides.borrow()[0].title, "S2");
        let idx = c.move_slide_up(1).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(c.slides.borrow()[0].title, "S1");
    }

    #[test]
    fn undo_redo_roundtrip_add_slide() {
        let c = DecksController::new(vec![slide("S1")], vec![]);
        c.add_slide(1, slide("S2"));
        assert_eq!(c.slide_count(), 2);
        assert!(c.undo());
        assert_eq!(c.slide_count(), 1);
        assert!(c.redo());
        assert_eq!(c.slide_count(), 2);
        assert!(!c.can_redo());
    }

    #[test]
    fn create_edit_save_reopen_flow() {
        // Full controller lifecycle: create, edit (add/reorder/delete),
        // write to a pptx, and confirm the reopened deck matches. Slide
        // titles aren't semantically stored in pptx by this reader/writer
        // (read_pptx synthesizes "Slide N" from position, matching what
        // real pptx slide titles usually are: placeholder text content,
        // not a plain field) — notes are, so that's the round-trip check.
        let mut s1 = slide("S1");
        s1.notes = "notes-1".into();
        let mut s2 = slide("S2");
        s2.notes = "notes-2".into();
        let mut s3 = slide("S3");
        s3.notes = "notes-3".into();
        let c = DecksController::new(vec![s1], vec![]);
        c.add_slide(1, s2);
        c.add_slide(2, s3);
        c.move_slide_down(0);
        c.delete_slide(2);
        assert_eq!(c.slide_count(), 2);
        let notes: Vec<String> = c.slides.borrow().iter().map(|s| s.notes.clone()).collect();

        let deck = crate::engine::Deck {
            slides: c.slides.borrow().clone(),
            masters: c.masters.borrow().clone(),
        };
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().with_extension("pptx");
        crate::write_deck(path.to_str().unwrap(), &deck).unwrap();
        let reopened = crate::read_deck(path.to_str().unwrap()).unwrap();
        let reopened_notes: Vec<String> = reopened.slides.iter().map(|s| s.notes.clone()).collect();
        assert_eq!(reopened_notes, notes);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn resize_and_rotate_object_with_undo() {
        let mut s = slide("S1");
        s.objects.push(SlideObject::Rect { x: 10.0, y: 10.0, w: 50.0, h: 50.0, rotation: 0.0 });
        let c = DecksController::new(vec![s], vec![]);
        c.resize_object(0, 0, (10.0, 10.0, 50.0, 50.0), (20.0, 20.0, 100.0, 80.0));
        assert_eq!(crate::undo::obj_bounds(&c.slides.borrow()[0].objects[0]), (20.0, 20.0, 100.0, 80.0));
        assert!(c.undo());
        assert_eq!(crate::undo::obj_bounds(&c.slides.borrow()[0].objects[0]), (10.0, 10.0, 50.0, 50.0));

        c.rotate_object(0, 0, 0.0, 45.0);
        assert_eq!(c.slides.borrow()[0].objects[0].rotation(), 45.0);
        assert!(c.undo());
        assert_eq!(c.slides.borrow()[0].objects[0].rotation(), 0.0);
    }

    #[test]
    fn align_and_distribute_objects_with_undo() {
        use crate::undo::{AlignMode, DistributeMode};
        let mut s = slide("S1");
        s.objects.push(SlideObject::Rect { x: 10.0, y: 10.0, w: 20.0, h: 20.0, rotation: 0.0 });
        s.objects.push(SlideObject::Rect { x: 50.0, y: 30.0, w: 20.0, h: 20.0, rotation: 0.0 });
        s.objects.push(SlideObject::Rect { x: 100.0, y: 50.0, w: 20.0, h: 20.0, rotation: 0.0 });
        let c = DecksController::new(vec![s], vec![]);

        c.align_objects(0, &[0, 1, 2], AlignMode::Left);
        assert_eq!(c.slides.borrow()[0].objects[0].x(), 10.0);
        assert_eq!(c.slides.borrow()[0].objects[1].x(), 10.0);
        assert_eq!(c.slides.borrow()[0].objects[2].x(), 10.0);
        assert!(c.undo());
        assert_eq!(c.slides.borrow()[0].objects[2].x(), 100.0);

        c.distribute_objects(0, &[0, 1, 2], DistributeMode::Horizontal);
        assert_eq!(c.slides.borrow()[0].objects[0].x(), 10.0);
        assert_eq!(c.slides.borrow()[0].objects[1].x(), 55.0);
        assert_eq!(c.slides.borrow()[0].objects[2].x(), 100.0);
        assert!(c.undo());
        assert_eq!(c.slides.borrow()[0].objects[1].x(), 50.0);
    }

    #[test]
    fn z_order_object_with_undo() {
        use crate::undo::ZOrderOp;
        let mut s = slide("S1");
        s.objects.push(SlideObject::Rect { x: 10.0, y: 10.0, w: 20.0, h: 20.0, rotation: 0.0 });
        s.objects.push(SlideObject::Circle { x: 50.0, y: 50.0, r: 10.0, rotation: 0.0 });
        let c = DecksController::new(vec![s], vec![]);

        c.z_order_object(0, 0, ZOrderOp::BringToFront);
        assert!(matches!(c.slides.borrow()[0].objects[1], SlideObject::Rect { .. }));
        assert!(c.undo());
        assert!(matches!(c.slides.borrow()[0].objects[0], SlideObject::Rect { .. }));
    }

    #[test]
    fn a_format_edit_is_one_undo_step_and_a_no_op_is_none() {
        use crate::format::FormatEdit;
        let mut s = slide("S1");
        s.objects.push(SlideObject::Rect { x: 10.0, y: 10.0, w: 20.0, h: 20.0, rotation: 0.0 });
        s.objects.push(SlideObject::Rect { x: 40.0, y: 10.0, w: 20.0, h: 20.0, rotation: 0.0 });
        let c = DecksController::new(vec![s], vec![]);
        assert!(c.format_objects(0, &[0, 1], &FormatEdit::Width(50.0)));
        assert_eq!(c.object_format(0, 1).unwrap().bounds.2, 50.0);
        assert!(!c.format_objects(0, &[0, 1], &FormatEdit::Width(50.0)), "nothing to change");
        assert!(c.undo());
        assert_eq!(c.object_format(0, 0).unwrap().bounds.2, 20.0);
        assert_eq!(c.object_format(0, 1).unwrap().bounds.2, 20.0, "both in the one step");
        assert!(!c.can_undo());
        assert!(c.dirty.get());
    }

    #[test]
    fn a_slides_transition_is_set_as_one_undo_step() {
        use crate::engine::Transition;
        let c = DecksController::new(vec![slide("S1"), slide("S2")], vec![]);
        assert!(c.set_transition(1, Transition::MagicMove));
        assert_eq!(c.slides.borrow()[1].transition, Transition::MagicMove);
        assert!(!c.set_transition(1, Transition::MagicMove), "no change, no step");
        assert!(!c.set_transition(9, Transition::Fade), "no such slide");
        assert!(c.undo());
        assert_eq!(c.slides.borrow()[1].transition, Transition::None);
        assert!(!c.can_undo());
    }

    #[test]
    fn builds_are_set_undoably_and_follow_deletes_and_reorders() {
        use crate::builds::{Build, BuildEffect};
        let mut s = slide("S1");
        for x in [0.0, 50.0, 100.0] {
            s.objects.push(SlideObject::Rect { x, y: 0.0, w: 20.0, h: 20.0, rotation: 0.0 });
        }
        let c = DecksController::new(vec![s], vec![]);
        assert!(c.set_build(0, 2, false, Some(BuildEffect::Dissolve)));
        assert!(c.set_build(0, 0, false, Some(BuildEffect::Appear)));
        assert!(!c.set_build(0, 0, false, Some(BuildEffect::Appear)), "no change");
        let builds = || c.slides.borrow()[0].builds.clone();
        assert_eq!(builds().iter().map(|b| b.object).collect::<Vec<_>>(), [2, 0], "in the order they were added");
        // Deleting object 1 renumbers the build on object 2.
        let obj = c.slides.borrow()[0].objects[1].clone();
        c.delete_object(0, 1, obj);
        assert_eq!(builds().iter().map(|b| b.object).collect::<Vec<_>>(), [1, 0]);
        assert!(c.undo());
        assert_eq!(builds().iter().map(|b| b.object).collect::<Vec<_>>(), [2, 0]);
        // Bringing object 0 to the front: it is now index 2.
        c.z_order_object(0, 0, crate::undo::ZOrderOp::BringToFront);
        assert_eq!(builds(), vec![
            Build { object: 1, effect: BuildEffect::Dissolve, out: false },
            Build { object: 2, effect: BuildEffect::Appear, out: false },
        ]);
        assert!(c.undo());
        assert!(c.set_build(0, 2, false, None));
        assert_eq!(builds().len(), 1);
    }
}
