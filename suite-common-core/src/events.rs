// events.rs — Generic observer/broadcaster pattern.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Pattern: LibreOffice svl/ SfxBroadcaster + SfxListener.
// Provides typed event broadcasting for undo state changes,
// selection changes, and other app-level notifications.

use std::cell::RefCell;
use std::rc::Rc;

/// A listener that receives hints of type H.
pub trait Listener<H> {
    fn on_event(&self, hint: &H);
}

/// Generic broadcaster: maintains a list of listeners and notifies them.
pub struct Broadcaster<H> {
    listeners: RefCell<Vec<Rc<dyn Listener<H>>>>,
}

impl<H> Broadcaster<H> {
    pub fn new() -> Self {
        Broadcaster { listeners: RefCell::new(Vec::new()) }
    }

    pub fn listen(&self, listener: Rc<dyn Listener<H>>) {
        self.listeners.borrow_mut().push(listener);
    }

    pub fn broadcast(&self, hint: H) {
        for listener in self.listeners.borrow().iter() {
            listener.on_event(&hint);
        }
    }
}

impl<H> Default for Broadcaster<H> {
    fn default() -> Self { Self::new() }
}

// ── Predefined hint types for suite-level events ──────────────────────

#[derive(Clone, Debug)]
pub enum Hint {
    UndoStateChanged { can_undo: bool, can_redo: bool },
    SlideSwitched { from: usize, to: usize },
    CellSelected { sheet: usize, row: usize, col: usize },
    DocumentModified { dirty: bool },
    ViewChanged,
    SelectionChanged,
    ObjectAdded { slide_idx: usize },
    ObjectRemoved { slide_idx: usize },
    SheetAdded { index: usize },
    SheetSwitched { from: usize, to: usize },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct FlagListener { called: Rc<Cell<bool>> }
    impl Listener<Hint> for FlagListener {
        fn on_event(&self, _hint: &Hint) {
            self.called.set(true);
        }
    }

    #[test]
    fn test_broadcast_reaches_listener() {
        let bc = Broadcaster::new();
        let flag = Rc::new(Cell::new(false));
        let listener: Rc<dyn Listener<Hint>> = Rc::new(FlagListener { called: flag.clone() });
        bc.listen(listener);
        bc.broadcast(Hint::DocumentModified { dirty: true });
        assert!(flag.get());
    }

    #[test]
    fn test_broadcast_no_listeners_does_not_panic() {
        let bc: Broadcaster<Hint> = Broadcaster::new();
        bc.broadcast(Hint::UndoStateChanged { can_undo: true, can_redo: false });
    }
}
