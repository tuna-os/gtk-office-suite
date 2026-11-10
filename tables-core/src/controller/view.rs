// SPDX-License-Identifier: GPL-3.0-or-later
//! View-level commands: protection, named ranges, filters, row/col visibility,
//! print setup, and sorting.
//! Split out of the former controller.rs (issue #227).

use std::collections::HashSet;

use crate::sheet::{col_label, FilterColumn, FilterDef, SheetModel, SortDirection};

use super::ops::{Op, SheetProp};

use super::core::WorkbookController;

impl WorkbookController {
    /// Set one sheet-wide property of sheet `sheet_id` as an undo step.
    fn set_prop(&mut self, description: &'static str, sheet_id: u32, prop: SheetProp) {
        self.apply_ops(description, vec![Op::SetProp { sheet: sheet_id, prop }]);
    }

    pub fn set_sheet_protection(&mut self, protected: bool, password_hash: Option<String>) {
        self.mutate_sheet("Toggle Sheet Protection", move |sheet| {
            sheet.protection.protected = protected;
            sheet.protection.password_hash = password_hash;
        });
    }

    pub fn set_cell_locked(&mut self, row: usize, col: usize, locked: bool) {
        self.mutate_sheet("Toggle Cell Lock", move |sheet| {
            sheet.cell_protections[row][col].locked = locked;
        });
    }

    /// Replace the active sheet's protection — whether it is protected and
    /// every permission — as one undo step. False when nothing changes.
    /// Changing protection is itself always authorized: unprotecting is an
    /// explicit operation (TBL-005), and undo restores the previous setting.
    pub fn set_protection(&mut self, protection: crate::sheet::SheetProtection) -> bool {
        let (sheet_id, before) = {
            let state = self.state.borrow();
            let sheet = state.sheet();
            (sheet.sheet_id, sheet.protection.clone())
        };
        if before == protection {
            return false;
        }
        let description = match (before.protected, protection.protected) {
            (false, true) => "Protect Sheet",
            (true, false) => "Unprotect Sheet",
            _ => "Sheet Protection Settings",
        };
        self.apply_ops(description, vec![Op::SetProp { sheet: sheet_id, prop: SheetProp::Protection(protection) }])
    }

    /// The active cell's lock and hidden-formula flags.
    pub fn active_cell_protection(&self) -> crate::sheet::CellProtection {
        let state = self.state.borrow();
        let sheet = state.sheet();
        sheet.cell_protections.get(sheet.selected_row).and_then(|r| r.get(sheet.selected_col)).cloned().unwrap_or_default()
    }

    /// Set the selected cells' `locked` and/or `hidden_formula` flags as one
    /// undo step. A protected sheet refuses it as a whole: locks change only
    /// while the sheet is unprotected. False when nothing changed.
    pub fn set_selection_cell_protection(&mut self, locked: Option<bool>, hidden_formula: Option<bool>) -> bool {
        let ops = {
            let state = self.state.borrow();
            let sheet = state.sheet();
            let (top, left, bottom, right) = sheet.selection_block();
            let mut ops = Vec::new();
            for row in top..=bottom.min(sheet.rows.saturating_sub(1)) {
                for col in left..=right.min(sheet.cols.saturating_sub(1)) {
                    let mut protection = sheet.cell_protections[row][col].clone();
                    protection.locked = locked.unwrap_or(protection.locked);
                    protection.hidden_formula = hidden_formula.unwrap_or(protection.hidden_formula);
                    if protection != sheet.cell_protections[row][col] {
                        ops.push(Op::SetLocked { sheet: sheet.sheet_id, row, col, protection });
                    }
                }
            }
            ops
        };
        if ops.is_empty() {
            return false;
        }
        let description = match (locked, hidden_formula) {
            (Some(true), _) => "Lock Cells",
            (Some(false), _) => "Unlock Cells",
            (None, Some(true)) => "Hide Formulas",
            (None, Some(false)) | (None, None) => "Show Formulas",
        };
        self.apply_ops(description, ops)
    }

    /// The reference a name for `sel` stands for, on the active sheet.
    fn name_formula(&self, sel: (usize, usize, usize, usize)) -> String {
        let (top, left, bottom, right) = sel;
        let sheet_name = self.state.borrow().sheet().name.clone();
        if top == bottom && left == right {
            format!("{}!${}${}", sheet_name, col_label(left), top + 1)
        } else {
            format!("{}!${}${}:${}${}", sheet_name, col_label(left), top + 1, col_label(right), bottom + 1)
        }
    }

    /// Why `name` can't name `sel`, if it can't (not a valid identifier,
    /// or already taken); nothing changes. Define Name offers its action
    /// only while this is `Ok`.
    pub fn check_name(&self, name: &str, sel: (usize, usize, usize, usize)) -> Result<(), String> {
        let formula = self.name_formula(sel);
        // is_valid_defined_name takes &mut self (it's read-only in effect,
        // but the upstream signature requires it).
        self.state.borrow_mut().engine.model.is_valid_defined_name(name, None, &formula).map(|_| ())
    }

    pub fn define_name(
        &mut self,
        name: &str,
        sel: (usize, usize, usize, usize),
    ) -> Result<(), String> {
        // Validate before creating an undo entry — an invalid name/formula
        // must never reach the undo stack.
        self.check_name(name, sel)?;
        let formula = self.name_formula(sel);
        self.apply_ops("Define Name", vec![Op::DefineName { name: name.to_string(), formula: Some(formula) }]);
        Ok(())
    }

    /// The paired ops that install `def` with its derived rows: one undo
    /// step restoring definition and visibility together. The `Filter`
    /// swap touches only the definition and `Filtered` only the rows, so
    /// the pair is exact whatever order the group applies in.
    fn filter_ops(sheet_id: u32, def: Option<FilterDef>, sheet: &SheetModel) -> Vec<Op> {
        let hidden = def.as_ref().map(|d| d.derive_hidden(sheet)).unwrap_or_default();
        vec![
            Op::SetProp { sheet: sheet_id, prop: SheetProp::Filter(def) },
            Op::SetProp { sheet: sheet_id, prop: SheetProp::Filtered(hidden) },
        ]
    }

    pub fn filter_by_value(&mut self, col: usize, needle: &str) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let sheet = state.sheet();
        let before = (*sheet.hidden_rows).clone();
        let after: HashSet<usize> = if needle.is_empty() {
            HashSet::new()
        } else {
            let needle_lower = needle.to_lowercase();
            (0..sheet.rows)
                .filter(|&r| {
                    (0..sheet.cols).any(|c| !sheet.cell(r, c).is_empty())
                        && !sheet.cell(r, col).to_lowercase().contains(&needle_lower)
                })
                .collect()
        };
        // The substring dialog is a session-only filter: it replaces any
        // explicit definition, which has no substring form to persist.
        let replace_def = sheet.filter.is_some();
        drop(sheet);
        drop(state);
        if before == after && !replace_def {
            return;
        }
        let mut ops = Vec::new();
        if replace_def {
            ops.push(Op::SetProp { sheet: sheet_id, prop: SheetProp::Filter(None) });
        }
        if before != after {
            ops.push(Op::SetProp { sheet: sheet_id, prop: SheetProp::Filtered(after) });
        }
        self.apply_ops("Filter Rows", ops);
    }

    pub fn clear_filter(&mut self) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let sheet = state.sheet();
        // Manual hides are independent: clearing restores filter-hidden
        // rows only, never rows the user deliberately hid.
        if sheet.filter.is_none() && sheet.hidden_rows.is_empty() {
            return;
        }
        let mut ops = Vec::new();
        if sheet.filter.is_some() {
            ops.push(Op::SetProp { sheet: sheet_id, prop: SheetProp::Filter(None) });
        }
        if !sheet.hidden_rows.is_empty() {
            ops.push(Op::SetProp { sheet: sheet_id, prop: SheetProp::Filtered(HashSet::new()) });
        }
        drop(sheet);
        drop(state);
        self.apply_ops("Clear Filter", ops);
    }

    pub fn hide_selected_rows(&mut self) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let sheet = state.sheet();
        let before = (*sheet.hidden_rows_manual).clone();
        let (r0, _, r1, _) = sheet.selection_rect();
        let mut after = before.clone();
        after.extend(r0..=r1);
        drop(sheet);
        drop(state);
        if before != after {
            self.set_prop("Hide Rows", sheet_id, SheetProp::HiddenRows(after));
        }
    }

    pub fn unhide_all_rows(&mut self) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let before = (*state.sheet().hidden_rows_manual).clone();
        drop(state);
        if !before.is_empty() {
            self.set_prop("Show Rows", sheet_id, SheetProp::HiddenRows(HashSet::new()));
        }
    }

    pub fn hide_selected_cols(&mut self) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let sheet = state.sheet();
        let before = (*sheet.hidden_cols).clone();
        let (_, c0, _, c1) = sheet.selection_rect();
        let mut after = before.clone();
        after.extend(c0..=c1);
        drop(sheet);
        drop(state);
        if before != after {
            self.set_prop("Hide Columns", sheet_id, SheetProp::HiddenCols(after));
        }
    }

    pub fn unhide_all_cols(&mut self) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let before = (*state.sheet().hidden_cols).clone();
        drop(state);
        if !before.is_empty() {
            self.set_prop("Show Columns", sheet_id, SheetProp::HiddenCols(HashSet::new()));
        }
    }

    pub fn set_print_area(&mut self, sel: (usize, usize, usize, usize)) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let before = state.sheet().print_area;
        drop(state);
        let after = Some(sel);
        if before != after {
            self.set_prop("Set Print Area", sheet_id, SheetProp::PrintArea(after));
        }
    }

    pub fn clear_print_area(&mut self) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let before = state.sheet().print_area;
        drop(state);
        if before.is_some() {
            self.set_prop("Clear Print Area", sheet_id, SheetProp::PrintArea(None));
        }
    }

    pub fn set_page_setup(&mut self, setup: suite_common_core::print::PageSetup) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let before = state.sheet().page_setup.clone();
        drop(state);
        if before != setup {
            self.set_prop("Page Setup", sheet_id, SheetProp::PageSetup(setup));
        }
    }

    /// A header click: sort ascending, then descending, then clear the
    /// sort indicator.
    pub fn toggle_sort(&mut self, col: usize) {
        use SortDirection::{Ascending, Descending};
        let sorted = self.state.borrow().sheet().sorted_col;
        match sorted {
            Some((current, Ascending)) if current == col => {
                self.sort_column(col, Descending);
            }
            Some((current, Descending)) if current == col => {
                self.mutate_sheet("Clear Sort Indicator", |sheet| sheet.sorted_col = None);
            }
            _ => {
                self.sort_column(col, Ascending);
            }
        }
    }

    /// Hide the rows whose value in `col` (as shown) is in `hidden`, and
    /// show the rest: the column menu's filter list. The criterion joins
    /// the sheet's explicit definition — every active column filters
    /// together — as one undo step; an empty `hidden` clears this
    /// column's criterion (and the definition when it was the last one).
    pub fn filter_column_values(&mut self, col: usize, hidden: &HashSet<String>) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let sheet = state.sheet();
        let mut def = sheet.filter.clone().unwrap_or(FilterDef { range: None, columns: Vec::new() });
        match def.columns.iter_mut().find(|criterion| criterion.col == col) {
            Some(criterion) => criterion.hidden = hidden.clone(),
            None => def.columns.push(FilterColumn { col, hidden: hidden.clone() }),
        }
        def.columns.retain(|criterion| !criterion.hidden.is_empty());
        def.columns.sort_by_key(|criterion| criterion.col);
        let def = if def.columns.is_empty() { None } else { Some(def) };
        // A no-op (same definition, same derived rows) pushes no step —
        // and, crucially, leaves session-only dialog rows alone.
        let hidden_rows = def.as_ref().map(|d| d.derive_hidden(&sheet)).unwrap_or_default();
        if sheet.filter == def && *sheet.hidden_rows == hidden_rows {
            return;
        }
        let ops = Self::filter_ops(sheet_id, def, &sheet);
        drop(sheet);
        drop(state);
        self.apply_ops("Filter Rows", ops);
    }

    /// Clear one column's criterion, leaving every other column's in
    /// place: the column menu's per-column clear. A no-op when the
    /// column carries no criterion.
    pub fn clear_column_filter(&mut self, col: usize) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let sheet = state.sheet();
        let mut def = match sheet.filter.clone() {
            Some(def) => def,
            None => return,
        };
        let before = def.columns.len();
        def.columns.retain(|criterion| criterion.col != col);
        if def.columns.len() == before {
            return;
        }
        let def = if def.columns.is_empty() { None } else { Some(def) };
        let ops = Self::filter_ops(sheet_id, def, &sheet);
        drop(sheet);
        drop(state);
        self.apply_ops("Clear Column Filter", ops);
    }

    /// Give the sheet's filter an explicit range (TBL-003): the range's
    /// top row is its label header and is never hidden. Corners may come
    /// either way round; the range clamps to the sheet. Replaces any
    /// rangeless session definition's extent while keeping its criteria.
    pub fn set_filter_range(&mut self, range: (usize, usize, usize, usize)) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let sheet = state.sheet();
        let (r0, c0, r1, c1) = range;
        let range = (
            r0.min(r1).min(sheet.rows.saturating_sub(1)),
            c0.min(c1).min(sheet.cols.saturating_sub(1)),
            r0.max(r1).min(sheet.rows.saturating_sub(1)),
            c0.max(c1).min(sheet.cols.saturating_sub(1)),
        );
        let mut def = sheet.filter.clone().unwrap_or(FilterDef { range: None, columns: Vec::new() });
        if def.range == Some(range) {
            return;
        }
        def.range = Some(range);
        let ops = Self::filter_ops(sheet_id, Some(def), &sheet);
        drop(sheet);
        drop(state);
        self.apply_ops("Set Filter Range", ops);
    }

    /// Sort the sheet's rows by `col` in `direction` (numbers numerically,
    /// text case-insensitively, blanks last), as one undo step. Returns
    /// whether the sort was applied: a reference the engine cannot
    /// translate aborts the sort with no history step rather than
    /// reordering rows under stale references.
    pub fn sort_column(&mut self, col: usize, direction: SortDirection) -> bool {
        use SortDirection::Ascending;

        let (sheet, start, rows, cols) = {
            let state = self.state.borrow();
            let sheet = state.sheet();
            (sheet.sheet_id, sheet.frozen_rows.min(sheet.rows), sheet.rows, sheet.cols)
        };
        if !self.authorize_cells(sheet, (start..rows).flat_map(|r| (0..cols).map(move |c| (r, c)))) {
            return false;
        }

        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let before_sheet = state.sheet().clone();
        if col >= before_sheet.cols {
            return false;
        }
        let new_direction = direction;
        let before_inputs: Vec<Vec<String>> = (0..before_sheet.rows)
            .map(|row| {
                (0..before_sheet.cols)
                    .map(|column| state.cell_input(row, column))
                    .collect()
            })
            .collect();
        // Numeric compare when both cells parse as numbers; otherwise a
        // case-insensitive string compare. The previous version always
        // parsed-with-fallback-to-0.0, so any non-numeric column (the
        // common case — sorting names, categories, anything text) had
        // every row compare equal and silently never actually reorder,
        // even though sorted_col (and the header arrow) still updated
        // as if it had. Caught via #113's visual-validation screenshots
        // (a text-column sort test) showing unsorted data despite a
        // passing sorted_col assertion — a real bug, not a rendering gap.
        //
        // Blank cells always sort last regardless of direction (Excel/
        // Sheets convention) — without this, sorting a lightly-used
        // sheet pushes real data to the bottom on ascending sort, since
        // "" < any non-empty string. Caught the same way: a GUI test on
        // the app's real 100-row default sheet (only 3 rows filled)
        // found the 3 values sorted correctly among themselves but
        // shoved to rows 97-99 by the 97 blank rows sorting ahead of
        // them.
        // Frozen rows are the header (Sheets' rule): they stay where they
        // are and only the rows below them are sorted. Without this a
        // header row sorted in among the data, as "Name" does among names.
        let header = before_sheet.frozen_rows.min(before_sheet.rows);
        let mut order: Vec<usize> = (0..header).collect();
        let mut body: Vec<usize> = (header..before_sheet.rows).collect();
        body.sort_by(|&left, &right| {
            let left_text = &before_sheet.data[left][col];
            let right_text = &before_sheet.data[right][col];
            let left_blank = left_text.trim().is_empty();
            let right_blank = right_text.trim().is_empty();
            match (left_blank, right_blank) {
                (true, true) => std::cmp::Ordering::Equal,
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                (false, false) => {
                    let cmp = match (left_text.parse::<f64>(), right_text.parse::<f64>()) {
                        (Ok(l), Ok(r)) => l.partial_cmp(&r).unwrap_or(std::cmp::Ordering::Equal),
                        _ => left_text.to_lowercase().cmp(&right_text.to_lowercase()),
                    };
                    if new_direction == Ascending {
                        cmp
                    } else {
                        cmp.reverse()
                    }
                }
            }
        });
        order.extend(body);
        drop(state);

        // The sort as ops: every row's inputs in one SetCells (references
        // moved with their cells), then each cell's format, style, border,
        // validation, note and lock where the new order changes them, and
        // the header arrow. Undo is their inverses.
        let mut cells = Vec::new();
        {
            let mut state = self.state.borrow_mut();
            for (new_row, &old_row) in order.iter().enumerate() {
                for (column, (input, current)) in before_inputs[old_row].iter().zip(&before_inputs[new_row]).enumerate() {
                    // The translation touches nothing yet — these strings
                    // become the op below — so a rejection aborts the sort
                    // with no history step rather than sorting rows under
                    // references that no longer point where they did.
                    let moved = match state.engine.move_input(input, (old_row, column), (new_row, column)) {
                        Ok(moved) => moved,
                        Err(_) => return false,
                    };
                    if moved != *current {
                        cells.push((new_row, column, moved));
                    }
                }
            }
        }
        let mut after_sheet = before_sheet.clone();
        after_sheet.sorted_col = Some((col, new_direction));
        after_sheet.formats = order.iter().map(|&row| before_sheet.formats[row].clone()).collect();
        after_sheet.borders = order.iter().map(|&row| before_sheet.borders[row].clone()).collect();
        after_sheet.styles = order.iter().map(|&row| before_sheet.styles[row].clone()).collect();
        after_sheet.validations = order.iter().map(|&row| before_sheet.validations[row].clone()).collect();
        after_sheet.notes = order.iter().map(|&row| before_sheet.notes[row].clone()).collect();
        // Locks travel with their rows too: left behind, a sort on a
        // protected sheet locked the cells the author had unlocked and
        // unlocked the ones they had not (#1987).
        after_sheet.cell_protections = order.iter().map(|&row| before_sheet.cell_protections[row].clone()).collect();
        let mut ops = vec![Op::SetCells { sheet: sheet_id, cells }];
        ops.extend(super::ops::diff_ops(&before_sheet, &after_sheet));
        self.apply_ops("Sort", ops)
    }
}
