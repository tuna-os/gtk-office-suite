// SPDX-License-Identifier: GPL-3.0-or-later
//! Workbook operations: RFC-0001 Phase 0 for Tables.
//!
//! Every change to a workbook can be expressed as an [`Op`], and applying
//! an op returns its exact inverse, so undo is "apply the inverses in
//! reverse" rather than a stored copy of the sheet. That is also the shape
//! a CRDT needs: a stream of small, self-describing changes.
//!
//! The ops follow the CRDT spike's findings (docs/rfc/0001-spike-results.md),
//! without taking a CRDT dependency (that choice is the owner's):
//!
//! 1. **Cell state is per-field registers.** A cell's value (`SetCell`),
//!    number format (`SetFormat`), style (`SetStyle`) and border
//!    (`SetBorder`) are separate ops, so two people changing different
//!    fields of one cell don't collide. That is the spike's
//!    `"<row>.<col>.<field>"` last-writer-wins map.
//! 2. **Rows and columns are sequences.** `Insert` and `Delete` address a
//!    position on one axis. The spike found a CRDT must key cells by a
//!    stable row id, not a position, or a concurrent insert shifts every
//!    key; an adapter maps these positions to ids at the boundary. Ops are
//!    applied in order, so within this log a position is exact.
//! 3. **Destructive ops carry what they destroy.** `Delete`'s inverse is an
//!    `Insert` holding the removed cells, and the formulas elsewhere whose
//!    references the delete broke (`relinks`), so undoing it restores the
//!    workbook exactly, not approximately.
//! 4. **Sheets are a sequence with stable ids.** A deleted sheet comes back
//!    with its IronCalc sheet id, so references and history still find it.
//!
//! The API has the shape of `letters_core::edit` (ADR 0010), so the two can
//! later share one history in suite-common-core: [`apply`] returns the
//! inverse ops of one op, and [`apply_all`] applies a group all or nothing
//! and returns the group's inverse, in the order that undoes it.
//!
//! Scope: every undoable edit is ops. Values, formats, styles, borders,
//! validations and locks per cell; rows and columns, merges, sizes and
//! freezes; sheets (add, rename, move, delete); names; and the sheet-wide
//! properties ([`SheetProp`]: sort arrow, filter, hidden lines, conditional
//! formats, charts, pivots, protection, print area, page setup), each set
//! whole. A sort is the `SetCells` that moves the rows plus the per-cell
//! ops for what moved with them. Closure edits (`mutate_sheet`) become ops
//! through [`diff_ops`]. There's no whole-sheet snapshot undo left.

use std::cell::RefCell;
use std::rc::Rc;

use suite_common_core::format::NumberFormat;

use super::state::WorkbookState;
use std::collections::HashSet;

use crate::sheet::{CellBorder, CellProtection, SheetModel, ValidationRule};
use crate::style::CellStyle;

/// Rows or columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Rows,
    Cols,
}

/// Everything one cell holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CellContent {
    /// What was typed: a value, or a formula with its `=`.
    pub input: String,
    pub format: NumberFormat,
    pub style: CellStyle,
    pub border: CellBorder,
    pub validation: Option<ValidationRule>,
    pub lock: CellProtection,
}

/// One row or column: its size and its cells in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub size: f64,
    pub cells: Vec<CellContent>,
}

/// A formula cell to put back as it was: `(sheet id, row, col, input)`.
pub type Relink = (u32, usize, usize, String);

/// A whole sheet, for putting a deleted one back.
#[derive(Clone)]
pub struct SheetContent {
    pub model: SheetModel,
    pub inputs: Vec<(usize, usize, String)>,
}

impl std::fmt::Debug for SheetContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SheetContent({} x {}, {} inputs)", self.model.rows, self.model.cols, self.inputs.len())
    }
}

/// One change to a workbook. A sheet is addressed by its stable id
/// (`SheetModel::sheet_id`, IronCalc's), not its position: an edit recorded
/// before sheets were added, moved or deleted still finds its own sheet,
/// or fails cleanly if the sheet is gone. `AddSheet` alone takes a
/// position, the place to put the new sheet.
#[derive(Clone, Debug)]
pub enum Op {
    SetCell { sheet: u32, row: usize, col: usize, input: String },
    SetFormat { sheet: u32, row: usize, col: usize, format: NumberFormat },
    SetStyle { sheet: u32, row: usize, col: usize, style: CellStyle },
    SetBorder { sheet: u32, row: usize, col: usize, border: CellBorder },
    /// Insert lines before `at`; blank ones if `lines` holds blank cells.
    Insert { sheet: u32, axis: Axis, at: usize, lines: Vec<Line>, relinks: Vec<Relink>, merges: Option<Vec<(usize, usize, usize, usize)>> },
    Delete { sheet: u32, axis: Axis, at: usize, count: usize },
    /// Merge `(row, col, rowspan, colspan)`.
    Merge { sheet: u32, range: (usize, usize, usize, usize) },
    Unmerge { sheet: u32, range: (usize, usize, usize, usize) },
    /// Set a row's height or a column's width, in px.
    Resize { sheet: u32, axis: Axis, index: usize, size: f64 },
    Freeze { sheet: u32, rows: usize, cols: usize },
    AddSheet { index: usize, name: String, sheet_id: Option<u32>, content: Option<Box<SheetContent>>, relinks: Vec<Relink> },
    DeleteSheet { sheet: u32 },
    RenameSheet { sheet: u32, name: String },
    /// Move a sheet to position `to`.
    MoveSheet { sheet: u32, to: usize },
    /// Several cells' inputs at once, evaluated once (paste, fill, sort).
    SetCells { sheet: u32, cells: Vec<(usize, usize, String)> },
    SetValidation { sheet: u32, row: usize, col: usize, rule: Option<ValidationRule> },
    SetLocked { sheet: u32, row: usize, col: usize, protection: CellProtection },
    /// One sheet-wide property: a last-writer-wins register each.
    SetProp { sheet: u32, prop: SheetProp },
    /// Define a workbook name (`formula`), or remove it (`None`).
    DefineName { name: String, formula: Option<String> },
}

/// The sheet-wide properties, each set whole. Small lists and sets (the
/// rules, the charts, the hidden rows) are one value: two people editing
/// the same list at once is rare, and last-writer-wins is what Sheets does.
#[derive(Clone, Debug, PartialEq)]
pub enum SheetProp {
    /// The column and direction the sheet was last sorted by (the header's
    /// arrow); sorting itself moves cells with `SetCells` and friends.
    Sorted(Option<(usize, crate::sheet::SortDirection)>),
    /// Rows hidden by a filter.
    Filtered(HashSet<usize>),
    /// Rows and columns hidden by hand.
    HiddenRows(HashSet<usize>),
    HiddenCols(HashSet<usize>),
    CondRules(Vec<crate::sheet::CondRule>),
    Charts(Vec<crate::sheet::ChartSpec>),
    Pivots(Vec<crate::sheet::PivotTableSpec>),
    Protection(crate::sheet::SheetProtection),
    PrintArea(Option<(usize, usize, usize, usize)>),
    PageSetup(suite_common_core::print::PageSetup),
}

impl SheetProp {
    /// Put this value on `sheet` and return the one it replaced.
    fn swap(&self, sheet: &mut SheetModel) -> SheetProp {
        use std::mem::replace;
        match self {
            SheetProp::Sorted(v) => SheetProp::Sorted(replace(&mut sheet.sorted_col, *v)),
            SheetProp::Filtered(v) => SheetProp::Filtered(replace(&mut sheet.hidden_rows, v.clone())),
            SheetProp::HiddenRows(v) => SheetProp::HiddenRows(replace(&mut sheet.hidden_rows_manual, v.clone())),
            SheetProp::HiddenCols(v) => SheetProp::HiddenCols(replace(&mut sheet.hidden_cols, v.clone())),
            SheetProp::CondRules(v) => SheetProp::CondRules(replace(&mut sheet.cond_rules, v.clone())),
            SheetProp::Charts(v) => SheetProp::Charts(replace(&mut sheet.charts, v.clone())),
            SheetProp::Pivots(v) => SheetProp::Pivots(replace(&mut sheet.pivot_tables, v.clone())),
            SheetProp::Protection(v) => SheetProp::Protection(replace(&mut sheet.protection, v.clone())),
            SheetProp::PrintArea(v) => SheetProp::PrintArea(replace(&mut sheet.print_area, *v)),
            SheetProp::PageSetup(v) => SheetProp::PageSetup(replace(&mut sheet.page_setup, v.clone())),
        }
    }

    /// Every property of `sheet`, in a fixed order.
    pub fn all(sheet: &SheetModel) -> Vec<SheetProp> {
        vec![
            SheetProp::Sorted(sheet.sorted_col),
            SheetProp::Filtered(sheet.hidden_rows.clone()),
            SheetProp::HiddenRows(sheet.hidden_rows_manual.clone()),
            SheetProp::HiddenCols(sheet.hidden_cols.clone()),
            SheetProp::CondRules(sheet.cond_rules.clone()),
            SheetProp::Charts(sheet.charts.clone()),
            SheetProp::Pivots(sheet.pivot_tables.clone()),
            SheetProp::Protection(sheet.protection.clone()),
            SheetProp::PrintArea(sheet.print_area),
            SheetProp::PageSetup(sheet.page_setup.clone()),
        ]
    }
}

fn sheet_ref(state: &WorkbookState, sheet: usize) -> Result<&Rc<RefCell<SheetModel>>, String> {
    state.sheets.get(sheet).ok_or_else(|| format!("no sheet {sheet}"))
}

fn check_cell(state: &WorkbookState, sheet: usize, row: usize, col: usize) -> Result<(), String> {
    let s = sheet_ref(state, sheet)?.borrow();
    if row < s.rows && col < s.cols { Ok(()) } else { Err(format!("cell ({row}, {col}) is outside sheet {sheet}")) }
}

/// Re-read a sheet's values from the engine after a structural change.
fn resync(state: &mut WorkbookState, sheet: usize) {
    let previous = state.active_sheet;
    if state.engine.set_active_sheet(sheet).is_err() {
        return;
    }
    state.active_sheet = sheet;
    state.sync_active_sheet();
    let _ = state.engine.set_active_sheet(previous);
    state.active_sheet = previous;
}


fn cell_content(state: &WorkbookState, sheet: usize, row: usize, col: usize) -> CellContent {
    let s = state.sheets[sheet].borrow();
    CellContent {
        input: state.engine.input_at(sheet, row, col),
        format: s.formats[row][col].clone(),
        style: s.styles[row][col].clone(),
        border: s.borders[row][col].clone(),
        validation: s.validations[row][col].clone(),
        lock: s.cell_protections[row][col].clone(),
    }
}

/// Put back formula cells whose text differs from what they should be.
fn apply_relinks(state: &mut WorkbookState, relinks: &[Relink]) {
    for (id, row, col, input) in relinks {
        let Some(sheet) = state.sheet_index_for_id(*id) else { continue };
        if state.engine.input_at(sheet, *row, *col) != *input {
            state.set_cell_input_on_sheet(sheet, *row, *col, input);
        }
    }
}

/// The workbook's formula cells at the positions `keep` accepts, as
/// relinks: what to put back if a delete breaks their references.
fn relinks_where(state: &WorkbookState, keep: impl Fn(usize, usize, usize) -> bool) -> Vec<Relink> {
    state
        .engine
        .formula_cells()
        .into_iter()
        .filter(|(s, r, c, _)| keep(*s, *r, *c))
        .filter_map(|(s, r, c, input)| Some((state.sheets.get(s)?.borrow().sheet_id, r, c, input)))
        .collect()
}

impl Op {
    /// The id of the sheet this op addresses, if it addresses one.
    pub fn sheet(&self) -> Option<u32> {
        match self {
            Op::SetCell { sheet, .. }
            | Op::SetFormat { sheet, .. }
            | Op::SetStyle { sheet, .. }
            | Op::SetBorder { sheet, .. }
            | Op::Insert { sheet, .. }
            | Op::Delete { sheet, .. }
            | Op::Merge { sheet, .. }
            | Op::Unmerge { sheet, .. }
            | Op::Resize { sheet, .. }
            | Op::Freeze { sheet, .. }
            | Op::RenameSheet { sheet, .. }
            | Op::MoveSheet { sheet, .. }
            | Op::SetCells { sheet, .. }
            | Op::SetValidation { sheet, .. }
            | Op::SetLocked { sheet, .. }
            | Op::SetProp { sheet, .. }
            | Op::DeleteSheet { sheet } => Some(*sheet),
            Op::AddSheet { .. } | Op::DefineName { .. } => None,
        }
    }

    /// Apply this op to `state` and return the op that undoes it exactly.
    /// On an error nothing has changed.
    fn apply_one(&self, state: &mut WorkbookState) -> Result<Op, String> {
        // The sheet's current position, for the ops that address one.
        let si = match self.sheet() {
            Some(id) => state.sheet_index_for_id(id).ok_or_else(|| format!("no sheet with id {id}"))?,
            None => 0,
        };
        match self {
            Op::SetCell { sheet, row, col, input } => {
                check_cell(state, si, *row, *col)?;
                let old = state.engine.input_at(si, *row, *col);
                state.set_cell_input_on_sheet(si, *row, *col, input);
                Ok(Op::SetCell { sheet: *sheet, row: *row, col: *col, input: old })
            }
            Op::SetFormat { sheet, row, col, format } => {
                check_cell(state, si, *row, *col)?;
                let old = std::mem::replace(&mut state.sheets[si].borrow_mut().formats[*row][*col], format.clone());
                Ok(Op::SetFormat { sheet: *sheet, row: *row, col: *col, format: old })
            }
            Op::SetStyle { sheet, row, col, style } => {
                check_cell(state, si, *row, *col)?;
                let old = std::mem::replace(&mut state.sheets[si].borrow_mut().styles[*row][*col], style.clone());
                Ok(Op::SetStyle { sheet: *sheet, row: *row, col: *col, style: old })
            }
            Op::SetBorder { sheet, row, col, border } => {
                check_cell(state, si, *row, *col)?;
                let old = std::mem::replace(&mut state.sheets[si].borrow_mut().borders[*row][*col], border.clone());
                Ok(Op::SetBorder { sheet: *sheet, row: *row, col: *col, border: old })
            }
            Op::Insert { sheet, axis, at, lines, relinks, merges } => {
                let (len, across) = {
                    let s = sheet_ref(state, si)?.borrow();
                    match axis {
                        Axis::Rows => (s.rows, s.cols),
                        Axis::Cols => (s.cols, s.rows),
                    }
                };
                let n = lines.len();
                if *at > len || n == 0 || lines.iter().any(|l| l.cells.len() != across) {
                    return Err(format!("can't insert {n} lines of {across} cells at {at} of {len}"));
                }
                match axis {
                    Axis::Rows => state.engine.insert_rows(si, *at, n)?,
                    Axis::Cols => state.engine.insert_cols(si, *at, n)?,
                }
                let mut inputs = Vec::new();
                {
                    let mut s = state.sheets[si].borrow_mut();
                    match axis {
                        Axis::Rows => s.insert_rows(*at, n),
                        Axis::Cols => s.insert_cols(*at, n),
                    }
                    for (i, line) in lines.iter().enumerate() {
                        let k = at + i;
                        match axis {
                            Axis::Rows => s.row_heights[k] = line.size,
                            Axis::Cols => s.col_widths[k] = line.size,
                        }
                        for (j, cell) in line.cells.iter().enumerate() {
                            let (r, c) = match axis {
                                Axis::Rows => (k, j),
                                Axis::Cols => (j, k),
                            };
                            s.formats[r][c] = cell.format.clone();
                            s.styles[r][c] = cell.style.clone();
                            s.borders[r][c] = cell.border.clone();
                            s.validations[r][c] = cell.validation.clone();
                            s.cell_protections[r][c] = cell.lock.clone();
                            if !cell.input.is_empty() {
                                inputs.push((r, c, cell.input.clone()));
                            }
                        }
                    }
                    if let Some(m) = merges {
                        s.merges = m.clone();
                    }
                }
                state.grow_engine();
                state.set_cell_inputs_on_sheet(si, inputs.iter().map(|(r, c, i)| (*r, *c, i.as_str())));
                apply_relinks(state, relinks);
                resync(state, si);
                Ok(Op::Delete { sheet: *sheet, axis: *axis, at: *at, count: n })
            }
            Op::Delete { sheet, axis, at, count } => {
                let (len, across) = {
                    let s = sheet_ref(state, si)?.borrow();
                    match axis {
                        Axis::Rows => (s.rows, s.cols),
                        Axis::Cols => (s.cols, s.rows),
                    }
                };
                // A sheet keeps at least one row and column.
                if *count == 0 || *at + *count > len || *count >= len {
                    return Err(format!("can't delete {count} lines at {at} of {len}"));
                }
                let lines: Vec<Line> = (*at..*at + *count)
                    .map(|k| Line {
                        size: {
                            let s = state.sheets[si].borrow();
                            match axis {
                                Axis::Rows => s.row_heights[k],
                                Axis::Cols => s.col_widths[k],
                            }
                        },
                        cells: (0..across)
                            .map(|j| match axis {
                                Axis::Rows => cell_content(state, si, k, j),
                                Axis::Cols => cell_content(state, si, j, k),
                            })
                            .collect(),
                    })
                    .collect();
                let inside = |s: usize, r: usize, c: usize| {
                    s == si
                        && match axis {
                            Axis::Rows => r >= *at && r < at + count,
                            Axis::Cols => c >= *at && c < at + count,
                        }
                };
                let relinks = relinks_where(state, |s, r, c| !inside(s, r, c));
                let merges = state.sheets[si].borrow().merges.clone();
                match axis {
                    Axis::Rows => state.engine.delete_rows(si, *at, *count)?,
                    Axis::Cols => state.engine.delete_cols(si, *at, *count)?,
                }
                {
                    let mut s = state.sheets[si].borrow_mut();
                    match axis {
                        Axis::Rows => s.delete_rows(*at, *count),
                        Axis::Cols => s.delete_cols(*at, *count),
                    }
                }
                resync(state, si);
                Ok(Op::Insert { sheet: *sheet, axis: *axis, at: *at, lines, relinks, merges: Some(merges) })
            }
            Op::Merge { sheet, range } => {
                let (r, c, rs, cs) = *range;
                let mut s = sheet_ref(state, si)?.borrow_mut();
                if rs == 0 || cs == 0 || r + rs > s.rows || c + cs > s.cols || s.merges.contains(range) {
                    return Err(format!("can't merge {range:?}"));
                }
                s.merges.push(*range);
                Ok(Op::Unmerge { sheet: *sheet, range: *range })
            }
            Op::Unmerge { sheet, range } => {
                let mut s = sheet_ref(state, si)?.borrow_mut();
                let at = s.merges.iter().position(|m| m == range).ok_or_else(|| format!("{range:?} isn't merged"))?;
                s.merges.remove(at);
                Ok(Op::Merge { sheet: *sheet, range: *range })
            }
            Op::Resize { sheet, axis, index, size } => {
                let mut s = sheet_ref(state, si)?.borrow_mut();
                let sizes = match axis {
                    Axis::Rows => &mut s.row_heights,
                    Axis::Cols => &mut s.col_widths,
                };
                let slot = sizes.get_mut(*index).ok_or_else(|| format!("no line {index}"))?;
                let old = std::mem::replace(slot, *size);
                Ok(Op::Resize { sheet: *sheet, axis: *axis, index: *index, size: old })
            }
            Op::Freeze { sheet, rows, cols } => {
                let mut s = sheet_ref(state, si)?.borrow_mut();
                if *rows > s.rows || *cols > s.cols {
                    return Err("freeze past the sheet".into());
                }
                let old = (std::mem::replace(&mut s.frozen_rows, *rows), std::mem::replace(&mut s.frozen_cols, *cols));
                Ok(Op::Freeze { sheet: *sheet, rows: old.0, cols: old.1 })
            }
            Op::RenameSheet { sheet, name } => {
                let old = sheet_ref(state, si)?.borrow().name.clone();
                state.rename_sheet(si, name)?;
                Ok(Op::RenameSheet { sheet: *sheet, name: old })
            }
            Op::MoveSheet { sheet, to } => {
                let n = state.sheets.len();
                if *to >= n {
                    return Err(format!("no position {to} for a sheet"));
                }
                let mut order: Vec<usize> = (0..n).collect();
                let moved = order.remove(si);
                order.insert(*to, moved);
                state.reorder_sheets(&order)?;
                Ok(Op::MoveSheet { sheet: *sheet, to: si })
            }
            Op::SetCells { sheet, cells } => {
                {
                    let s = sheet_ref(state, si)?.borrow();
                    if let Some((r, c, _)) = cells.iter().find(|(r, c, _)| *r >= s.rows || *c >= s.cols) {
                        return Err(format!("cell ({r}, {c}) is outside sheet {si}"));
                    }
                }
                let old: Vec<(usize, usize, String)> =
                    cells.iter().map(|(r, c, _)| (*r, *c, state.engine.input_at(si, *r, *c))).collect();
                state.set_cell_inputs_on_sheet(si, cells.iter().map(|(r, c, i)| (*r, *c, i.as_str())));
                // Undone in reverse, so a cell set twice in one op gets its
                // first value back.
                Ok(Op::SetCells { sheet: *sheet, cells: old.into_iter().rev().collect() })
            }
            Op::SetValidation { sheet, row, col, rule } => {
                check_cell(state, si, *row, *col)?;
                let old = std::mem::replace(&mut state.sheets[si].borrow_mut().validations[*row][*col], rule.clone());
                Ok(Op::SetValidation { sheet: *sheet, row: *row, col: *col, rule: old })
            }
            Op::SetLocked { sheet, row, col, protection } => {
                check_cell(state, si, *row, *col)?;
                let old = std::mem::replace(&mut state.sheets[si].borrow_mut().cell_protections[*row][*col], protection.clone());
                Ok(Op::SetLocked { sheet: *sheet, row: *row, col: *col, protection: old })
            }
            Op::SetProp { sheet, prop } => {
                let old = prop.swap(&mut sheet_ref(state, si)?.borrow_mut());
                Ok(Op::SetProp { sheet: *sheet, prop: old })
            }
            Op::DefineName { name, formula } => {
                let old = state.engine.defined_name(name);
                if old.as_ref() == formula.as_ref() {
                    return Ok(Op::DefineName { name: name.clone(), formula: old });
                }
                state.engine.set_defined_name(name, formula.as_deref())?;
                state.engine.evaluate();
                for s in 0..state.sheets.len() {
                    resync(state, s);
                }
                Ok(Op::DefineName { name: name.clone(), formula: old })
            }
            Op::AddSheet { index, name, sheet_id, content, relinks } => {
                if *index > state.sheets.len() {
                    return Err(format!("no position {index} for a sheet"));
                }
                state.engine.insert_sheet(*index, name, *sheet_id)?;
                let id = state.engine.sheet_id_at(*index).unwrap_or(0);
                let mut model = match content {
                    Some(c) => c.model.clone(),
                    None => SheetModel::new(name, state.engine.rows, state.engine.cols, id),
                };
                model.name = name.clone();
                model.sheet_id = id;
                state.sheets.insert(*index, Rc::new(RefCell::new(model)));
                state.active_sheet = state.engine.active_sheet();
                state.grow_engine();
                if let Some(c) = content {
                    state.set_cell_inputs_on_sheet(*index, c.inputs.iter().map(|(r, col, i)| (*r, *col, i.as_str())));
                }
                apply_relinks(state, relinks);
                for s in 0..state.sheets.len() {
                    resync(state, s);
                }
                Ok(Op::DeleteSheet { sheet: id })
            }
            Op::DeleteSheet { .. } => {
                let index = &si;
                if state.sheets.len() == 1 {
                    return Err("can't delete the only sheet".into());
                }
                let model = state.sheets[*index].borrow().clone();
                let inputs: Vec<(usize, usize, String)> = (0..model.rows)
                    .flat_map(|r| (0..model.cols).map(move |c| (r, c)))
                    .filter_map(|(r, c)| {
                        let input = state.engine.input_at(*index, r, c);
                        (!input.is_empty()).then_some((r, c, input))
                    })
                    .collect();
                let relinks = relinks_where(state, |s, _, _| s != *index);
                let (name, sheet_id) = (model.name.clone(), model.sheet_id);
                state.delete_sheet(*index)?;
                for s in 0..state.sheets.len() {
                    resync(state, s);
                }
                Ok(Op::AddSheet { index: *index, name, sheet_id: Some(sheet_id), content: Some(Box::new(SheetContent { model, inputs })), relinks })
            }
        }
    }
}

/// The ops that turn `before` into `after` (the same sheet, with the same
/// size and values): per cell, its format, style, border, validation and
/// lock; per line, its size; the merges, the freeze and the sheet-wide
/// properties. Selection and view state aren't document changes.
pub fn diff_ops(before: &SheetModel, after: &SheetModel) -> Vec<Op> {
    let sheet = before.sheet_id;
    let mut ops = Vec::new();
    for r in 0..before.rows.min(after.rows) {
        for c in 0..before.cols.min(after.cols) {
            let (row, col) = (r, c);
            if before.formats[r][c] != after.formats[r][c] {
                ops.push(Op::SetFormat { sheet, row, col, format: after.formats[r][c].clone() });
            }
            if before.styles[r][c] != after.styles[r][c] {
                ops.push(Op::SetStyle { sheet, row, col, style: after.styles[r][c].clone() });
            }
            if before.borders[r][c] != after.borders[r][c] {
                ops.push(Op::SetBorder { sheet, row, col, border: after.borders[r][c].clone() });
            }
            if before.validations[r][c] != after.validations[r][c] {
                ops.push(Op::SetValidation { sheet, row, col, rule: after.validations[r][c].clone() });
            }
            if before.cell_protections[r][c] != after.cell_protections[r][c] {
                ops.push(Op::SetLocked { sheet, row, col, protection: after.cell_protections[r][c].clone() });
            }
        }
    }
    for (axis, old, new) in [(Axis::Rows, &before.row_heights, &after.row_heights), (Axis::Cols, &before.col_widths, &after.col_widths)] {
        for (index, (a, b)) in old.iter().zip(new).enumerate() {
            if (a - b).abs() > f64::EPSILON {
                ops.push(Op::Resize { sheet, axis, index, size: *b });
            }
        }
    }
    for range in before.merges.iter().filter(|m| !after.merges.contains(m)) {
        ops.push(Op::Unmerge { sheet, range: *range });
    }
    for range in after.merges.iter().filter(|m| !before.merges.contains(m)) {
        ops.push(Op::Merge { sheet, range: *range });
    }
    if (before.frozen_rows, before.frozen_cols) != (after.frozen_rows, after.frozen_cols) {
        ops.push(Op::Freeze { sheet, rows: after.frozen_rows, cols: after.frozen_cols });
    }
    for (a, b) in SheetProp::all(before).into_iter().zip(SheetProp::all(after)) {
        if a != b {
            ops.push(Op::SetProp { sheet, prop: b });
        }
    }
    ops
}

/// Tables' ops in the suite's one op shape (ADR 0011): the shared
/// `History` undoes and redoes them.
impl suite_common_core::ops::Op for Op {
    type Doc = WorkbookState;
    type Error = String;

    fn apply(&self, state: &mut WorkbookState) -> Result<Vec<Op>, String> {
        self.apply_one(state).map(|inverse| vec![inverse])
    }
}

/// Apply `op` and return the ops that undo it.
pub fn apply(state: &mut WorkbookState, op: &Op) -> Result<Vec<Op>, String> {
    suite_common_core::ops::Op::apply(op, state)
}

/// Apply `ops` in order, all or nothing: if one fails, the ones before it
/// are undone and the error returned. On success, the ops that undo the
/// whole group, in the order to apply them.
pub fn apply_all(state: &mut WorkbookState, ops: &[Op]) -> Result<Vec<Op>, String> {
    suite_common_core::ops::apply_all(state, ops)
}

/// Blank lines to insert: `count` rows or columns of `sheet`'s default size.
pub fn blank_lines(state: &WorkbookState, sheet: usize, axis: Axis, count: usize) -> Vec<Line> {
    let s = state.sheets[sheet].borrow();
    let (size, across) = match axis {
        Axis::Rows => (crate::sheet::ROW_HEIGHT, s.cols),
        Axis::Cols => (crate::sheet::COL_WIDTH, s.rows),
    };
    vec![Line { size, cells: vec![CellContent::default(); across] }; count]
}

/// After `ops` (an edit, its undo or its redo), show the sheet they
/// changed, as Sheets and Excel do: undoing an edit on another sheet brings
/// that sheet up, and adding a sheet (or undoing its delete) shows it. A
/// sheet that no longer exists (the ops deleted it) is skipped.
pub(super) fn show_changed_sheet(state: &mut WorkbookState, ops: &[Op]) {
    let target = ops.iter().find_map(|op| match op {
        Op::AddSheet { index, .. } => Some(*index).filter(|i| *i < state.sheets.len()),
        _ => op.sheet().and_then(|id| state.sheet_index_for_id(id)),
    });
    if let Some(index) = target {
        if index != state.active_sheet {
            let _ = state.switch_sheet(index);
        }
    }
}

impl super::core::WorkbookController {
    /// Apply `ops` as one undo step named `description`, all or nothing.
    /// Returns false (and records nothing) when the group doesn't apply.
    pub fn apply_ops(&mut self, description: impl Into<String>, ops: Vec<Op>) -> bool {
        if ops.is_empty() {
            return false;
        }
        let applied = {
            let mut state = self.state.borrow_mut();
            let applied = apply_all(&mut state, &ops);
            if applied.is_ok() {
                show_changed_sheet(&mut state, &ops);
            }
            applied
        };
        let Ok(inverses) = applied else { return false };
        self.record(description.into(), inverses);
        true
    }

    /// Record a row or column resize the user has already dragged to
    /// `index`'s current size, from `old`, as one undoable `Resize`.
    pub fn record_resize(&mut self, axis: Axis, index: usize, old: f64) {
        let (sheet, new) = {
            let state = self.state.borrow();
            let mut s = state.sheet_mut();
            let id = s.sheet_id;
            let sizes = match axis {
                Axis::Rows => &mut s.row_heights,
                Axis::Cols => &mut s.col_widths,
            };
            let Some(slot) = sizes.get_mut(index) else { return };
            (id, std::mem::replace(slot, old))
        };
        if (new - old).abs() > f64::EPSILON {
            let what = if axis == Axis::Rows { "Resize Row" } else { "Resize Column" };
            self.apply_ops(what, vec![Op::Resize { sheet, axis, index, size: new }]);
        }
    }

    /// Merge the selection (a 2x2 block from the active cell when one cell
    /// is selected), or unmerge the block the active cell anchors.
    pub fn toggle_merge(&mut self) {
        let op = {
            let state = self.state.borrow();
            let s = state.sheet();
            let (r, c) = (s.selected_row, s.selected_col);
            match s.merges.iter().find(|m| (m.0, m.1) == (r, c)) {
                Some(&range) => Op::Unmerge { sheet: s.sheet_id, range },
                None => {
                    let (r0, c0, r1, c1) = s.selection_rect();
                    let range = if (r0, c0) == (r1, c1) { (r, c, 2, 2) } else { (r0, c0, r1 - r0 + 1, c1 - c0 + 1) };
                    Op::Merge { sheet: s.sheet_id, range }
                }
            }
        };
        let what = if matches!(op, Op::Unmerge { .. }) { "Unmerge Cells" } else { "Merge Cells" };
        self.apply_ops(what, vec![op]);
    }

    /// Insert `count` blank rows or columns before `at` on the active sheet.
    pub fn insert_lines(&mut self, axis: Axis, at: usize, count: usize) {
        let op = {
            let state = self.state.borrow();
            let (pos, id) = (state.active_sheet, state.sheet().sheet_id);
            Op::Insert { sheet: id, axis, at, lines: blank_lines(&state, pos, axis, count), relinks: Vec::new(), merges: None }
        };
        self.apply_ops(if axis == Axis::Rows { "Insert Rows" } else { "Insert Columns" }, vec![op]);
    }

    /// Delete `count` rows or columns from `at` on the active sheet.
    pub fn delete_lines(&mut self, axis: Axis, at: usize, count: usize) {
        let sheet = self.state.borrow().sheet().sheet_id;
        self.apply_ops(if axis == Axis::Rows { "Delete Rows" } else { "Delete Columns" }, vec![Op::Delete { sheet, axis, at, count }]);
    }
}

/// Everything the ops change, for comparing two workbooks: sheet names and
/// ids, sizes, and every cell's input, format, style and border, merges,
/// line sizes and freezes.
#[derive(Debug, PartialEq)]
pub struct WorkbookImage {
    pub sheets: Vec<SheetImage>,
    /// Workbook-level names, `(name, formula)`, sorted.
    pub names: Vec<(String, String)>,
}

#[derive(Debug, PartialEq)]
pub struct SheetImage {
    pub name: String,
    pub sheet_id: u32,
    pub size: (usize, usize),
    pub inputs: Vec<(usize, usize, String)>,
    pub formats: Vec<Vec<NumberFormat>>,
    pub styles: Vec<Vec<CellStyle>>,
    pub borders: Vec<Vec<CellBorder>>,
    pub merges: Vec<(usize, usize, usize, usize)>,
    pub row_heights: Vec<f64>,
    pub col_widths: Vec<f64>,
    pub frozen: (usize, usize),
    pub values: Vec<Vec<String>>,
    pub validations: Vec<Vec<Option<ValidationRule>>>,
    pub locks: Vec<Vec<CellProtection>>,
    pub props: Vec<SheetProp>,
}

impl WorkbookImage {
    pub fn of(state: &WorkbookState) -> Self {
        let sheets = state
            .sheets
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let s = s.borrow();
                let inputs = (0..s.rows)
                    .flat_map(|r| (0..s.cols).map(move |c| (r, c)))
                    .filter_map(|(r, c)| {
                        let input = state.engine.input_at(i, r, c);
                        (!input.is_empty()).then_some((r, c, input))
                    })
                    .collect();
                let mut merges = s.merges.clone();
                merges.sort_unstable();
                SheetImage {
                    name: s.name.clone(),
                    sheet_id: s.sheet_id,
                    size: (s.rows, s.cols),
                    inputs,
                    formats: s.formats.clone(),
                    styles: s.styles.clone(),
                    borders: s.borders.clone(),
                    merges,
                    row_heights: s.row_heights.clone(),
                    col_widths: s.col_widths.clone(),
                    frozen: (s.frozen_rows, s.frozen_cols),
                    values: (0..s.rows).map(|r| (0..s.cols).map(|c| state.engine.cell_at(i, r, c)).collect()).collect(),
                    validations: s.validations.clone(),
                    locks: s.cell_protections.clone(),
                    props: SheetProp::all(&s),
                }
            })
            .collect();
        let mut names: Vec<(String, String)> =
            state.engine.model.workbook.defined_names.iter().map(|n| (n.name.clone(), n.formula.clone())).collect();
        names.sort();
        WorkbookImage { sheets, names }
    }
}
