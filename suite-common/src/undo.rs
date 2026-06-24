// undo.rs — Generic undo/redo via Command pattern.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Shared across Letters, Tables, and Decks.
// Pattern #27 from AGENT-REFERENCE-LIBRARY.md.

use std::cell::RefCell;
use std::rc::Rc;

/// A reversible command that operates on state T.
pub trait Command<T> {
    fn apply(&self, state: &mut T);
    fn undo(&self, state: &mut T);
    fn description(&self) -> &str;
}

/// Manages undo/redo stacks for a shared state.
pub struct UndoManager<T> {
    undo_stack: Vec<Box<dyn Command<T>>>,
    redo_stack: Vec<Box<dyn Command<T>>>,
    pub state: Rc<RefCell<T>>,
}

impl<T> UndoManager<T> {
    pub fn new(state: Rc<RefCell<T>>) -> Self {
        UndoManager { undo_stack: Vec::new(), redo_stack: Vec::new(), state }
    }

    /// Execute a command: apply it, push to undo stack, clear redo.
    pub fn execute(&mut self, cmd: Box<dyn Command<T>>) {
        cmd.apply(&mut self.state.borrow_mut());
        self.undo_stack.push(cmd);
        self.redo_stack.clear();
    }

    pub fn undo(&mut self) -> bool {
        if let Some(cmd) = self.undo_stack.pop() {
            cmd.undo(&mut self.state.borrow_mut());
            self.redo_stack.push(cmd);
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(cmd) = self.redo_stack.pop() {
            cmd.apply(&mut self.state.borrow_mut());
            self.undo_stack.push(cmd);
            true
        } else {
            false
        }
    }

    pub fn can_undo(&self) -> bool { !self.undo_stack.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo_stack.is_empty() }
}
