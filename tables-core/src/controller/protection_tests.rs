// SPDX-License-Identifier: GPL-3.0-or-later

use super::ops::{apply_all, SheetProp};
use super::{Axis, BorderPreset, Op, WorkbookController, WorkbookImage};
use crate::fragment::{Fragment, GridCell};
use crate::sheet::{BorderStyle, PivotAggFunc, PivotDataField, PivotField, PivotTableSpec, SortDirection};

fn grid(values: &[&[&str]]) -> Fragment {
    Fragment::Grid(values.iter().map(|row| row.iter().map(|value| GridCell {
        value: (*value).into(), formula: None, num_format: None,
    }).collect()).collect())
}

fn select(controller: &WorkbookController, range: (usize, usize, usize, usize)) {
    let state = controller.state.borrow();
    let mut sheet = state.sheet_mut();
    (sheet.selected_row, sheet.selected_col, sheet.sel_end_row, sheet.sel_end_col) = range;
}

fn mixed_sheet(later: bool) -> WorkbookController {
    let mut controller = WorkbookController::new(8, 4).unwrap();
    if later {
        let mut state = controller.state.borrow_mut();
        state.add_sheet("Later".into(), 8, 4).unwrap();
        state.switch_sheet(1).unwrap();
    }
    controller.edit_cell(0, 0, "=10+2");
    controller.edit_cell(1, 0, "2");
    controller.edit_cell(2, 0, "1");
    controller.set_cell_locked(1, 0, false);
    controller.mutate_sheet("Source metadata", |sheet| {
        sheet.styles[0][0].bold = true;
        sheet.notes[2][0] = Some("Keep this note".into());
    });
    controller.set_sheet_protection(true, None);
    // Every rejection below must preserve a nonempty redo stack and a clean
    // dirty flag, not merely avoid adding a new undo description.
    controller.edit_cell(1, 0, "redo sentinel");
    assert!(controller.undo());
    controller.mark_clean();
    controller
}

fn assert_rejected(controller: &mut WorkbookController, operation: impl FnOnce(&mut WorkbookController)) {
    let before = WorkbookImage::of(&controller.state.borrow());
    let model_values: Vec<_> = controller.state.borrow().sheets.iter().map(|s| s.borrow().data.clone()).collect();
    let active = controller.state.borrow().active_sheet;
    let history = (controller.can_undo(), controller.can_redo(),
        controller.undo_description().map(str::to_owned), controller.redo_description().map(str::to_owned));
    let dirty = controller.is_dirty();
    let count = controller.protection_rejection_count();
    operation(controller);
    assert_eq!(controller.protection_rejection_count(), count + 1);
    assert_eq!(WorkbookImage::of(&controller.state.borrow()), before);
    assert_eq!(controller.state.borrow().active_sheet, active);
    for (sheet, values) in controller.state.borrow().sheets.iter().zip(model_values) {
        assert_eq!(sheet.borrow().data, values, "presentation values must agree with the untouched engine");
    }
    assert_eq!((controller.can_undo(), controller.can_redo(),
        controller.undo_description().map(str::to_owned), controller.redo_description().map(str::to_owned)), history);
    assert_eq!(controller.is_dirty(), dirty);
}

#[test]
fn mixed_paste_fill_and_clear_are_atomic_on_first_and_later_sheets() {
    for later in [false, true] {
        let mut controller = mixed_sheet(later);
        assert_rejected(&mut controller, |c| c.paste_fragment(1, 0, &grid(&[&["new"], &["1"]])));
        // The locked destination above already equals its pasted value: it
        // must not disappear from authorization when no-op cells are filtered.
        assert_rejected(&mut controller, |c| c.fill((0, 0, 0, 0), 2, 0));
        select(&controller, (1, 0, 2, 0));
        assert_rejected(&mut controller, |c| c.clear_selection());
        assert!(controller.redo(), "the rejected commands did not consume redo");
        assert_eq!(controller.state.borrow().sheet().cell(1, 0), "redo sentinel");
    }
}

#[test]
fn mixed_format_and_note_operations_are_atomic() {
    let mut controller = mixed_sheet(false);
    select(&controller, (1, 0, 2, 0));
    assert_rejected(&mut controller, |c| c.format_selection("Bold", |style| style.bold = true));
    assert_rejected(&mut controller, |c| c.border_selection(BorderPreset::All, BorderStyle::Solid, (0.0, 0.0, 0.0)));
    assert_rejected(&mut controller, |c| { c.set_note(2, 0, Some("overwrite".into())); });
    assert_rejected(&mut controller, |c| { c.mutate_sheet("Compound metadata", |sheet| {
        sheet.styles[1][0].italic = true;
        sheet.notes[2][0] = None;
    }); });
}

#[test]
fn protected_sort_structure_and_lock_changes_are_atomic() {
    let mut controller = mixed_sheet(true);
    assert_rejected(&mut controller, |c| { c.sort_column(0, SortDirection::Ascending); });
    for axis in [Axis::Rows, Axis::Cols] {
        assert_rejected(&mut controller, |c| c.insert_lines(axis, 1, 1));
        assert_rejected(&mut controller, |c| c.delete_lines(axis, 1, 1));
    }
    assert_rejected(&mut controller, |c| c.toggle_merge());
    assert_rejected(&mut controller, |c| { c.mutate_sheet("Freeze", |sheet| sheet.frozen_rows = 1); });
    assert_rejected(&mut controller, |c| c.set_cell_locked(2, 0, false));
    for axis in [Axis::Rows, Axis::Cols] {
        assert_rejected(&mut controller, |c| { assert!(!c.begin_resize(axis)); });
    }
    let old = controller.state.borrow().sheet().col_widths[0];
    assert_rejected(&mut controller, |c| {
        // Legacy drag previews may already have changed the model by commit.
        c.state.borrow().sheet_mut().col_widths[0] = old + 20.0;
        c.record_resize(Axis::Cols, 0, old);
    });
}

#[test]
fn whole_group_is_checked_before_unprotect_or_later_sheet_write() {
    let mut controller = mixed_sheet(true);
    controller.state.borrow_mut().switch_sheet(0).unwrap();
    let (first, later, unprotected) = {
        let state = controller.state.borrow();
        let later = state.sheets[1].borrow();
        let mut protection = later.protection.clone();
        protection.protected = false;
        let first = state.sheets[0].borrow().sheet_id;
        (first, later.sheet_id, protection)
    };
    assert_rejected(&mut controller, |c| {
        assert!(!c.apply_ops("Compound write", vec![
            Op::SetCell { sheet: first, row: 0, col: 0, input: "must not land".into() },
            Op::SetProp { sheet: later, prop: SheetProp::Protection(unprotected) },
            Op::SetCell { sheet: later, row: 2, col: 0, input: "locked".into() },
        ]));
    });
    assert_rejected(&mut controller, |c| {
        assert!(!c.apply_ops("Raw batch", vec![Op::SetCells {
            sheet: later,
            cells: vec![(1, 0, "unlocked first".into()), (2, 0, "locked last".into())],
        }]));
    });
    assert_rejected(&mut controller, |c| {
        let lines = super::blank_lines(&c.state.borrow(), 0, Axis::Rows, 1);
        assert!(!c.apply_ops("Restoration payload is not a user bypass", vec![Op::Insert {
            sheet: first, axis: Axis::Rows, at: 0,
            lines,
            relinks: vec![(later, 2, 0, "locked relink".into())], merges: None,
        }]));
    });
}

#[test]
fn unlocked_compound_edits_remain_available_and_undoable() {
    let mut controller = WorkbookController::new(4, 2).unwrap();
    controller.edit_cell(0, 0, "3");
    controller.mutate_sheet("Unlock inputs", |sheet| {
        for row in &mut sheet.cell_protections {
            for cell in row { cell.locked = false; }
        }
        // Sorting is its own permission (TBL-005); unlocked cells alone
        // do not grant it.
        sheet.protection.allow_sort = true;
    });
    controller.set_sheet_protection(true, None);
    let before = WorkbookImage::of(&controller.state.borrow());
    let count = controller.protection_rejection_count();
    controller.paste_fragment(1, 0, &grid(&[&["2"], &["1"]]));
    controller.fill((0, 0, 0, 0), 0, 1);
    select(&controller, (1, 0, 2, 0));
    controller.format_selection("Bold", |style| style.bold = true);
    controller.border_selection(BorderPreset::All, BorderStyle::Solid, (0.0, 0.0, 0.0));
    assert!(controller.set_note(1, 0, Some("Editable".into())));
    controller.sort_column(0, SortDirection::Ascending);
    assert_eq!(controller.state.borrow().sheet().cell(0, 0), "1");
    controller.clear_selection();
    assert_eq!(controller.state.borrow().sheet().cell(1, 0), "");
    assert_eq!(controller.protection_rejection_count(), count);
    for _ in 0..7 { assert!(controller.undo()); }
    assert_eq!(WorkbookImage::of(&controller.state.borrow()), before);
    for _ in 0..7 { assert!(controller.redo()); }
    assert!(controller.sheet_is_protected());
}

#[test]
fn pivot_rejection_keeps_registration_and_output_atomic_on_later_sheet() {
    let mut controller = mixed_sheet(true);
    controller.set_sheet_protection(false, None);
    controller.edit_cell(0, 0, "Region");
    controller.edit_cell(0, 1, "Sales");
    controller.edit_cell(1, 0, "North");
    controller.edit_cell(1, 1, "10");
    for (row, col) in [(4, 0), (4, 1), (5, 0)] { controller.set_cell_locked(row, col, false); }
    controller.mutate_sheet("Allow pivot tables", |sheet| sheet.protection.allow_pivot_tables = true);
    controller.set_sheet_protection(true, None);
    controller.mark_clean();
    let pivot = PivotTableSpec {
        name: "Protected summary".into(), source_range: (0, 0, 1, 1), target_cell: (4, 0),
        row_fields: vec![PivotField { col_index: 0, name: "Region".into() }], col_fields: vec![],
        data_fields: vec![PivotDataField { col_index: 1, name: "Sales".into(), func: PivotAggFunc::Sum }],
    };
    assert_rejected(&mut controller, |c| c.add_pivot_table(pivot.clone()));
    controller.set_sheet_protection(false, None);
    controller.set_cell_locked(5, 1, false);
    controller.set_sheet_protection(true, None);
    let before = WorkbookImage::of(&controller.state.borrow());
    controller.add_pivot_table(pivot);
    assert_eq!(controller.state.borrow().sheet().cell(5, 1), "10");
    assert_eq!(controller.state.borrow().sheet().pivot_tables.len(), 1);
    assert!(controller.undo());
    assert_eq!(WorkbookImage::of(&controller.state.borrow()), before);
    assert!(controller.redo());
    assert_eq!(controller.state.borrow().sheet().cell(5, 1), "10");
}

#[test]
fn trusted_replay_and_undo_restore_authorized_locked_cells() {
    let mut controller = WorkbookController::new(2, 2).unwrap();
    let (sheet, protected) = {
        let state = controller.state.borrow();
        let sheet = state.sheet();
        let mut protection = sheet.protection.clone();
        protection.protected = true;
        (sheet.sheet_id, protection)
    };
    // Authorized against the initially unprotected sheet. Its inverse writes
    // the cell before undoing Protection, so an executor-level guard is wrong.
    assert!(controller.apply_ops("Protect and initialize", vec![
        Op::SetProp { sheet, prop: SheetProp::Protection(protected) },
        Op::SetCell { sheet, row: 0, col: 0, input: "authorized".into() },
    ]));
    assert!(controller.sheet_is_protected());
    assert!(controller.undo());
    assert_eq!(controller.state.borrow().sheet().cell(0, 0), "");
    assert!(controller.redo());
    assert_eq!(controller.state.borrow().sheet().cell(0, 0), "authorized");
    let before = WorkbookImage::of(&controller.state.borrow());
    let inverse = apply_all(&mut controller.state.borrow_mut(), &[
        Op::SetCell { sheet, row: 0, col: 0, input: "recovered".into() },
    ]).unwrap();
    assert_eq!(controller.state.borrow().sheet().cell(0, 0), "recovered");
    apply_all(&mut controller.state.borrow_mut(), &inverse).unwrap();
    assert_eq!(WorkbookImage::of(&controller.state.borrow()), before);
    assert_eq!(controller.protection_rejection_count(), 0);
}

#[test]
fn rejection_feedback_does_not_leak_into_later_noops_or_unlocked_edits() {
    let mut controller = mixed_sheet(false);
    assert_rejected(&mut controller, |c| c.edit_cell(2, 0, "refused"));
    let count = controller.protection_rejection_count();
    controller.edit_cell(1, 0, "2"); // unchanged, returns before apply_ops
    controller.fill((1, 0, 1, 0), 1, 0); // no fill direction
    controller.paste_fragment(99, 99, &grid(&[&["outside"]]));
    assert_eq!(controller.protection_rejection_count(), count);
    controller.edit_cell(1, 0, "allowed");
    assert_eq!(controller.protection_rejection_count(), count);
    assert_eq!(controller.state.borrow().sheet().cell(1, 0), "allowed");
}

#[test]
fn modeled_format_permission_allows_formatting_but_not_locked_values() {
    let mut controller = mixed_sheet(false);
    controller.mutate_sheet("Allow formatting", |sheet| sheet.protection.allow_format_cells = true);
    select(&controller, (1, 0, 2, 0));
    let count = controller.protection_rejection_count();
    controller.format_selection("Bold", |style| style.bold = true);
    controller.border_selection(BorderPreset::All, BorderStyle::Solid, (0.0, 0.0, 0.0));
    assert!(controller.state.borrow().sheet().styles[2][0].bold);
    assert!(!controller.state.borrow().sheet().borders[2][0].is_none());
    assert_eq!(controller.protection_rejection_count(), count);
    assert_rejected(&mut controller, |c| c.edit_cell(2, 0, "still locked"));
    assert_rejected(&mut controller, |c| c.clear_selection());
    assert_rejected(&mut controller, |c| c.set_cell_locked(2, 0, false));
}

#[test]
fn modeled_row_permissions_are_independent_and_do_not_allow_columns() {
    let mut controller = mixed_sheet(true);
    // Row 2 holds only unlocked cells, so a delete permission can remove it.
    controller.set_sheet_protection(false, None);
    for col in 1..4 { controller.set_cell_locked(1, col, false); }
    controller.set_sheet_protection(true, None);
    controller.mutate_sheet("Allow row insertion", |sheet| sheet.protection.allow_insert_rows = true);
    let count = controller.protection_rejection_count();
    controller.insert_lines(Axis::Rows, 1, 1);
    assert_eq!(controller.state.borrow().sheet().rows, 9);
    assert_eq!(controller.state.borrow().sheet().cell(3, 0), "1");
    assert_eq!(controller.protection_rejection_count(), count);
    assert_rejected(&mut controller, |c| c.delete_lines(Axis::Rows, 2, 1));
    assert_rejected(&mut controller, |c| c.insert_lines(Axis::Cols, 1, 1));
    assert_rejected(&mut controller, |c| c.edit_cell(3, 0, "still locked"));
    controller.mutate_sheet("Allow only row deletion", |sheet| {
        sheet.protection.allow_insert_rows = false;
        sheet.protection.allow_delete_rows = true;
    });
    // The inserted row is locked, like Excel's and Calc's: the delete
    // permission does not bypass the locked-cell rule.
    assert_rejected(&mut controller, |c| c.delete_lines(Axis::Rows, 1, 1));
    let before = WorkbookImage::of(&controller.state.borrow());
    let count = controller.protection_rejection_count();
    controller.delete_lines(Axis::Rows, 2, 1);
    assert_eq!(controller.state.borrow().sheet().rows, 8);
    assert_eq!(controller.state.borrow().sheet().cell(2, 0), "1");
    assert_eq!(controller.protection_rejection_count(), count);
    assert_rejected(&mut controller, |c| c.insert_lines(Axis::Rows, 1, 1));
    assert_rejected(&mut controller, |c| c.delete_lines(Axis::Cols, 1, 1));
    // Deletion's inverse inserts rows despite the current insert permission.
    assert!(controller.undo());
    assert_eq!(WorkbookImage::of(&controller.state.borrow()), before);
    assert!(controller.redo());
    assert_eq!(controller.state.borrow().sheet().rows, 8);
}

#[test]
fn sheet_protection_does_not_become_workbook_or_print_protection() {
    let mut controller = mixed_sheet(true);
    // Filtering is the sheet's autoFilter permission (TBL-005).
    assert_rejected(&mut controller, |c| c.filter_by_value(0, "2"));
    controller.mutate_sheet("Allow filters", |sheet| sheet.protection.allow_autofilter = true);
    let count = controller.protection_rejection_count();
    controller.filter_by_value(0, "2");
    assert!(!controller.state.borrow().sheet().hidden_rows.is_empty());
    controller.clear_filter();
    controller.set_print_area((0, 0, 2, 1));
    assert_eq!(controller.state.borrow().sheet().print_area, Some((0, 0, 2, 1)));
    assert!(controller.rename_sheet(1, "Renamed"));
    assert!(controller.move_sheet(1, 0));
    assert!(controller.sheet_is_protected());
    assert!(controller.delete_sheet(0));
    assert_eq!(controller.state.borrow().sheets.len(), 1);
    assert!(controller.undo());
    assert_eq!(controller.state.borrow().sheets.len(), 2);
    assert!(controller.sheet_is_protected());
    assert_eq!(controller.protection_rejection_count(), count);
}

fn input_shown(controller: &WorkbookController, row: usize, col: usize) -> Option<String> {
    controller.state.borrow().presented_input(row, col)
}

fn copied(controller: &WorkbookController, range: (usize, usize, usize, usize)) -> Fragment {
    let state = controller.state.borrow();
    let sheet = state.sheet();
    crate::fragment::copy_range(&sheet, &state.engine, range.0, range.1, range.2, range.3)
}

/// TBL-005: a protected sheet's hidden formulas show their values, never
/// their text — in the formula bar's input, the clipboard (every format),
/// a fill from them, and a hidden spill anchor — while unprotecting brings
/// the input back and an unlocked hidden cell stays editable.
#[test]
fn tbl005_hidden_formula_all_surfaces() {
    let mut controller = WorkbookController::new(4, 2).unwrap();
    controller.edit_cell(0, 0, "=6*7");
    controller.edit_cell(0, 1, "=A1+1");
    controller.edit_cell(2, 0, "=SEQUENCE(2)");
    select(&controller, (0, 0, 2, 1));
    assert!(controller.set_selection_cell_protection(None, Some(true)));
    select(&controller, (0, 1, 1, 1));
    assert!(controller.set_selection_cell_protection(Some(false), None));
    assert_eq!(input_shown(&controller, 0, 0).as_deref(), Some("=6*7"), "an inactive hidden flag hides nothing");

    let mut protection = controller.state.borrow().sheet().protection.clone();
    protection.protected = true;
    assert!(controller.set_protection(protection));
    assert_eq!(controller.undo_description(), Some("Protect Sheet"));
    for (row, col) in [(0, 0), (0, 1), (2, 0)] {
        assert_eq!(input_shown(&controller, row, col), None, "{row},{col}: the formula bar withholds it");
    }
    assert_eq!(controller.state.borrow().displayed_value(0, 0), "42", "the value still shows");
    assert_eq!(input_shown(&controller, 3, 0).as_deref(), Some(""), "a spilled cell has no input to show");
    let fragment = copied(&controller, (0, 0, 3, 1));
    let Fragment::Grid(rows) = &fragment else { panic!("a grid") };
    assert!(rows.iter().flatten().all(|cell| cell.formula.is_none()), "{rows:?}");
    assert_eq!((rows[0][0].value.as_str(), rows[0][1].value.as_str()), ("42", "43"));
    for text in [fragment.to_json(), fragment.to_html(), fragment.to_plain()] {
        assert!(!text.contains("6*7") && !text.contains("A1+1") && !text.contains("SEQUENCE"), "{text}");
    }

    // A fill from the unlocked hidden cell would copy its formula below.
    controller.set_sheet_protection(false, None);
    controller.set_cell_locked(1, 1, false);
    controller.set_sheet_protection(true, None);
    assert_rejected(&mut controller, |c| c.fill((0, 1, 0, 1), 1, 1));
    // The bar's untouched empty text is no edit; typing replaces the cell.
    let history = controller.undo_description().map(str::to_owned);
    controller.commit_formula_bar(0, 1, "");
    assert_eq!(controller.state.borrow().cell_input(0, 1), "=A1+1");
    assert_eq!(controller.undo_description().map(str::to_owned), history);
    controller.commit_formula_bar(0, 1, "5");
    assert_eq!(controller.state.borrow().displayed_value(0, 1), "5");
    assert!(controller.undo());
    assert_eq!(controller.state.borrow().cell_input(0, 1), "=A1+1");

    controller.set_sheet_protection(false, None);
    assert_eq!(input_shown(&controller, 0, 0).as_deref(), Some("=6*7"));
    let Fragment::Grid(rows) = copied(&controller, (0, 0, 0, 0)) else { panic!("a grid") };
    assert_eq!(rows[0][0].formula.as_deref(), Some("6*7"));
}

/// TBL-005: the selection permissions decide which cells can be selected,
/// and a cell that cannot be selected cannot be edited, formatted, shown
/// or copied as input either — whichever path asks.
#[test]
fn tbl005_selection_permissions() {
    let mut controller = WorkbookController::new(4, 3).unwrap();
    controller.edit_cell(0, 0, "locked");
    controller.edit_cell(0, 1, "=1+1");
    controller.mutate_sheet("Unlock column B", |sheet| {
        for row in 0..4 { sheet.cell_protections[row][1].locked = false; }
    });
    let mut protection = controller.state.borrow().sheet().protection.clone();
    protection.protected = true;
    protection.allow_select_locked = false;
    protection.allow_format_cells = true;
    assert!(controller.set_protection(protection.clone()));
    {
        let state = controller.state.borrow();
        let mut sheet = state.sheet_mut();
        sheet.select_cell(0, 0);
        assert!(sheet.settle_selection(), "protection moves the selection off a locked cell");
        assert_eq!(sheet.selection_block(), (0, 1, 0, 1));
        assert!(!sheet.try_select_cell(1, 0));
        assert!(!sheet.try_select_block(0, 0, 1, 1), "the name box cannot select a locked cell");
    }
    assert!(controller.refuses_edit(0, 0) && !controller.refuses_edit(0, 1));
    assert_eq!(input_shown(&controller, 0, 0), None);
    assert_eq!(input_shown(&controller, 0, 1).as_deref(), Some("=1+1"));
    controller.paste_fragment(1, 1, &grid(&[&["x"], &["y"]]));
    assert_eq!(controller.state.borrow().sheet().cell(2, 1), "y");

    // Both options off: nothing can be selected, so nothing is edited,
    // formatted or shown, even through a stale selection.
    protection.allow_select_unlocked = false;
    assert!(controller.set_protection(protection));
    assert_eq!(controller.undo_description(), Some("Sheet Protection Settings"));
    {
        let state = controller.state.borrow();
        let mut sheet = state.sheet_mut();
        assert!(!sheet.settle_selection());
        assert!(!sheet.try_step_selection(1, 0, false));
    }
    assert!(controller.refuses_edit(0, 1));
    assert_eq!(input_shown(&controller, 0, 1), None);
    assert_rejected(&mut controller, |c| c.edit_cell(0, 1, "no"));
    select(&controller, (0, 1, 0, 1));
    assert_rejected(&mut controller, |c| c.format_selection("Bold", |s| s.bold = true));
    assert_rejected(&mut controller, |c| c.clear_selection());
    let Fragment::Grid(rows) = copied(&controller, (0, 1, 0, 1)) else { panic!("a grid") };
    assert_eq!((rows[0][0].value.as_str(), rows[0][0].formula.as_deref()), ("2", None));
    // Changing the settings back is one undo away.
    assert!(controller.undo());
    assert!(!controller.refuses_edit(0, 1));
}

/// TBL-005 step 3: each permission the format models governs its own
/// operations on a protected sheet, and none bypasses the locked-cell rule.
#[test]
fn tbl005_every_modeled_permission_governs_its_operations() {
    fn chart(sheet: &mut crate::sheet::SheetModel) {
        use crate::sheet::{ChartKind, ChartSeries, ChartSpec, LegendPosition};
        sheet.charts.push(ChartSpec {
            kind: ChartKind::Bar, title: "Protected chart".into(),
            x_axis_title: None, y_axis_title: None, legend_position: LegendPosition::Right,
            series: vec![ChartSeries { name: String::new(), cat: (0, 0, 1), val: (0, 1, 1), color: None }],
            cat: (0, 0, 1), val: (0, 1, 1), anchor: (0, 2),
            width_px: 480.0, height_px: 280.0,
        });
    }
    let mut controller = mixed_sheet(false);
    type Allow = fn(&mut crate::sheet::SheetProtection) -> &mut bool;
    type Operation = fn(&mut WorkbookController);
    let cases: [(&str, Allow, Operation); 7] = [
        ("insert columns", |p| &mut p.allow_insert_columns, |c| c.insert_lines(Axis::Cols, 1, 1)),
        ("hide rows", |p| &mut p.allow_format_rows, |c| {
            select(c, (3, 0, 3, 0));
            c.hide_selected_rows();
        }),
        ("hide columns", |p| &mut p.allow_format_columns, |c| {
            select(c, (0, 3, 0, 3));
            c.hide_selected_cols();
        }),
        ("column widths", |p| &mut p.allow_format_columns, |c| { c.begin_resize(Axis::Cols); }),
        ("row heights", |p| &mut p.allow_format_rows, |c| { c.begin_resize(Axis::Rows); }),
        ("filters", |p| &mut p.allow_autofilter, |c| c.filter_by_value(0, "2")),
        ("charts", |p| &mut p.allow_edit_objects, |c| { c.mutate_sheet("Add Chart", chart); }),
    ];
    for (name, allow, operation) in cases {
        controller.mutate_sheet("Disallow", move |sheet| *allow(&mut sheet.protection) = false);
        assert_rejected(&mut controller, operation);
        controller.mutate_sheet("Allow", move |sheet| *allow(&mut sheet.protection) = true);
        let count = controller.protection_rejection_count();
        operation(&mut controller);
        assert_eq!(controller.protection_rejection_count(), count, "{name} is allowed with its permission");
    }
    assert!(controller.state.borrow().sheet().is_row_hidden(3));
    assert_eq!(controller.state.borrow().sheet().charts.len(), 1);
    // Delete and sort permissions still refuse lines and bodies holding
    // locked cells.
    controller.mutate_sheet("Allow deletes and sorting", |sheet| {
        sheet.protection.allow_delete_columns = true;
        sheet.protection.allow_delete_rows = true;
        sheet.protection.allow_sort = true;
    });
    assert_rejected(&mut controller, |c| c.delete_lines(Axis::Cols, 0, 1));
    assert_rejected(&mut controller, |c| c.delete_lines(Axis::Rows, 0, 1));
    assert_rejected(&mut controller, |c| { c.sort_column(0, SortDirection::Ascending); });
}

/// RFC-0005 question 4: a viewer's workbook. Every user edit goes through
/// `apply_ops`, which refuses; undo and redo refuse; protection's feedback
/// counter is not touched; trusted replay (how a peer's update is applied)
/// still works, and the history comes back when the mode is lifted.
#[test]
fn a_read_only_workbook_refuses_user_edits_undo_and_redo_but_takes_replay() {
    let mut controller = WorkbookController::new(8, 4).unwrap();
    controller.edit_cell(0, 0, "1");
    controller.edit_cell(0, 1, "2");
    assert!(controller.undo());
    controller.mark_clean();
    let sheet = controller.state.borrow().sheet().sheet_id;
    assert!(!controller.is_read_only());

    controller.set_read_only(true);
    assert!(controller.is_read_only());
    let before = WorkbookImage::of(&controller.state.borrow());
    let count = controller.protection_rejection_count();
    controller.edit_cell(2, 0, "refused");
    controller.paste_fragment(3, 0, &grid(&[&["x"], &["y"]]));
    controller.fill((0, 0, 0, 0), 0, 1);
    controller.insert_lines(Axis::Rows, 0, 1);
    controller.clear_selection();
    controller.format_selection("Bold", |style| style.bold = true);
    assert!(!controller.add_sheet("Nope"));
    assert!(!controller.set_note(0, 0, Some("a note".into())));
    assert!(!controller.apply_ops("Raw", vec![Op::SetCell { sheet, row: 4, col: 0, input: "raw".into() }]));
    assert!(!controller.begin_resize(Axis::Cols), "a resize drag does not start");
    assert!(!controller.undo() && !controller.redo());
    assert!(!controller.can_undo() && !controller.can_redo(), "a viewer has nothing to undo");
    assert_eq!(WorkbookImage::of(&controller.state.borrow()), before);
    assert!(!controller.is_dirty(), "nothing was edited");
    assert_eq!(controller.protection_rejection_count(), count, "this is not protection's refusal");

    // What applying a peer's update does is trusted replay, not a user edit.
    apply_all(&mut controller.state.borrow_mut(), &[Op::SetCell { sheet, row: 5, col: 0, input: "peer".into() }]).unwrap();
    assert_eq!(controller.state.borrow().cell_input(5, 0), "peer");

    controller.set_read_only(false);
    assert!(controller.can_undo() && controller.can_redo(), "the history was kept, not cleared");
    controller.edit_cell(2, 1, "ok");
    assert_eq!(controller.state.borrow().cell_input(2, 1), "ok");
    assert_eq!(controller.undo_description(), Some("Edit Cell"));
}
