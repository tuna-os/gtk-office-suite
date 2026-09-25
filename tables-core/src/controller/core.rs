// SPDX-License-Identifier: GPL-3.0-or-later
//! Workbook controller core: state ownership, history listeners, and the undo facade.
//! Split out of the former controller.rs (issue #227).

use std::cell::RefCell;
use std::rc::Rc;

use suite_common_core::events::{Broadcaster, Hint, Listener};
use suite_common_core::ops::History;

use super::ops::{show_changed_sheet, Op};


use super::state::*;

pub struct WorkbookController {
    pub state: Rc<RefCell<WorkbookState>>,
    /// The suite's one undo history (ADR 0011), of op groups' inverses.
    history: History<Op>,
    /// Each step's name ("Bold", "Delete Sheet"), in step with `history`:
    /// the undo stack's names, then the redo stack's.
    names: (Vec<String>, Vec<String>),
    broadcaster: Rc<Broadcaster<Hint>>,
    /// True whenever the workbook differs from what's on disk. Set by any
    /// executed/undone/redone command, cleared only by [`Self::mark_clean`]
    /// after a successful save. Undoing back to a prior save point does not
    /// re-clean the flag — a conservative simplification (issue #99).
    dirty: bool,
    /// The workbook's on-disk path, or `None` for an unsaved new document.
    /// Canonical document identity (#103) — window.rs reads/writes this
    /// shared cell rather than tracking its own copy.
    pub file_path: Rc<RefCell<Option<std::path::PathBuf>>>,
}

impl WorkbookController {
    pub fn new(rows: usize, cols: usize) -> Result<Self, String> {
        let state = Rc::new(RefCell::new(WorkbookState::new(rows, cols)?));
        Ok(Self {
            state,
            history: History::default(),
            names: (Vec::new(), Vec::new()),
            broadcaster: Rc::new(Broadcaster::new()),
            dirty: false,
            file_path: Rc::new(RefCell::new(None)),
        })
    }

    pub fn listen_history(&self, listener: Rc<dyn Listener<Hint>>) {
        self.broadcaster.listen(listener);
    }

    fn notify(&self) {
        self.broadcaster.broadcast(Hint::UndoStateChanged {
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
        });
    }

    /// Record an applied group's inverse as one undo step named `name`, and
    /// mark the workbook dirty. Every edit comes through here (via
    /// `apply_ops`), so the dirty flag can't drift from the history.
    pub(super) fn record(&mut self, name: String, inverse: Vec<Op>) {
        self.history.record(inverse);
        self.names.0.push(name);
        self.names.1.clear();
        self.dirty = true;
        self.notify();
    }

    fn step(&mut self, redo: bool) -> bool {
        let applied = {
            let mut state = self.state.borrow_mut();
            let applied = if redo { self.history.redo(&mut state) } else { self.history.undo(&mut state) };
            if let Some(ops) = &applied {
                show_changed_sheet(&mut state, ops);
            }
            applied
        };
        let (from, to) = if redo { (&mut self.names.1, &mut self.names.0) } else { (&mut self.names.0, &mut self.names.1) };
        let did = applied.is_some();
        if did {
            to.extend(from.pop());
            self.dirty = true;
        } else if !self.history.can_undo() && !self.history.can_redo() {
            // A step that no longer applied: the history dropped itself.
            self.names = (Vec::new(), Vec::new());
        }
        self.notify();
        did
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn undo(&mut self) -> bool {
        self.step(false)
    }

    pub fn redo(&mut self) -> bool {
        self.step(true)
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn undo_description(&self) -> Option<&str> {
        self.names.0.last().map(String::as_str)
    }

    pub fn redo_description(&self) -> Option<&str> {
        self.names.1.last().map(String::as_str)
    }
}
