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
    /// Monotonic feedback token; compare before/after a user operation so a
    /// rejection cannot leak into a later no-op. Trusted replay never changes it.
    pub(super) protection_rejections: u64,
    /// A viewer's workbook (DCP role `viewer`, RFC-0005 question 4): every
    /// user edit, undo and redo is refused, with no history step and no
    /// rejection counted (protection's feedback text would be wrong).
    /// Trusted replay (`ops::apply`, a remote update applied by the
    /// collaboration layer) is not a user edit and is not affected.
    pub(super) read_only: bool,
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
            protection_rejections: 0,
            read_only: false,
            file_path: Rc::new(RefCell::new(None)),
        })
    }

    pub fn listen_history(&self, listener: Rc<dyn Listener<Hint>>) {
        self.broadcaster.listen(listener);
    }

    fn notify(&self) {
        self.broadcaster.broadcast(Hint::UndoStateChanged { can_undo: self.can_undo(), can_redo: self.can_redo() });
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
        if self.read_only {
            return false;
        }
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
        } else {
            // Nothing to step (no name either), or a step that no longer
            // applied: the history dropped that step alone, and its name
            // goes with it, so the names stay in step with the stacks.
            from.pop();
        }
        self.notify();
        did
    }

    /// Changes only when protection rejects a user operation. Callers compare
    /// the token before and after their operation to show native feedback.
    pub fn protection_rejection_count(&self) -> u64 {
        self.protection_rejections
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

    /// Whether the workbook refuses user edits (a viewer's, see `read_only`).
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Make the workbook read-only, or editable again. Listeners hear the
    /// undo state change (a read-only workbook cannot undo or redo).
    pub fn set_read_only(&mut self, read_only: bool) {
        if self.read_only != read_only {
            self.read_only = read_only;
            self.notify();
        }
    }

    pub fn undo(&mut self) -> bool {
        self.step(false)
    }

    pub fn redo(&mut self) -> bool {
        self.step(true)
    }

    pub fn can_undo(&self) -> bool {
        !self.read_only && self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        !self.read_only && self.history.can_redo()
    }

    pub fn undo_description(&self) -> Option<&str> {
        self.names.0.last().map(String::as_str)
    }

    pub fn redo_description(&self) -> Option<&str> {
        self.names.1.last().map(String::as_str)
    }
}
