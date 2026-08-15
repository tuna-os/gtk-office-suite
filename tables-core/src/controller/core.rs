// SPDX-License-Identifier: GPL-3.0-or-later
//! Workbook controller core: state ownership, history listeners, and the undo facade.
//! Split out of the former controller.rs (issue #227).

use std::cell::RefCell;
use std::rc::Rc;

use suite_common_core::events::{Broadcaster, Hint, Listener};
use suite_common_core::undo::{Command, UndoManager};

use crate::sheet::SheetModel;

use super::state::*;

pub struct WorkbookController {
    pub state: Rc<RefCell<WorkbookState>>,
    pub(super) undo: UndoManager<WorkbookState>,
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
        let mut undo = UndoManager::new(state.clone());
        undo.broadcaster = Some(Rc::new(Broadcaster::new()));
        Ok(Self {
            state,
            undo,
            dirty: false,
            file_path: Rc::new(RefCell::new(None)),
        })
    }

    pub fn listen_history(&self, listener: Rc<dyn Listener<Hint>>) {
        if let Some(broadcaster) = &self.undo.broadcaster {
            broadcaster.listen(listener);
        }
    }

    /// Execute a command and mark the workbook dirty. Every mutation must
    /// route through this (not `self.undo.execute` directly) so dirty state
    /// can never drift from what the undo history actually did.
    pub(super) fn execute(&mut self, cmd: Box<dyn Command<WorkbookState>>) {
        self.undo.execute(cmd);
        self.dirty = true;
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

    pub fn record_sheet_mutation(&mut self, description: &'static str, before: SheetModel) {
        let after = self.state.borrow().sheet().clone();
        let sheet_id = self.state.borrow().sheet().sheet_id;
        self.execute(Box::new(SheetSnapshotCommand {
            sheet_id,
            before,
            after,
            description,
        }));
    }

    pub fn undo(&mut self) -> bool {
        let did = self.undo.undo();
        if did {
            self.dirty = true;
        }
        did
    }

    pub fn redo(&mut self) -> bool {
        let did = self.undo.redo();
        if did {
            self.dirty = true;
        }
        did
    }

    pub fn can_undo(&self) -> bool {
        self.undo.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.undo.can_redo()
    }

    pub fn undo_description(&self) -> Option<&str> {
        self.undo.undo_description()
    }


    pub fn redo_description(&self) -> Option<&str> {
        self.undo.redo_description()
    }
}
