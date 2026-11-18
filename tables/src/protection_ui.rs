// protection_ui.rs — sheet protection in the app (TBL-005).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The Protect Sheet toggle, the Protection Settings dialog, the cell
// Locked/Hide Formula commands, and what the formula bar shows for a cell
// whose input protection withholds. Every rule lives in tables-core
// (`protection.rs`, `controller/protection.rs`); this only wires GTK: each
// change is one controller operation, refusals use the shared native
// notice, and focus returns to the grid.
//
// Protection restricts editing in spreadsheet apps. It is not a security
// boundary and does not encrypt the file — the dialog says so.

use adw::prelude::*;
use gtk4::{self as gtk, gio};
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;
use tables_core::controller::{WorkbookController, WorkbookState};
use tables_core::sheet::SheetProtection;

type Ctl = Rc<RefCell<WorkbookController>>;

/// What the formula bar says when it is empty for an ordinary cell.
const PLACEHOLDER: &str = "Formula or value\u{2026}";
/// What it says, visibly and to screen readers, for a withheld input.
const WITHHELD: &str = "Contents hidden \u{2014} the sheet is protected";
/// A refused selection (mouse, keys or name box).
pub(crate) const UNSELECTABLE: &str = "This sheet's protection doesn't allow selecting that cell.";

/// Post `text` on the window's toast overlay above `widget`.
pub(crate) fn notify(widget: &impl IsA<gtk::Widget>, text: &str) {
    suite_common::toast_manager::show_near(widget, &suite_common::i18n(text));
}

/// Show (`row`, `col`)'s input in the formula bar — or nothing, with a
/// placeholder and accessible description saying why, when protection
/// withholds it. The state borrow ends before `set_text`, whose `changed`
/// handler re-borrows it.
pub(crate) fn show_input(fx: &gtk::Entry, state: &Rc<RefCell<WorkbookState>>, row: usize, col: usize) {
    let shown = state.borrow().presented_input(row, col);
    match shown {
        Some(text) => {
            if fx.text().as_str() != text {
                fx.set_text(&text);
            }
            fx.set_placeholder_text(Some(&suite_common::i18n(PLACEHOLDER)));
            fx.reset_property(gtk::AccessibleProperty::Description);
        }
        None => {
            if !fx.text().is_empty() {
                fx.set_text("");
            }
            let withheld = suite_common::i18n(WITHHELD);
            fx.set_placeholder_text(Some(&withheld));
            fx.update_property(&[gtk::accessible::Property::Description(&withheld)]);
        }
    }
}

/// Keep the formula bar on the active cell while it isn't being edited:
/// after a jump, undo, or a protection change, it must neither show a
/// formula that is now hidden nor stale text from another cell. Also
/// moves a selection that protection no longer allows (`settle_selection`).
pub(crate) fn sync(fx: &gtk::Entry, state: &Rc<RefCell<WorkbookState>>) {
    let cell = {
        let Ok(st) = state.try_borrow() else { return };
        let Ok(mut sheet) = st.sheets[st.active_sheet].try_borrow_mut() else { return };
        sheet.settle_selection();
        (sheet.selected_row, sheet.selected_col)
    };
    if fx.state_flags().contains(gtk::StateFlags::FOCUS_WITHIN) {
        return;
    }
    // Only a withheld input, or a bar still saying one is withheld, needs
    // the bar changed here; an ordinary cell's bar is the editing paths'.
    let withheld = state.try_borrow().is_ok_and(|st| st.presented_input(cell.0, cell.1).is_none());
    let showing_withheld = fx.placeholder_text().is_some_and(|p| p.as_str() == suite_common::i18n(WITHHELD));
    if withheld || showing_withheld {
        show_input(fx, state, cell.0, cell.1);
    }
}

/// A permission's switch in the settings dialog.
struct Permission {
    label: &'static str,
    note: Option<&'static str>,
    field: fn(&mut SheetProtection) -> &mut bool,
}

const PERMISSIONS: [Permission; 15] = [
    Permission { label: "Select locked cells", note: None, field: |p| &mut p.allow_select_locked },
    Permission { label: "Select unlocked cells", note: None, field: |p| &mut p.allow_select_unlocked },
    Permission { label: "Format cells", note: None, field: |p| &mut p.allow_format_cells },
    Permission { label: "Format columns", note: Some("Widths and hiding"), field: |p| &mut p.allow_format_columns },
    Permission { label: "Format rows", note: Some("Heights and hiding"), field: |p| &mut p.allow_format_rows },
    Permission { label: "Insert columns", note: None, field: |p| &mut p.allow_insert_columns },
    Permission { label: "Insert rows", note: None, field: |p| &mut p.allow_insert_rows },
    Permission { label: "Delete columns", note: Some("Only columns with no locked cells"), field: |p| &mut p.allow_delete_columns },
    Permission { label: "Delete rows", note: Some("Only rows with no locked cells"), field: |p| &mut p.allow_delete_rows },
    Permission { label: "Sort", note: Some("Only unlocked cells"), field: |p| &mut p.allow_sort },
    Permission { label: "Use filters", note: None, field: |p| &mut p.allow_autofilter },
    Permission { label: "Use pivot tables", note: None, field: |p| &mut p.allow_pivot_tables },
    Permission { label: "Edit charts and objects", note: None, field: |p| &mut p.allow_edit_objects },
    Permission { label: "Insert hyperlinks", note: Some("Kept for other spreadsheet apps"), field: |p| &mut p.allow_insert_hyperlinks },
    Permission { label: "Edit scenarios", note: Some("Kept for other spreadsheet apps"), field: |p| &mut p.allow_edit_scenarios },
];

/// Apply `protection` to the active sheet as one operation, then refresh
/// and hand focus back to the grid.
fn set(ctl: &Ctl, grid: &gtk::DrawingArea, refresh: &Rc<dyn Fn()>, protection: SheetProtection) -> bool {
    let changed = ctl.borrow_mut().set_protection(protection);
    refresh();
    grid.queue_draw();
    grid.grab_focus();
    changed
}

/// The Protection Settings dialog: whether the sheet is protected and what
/// it allows, applied together as one undo step. Cancel changes nothing.
fn show_settings(ctl: &Ctl, grid: &gtk::DrawingArea, refresh: &Rc<dyn Fn()>) {
    let current = ctl.borrow().state.borrow().sheet().protection.clone();
    let notice = gtk::Label::new(Some(&suite_common::i18n(
        "Protection stops changes to locked cells in spreadsheet apps. It is not a security \
         measure and does not encrypt the file.",
    )));
    notice.set_wrap(true);
    notice.set_xalign(0.0);
    let protect = gtk::CheckButton::with_label(&suite_common::i18n("Protect this sheet"));
    // Opening the dialog is asking to protect, as in Excel and Calc:
    // unticking it is how the dialog unprotects.
    protect.set_active(true);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("boxed-list");
    let mut checks = Vec::new();
    for permission in &PERMISSIONS {
        let mut probe = current.clone();
        let check = gtk::CheckButton::with_label(&suite_common::i18n(permission.label));
        check.set_active(*(permission.field)(&mut probe));
        if let Some(note) = permission.note {
            check.set_tooltip_text(Some(&suite_common::i18n(note)));
            check.update_property(&[gtk::accessible::Property::Description(&suite_common::i18n(note))]);
        }
        list.append(&check);
        checks.push(check);
    }
    let heading = gtk::Label::new(Some(&suite_common::i18n("While protected, allow:")));
    heading.set_xalign(0.0);
    heading.add_css_class("heading");
    // The protect switch comes first, as the dialog's subject.
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(12);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.append(&notice);
    content.append(&protect);
    content.append(&heading);
    content.append(&list);
    let scroller = gtk::ScrolledWindow::builder()
        .child(&content)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(480)
        .build();
    let suite_common::dialogs::ActionDialog { dialog, action } = suite_common::dialogs::action_dialog(
        &suite_common::i18n("Sheet Protection"),
        &suite_common::i18n("_Apply"),
        420,
        &scroller,
    );
    {
        let grid = grid.clone();
        dialog.connect_closed(move |_| {
            grid.grab_focus();
        });
    }
    {
        let (ctl, grid, refresh, dialog) = (ctl.clone(), grid.clone(), refresh.clone(), dialog.clone());
        action.connect_clicked(move |_| {
            let mut protection = current.clone();
            protection.protected = protect.is_active();
            for (permission, check) in PERMISSIONS.iter().zip(&checks) {
                *(permission.field)(&mut protection) = check.is_active();
            }
            dialog.close();
            set(&ctl, &grid, &refresh, protection);
        });
    }
    dialog.present(grid.root().as_ref());
}

/// Toggle the active cell's lock (or, with `hidden`, its hidden-formula
/// flag) on the whole selection; a protected sheet refuses with the shared
/// notice.
fn toggle_cells(ctl: &Ctl, grid: &gtk::DrawingArea, refresh: &Rc<dyn Fn()>, hidden: bool) {
    let current = ctl.borrow().active_cell_protection();
    let accepted = crate::sheet_actions::apply(ctl, grid, |c| {
        if hidden {
            c.set_selection_cell_protection(None, Some(!current.hidden_formula));
        } else {
            c.set_selection_cell_protection(Some(!current.locked), None);
        }
    });
    if accepted {
        refresh();
        grid.queue_draw();
    }
}

pub(crate) fn install(app: &impl IsA<gio::ActionMap>, ctl: &Ctl, grid: &gtk::DrawingArea, refresh: &Rc<dyn Fn()>) {
    {
        let (ctl, grid, refresh) = (ctl.clone(), grid.clone(), refresh.clone());
        let act = gio::SimpleAction::new("toggle-sheet-protection", None);
        act.connect_activate(move |_, _| {
            let mut protection = ctl.borrow().state.borrow().sheet().protection.clone();
            protection.protected = !protection.protected;
            let protect = protection.protected;
            if set(&ctl, &grid, &refresh, protection) {
                notify(&grid, if protect { "Sheet protected" } else { "Sheet unprotected" });
            }
        });
        app.add_action(&act);
    }
    {
        let (ctl, grid, refresh) = (ctl.clone(), grid.clone(), refresh.clone());
        let act = gio::SimpleAction::new("protection-settings", None);
        act.connect_activate(move |_, _| show_settings(&ctl, &grid, &refresh));
        app.add_action(&act);
    }
    for (name, hidden) in [("toggle-cell-locked", false), ("toggle-formula-hidden", true)] {
        let (ctl, grid, refresh) = (ctl.clone(), grid.clone(), refresh.clone());
        let act = gio::SimpleAction::new(name, None);
        act.connect_activate(move |_, _| toggle_cells(&ctl, &grid, &refresh, hidden));
        app.add_action(&act);
    }
    suite_common::actions::register_labels(&[
        ("app.toggle-sheet-protection", &suite_common::i18n("Protect Sheet")),
        ("app.protection-settings", &suite_common::i18n("Sheet Protection Settings…")),
        ("app.toggle-cell-locked", &suite_common::i18n("Lock or Unlock Cells")),
        ("app.toggle-formula-hidden", &suite_common::i18n("Hide or Show Formulas")),
    ]);
}

/// The Format inspector's Protection group: the active cell's Locked and
/// Hide Formula switches, each one controller operation on the selection.
/// Returns the group and its sync.
pub(crate) fn inspector_group(ctl: &Ctl, grid: &gtk::DrawingArea, syncing: &Rc<std::cell::Cell<bool>>) -> (adw::PreferencesGroup, Rc<dyn Fn()>) {
    let group = adw::PreferencesGroup::builder()
        .title(suite_common::i18n("Protection"))
        .description(suite_common::i18n("Takes effect when the sheet is protected."))
        .build();
    let locked = adw::SwitchRow::builder().title(suite_common::i18n("Locked")).build();
    let hidden = adw::SwitchRow::builder().title(suite_common::i18n("Hide Formula")).build();
    group.add(&locked);
    group.add(&hidden);
    let sync: Rc<dyn Fn()> = {
        let (ctl, syncing, locked, hidden) = (ctl.clone(), syncing.clone(), locked.clone(), hidden.clone());
        Rc::new(move || {
            let Ok(c) = ctl.try_borrow() else { return };
            let cell = c.active_cell_protection();
            drop(c);
            let was = syncing.replace(true);
            locked.set_active(cell.locked);
            hidden.set_active(cell.hidden_formula);
            syncing.set(was);
        })
    };
    for (row, is_hidden) in [(&locked, false), (&hidden, true)] {
        let (ctl, grid, syncing, sync) = (ctl.clone(), grid.clone(), syncing.clone(), sync.clone());
        row.connect_active_notify(move |r| {
            if syncing.get() {
                return;
            }
            let on = r.is_active();
            crate::sheet_actions::apply(&ctl, &grid, |c| {
                if is_hidden {
                    c.set_selection_cell_protection(None, Some(on));
                } else {
                    c.set_selection_cell_protection(Some(on), None);
                }
            });
            // A refusal leaves the cells as they were: show that.
            sync();
            grid.queue_draw();
        });
    }
    (group, sync)
}
