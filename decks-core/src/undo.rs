// undo.rs — Decks-specific commands implementing suite_common::undo::Command.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pattern #27 from AGENT-REFERENCE-LIBRARY.md.
// Uses the generic UndoManager from suite-common.

use suite_common_core::undo::Command;
use crate::engine::{Slide, SlideObject};

// ── Helper for offsetting object coordinates ───────────────────────────

fn offset_object(obj: &mut SlideObject, dx: f64, dy: f64) {
    match obj {
        SlideObject::TextBox { x, y, .. }
        | SlideObject::Rect { x, y, .. }
        | SlideObject::Image { x, y, .. }
        | SlideObject::Circle { x, y, .. } => {
            *x += dx;
            *y += dy;
        }
    }
}

/// Set absolute position of any object.
pub fn set_obj_position(obj: &mut SlideObject, nx: f64, ny: f64) {
    match obj {
        SlideObject::TextBox { x, y, .. }
        | SlideObject::Rect { x, y, .. }
        | SlideObject::Image { x, y, .. }
        | SlideObject::Circle { x, y, .. } => {
            *x = nx;
            *y = ny;
        }
    }
}

/// Get position of any object as (x, y).
pub fn obj_position(obj: &SlideObject) -> (f64, f64) {
    match obj {
        SlideObject::TextBox { x, y, .. }
        | SlideObject::Rect { x, y, .. }
        | SlideObject::Image { x, y, .. }
        | SlideObject::Circle { x, y, .. } => (*x, *y),
    }
}

/// Get rotation angle in degrees of any object.
pub fn obj_rotation(obj: &SlideObject) -> f64 {
    match obj {
        SlideObject::TextBox { rotation, .. }
        | SlideObject::Rect { rotation, .. }
        | SlideObject::Circle { rotation, .. }
        | SlideObject::Image { rotation, .. } => *rotation,
    }
}

/// Set rotation angle in degrees of any object.
pub fn set_obj_rotation(obj: &mut SlideObject, angle: f64) {
    match obj {
        SlideObject::TextBox { rotation, .. }
        | SlideObject::Rect { rotation, .. }
        | SlideObject::Circle { rotation, .. }
        | SlideObject::Image { rotation, .. } => *rotation = angle,
    }
}

/// Get bounding rectangle of any object as (x, y, w, h).
pub fn obj_bounds(obj: &SlideObject) -> (f64, f64, f64, f64) {
    match obj {
        SlideObject::TextBox { x, y, w, h, .. }
        | SlideObject::Rect { x, y, w, h, .. }
        | SlideObject::Image { x, y, w, h, .. } => (*x, *y, *w, *h),
        SlideObject::Circle { x, y, r, .. } => (*x - *r, *y - *r, *r * 2.0, *r * 2.0),
    }
}

/// Set bounding rectangle of any object (x, y, w, h).
pub fn set_obj_bounds(obj: &mut SlideObject, nx: f64, ny: f64, nw: f64, nh: f64) {
    match obj {
        SlideObject::TextBox { x, y, w, h, .. }
        | SlideObject::Rect { x, y, w, h, .. }
        | SlideObject::Image { x, y, w, h, .. } => {
            *x = nx;
            *y = ny;
            *w = nw.max(10.0);
            *h = nh.max(10.0);
        }
        SlideObject::Circle { x, y, r, .. } => {
            *x = nx + nw / 2.0;
            *y = ny + nh / 2.0;
            *r = (nw.min(nh) / 2.0).max(5.0);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignMode {
    Left,
    Center,
    Right,
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistributeMode {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZOrderOp {
    BringToFront,
    SendToBack,
    BringForward,
    SendBackward,
}

pub struct ResizeObjectCmd {
    pub slide_idx: usize,
    pub index: usize,
    pub old_bounds: (f64, f64, f64, f64),
    pub new_bounds: (f64, f64, f64, f64),
}

impl Command<Vec<Slide>> for ResizeObjectCmd {
    fn apply(&self, slides: &mut Vec<Slide>) {
        if self.slide_idx < slides.len() {
            if let Some(obj) = slides[self.slide_idx].objects.get_mut(self.index) {
                let (nx, ny, nw, nh) = self.new_bounds;
                set_obj_bounds(obj, nx, ny, nw, nh);
            }
        }
    }
    fn undo(&self, slides: &mut Vec<Slide>) {
        if self.slide_idx < slides.len() {
            if let Some(obj) = slides[self.slide_idx].objects.get_mut(self.index) {
                let (ox, oy, ow, oh) = self.old_bounds;
                set_obj_bounds(obj, ox, oy, ow, oh);
            }
        }
    }
    fn description(&self) -> &str { "Resize Object" }
}

pub struct RotateObjectCmd {
    pub slide_idx: usize,
    pub index: usize,
    pub old_angle: f64,
    pub new_angle: f64,
}

impl Command<Vec<Slide>> for RotateObjectCmd {
    fn apply(&self, slides: &mut Vec<Slide>) {
        if self.slide_idx < slides.len() {
            if let Some(obj) = slides[self.slide_idx].objects.get_mut(self.index) {
                set_obj_rotation(obj, self.new_angle);
            }
        }
    }
    fn undo(&self, slides: &mut Vec<Slide>) {
        if self.slide_idx < slides.len() {
            if let Some(obj) = slides[self.slide_idx].objects.get_mut(self.index) {
                set_obj_rotation(obj, self.old_angle);
            }
        }
    }
    fn description(&self) -> &str { "Rotate Object" }
}

pub struct AlignObjectsCmd {
    pub slide_idx: usize,
    pub indices: Vec<usize>,
    pub old_positions: Vec<(f64, f64)>,
    pub new_positions: Vec<(f64, f64)>,
}

impl Command<Vec<Slide>> for AlignObjectsCmd {
    fn apply(&self, slides: &mut Vec<Slide>) {
        if self.slide_idx < slides.len() {
            for (&idx, &(nx, ny)) in self.indices.iter().zip(self.new_positions.iter()) {
                if let Some(obj) = slides[self.slide_idx].objects.get_mut(idx) {
                    set_obj_position(obj, nx, ny);
                }
            }
        }
    }
    fn undo(&self, slides: &mut Vec<Slide>) {
        if self.slide_idx < slides.len() {
            for (&idx, &(ox, oy)) in self.indices.iter().zip(self.old_positions.iter()) {
                if let Some(obj) = slides[self.slide_idx].objects.get_mut(idx) {
                    set_obj_position(obj, ox, oy);
                }
            }
        }
    }
    fn description(&self) -> &str { "Align Objects" }
}

pub struct ZOrderCmd {
    pub slide_idx: usize,
    pub old_objects: Vec<SlideObject>,
    pub new_objects: Vec<SlideObject>,
}

impl Command<Vec<Slide>> for ZOrderCmd {
    fn apply(&self, slides: &mut Vec<Slide>) {
        if self.slide_idx < slides.len() {
            slides[self.slide_idx].objects = self.new_objects.clone();
        }
    }
    fn undo(&self, slides: &mut Vec<Slide>) {
        if self.slide_idx < slides.len() {
            slides[self.slide_idx].objects = self.old_objects.clone();
        }
    }
    fn description(&self) -> &str { "Reorder Object" }
}

// ── Concrete commands ──────────────────────────────────────────────────

pub struct AddObjectCmd {
    pub slide_idx: usize,
    pub object: SlideObject,
}

impl AddObjectCmd {
    pub fn new(slide_idx: usize, object: SlideObject) -> Self {
        Self { slide_idx, object }
    }
}

impl Command<Vec<Slide>> for AddObjectCmd {
    fn apply(&self, slides: &mut Vec<Slide>) {
        if self.slide_idx < slides.len() {
            slides[self.slide_idx].objects.push(self.object.clone());
        }
    }
    fn undo(&self, slides: &mut Vec<Slide>) {
        if self.slide_idx < slides.len() {
            slides[self.slide_idx].objects.pop();
        }
    }
    fn description(&self) -> &str { "Add Object" }
}

pub struct DeleteObjectCmd {
    pub slide_idx: usize,
    pub index: usize,
    pub object: SlideObject,
}

impl DeleteObjectCmd {
    pub fn new(slide_idx: usize, index: usize, object: SlideObject) -> Self {
        Self { slide_idx, index, object }
    }
}

impl Command<Vec<Slide>> for DeleteObjectCmd {
    fn apply(&self, slides: &mut Vec<Slide>) {
        if self.slide_idx < slides.len() && self.index < slides[self.slide_idx].objects.len() {
            slides[self.slide_idx].objects.remove(self.index);
        }
    }
    fn undo(&self, slides: &mut Vec<Slide>) {
        if self.slide_idx < slides.len() {
            slides[self.slide_idx].objects.insert(self.index, self.object.clone());
        }
    }
    fn description(&self) -> &str { "Delete Object" }
}

pub struct MoveObjectCmd {
    pub slide_idx: usize,
    pub index: usize,
    pub dx: f64,
    pub dy: f64,
}

impl Command<Vec<Slide>> for MoveObjectCmd {
    fn apply(&self, slides: &mut Vec<Slide>) {
        if self.slide_idx < slides.len() {
            if let Some(obj) = slides[self.slide_idx].objects.get_mut(self.index) {
                offset_object(obj, self.dx, self.dy);
            }
        }
    }
    fn undo(&self, slides: &mut Vec<Slide>) {
        if self.slide_idx < slides.len() {
            if let Some(obj) = slides[self.slide_idx].objects.get_mut(self.index) {
                offset_object(obj, -self.dx, -self.dy);
            }
        }
    }
    fn description(&self) -> &str { "Move Object" }
}

pub struct ChangeTextCmd {
    pub slide_idx: usize,
    pub index: usize,
    pub old_text: String,
    pub new_text: String,
}

impl Command<Vec<Slide>> for ChangeTextCmd {
    fn apply(&self, slides: &mut Vec<Slide>) {
        if self.slide_idx < slides.len() {
            if let Some(SlideObject::TextBox { ref mut text, .. }) =
                slides[self.slide_idx].objects.get_mut(self.index)
            {
                *text = self.new_text.clone();
            }
        }
    }
    fn undo(&self, slides: &mut Vec<Slide>) {
        if self.slide_idx < slides.len() {
            if let Some(SlideObject::TextBox { ref mut text, .. }) =
                slides[self.slide_idx].objects.get_mut(self.index)
            {
                *text = self.old_text.clone();
            }
        }
    }
    fn description(&self) -> &str { "Edit text" }
}

pub struct AddSlideCmd {
    pub index: usize,
    pub slide: Slide,
}

impl Command<Vec<Slide>> for AddSlideCmd {
    fn apply(&self, slides: &mut Vec<Slide>) {
        if self.index <= slides.len() {
            slides.insert(self.index, self.slide.clone());
        }
    }
    fn undo(&self, slides: &mut Vec<Slide>) {
        if self.index < slides.len() {
            slides.remove(self.index);
        }
    }
    fn description(&self) -> &str { "Add slide" }
}

pub struct DeleteSlideCmd {
    pub index: usize,
    pub slide: Slide,
}

impl Command<Vec<Slide>> for DeleteSlideCmd {
    fn apply(&self, slides: &mut Vec<Slide>) {
        if self.index < slides.len() {
            slides.remove(self.index);
        }
    }
    fn undo(&self, slides: &mut Vec<Slide>) {
        if self.index <= slides.len() {
            slides.insert(self.index, self.slide.clone());
        }
    }
    fn description(&self) -> &str { "Delete slide" }
}

pub struct ReorderSlidesCmd {
    pub from: usize,
    pub to: usize,
}

impl Command<Vec<Slide>> for ReorderSlidesCmd {
    fn apply(&self, slides: &mut Vec<Slide>) {
        if self.from < slides.len() && self.to < slides.len() {
            slides.swap(self.from, self.to);
        }
    }
    fn undo(&self, slides: &mut Vec<Slide>) {
        if self.from < slides.len() && self.to < slides.len() {
            slides.swap(self.from, self.to);
        }
    }
    fn description(&self) -> &str { "Reorder Slides" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use suite_common_core::undo::Command;

    fn make_slides() -> Vec<Slide> {
        vec![Slide { title: "S1".into(), background: "#fff".into(), objects: vec![], notes: String::new(), master_idx: Some(0) }]
    }

    #[test]
    fn test_add_object_undo() {
        let mut slides = make_slides();
        let obj = SlideObject::Rect { x: 10.0, y: 10.0, w: 100.0, h: 50.0, rotation: 0.0 };
        let cmd = AddObjectCmd::new(0, obj);
        cmd.apply(&mut slides);
        assert_eq!(slides[0].objects.len(), 1);
        cmd.undo(&mut slides);
        assert_eq!(slides[0].objects.len(), 0);
    }

    #[test]
    fn test_delete_object_undo() {
        let mut slides = make_slides();
        let obj = SlideObject::Rect { x: 10.0, y: 10.0, w: 100.0, h: 50.0, rotation: 0.0 };
        slides[0].objects.push(obj.clone());
        let cmd = DeleteObjectCmd::new(0, 0, obj);
        cmd.apply(&mut slides);
        assert_eq!(slides[0].objects.len(), 0);
        cmd.undo(&mut slides);
        assert_eq!(slides[0].objects.len(), 1);
    }

    #[test]
    fn test_move_object_undo() {
        let mut slides = make_slides();
        slides[0].objects.push(SlideObject::Rect { x: 10.0, y: 10.0, w: 100.0, h: 50.0, rotation: 0.0 });
        let cmd = MoveObjectCmd { slide_idx: 0, index: 0, dx: 10.0, dy: 20.0 };
        cmd.apply(&mut slides);
        let (x, y) = obj_position(&slides[0].objects[0]);
        assert_eq!(x, 20.0);
        assert_eq!(y, 30.0);
        cmd.undo(&mut slides);
        let (x, y) = obj_position(&slides[0].objects[0]);
        assert_eq!(x, 10.0);
        assert_eq!(y, 10.0);
    }

    #[test]
    fn test_change_text_undo() {
        let mut slides = make_slides();
        slides[0].objects.push(SlideObject::TextBox { text: "old".into(), x: 0.0, y: 0.0, w: 100.0, h: 20.0, runs: vec![], rotation: 0.0 });
        let cmd = ChangeTextCmd { slide_idx: 0, index: 0, old_text: "old".into(), new_text: "new".into() };
        cmd.apply(&mut slides);
        if let SlideObject::TextBox { text, .. } = &slides[0].objects[0] {
            assert_eq!(text, "new");
        } else { panic!(); }
        cmd.undo(&mut slides);
        if let SlideObject::TextBox { text, .. } = &slides[0].objects[0] {
            assert_eq!(text, "old");
        } else { panic!(); }
    }

    #[test]
    fn test_add_slide_undo() {
        let mut slides = make_slides();
        let new_slide = Slide { title: "S2".into(), background: "#fff".into(), objects: vec![], notes: String::new(), master_idx: Some(0) };
        let cmd = AddSlideCmd { index: 1, slide: new_slide };
        cmd.apply(&mut slides);
        assert_eq!(slides.len(), 2);
        cmd.undo(&mut slides);
        assert_eq!(slides.len(), 1);
    }

    #[test]
    fn test_delete_slide_undo() {
        let mut slides = make_slides();
        let s2 = Slide { title: "S2".into(), background: "#fff".into(), objects: vec![], notes: String::new(), master_idx: Some(0) };
        slides.push(s2.clone());
        let cmd = DeleteSlideCmd { index: 1, slide: s2 };
        cmd.apply(&mut slides);
        assert_eq!(slides.len(), 1);
        cmd.undo(&mut slides);
        assert_eq!(slides.len(), 2);
    }

    #[test]
    fn test_reorder_slides_undo() {
        let mut slides = make_slides();
        let s2 = Slide { title: "S2".into(), background: "#fff".into(), objects: vec![], notes: String::new(), master_idx: Some(0) };
        slides.push(s2);
        let cmd = ReorderSlidesCmd { from: 0, to: 1 };
        cmd.apply(&mut slides);
        assert_eq!(slides[0].title, "S2");
        cmd.undo(&mut slides);
        assert_eq!(slides[0].title, "S1");
    }

    #[test]
    fn test_slide_notes_preserved() {
        let slide = Slide { title: "S1".into(), background: "#fff".into(),
            objects: vec![], notes: "My notes".into(), master_idx: Some(0) };
        assert_eq!(slide.notes, "My notes");
        // Cloning preserves notes
        assert_eq!(slide.clone().notes, "My notes");
    }

    #[test]
    fn test_add_image_object() {
        let mut slides = make_slides();
        let img = SlideObject::Image { path: "/tmp/test.png".into(), x: 0.0, y: 0.0, w: 100.0, h: 100.0, rotation: 0.0 };
        let cmd = AddObjectCmd::new(0, img);
        cmd.apply(&mut slides);
        assert_eq!(slides[0].objects.len(), 1);
        match &slides[0].objects[0] {
            SlideObject::Image { path, x, y, .. } => {
                assert_eq!(path, "/tmp/test.png");
                assert_eq!(*x, 0.0);
                assert_eq!(*y, 0.0);
            }
            _ => panic!("Expected Image"),
        }
        cmd.undo(&mut slides);
        assert_eq!(slides[0].objects.len(), 0);
    }
}
