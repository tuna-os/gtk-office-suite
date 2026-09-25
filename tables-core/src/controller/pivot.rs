// SPDX-License-Identifier: GPL-3.0-or-later
//! Pivot-table support: table registration and evaluation.
//! Split out of the former controller.rs (issue #227).


use super::core::WorkbookController;
use super::ops::{Op, SheetProp};

impl WorkbookController {
    /// Add `pivot` to the active sheet and write its table at its target
    /// cell, as one undo step (the table's cells are ordinary inputs).
    pub fn add_pivot_table(&mut self, pivot: crate::sheet::PivotTableSpec) {
        let evaluated_data = self.evaluate_pivot_table(&pivot);
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

    pub fn evaluate_pivot_table(&self, pivot: &crate::sheet::PivotTableSpec) -> Vec<Vec<String>> {
        use crate::sheet::PivotAggFunc;
        use std::collections::BTreeMap;

        let state = self.state.borrow();
        let sheet = state.sheet();
        let (r0, c0, r1, c1) = pivot.source_range;

        if r0 >= r1 || c0 > c1 {
            return vec![vec!["[Empty Pivot]".into()]];
        }

        // Group rows by row_fields
        let mut groups: BTreeMap<Vec<String>, Vec<usize>> = BTreeMap::new();
        for r in (r0 + 1)..=r1 {
            let key: Vec<String> = pivot
                .row_fields
                .iter()
                .map(|f| sheet.data[r].get(f.col_index).cloned().unwrap_or_default())
                .collect();
            groups.entry(key).or_default().push(r);
        }

        let mut grid = Vec::new();

        // Header row
        let mut header = Vec::new();
        for f in &pivot.row_fields {
            header.push(f.name.clone());
        }
        for d in &pivot.data_fields {
            header.push(format!("{:?} of {}", d.func, d.name));
        }
        grid.push(header);

        // Group summary rows
        for (row_key, row_indices) in groups {
            let mut line = row_key;
            for df in &pivot.data_fields {
                let values: Vec<f64> = row_indices
                    .iter()
                    .filter_map(|&r| {
                        sheet.data[r]
                            .get(df.col_index)
                            .and_then(|v| v.parse::<f64>().ok())
                    })
                    .collect();

                let res = match df.func {
                    PivotAggFunc::Sum => values.iter().sum::<f64>(),
                    PivotAggFunc::Count => values.len() as f64,
                    PivotAggFunc::Average => {
                        if values.is_empty() {
                            0.0
                        } else {
                            values.iter().sum::<f64>() / values.len() as f64
                        }
                    }
                    PivotAggFunc::Min => values.iter().copied().fold(f64::INFINITY, f64::min),
                    PivotAggFunc::Max => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                };
                let val_str = if res.is_infinite() {
                    "0".into()
                } else if res.fract() == 0.0 {
                    (res as i64).to_string()
                } else {
                    format!("{:.2}", res)
                };
                line.push(val_str);
            }
            grid.push(line);
        }

        grid
    }
}
