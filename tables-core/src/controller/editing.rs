// SPDX-License-Identifier: GPL-3.0-or-later
//! Cell-level editing commands: direct edits, sheet mutations, paste, and fill.
//! Split out of the former controller.rs (issue #227).

use crate::fill::{extend_fill, infer_fill, FillDirection};
use crate::fragment::Fragment;
use crate::sheet::SheetModel;


use super::core::WorkbookController;

impl WorkbookController {
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
    pub fn fill(&mut self, sel: (usize, usize, usize, usize), drag_row: usize, drag_col: usize) {
        let Some((direction, distance)) = infer_fill(sel, drag_row, drag_col) else {
            return;
        };
        let (top, left, bottom, right) = sel;
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;

        // `formula_source` is the originating cell of a copied formula —
        // extend_fill/tile_fill only ever carry formula text verbatim,
        // so the reference shift happens in a second pass below, once we
        // have mutable access to the engine.
        struct Change {
            row: usize,
            col: usize,
            new_input: String,
            formula_source: Option<(usize, usize)>,
        }
        let mut changes: Vec<Change> = Vec::new();
        match direction {
            FillDirection::Down => {
                for c in left..=right {
                    let source: Vec<_> = (top..=bottom)
                        .map(|r| {
                            let input = state.cell_input(r, c);
                            let is_formula = input.starts_with('=');
                            (input, is_formula)
                        })
                        .collect();
                    let filled = extend_fill(&source, distance);
                    for (i, (input, is_formula)) in filled.into_iter().enumerate() {
                        let row = bottom + 1 + i;
                        let old_input = state.cell_input(row, c);
                        if old_input != input {
                            let formula_source = is_formula.then(|| (top + i % source.len(), c));
                            changes.push(Change {
                                row,
                                col: c,
                                new_input: input,
                                formula_source,
                            });
                        }
                    }
                }
            }
            FillDirection::Right => {
                for r in top..=bottom {
                    let source: Vec<_> = (left..=right)
                        .map(|c| {
                            let input = state.cell_input(r, c);
                            let is_formula = input.starts_with('=');
                            (input, is_formula)
                        })
                        .collect();
                    let filled = extend_fill(&source, distance);
                    for (i, (input, is_formula)) in filled.into_iter().enumerate() {
                        let col = right + 1 + i;
                        let old_input = state.cell_input(r, col);
                        if old_input != input {
                            let formula_source = is_formula.then(|| (r, left + i % source.len()));
                            changes.push(Change {
                                row: r,
                                col,
                                new_input: input,
                                formula_source,
                            });
                        }
                    }
                }
            }
            FillDirection::Up => {
                for c in left..=right {
                    let source: Vec<_> = (top..=bottom)
                        .map(|r| {
                            let input = state.cell_input(r, c);
                            let is_formula = input.starts_with('=');
                            (input, is_formula)
                        })
                        .collect();
                    let filled = extend_fill(&source, distance);
                    // Adjacent-to-selection cell (top - 1) gets the first
                    // tile element, same convention as Down's bottom + 1.
                    for (i, (input, is_formula)) in filled.into_iter().enumerate() {
                        let row = top - 1 - i;
                        let old_input = state.cell_input(row, c);
                        if old_input != input {
                            let formula_source = is_formula.then(|| (top + i % source.len(), c));
                            changes.push(Change {
                                row,
                                col: c,
                                new_input: input,
                                formula_source,
                            });
                        }
                    }
                }
            }
            FillDirection::Left => {
                for r in top..=bottom {
                    let source: Vec<_> = (left..=right)
                        .map(|c| {
                            let input = state.cell_input(r, c);
                            let is_formula = input.starts_with('=');
                            (input, is_formula)
                        })
                        .collect();
                    let filled = extend_fill(&source, distance);
                    for (i, (input, is_formula)) in filled.into_iter().enumerate() {
                        let col = left - 1 - i;
                        let old_input = state.cell_input(r, col);
                        if old_input != input {
                            let formula_source = is_formula.then(|| (r, left + i % source.len()));
                            changes.push(Change {
                                row: r,
                                col,
                                new_input: input,
                                formula_source,
                            });
                        }
                    }
                }
            }
        }
        drop(state);
        if changes.iter().any(|c| c.formula_source.is_some()) {
            let mut state = self.state.borrow_mut();
            for change in changes.iter_mut() {
                if let Some(source) = change.formula_source {
                    change.new_input = state.engine.extend_input(
                        &change.new_input,
                        source,
                        (change.row, change.col),
                    );
                }
            }
        }
        let cells: Vec<(usize, usize, String)> = changes.into_iter().map(|c| (c.row, c.col, c.new_input)).collect();
        if !cells.is_empty() {
            self.apply_ops("Fill", vec![super::ops::Op::SetCells { sheet: sheet_id, cells }]);
        }
    }
}
