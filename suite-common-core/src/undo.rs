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
    pub broadcaster: Option<Rc<crate::events::Broadcaster<crate::events::Hint>>>,
}

impl<T> UndoManager<T> {
    pub fn new(state: Rc<RefCell<T>>) -> Self {
        UndoManager {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            state,
            broadcaster: None,
        }
    }

    /// Execute a command: apply it, push to undo stack, clear redo.
    pub fn execute(&mut self, cmd: Box<dyn Command<T>>) {
        cmd.apply(&mut self.state.borrow_mut());
        self.undo_stack.push(cmd);
        self.redo_stack.clear();
        self.notify();
    }

    pub fn undo(&mut self) -> bool {
        if let Some(cmd) = self.undo_stack.pop() {
            cmd.undo(&mut self.state.borrow_mut());
            self.redo_stack.push(cmd);
            self.notify();
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(cmd) = self.redo_stack.pop() {
            cmd.apply(&mut self.state.borrow_mut());
            self.undo_stack.push(cmd);
            self.notify();
            true
        } else {
            false
        }
    }

    fn notify(&self) {
        if let Some(ref bc) = self.broadcaster {
            bc.broadcast(crate::events::Hint::UndoStateChanged {
                can_undo: !self.undo_stack.is_empty(),
                can_redo: !self.redo_stack.is_empty(),
            });
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
    pub fn undo_description(&self) -> Option<&str> {
        self.undo_stack.last().map(|command| command.description())
    }
    pub fn redo_description(&self) -> Option<&str> {
        self.redo_stack.last().map(|command| command.description())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CounterCmd {
        delta: i32,
    }
    impl Command<i32> for CounterCmd {
        fn apply(&self, state: &mut i32) {
            *state += self.delta;
        }
        fn undo(&self, state: &mut i32) {
            *state -= self.delta;
        }
        fn description(&self) -> &str {
            "counter"
        }
    }

    #[test]
    fn test_execute_and_undo() {
        let state = Rc::new(RefCell::new(0i32));
        let mut mgr = UndoManager::new(state.clone());
        mgr.execute(Box::new(CounterCmd { delta: 5 }));
        assert_eq!(*state.borrow(), 5);
        assert!(mgr.can_undo());
        mgr.undo();
        assert_eq!(*state.borrow(), 0);
        assert!(!mgr.can_undo());
        assert!(mgr.can_redo());
    }

    #[test]
    fn test_redo() {
        let state = Rc::new(RefCell::new(0i32));
        let mut mgr = UndoManager::new(state.clone());
        mgr.execute(Box::new(CounterCmd { delta: 10 }));
        mgr.undo();
        mgr.redo();
        assert_eq!(*state.borrow(), 10);
    }

    #[test]
    fn test_redo_cleared_on_new_execute() {
        let state = Rc::new(RefCell::new(0i32));
        let mut mgr = UndoManager::new(state.clone());
        mgr.execute(Box::new(CounterCmd { delta: 1 }));
        mgr.undo();
        assert!(mgr.can_redo());
        mgr.execute(Box::new(CounterCmd { delta: 2 }));
        assert!(!mgr.can_redo());
        assert_eq!(*state.borrow(), 2);
    }

    #[test]
    fn test_multiple_undos() {
        let state = Rc::new(RefCell::new(0i32));
        let mut mgr = UndoManager::new(state.clone());
        mgr.execute(Box::new(CounterCmd { delta: 1 }));
        mgr.execute(Box::new(CounterCmd { delta: 2 }));
        mgr.execute(Box::new(CounterCmd { delta: 3 }));
        assert_eq!(*state.borrow(), 6);
        mgr.undo();
        assert_eq!(*state.borrow(), 3);
        mgr.undo();
        assert_eq!(*state.borrow(), 1);
        mgr.undo();
        assert_eq!(*state.borrow(), 0);
    }
}
