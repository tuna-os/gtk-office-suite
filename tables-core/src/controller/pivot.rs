// SPDX-License-Identifier: GPL-3.0-or-later
//! Pivot-table support: table registration and evaluation.
//! Split out of the former controller.rs (issue #227).


use super::core::WorkbookController;
use super::ops::{Op, SheetProp};

impl WorkbookController {
    /// Add `pivot` to the active sheet and write its table at its target
    /// cell, as one undo step (the table's cells are ordinary inputs).
    pub fn add_pivot_table(&mut self, mut pivot: crate::sheet::PivotTableSpec) {
        let evaluated_data = self.evaluate_pivot_table(&pivot);
        pivot.extent = extent_of(&evaluated_data);
        let (start_r, start_c) = pivot.target_cell;
        let (rows, cols, sheet_id, mut pivots) = {
            let state = self.state.borrow();
            let sheet = state.sheet();
            (sheet.rows, sheet.cols, sheet.sheet_id, sheet.pivot_tables.clone())
        };
        pivots.push(pivot);
        let mut cells = Vec::new();
        for (dr, row) in evaluated_data.into_iter().enumerate() {
            for (dc, val) in row.into_iter().enumerate() {
                if start_r + dr < rows && start_c + dc < cols {
                    cells.push((start_r + dr, start_c + dc, val));
                }
            }
        }
        self.apply_ops(
            "Add Pivot Table",
            vec![
                Op::SetProp { sheet: sheet_id, prop: SheetProp::Pivots(pivots) },
                Op::SetCells { sheet: sheet_id, cells },
            ],
        );
    }

    /// Recompute every pivot table on the active sheet from its source as
    /// it is now, as one undo step, and return how many were refreshed (0 when
    /// there are none, or the sheet's protection refuses the change). Each
    /// table is rewritten at its target; cells of the previous table that
    /// the new one no longer covers are cleared, so a group that vanished
    /// from the source leaves no stale row behind. Like Calc's and Excel's
    /// pivots, a pivot is not rewritten on every edit, only when refreshed.
    pub fn refresh_pivot_tables(&mut self) -> usize {
        let (rows, cols, sheet_id, mut pivots) = {
            let state = self.state.borrow();
            let sheet = state.sheet();
            (sheet.rows, sheet.cols, sheet.sheet_id, sheet.pivot_tables.clone())
        };
        if pivots.is_empty() {
            return 0;
        }
        let mut written = std::collections::BTreeMap::new();
        let mut cleared = std::collections::BTreeSet::new();
        for pivot in &mut pivots {
            let grid = self.evaluate_pivot_table(pivot);
            let (start_r, start_c) = pivot.target_cell;
            for dr in 0..pivot.extent.0 {
                for dc in 0..pivot.extent.1 {
                    cleared.insert((start_r + dr, start_c + dc));
                }
            }
            pivot.extent = extent_of(&grid);
            for (dr, row) in grid.into_iter().enumerate() {
                for (dc, val) in row.into_iter().enumerate() {
                    written.insert((start_r + dr, start_c + dc), val);
                }
            }
        }
        let count = pivots.len();
        let mut cells: Vec<(usize, usize, String)> = cleared
            .into_iter()
            .filter(|at| !written.contains_key(at))
            .map(|(r, c)| (r, c, String::new()))
            .collect();
        cells.extend(written.into_iter().map(|((r, c), v)| (r, c, v)));
        cells.retain(|(r, c, _)| *r < rows && *c < cols);
        let applied = self.apply_ops(
            "Refresh Pivot Tables",
            vec![
                Op::SetProp { sheet: sheet_id, prop: SheetProp::Pivots(pivots) },
                Op::SetCells { sheet: sheet_id, cells },
            ],
        );
        if applied { count } else { 0 }
    }

    pub fn evaluate_pivot_table(&self, pivot: &crate::sheet::PivotTableSpec) -> Vec<Vec<String>> {
        use crate::sheet::PivotAggFunc;
        use std::collections::{BTreeMap, BTreeSet};

        let state = self.state.borrow();
        let sheet = state.sheet();
        let (r0, _c0, r1, _c1) = pivot.source_range;

        if r0 >= r1 || pivot.data_fields.is_empty() || r0 >= sheet.data.len() {
            return vec![vec!["[Empty Pivot]".into()]];
        }

        // Keep row and column categories ordered and aggregate each intersection
        // from the source rows. `source_range` is inclusive; its first row is
        // the header row and is not part of the data.
        let mut groups: BTreeMap<Vec<String>, BTreeMap<Vec<String>, Vec<usize>>> = BTreeMap::new();
        let mut column_keys = BTreeSet::new();
        for r in (r0 + 1)..=r1.min(sheet.data.len() - 1) {
            let key_for = |fields: &[crate::sheet::PivotField]| {
                fields
                    .iter()
                    .map(|field| {
                        sheet.data[r]
                            .get(field.col_index)
                            .cloned()
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
            };
            let row_key = key_for(&pivot.row_fields);
            let column_key = key_for(&pivot.col_fields);
            column_keys.insert(column_key.clone());
            groups
                .entry(row_key)
                .or_default()
                .entry(column_key)
                .or_default()
                .push(r);
        }

        // Without column fields, retain the traditional one-summary-column
        // layout. With column fields, each category gets one column per value
        // field, yielding a cross-tab rather than silently ignoring the fields.
        if pivot.col_fields.is_empty() {
            column_keys.insert(Vec::new());
        }
        let mut header: Vec<String> = pivot
            .row_fields
            .iter()
            .map(|field| field.name.clone())
            .collect();
        for column_key in &column_keys {
            for data_field in &pivot.data_fields {
                let label = format!("{:?} of {}", data_field.func, data_field.name);
                header.push(if column_key.is_empty() {
                    label
                } else {
                    format!("{} - {}", column_key.join(" / "), label)
                });
            }
        }
        let mut grid = vec![header];

        for (row_key, column_groups) in groups {
            let mut line = row_key;
            for column_key in &column_keys {
                let row_indices = column_groups.get(column_key);
                for data_field in &pivot.data_fields {
                    let values: Vec<f64> = row_indices
                        .into_iter()
                        .flatten()
                        .filter_map(|&r| {
                            sheet.data[r]
                                .get(data_field.col_index)
                                .and_then(|value| value.parse::<f64>().ok())
                        })
                        .collect();

                    let result = match data_field.func {
                        PivotAggFunc::Sum => values.iter().sum::<f64>(),
                        PivotAggFunc::Count => values.len() as f64,
                        PivotAggFunc::Average if !values.is_empty() => {
                            values.iter().sum::<f64>() / values.len() as f64
                        }
                        PivotAggFunc::Average => 0.0,
                        PivotAggFunc::Min => values.iter().copied().reduce(f64::min).unwrap_or(0.0),
                        PivotAggFunc::Max => values.iter().copied().reduce(f64::max).unwrap_or(0.0),
                    };
                    line.push(if result == 0.0 {
                        "0".into()
                    } else if result.fract() == 0.0 {
                        format!("{result:.0}")
                    } else {
                        format!("{result:.2}")
                    });
                }
            }
            grid.push(line);
        }

        grid
    }
}

/// The rows and columns a written pivot table covers.
fn extent_of(grid: &[Vec<String>]) -> (usize, usize) {
    (grid.len(), grid.iter().map(Vec::len).max().unwrap_or(0))
}
