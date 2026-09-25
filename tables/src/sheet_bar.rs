// SPDX-License-Identifier: GPL-3.0-or-later
//! Keeping the sheet switcher in step with the workbook.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use tables_core::controller::WorkbookState;

/// Rebuild the sheet-name list from the live sheets, if it differs. Used
/// after any structural change (add, delete, rename, move, and their undo)
/// so the switcher never drifts from `WorkbookState::sheets`.
pub fn refresh_names(sm: &gtk4::StringList, state: &Rc<RefCell<WorkbookState>>) {
    let names: Vec<String> = state.borrow().sheets.iter().map(|sheet| sheet.borrow().name.clone()).collect();
    let shown: Vec<String> = (0..sm.n_items()).filter_map(|i| sm.string(i)).map(String::from).collect();
    if shown != names {
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        sm.splice(0, sm.n_items(), &refs);
    }
}

/// After undo or redo: the names as they now are, and the sheet the step
/// changed selected (the controller switched to it). Call with no borrow
/// of the controller held: `set_selected` fires the switcher's
/// selected-notify synchronously, and that handler borrows it.
pub fn follow(sm: &gtk4::StringList, sd: &gtk4::DropDown, state: &Rc<RefCell<WorkbookState>>) {
    // Read it first: replacing the names can move the switcher's selection,
    // and its handler switches the workbook to wherever that lands.
    let active = state.borrow().active_sheet as u32;
    refresh_names(sm, state);
    if sd.selected() != active {
        sd.set_selected(active);
    } else if state.borrow().active_sheet as u32 != active {
        let _ = state.borrow_mut().switch_sheet(active as usize);
    }
}
