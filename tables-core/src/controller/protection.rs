// SPDX-License-Identifier: GPL-3.0-or-later
//! User authorization, deliberately separate from trusted op replay/undo.

use super::core::WorkbookController;
use super::ops::{Axis, Op, SheetProp};
use super::state::WorkbookState;
use crate::sheet::SheetModel;

fn locked(sheet: &SheetModel, row: usize, col: usize) -> bool {
    sheet.protection.protected
        && sheet.cell_protections.get(row).and_then(|r| r.get(col)).is_some_and(|cell| cell.locked)
}

/// Whether a user edit may not change (`row`, `col`)'s content: the cell is
/// locked on a protected sheet, or protection does not let the user select
/// it (TBL-005) — a cell that cannot be selected cannot be edited either.
/// A cell outside the sheet is no one's to guard.
fn guarded(sheet: &SheetModel, row: usize, col: usize) -> bool {
    let inside = row < sheet.rows && col < sheet.cols;
    locked(sheet, row, col) || (inside && sheet.protection.protected && !sheet.may_select(row, col))
}

/// Whether any cell of the lines `at..at + count` along `axis` is locked:
/// a delete permission removes unlocked lines only, as in Excel and Calc.
fn lines_hold_locked(sheet: &SheetModel, axis: Axis, at: usize, count: usize) -> bool {
    match axis {
        Axis::Rows => (at..(at + count).min(sheet.rows)).any(|r| (0..sheet.cols).any(|c| locked(sheet, r, c))),
        Axis::Cols => (0..sheet.rows).any(|r| (at..(at + count).min(sheet.cols)).any(|c| locked(sheet, r, c))),
    }
}

fn refuses_op(state: &WorkbookState, op: &Op) -> bool {
    // Relinks are normally produced by undo, but public user op groups must
    // not use their restoration payload to write a protected cell elsewhere.
    let relinks = match op {
        Op::Insert { relinks, .. } | Op::AddSheet { relinks, .. } => relinks.as_slice(),
        _ => &[],
    };
    if relinks.iter().any(|(id, row, col, _)| {
        state.sheet_index_for_id(*id).is_some_and(|i| guarded(&state.sheets[i].borrow(), *row, *col))
    }) {
        return true;
    }
    let Some(index) = op.sheet().and_then(|id| state.sheet_index_for_id(id)) else { return false };
    let sheet = state.sheets[index].borrow();
    if !sheet.protection.protected {
        return false;
    }
    let allow = &sheet.protection;
    match op {
        Op::SetCell { row, col, .. }
        | Op::SetValidation { row, col, .. }
        | Op::SetNote { row, col, .. } => guarded(&sheet, *row, *col),
        Op::SetCells { cells, .. } => cells.iter().any(|(row, col, _)| guarded(&sheet, *row, *col)),
        Op::SetFormat { row, col, .. }
        | Op::SetStyle { row, col, .. }
        | Op::SetBorder { row, col, .. } => {
            (!allow.allow_format_cells && locked(&sheet, *row, *col)) || !sheet.may_select(*row, *col)
        }
        Op::Insert { axis: Axis::Rows, .. } => !allow.allow_insert_rows,
        Op::Insert { axis: Axis::Cols, .. } => !allow.allow_insert_columns,
        Op::Delete { axis: Axis::Rows, at, count, .. } => {
            !allow.allow_delete_rows || lines_hold_locked(&sheet, Axis::Rows, *at, *count)
        }
        Op::Delete { axis: Axis::Cols, at, count, .. } => {
            !allow.allow_delete_columns || lines_hold_locked(&sheet, Axis::Cols, *at, *count)
        }
        Op::Resize { axis: Axis::Rows, .. } | Op::SetProp { prop: SheetProp::HiddenRows(_), .. } => !allow.allow_format_rows,
        Op::Resize { axis: Axis::Cols, .. } | Op::SetProp { prop: SheetProp::HiddenCols(_), .. } => !allow.allow_format_columns,
        // Unprotect is explicit; it does not authorize any other op in this
        // group. Authorization uses the state before the entire command.
        Op::SetProp { prop: SheetProp::Protection(_), .. } => false,
        // Pivot registration accompanies its output SetCells, checked above.
        Op::SetProp { prop: SheetProp::Pivots(_), .. } => !allow.allow_pivot_tables,
        // A sort permission moves unlocked rows only: its SetCells are
        // checked above, and a locked cell in the sorted body refuses it.
        Op::SetProp { prop: SheetProp::Sorted(_), .. } => {
            !allow.allow_sort
                || (sheet.frozen_rows.min(sheet.rows)..sheet.rows)
                    .any(|row| (0..sheet.cols).any(|col| locked(&sheet, row, col)))
        }
        Op::SetProp { prop: SheetProp::Filter(_) | SheetProp::Filtered(_), .. } => !allow.allow_autofilter,
        Op::SetProp { prop: SheetProp::Charts(_), .. } => !allow.allow_edit_objects,
        // Changing locks themselves, merges and freezes have no permission:
        // they require unprotecting first.
        Op::SetLocked { .. }
        | Op::Merge { .. }
        | Op::Unmerge { .. }
        | Op::Freeze { .. } => true,
        // Sheet protection is not workbook structure protection or a
        // prohibition on print settings or conditional rules.
        Op::DeleteSheet { .. }
        | Op::RenameSheet { .. }
        | Op::MoveSheet { .. }
        | Op::SetProp { .. }
        | Op::AddSheet { .. }
        | Op::DefineName { .. } => false,
    }
}

impl WorkbookController {
    /// Preflight the complete group without executing even its first op.
    /// A read-only workbook refuses every group, without counting a
    /// protection rejection (`read_only`).
    pub(super) fn authorize_ops(&mut self, ops: &[Op]) -> bool {
        if self.read_only {
            return false;
        }
        let refused = {
            let state = self.state.borrow();
            ops.iter().any(|op| refuses_op(&state, op))
        };
        if refused {
            self.protection_rejections = self.protection_rejections.wrapping_add(1);
        }
        !refused
    }

    /// Check intended destinations before no-op filtering or formula
    /// translation. A compound selection containing a locked (or, under
    /// TBL-005's selection permissions, unselectable) cell is refused as a
    /// whole, even if that cell already contains the requested value.
    pub(super) fn authorize_cells(&mut self, sheet_id: u32, cells: impl IntoIterator<Item = (usize, usize)>) -> bool {
        let refused = {
            let state = self.state.borrow();
            state.sheet_index_for_id(sheet_id).is_some_and(|index| {
                let sheet = state.sheets[index].borrow();
                sheet.protection.protected && cells.into_iter().any(|(row, col)| guarded(&sheet, row, col))
            })
        };
        if refused {
            self.refuse();
        }
        !refused
    }

    /// Count a user operation protection refused, for native feedback.
    pub(super) fn refuse(&mut self) {
        self.protection_rejections = self.protection_rejections.wrapping_add(1);
    }

    /// Check that no source of a copy-like operation (fill) is a hidden
    /// formula: its result could otherwise reveal the formula elsewhere.
    pub(super) fn authorize_sources(&mut self, sheet_id: u32, cells: impl IntoIterator<Item = (usize, usize)>) -> bool {
        let refused = {
            let state = self.state.borrow();
            state.sheet_index_for_id(sheet_id).is_some_and(|index| {
                let sheet = state.sheets[index].borrow();
                cells.into_iter().any(|(row, col)| sheet.formula_hidden(row, col))
            })
        };
        if refused {
            self.refuse();
        }
        !refused
    }

    /// Whether an edit to (`row`, `col`) on the active sheet is refused:
    /// the sheet is protected and the cell is locked or cannot be selected.
    /// [`Self::edit_cell`] ignores such an edit; the app asks first so it
    /// can say why.
    pub fn refuses_edit(&self, row: usize, col: usize) -> bool {
        guarded(&self.state.borrow().sheet(), row, col)
    }

    /// Commit the formula bar's text to (`row`, `col`). The bar shows
    /// nothing for an input protection withholds (TBL-005); committing that
    /// untouched empty text is not an edit, so it cannot clear the hidden
    /// formula of an unlocked cell. Anything typed is an ordinary edit.
    pub fn commit_formula_bar(&mut self, row: usize, col: usize, text: &str) {
        let withheld = self.state.borrow().presented_input(row, col).is_none();
        if withheld && text.is_empty() {
            return;
        }
        self.edit_cell(row, col, text);
    }

    pub(super) fn authorize_format_cells(&mut self, sheet_id: u32, cells: impl IntoIterator<Item = (usize, usize)>) -> bool {
        let allowed = {
            let state = self.state.borrow();
            state.sheet_index_for_id(sheet_id)
                .is_some_and(|index| state.sheets[index].borrow().protection.allow_format_cells)
        };
        allowed || self.authorize_cells(sheet_id, cells)
    }
}
