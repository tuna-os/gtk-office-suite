// sheet_actions.rs — insert and delete rows and columns, and sheet
// protection, as app actions (#1277).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The controller has had `insert_lines`/`delete_lines` and sheet
// protection for a long time, each an undo step, but nothing in the app
// reached them: no menu item, no action. And an edit to a locked cell on a
// protected sheet was dropped without a word. These are the actions, on
// the toolbar's extended section (the "More" menu when narrow), and the
// message a refused edit gets. Freezing rows is here too: frozen rows are
// the header a sort leaves in place, and the file formats already carried
// them, but the app could not set them.

use adw::prelude::*;
use gtk4::{self as gtk, gio};
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;
use tables_core::controller::{Axis, WorkbookController};

type Ctl = Rc<RefCell<WorkbookController>>;

/// The toolbar entries for the actions installed by [`install`].
/// The toolbar's text styling, always shown.
pub(crate) fn primary_toolbar() -> Vec<suite_common::ToolbarItem> {
    vec![
        ("format-text-bold-symbolic", "Bold (Ctrl+B)", "app.bold"),
        ("format-text-italic-symbolic", "Italic (Ctrl+I)", "app.italic"),
        ("format-text-underline-symbolic", "Underline (Ctrl+U)", "app.underline"),
    ]
}

pub(crate) fn toolbar_items() -> Vec<suite_common::ToolbarItem> {
    vec![
        ("office-row-insert-symbolic", "Insert rows above", "app.insert-rows"),
        ("office-column-insert-symbolic", "Insert columns left", "app.insert-cols"),
        ("office-row-delete-symbolic", "Delete selected rows", "app.delete-rows"),
        ("office-column-delete-symbolic", "Delete selected columns", "app.delete-cols"),
        ("view-pin-symbolic", "Freeze rows above the selection", "app.toggle-freeze-rows"),
        ("changes-prevent-symbolic", "Protect sheet", "app.toggle-sheet-protection"),
    ]
}

fn toast(toasts: &adw::ToastOverlay, text: &str) {
    let t = adw::Toast::new(text);
    t.set_timeout(3);
    toasts.add_toast(t);
}

/// What a refused edit says: the edit is not lost silently.
const PROTECTED: &str = "This sheet is protected. Unprotect it to make changes.";

/// When an edit to (`row`, `col`) would be refused because the sheet is
/// protected, say so and return true; the caller then skips the edit.
pub(crate) fn refuse_locked(ctl: &Ctl, toasts: &adw::ToastOverlay, row: usize, col: usize) -> bool {
    let refused = ctl.borrow().refuses_edit(row, col);
    if refused {
        toast(toasts, PROTECTED);
    }
    refused
}

pub(crate) fn install(app: &impl IsA<gio::ActionMap>, ctl: &Ctl, grid: &gtk::DrawingArea, toasts: &adw::ToastOverlay, refresh: Rc<dyn Fn()>) {
    // Bold, Italic and Underline toggle on the selection, from the active
    // cell's style, as in every other spreadsheet. Tables had no such
    // actions: Ctrl+B did nothing, and the toolbar had no text styling.
    type Flag = fn(&mut tables_core::style::CellStyle) -> &mut bool;
    let flags: [(&str, &'static str, Flag); 3] = [
        ("bold", "Bold", |s| &mut s.bold),
        ("italic", "Italic", |s| &mut s.italic),
        ("underline", "Underline", |s| &mut s.underline),
    ];
    for (name, description, field) in flags {
        let (ctl, grid, toasts, refresh) = (ctl.clone(), grid.clone(), toasts.clone(), refresh.clone());
        let act = gio::SimpleAction::new(name, None);
        act.connect_activate(move |_, _| {
            if ctl.borrow().sheet_is_protected() {
                toast(&toasts, PROTECTED);
                return;
            }
            let (mut style, _) = ctl.borrow().active_style();
            let on = !*field(&mut style);
            ctl.borrow_mut().format_selection(description, move |s| *field(s) = on);
            grid.queue_draw();
            refresh();
        });
        app.add_action(&act);
        suite_common::actions::register_labels(&[(&format!("app.{name}"), &suite_common::i18n(description))]);
    }
    // (name, axis, insert?) — inserts go before the selection's first line,
    // as many lines as the selection spans; deletes remove the spanned lines.
    for (name, axis, insert) in [
        ("insert-rows", Axis::Rows, true),
        ("insert-cols", Axis::Cols, true),
        ("delete-rows", Axis::Rows, false),
        ("delete-cols", Axis::Cols, false),
    ] {
        let (ctl, grid, toasts, refresh) = (ctl.clone(), grid.clone(), toasts.clone(), refresh.clone());
        let act = gio::SimpleAction::new(name, None);
        act.connect_activate(move |_, _| {
            if ctl.borrow().sheet_is_protected() {
                toast(&toasts, PROTECTED);
                return;
            }
            let (top, left, bottom, right) = ctl.borrow().state.borrow().sheet().selection_rect();
            let (at, count) = match axis {
                Axis::Rows => (top, bottom - top + 1),
                Axis::Cols => (left, right - left + 1),
            };
            if insert {
                ctl.borrow_mut().insert_lines(axis, at, count);
            } else {
                ctl.borrow_mut().delete_lines(axis, at, count);
            }
            refresh();
            grid.queue_draw();
        });
        app.add_action(&act);
    }

    {
        let (ctl, grid, toasts, refresh) = (ctl.clone(), grid.clone(), toasts.clone(), refresh.clone());
        let act = gio::SimpleAction::new("toggle-freeze-rows", None);
        act.connect_activate(move |_, _| {
            // Freeze the rows above the selection (the top row when the
            // selection starts there); unfreeze when rows are frozen.
            let (frozen, top) = {
                let c = ctl.borrow();
                let state = c.state.borrow();
                let sheet = state.sheet();
                (sheet.frozen_rows, sheet.selection_rect().0)
            };
            let rows = if frozen > 0 { 0 } else { top.max(1) };
            ctl.borrow_mut().mutate_sheet("Freeze Rows", move |sheet| sheet.frozen_rows = rows);
            toast(&toasts, if rows > 0 { "Rows frozen" } else { "Rows unfrozen" });
            refresh();
            grid.queue_draw();
        });
        app.add_action(&act);
    }

    let (ctl, grid, toasts) = (ctl.clone(), grid.clone(), toasts.clone());
    let act = gio::SimpleAction::new("toggle-sheet-protection", None);
    act.connect_activate(move |_, _| {
        let protect = !ctl.borrow().sheet_is_protected();
        ctl.borrow_mut().set_sheet_protection(protect, None);
        toast(&toasts, if protect { "Sheet protected" } else { "Sheet unprotected" });
        grid.queue_draw();
    });
    app.add_action(&act);
}
