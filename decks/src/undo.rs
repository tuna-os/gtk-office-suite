// undo.rs — Decks-specific commands implementing suite_common::undo::Command.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pattern #27 from AGENT-REFERENCE-LIBRARY.md.
// Uses the generic UndoManager from suite-common.

use suite_common::undo::Command;
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

/// Get position of any object as (x, y).
pub fn obj_position(obj: &SlideObject) -> (f64, f64) {
    match obj {
        SlideObject::TextBox { x, y, .. }
        | SlideObject::Rect { x, y, .. }
        | SlideObject::Image { x, y, .. }
        | SlideObject::Circle { x, y, .. } => (*x, *y),
    }
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
    fn description(&self) -> &str { "Edit Text" }
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
    fn description(&self) -> &str { "Add Slide" }
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
    fn description(&self) -> &str { "Delete Slide" }
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
