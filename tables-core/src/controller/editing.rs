// SPDX-License-Identifier: GPL-3.0-or-later
//! Cell-level editing commands: direct edits, sheet mutations, paste, and fill.
//! Split out of the former controller.rs (issue #227).

use crate::fill::{extend_fill_kind, infer_fill, FillDirection};
use suite_common_core::format::NumberFormatKind;

/// A cell position, `(row, col)`.
type Pos = (usize, usize);
use crate::fragment::Fragment;
use crate::sheet::SheetModel;


use super::core::WorkbookController;

impl WorkbookController {
    /// Set, change or (with `None` or empty text) remove the note on a
    /// cell of the active sheet, as one undo step. False if nothing
    /// changed.
    pub fn set_note(&mut self, row: usize, col: usize, note: Option<String>) -> bool {
        let (sheet, old) = {
            let state = self.state.borrow();
            let s = state.sheet();
            let Some(old) = s.notes.get(row).and_then(|r| r.get(col)) else { return false };
            (s.sheet_id, old.clone())
        };
        let note = note.filter(|n| !n.trim().is_empty());
        if note == old {
            return false;
        }
        let description = if note.is_none() { "Delete Note" } else { "Edit Note" };
        self.apply_ops(description, vec![super::ops::Op::SetNote { sheet, row, col, note }])
    }

    pub fn edit_cell(&mut self, row: usize, col: usize, input: impl Into<String>) {
        let new_input = input.into();
        let state = self.state.borrow();
        let sheet = state.sheet();
        if sheet.protection.protected && sheet.cell_protections[row][col].locked {
            return;
        }
        let old_input = state.cell_input(row, col);
        let active = sheet.sheet_id;
        drop(sheet);
        drop(state);
        if old_input == new_input {
            return;
        }
        self.apply_ops("Edit Cell", vec![super::ops::Op::SetCell { sheet: active, row, col, input: new_input }]);
    }

    /// Change the active sheet's formatting and properties with a
    /// closure, recorded as the ops that make the difference (one undo
    /// step). The closure works on a copy; what it changed becomes
    /// `SetFormat`, `SetStyle`, `SetProp`… ops (`super::ops::diff_ops`).
    /// Cell values and the sheet's size don't change this way: those go
    /// through `edit_cell`, paste, fill and the row/column ops.
    pub fn mutate_sheet(
        &mut self,
        description: &'static str,
        mutation: impl FnOnce(&mut SheetModel),
    ) {
        let before = self.state.borrow().sheet().clone();
        let mut after = before.clone();
        mutation(&mut after);
        debug_assert!(
            after.data == before.data && (after.rows, after.cols) == (before.rows, before.cols),
            "mutate_sheet can't change values or size"
        );
        self.apply_ops(description, super::ops::diff_ops(&before, &after));
    }

    pub fn paste_fragment(&mut self, row: usize, col: usize, fragment: &Fragment) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let (rows, cols) = {
            let sheet = state.sheet();
            (sheet.rows, sheet.cols)
        };
        let mut new_inputs = Vec::new();
        match fragment {
            Fragment::Grid(grid) => {
                for (dr, source_row) in grid.iter().enumerate() {
                    for (dc, cell) in source_row.iter().enumerate() {
                        if row + dr >= rows || col + dc >= cols {
                            continue;
                        }
                        let input = cell
                            .formula
                            .as_ref()
                            .map(|formula| format!("={formula}"))
                            .unwrap_or_else(|| cell.value.clone());
                        new_inputs.push((row + dr, col + dc, input));
                    }
                }
            }
            Fragment::Text(paragraphs) => {
                for (dr, paragraph) in paragraphs.iter().enumerate() {
                    if row + dr < rows && col < cols {
                        new_inputs.push((row + dr, col, paragraph.text()));
                    }
                }
            }
        }
        let cells: Vec<(usize, usize, String)> =
            new_inputs.into_iter().filter(|(r, c, new_input)| state.cell_input(*r, *c) != *new_input).collect();
        drop(state);
        if !cells.is_empty() {
            self.apply_ops("Paste Cells", vec![super::ops::Op::SetCells { sheet: sheet_id, cells }]);
        }
    }
    /// Drag the fill handle of `sel` to `(drag_row, drag_col)`: continue
    /// what the selection holds into the cells dragged over, as one undo
    /// step, the way Excel and Calc's autofill does (`crate::fill`):
    /// number series, dates, weekdays and months, numbered text, and
    /// anything else repeated. Filling up or left continues the series
    /// backwards. The cells filled take the formats, styles and borders of
    /// the cells they continue.
    pub fn fill(&mut self, sel: (usize, usize, usize, usize), drag_row: usize, drag_col: usize) {
        use super::ops::Op;
        let Some((direction, distance)) = infer_fill(sel, drag_row, drag_col) else {
            return;
        };
        let (top, left, bottom, right) = sel;
        let state = self.state.borrow();
        let sheet = state.sheet();
        let sheet_id = sheet.sheet_id;

        // Each line runs along the drag: its source cells in drag order,
        // and where its filled cells go.
        let vertical = matches!(direction, FillDirection::Down | FillDirection::Up);
        let lines: Vec<(Vec<Pos>, Vec<Pos>)> = if vertical {
            (left..=right)
                .map(|c| match direction {
                    FillDirection::Down => ((top..=bottom).map(|r| (r, c)).collect(), (1..=distance).map(|i| (bottom + i, c)).collect()),
                    _ => ((top..=bottom).rev().map(|r| (r, c)).collect(), (1..=distance).map(|i| (top - i, c)).collect()),
                })
                .collect()
        } else {
            (top..=bottom)
                .map(|r| match direction {
                    FillDirection::Right => ((left..=right).map(|c| (r, c)).collect(), (1..=distance).map(|i| (r, right + i)).collect()),
                    _ => ((left..=right).rev().map(|c| (r, c)).collect(), (1..=distance).map(|i| (r, left - i)).collect()),
                })
                .collect()
        };

        // `formula_source` is the cell a copied formula came from: its
        // references shift in a second pass, with the engine.
        let mut cells: Vec<(usize, usize, String, Option<Pos>)> = Vec::new();
        let mut ops: Vec<Op> = Vec::new();
        for (sources, targets) in &lines {
            let source: Vec<(String, bool)> = sources
                .iter()
                .map(|&(r, c)| {
                    let input = state.cell_input(r, c);
                    let is_formula = input.starts_with('=');
                    (input, is_formula)
                })
                .collect();
            let dates = sources.iter().all(|&(r, c)| {
                matches!(sheet.formats[r][c].kind, NumberFormatKind::Date(_) | NumberFormatKind::DateTime(_))
            });
            let filled = extend_fill_kind(&source, targets.len(), dates);
            for (i, ((input, is_formula), &(row, col))) in filled.into_iter().zip(targets).enumerate() {
                let from = sources[i % sources.len()];
                if state.cell_input(row, col) != input {
                    cells.push((row, col, input, is_formula.then_some(from)));
                }
                if sheet.formats[row][col] != sheet.formats[from.0][from.1] {
                    ops.push(Op::SetFormat { sheet: sheet_id, row, col, format: sheet.formats[from.0][from.1].clone() });
                }
                if sheet.styles[row][col] != sheet.styles[from.0][from.1] {
                    ops.push(Op::SetStyle { sheet: sheet_id, row, col, style: sheet.styles[from.0][from.1].clone() });
                }
                if sheet.borders[row][col] != sheet.borders[from.0][from.1] {
                    ops.push(Op::SetBorder { sheet: sheet_id, row, col, border: sheet.borders[from.0][from.1].clone() });
                }
            }
        }
        drop(sheet);
        drop(state);
        if cells.iter().any(|c| c.3.is_some()) {
            let mut state = self.state.borrow_mut();
            for (row, col, input, from) in cells.iter_mut() {
                if let Some(from) = *from {
                    *input = state.engine.extend_input(input, from, (*row, *col));
                }
            }
        }
        let cells: Vec<(usize, usize, String)> = cells.into_iter().map(|(r, c, input, _)| (r, c, input)).collect();
        if !cells.is_empty() {
            ops.insert(0, Op::SetCells { sheet: sheet_id, cells });
        }
        if !ops.is_empty() {
            self.apply_ops("Fill", ops);
        }
    }
}
