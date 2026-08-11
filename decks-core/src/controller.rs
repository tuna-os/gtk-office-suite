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

use suite_common_core::events::{Broadcaster, Hint, Listener};
use suite_common_core::undo::UndoManager;

use crate::engine::{MasterSlide, Slide, SlideObject};
use crate::undo::{
    AddObjectCmd, AddSlideCmd, ChangeTextCmd, DeleteObjectCmd, DeleteSlideCmd, MoveObjectCmd,
    ReorderSlidesCmd,
};

/// Marks the deck dirty on any undo-stack mutation (execute, undo, or
/// redo) — driven by the undo manager's own broadcaster rather than set
/// at each call site, so it can't drift from what the undo history did.
struct DirtyListener {
    dirty: Rc<Cell<bool>>,
}

impl Listener<Hint> for DirtyListener {
    fn on_event(&self, hint: &Hint) {
        if let Hint::UndoStateChanged { .. } = hint {
            self.dirty.set(true);
        }
    }
}

pub struct DecksController {
    pub slides: Rc<RefCell<Vec<Slide>>>,
    pub masters: Rc<RefCell<Vec<MasterSlide>>>,
    pub dirty: Rc<Cell<bool>>,
    /// The deck's on-disk path, or `None` for an unsaved new document.
    /// Canonical document identity (#103) — window.rs reads/writes this
    /// shared cell rather than tracking its own copy (mirrors
    /// tables_core::controller::WorkbookController::file_path).
    pub file_path: Rc<RefCell<Option<String>>>,
    undo: RefCell<UndoManager<Vec<Slide>>>,
}

impl DecksController {
    pub fn new(slides: Vec<Slide>, masters: Vec<MasterSlide>) -> Self {
        let slides = Rc::new(RefCell::new(slides));
        let mut undo = UndoManager::new(slides.clone());
        let dirty = Rc::new(Cell::new(false));
        undo.broadcaster = Some(Rc::new(Broadcaster::new()));
        undo.broadcaster
            .as_ref()
            .unwrap()
            .listen(Rc::new(DirtyListener { dirty: dirty.clone() }));
        Self {
            slides,
            masters: Rc::new(RefCell::new(masters)),
            dirty,
            file_path: Rc::new(RefCell::new(None)),
            undo: RefCell::new(undo),
        }
    }

    pub fn slide_count(&self) -> usize {
        self.slides.borrow().len()
    }

    /// Insert a new slide at `index` (typically the current slide count)
    /// and return the index it landed at.
    pub fn add_slide(&self, index: usize, slide: Slide) -> usize {
        self.undo.borrow_mut().execute(Box::new(AddSlideCmd { index, slide }));
        index
    }

    /// Delete the slide at `index`. Returns the index that should now be
    /// selected, or `None` if `index` was the only slide (deletion is a
    /// no-op — Decks always keeps at least one slide).
    pub fn delete_slide(&self, index: usize) -> Option<usize> {
        let (removed, new_selected) = {
            let slides = self.slides.borrow();
            if slides.len() <= 1 || index >= slides.len() {
                return None;
            }
            (slides[index].clone(), index.min(slides.len().saturating_sub(2)))
        };
        self.undo.borrow_mut().execute(Box::new(DeleteSlideCmd { index, slide: removed }));
        Some(new_selected)
    }

    /// Swap the slide at `index` with its predecessor. Returns the new
    /// index of the moved slide, or `None` if it was already first.
    pub fn move_slide_up(&self, index: usize) -> Option<usize> {
        if index == 0 {
            return None;
        }
        self.undo.borrow_mut().execute(Box::new(ReorderSlidesCmd { from: index, to: index - 1 }));
        Some(index - 1)
    }

    /// Swap the slide at `index` with its successor. Returns the new
    /// index of the moved slide, or `None` if it was already last.
    pub fn move_slide_down(&self, index: usize) -> Option<usize> {
        if index + 1 >= self.slides.borrow().len() {
            return None;
        }
        self.undo.borrow_mut().execute(Box::new(ReorderSlidesCmd { from: index, to: index + 1 }));
        Some(index + 1)
    }

    /// Escape hatch for object-level commands that don't yet have their
    /// own controller method — still routed through the same owned undo
    /// history rather than a second one, so slide-list and object edits
    /// never desync.
    pub fn execute(&self, cmd: Box<dyn suite_common_core::undo::Command<Vec<Slide>>>) {
        self.undo.borrow_mut().execute(cmd);
    }

    pub fn add_object(&self, slide_idx: usize, object: SlideObject) {
        self.execute(Box::new(AddObjectCmd::new(slide_idx, object)));
    }

    pub fn delete_object(&self, slide_idx: usize, index: usize, object: SlideObject) {
        self.execute(Box::new(DeleteObjectCmd::new(slide_idx, index, object)));
    }

    pub fn move_object(&self, slide_idx: usize, index: usize, dx: f64, dy: f64) {
        self.execute(Box::new(MoveObjectCmd { slide_idx, index, dx, dy }));
    }

    pub fn change_text(&self, slide_idx: usize, index: usize, old_text: String, new_text: String) {
        self.execute(Box::new(ChangeTextCmd { slide_idx, index, old_text, new_text }));
    }

    pub fn resize_object(&self, slide_idx: usize, index: usize, old_bounds: (f64, f64, f64, f64), new_bounds: (f64, f64, f64, f64)) {
        self.execute(Box::new(crate::undo::ResizeObjectCmd { slide_idx, index, old_bounds, new_bounds }));
    }

    pub fn rotate_object(&self, slide_idx: usize, index: usize, old_angle: f64, new_angle: f64) {
        self.execute(Box::new(crate::undo::RotateObjectCmd { slide_idx, index, old_angle, new_angle }));
    }

    pub fn align_objects(&self, slide_idx: usize, indices: &[usize], mode: crate::undo::AlignMode) {
        use crate::undo::{obj_bounds, AlignMode, AlignObjectsCmd};
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

        self.execute(Box::new(AlignObjectsCmd {
            slide_idx,
            indices: indices.to_vec(),
            old_positions,
            new_positions,
        }));
    }

    pub fn distribute_objects(&self, slide_idx: usize, indices: &[usize], mode: crate::undo::DistributeMode) {
        use crate::undo::{obj_bounds, DistributeMode, AlignObjectsCmd};
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
            self.execute(Box::new(AlignObjectsCmd {
                slide_idx,
                indices: sorted_indices,
                old_positions: old_pos,
                new_positions: new_pos,
            }));
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
            self.execute(Box::new(AlignObjectsCmd {
                slide_idx,
                indices: sorted_indices,
                old_positions: old_pos,
                new_positions: new_pos,
            }));
        }
    }

    pub fn z_order_object(&self, slide_idx: usize, index: usize, op: crate::undo::ZOrderOp) {
        use crate::undo::{ZOrderCmd, ZOrderOp};
        let slides = self.slides.borrow();
        if slide_idx >= slides.len() || index >= slides[slide_idx].objects.len() { return; }
        let old_objects = slides[slide_idx].objects.clone();
        let mut new_objects = old_objects.clone();
        let len = new_objects.len();

        match op {
            ZOrderOp::BringToFront => {
                let obj = new_objects.remove(index);
                new_objects.push(obj);
            }
            ZOrderOp::SendToBack => {
                let obj = new_objects.remove(index);
                new_objects.insert(0, obj);
            }
            ZOrderOp::BringForward => {
                if index + 1 < len {
                    new_objects.swap(index, index + 1);
                }
            }
            ZOrderOp::SendBackward => {
                if index > 0 {
                    new_objects.swap(index, index - 1);
                }
            }
        }
        drop(slides);
        self.execute(Box::new(ZOrderCmd { slide_idx, old_objects, new_objects }));
    }

    pub fn undo(&self) -> bool {
        self.undo.borrow_mut().undo()
    }

    pub fn redo(&self) -> bool {
        self.undo.borrow_mut().redo()
    }

    pub fn can_undo(&self) -> bool {
        self.undo.borrow().can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.undo.borrow().can_redo()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Slide;

    fn slide(title: &str) -> Slide {
        Slide { title: title.into(), background: "#fff".into(), objects: vec![], notes: String::new(), master_idx: Some(0) }
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
        s.objects.push(SlideObject::TextBox { text: "old".into(), x: 0.0, y: 0.0, w: 10.0, h: 10.0, runs: vec![], rotation: 0.0 });
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
}
