// fragment.rs — Tables side of the cross-app clipboard.
// SPDX-License-Identifier: GPL-3.0-or-later

pub use letters_core::fragment::{Fragment, GridCell, MIME};

use crate::engine::TablesEngine;
use crate::sheet::SheetModel;

/// Copy a rectangular selection as a Fragment (values + formulas).
pub fn copy_range(
    sheet: &SheetModel,
    engine: &TablesEngine,
    r0: usize, c0: usize, r1: usize, c1: usize,
) -> Fragment {
    let mut rows = Vec::new();
    for r in r0..=r1.min(sheet.rows.saturating_sub(1)) {
        let mut row = Vec::new();
        for c in c0..=c1.min(sheet.cols.saturating_sub(1)) {
            row.push(GridCell {
                value: sheet.data[r][c].clone(),
                formula: engine.formula(r, c),
                num_format: None,
            });
        }
        rows.push(row);
    }
    Fragment::Grid(rows)
}

/// Paste a fragment at (row, col): grids land cell-by-cell (formulas
/// re-entered so they re-evaluate), text lands line-per-row.
pub fn paste_at(
    engine: &mut TablesEngine,
    row: usize, col: usize,
    frag: &Fragment,
) {
    match frag {
        Fragment::Grid(rows) => {
            for (dr, r) in rows.iter().enumerate() {
                for (dc, cell) in r.iter().enumerate() {
                    let text = match &cell.formula {
                        Some(f) => format!("={f}"),
                        None => cell.value.clone(),
                    };
                    engine.set_cell_text(row + dr, col + dc, &text);
                }
            }
        }
        Fragment::Text(paras) => {
            for (dr, p) in paras.iter().enumerate() {
                engine.set_cell_text(row + dr, col, &p.text());
            }
        }
    }
    engine.evaluate();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_paste_round_trip_keeps_formulas_live() {
        let mut e = TablesEngine::new(10, 10).unwrap();
        e.set_cell_text(0, 0, "2");
        e.set_cell_text(0, 1, "3");
        e.set_cell_text(1, 0, "=A1+B1");
        e.evaluate();
        let mut sheet = SheetModel::new("S", 10, 10, 0);
        sheet.sync_from_engine(&e);

        let frag = copy_range(&sheet, &e, 0, 0, 1, 1);
        // Paste into a fresh engine at C1 (row 0, col 2)
        let mut e2 = TablesEngine::new(10, 10).unwrap();
        paste_at(&mut e2, 0, 2, &frag);
        assert_eq!(e2.cell(0, 2), "2");
        // The formula was re-entered relative to its new home and evaluates.
        assert!(e2.has_formula(1, 2), "formula did not survive paste");
    }

    #[test]
    fn text_fragment_pastes_line_per_row() {
        let d = letters_core::model::Document::from_plain_text("alpha\nbeta");
        let frag = Fragment::Text(d.paragraphs);
        let mut e = TablesEngine::new(5, 5).unwrap();
        paste_at(&mut e, 0, 0, &frag);
        assert_eq!(e.cell(0, 0), "alpha");
        assert_eq!(e.cell(1, 0), "beta");
    }
}
