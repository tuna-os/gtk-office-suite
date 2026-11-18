// SPDX-License-Identifier: GPL-3.0-or-later
//! The Insert Pivot Table dialog (#1657): pick the row, column and value
//! fields of the selected table, how to summarize them and where the pivot
//! goes, and the controller writes it as one undo step. Refresh Pivot Tables
//! rewrites the sheet's pivots from their sources as they are now.
//!
//! Only the picking lives here. The fields, the spec, the placement check
//! and the summary are `tables_core` (`PivotTableSpec::source_fields`,
//! `from_choice`, `WorkbookController::add_pivot_table` and
//! `refresh_pivot_tables`), so no grouping or arithmetic is done in GTK code.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::{self as gtk, prelude::*};
use libadwaita as adw;
use adw::prelude::*;

use tables_core::controller::{WorkbookController, WorkbookState};
use tables_core::sheet::{col_label, parse_cell_ref, PivotAggFunc, PivotTableSpec};

/// Returns the callback the `insert-pivot-table` action runs.
///
/// `parent` is filled in once the window exists, as for the chart dialog.
pub fn opener(
    parent: Rc<RefCell<Option<adw::ApplicationWindow>>>,
    state: Rc<RefCell<WorkbookState>>,
    controller: Rc<RefCell<WorkbookController>>,
    grid: gtk::DrawingArea,
) -> Box<dyn Fn()> {
    Box::new(move || {
        let (range, fields) = {
            let st = state.borrow();
            let sheet = st.sheets[st.active_sheet].borrow();
            let range = sheet.selection_block();
            (range, PivotTableSpec::source_fields(&sheet, range))
        };
        if fields.is_empty() {
            toast(&grid, &suite_common::i18n("Select a table with a header row and at least one row of data to make a pivot table from it"));
            return;
        }

        let names: Vec<&str> = fields.iter().map(String::as_str).collect();
        let combo = |title: &str, items: &[&str], selected: u32| {
            let row = adw::ComboRow::builder()
                .title(suite_common::i18n(title))
                .model(&gtk::StringList::new(items))
                .build();
            row.set_selected(selected);
            row
        };
        // Columns: none, or one field. Values: the last field, which in a
        // table of labels then figures is usually the figure.
        let mut column_items = vec![suite_common::i18n("(None)")];
        column_items.extend(fields.iter().cloned());
        let column_refs: Vec<&str> = column_items.iter().map(String::as_str).collect();
        let last = (fields.len() - 1) as u32;
        let row_field = combo("Rows", &names, 0);
        let column_field = combo("Columns", &column_refs, 0);
        let value_field = combo("Values", &names, last);
        let funcs: Vec<String> = PivotAggFunc::ALL.iter().map(|f| suite_common::i18n(f.label())).collect();
        let func_refs: Vec<&str> = funcs.iter().map(String::as_str).collect();
        let func_field = combo("Summarize By", &func_refs, 0);
        let (tr, tc) = PivotTableSpec::default_target(range);
        let place_at = adw::EntryRow::builder()
            .title(suite_common::i18n("Place At"))
            .text(format!("{}{}", col_label(tc), tr + 1))
            .build();

        let group = adw::PreferencesGroup::builder()
            .title(suite_common::i18n("Fields"))
            .description(suite_common::i18n(
                "Place At is the pivot table's top-left cell, outside the selection. Refresh Pivot Tables updates it after the selection's data changes. Its results are saved; its layout is not, so it is not kept as a pivot table when the workbook is saved.",
            ))
            .build();
        for row in [&row_field, &column_field, &value_field, &func_field] {
            group.add(row);
        }
        group.add(&place_at);

        let suite_common::dialogs::ActionDialog { dialog, action: insert_btn } = suite_common::dialogs::action_dialog(
            &suite_common::i18n("Insert Pivot Table"),
            &suite_common::i18n("_Insert"),
            480,
            &suite_common::dialogs::form_body(&[group.upcast_ref()]),
        );
        {
            let (state, ctl, grid, dlg) = (state.clone(), controller.clone(), grid.clone(), dialog.clone());
            insert_btn.connect_clicked(move |_| {
                let column = match column_field.selected() {
                    0 => None,
                    i => Some(i as usize - 1),
                };
                let func = PivotAggFunc::ALL[func_field.selected() as usize % PivotAggFunc::ALL.len()];
                let Some(target) = parse_cell_ref(&place_at.text()) else {
                    toast(&grid, &suite_common::i18n("Place At needs a cell reference, such as D1"));
                    return;
                };
                let spec = {
                    let st = state.borrow();
                    let sheet = st.sheets[st.active_sheet].borrow();
                    PivotTableSpec::from_choice(
                        &sheet,
                        range,
                        row_field.selected() as usize,
                        column,
                        value_field.selected() as usize,
                        func,
                        Some(target),
                    )
                };
                let Some(spec) = spec else {
                    toast(&grid, &suite_common::i18n("Place the pivot table outside the selection, which it would overwrite"));
                    return;
                };
                crate::sheet_actions::apply(&ctl, &grid, |c| c.add_pivot_table(spec));
                dlg.close();
            });
        }
        let pw = parent.borrow().clone();
        dialog.present(pw.as_ref());
    })
}

/// Returns the callback the `refresh-pivot-tables` action runs: the
/// controller rewrites the sheet's pivots from their sources, as one undo
/// step, and a sheet with none says so.
pub fn refresher(controller: Rc<RefCell<WorkbookController>>, grid: gtk::DrawingArea) -> Box<dyn Fn()> {
    Box::new(move || {
        if controller.borrow().state.borrow().sheet().pivot_tables.is_empty() {
            toast(&grid, &suite_common::i18n("This sheet has no pivot tables to refresh"));
            return;
        }
        crate::sheet_actions::apply(&controller, &grid, |c| {
            c.refresh_pivot_tables();
        });
    })
}

/// A toast in the window's overlay, as the controller's refusals use.
fn toast(grid: &gtk::DrawingArea, text: &str) {
    suite_common::toast_manager::show_near(grid, text);
}
