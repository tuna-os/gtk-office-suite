// fragment.rs — Tables side of the cross-app clipboard.
// SPDX-License-Identifier: GPL-3.0-or-later

pub use suite_common_core::document::{GridCell, MIME};

/// A copied piece of content: the suite clipboard's neutral form (ADR 0012).
/// A Letters or Decks copy reads as neutral text, with what Tables cannot
/// use left out.
pub type Fragment = suite_common_core::document::Fragment;

use crate::engine::TablesEngine;
use crate::sheet::SheetModel;

/// Copy a rectangular selection as a Fragment: each cell's editable input
/// (its formula, or the text it was typed as from the engine) plus the
/// formula flag. The projection (`sheet.data`) is never copied as content:
/// pasting a displayed result back would flatten formulas — and spill
/// results — into constants. The exception is an input protection
/// withholds (TBL-005: a hidden formula on a protected sheet, or a cell
/// that cannot be selected): it copies as its shown value only, so no
/// clipboard format carries the formula.
pub fn copy_range(
    sheet: &SheetModel,
    engine: &TablesEngine,
    r0: usize, c0: usize, r1: usize, c1: usize,
) -> Fragment {
    let mut rows = Vec::new();
    for r in r0..=r1.min(sheet.rows.saturating_sub(1)) {
        let mut row = Vec::new();
        for c in c0..=c1.min(sheet.cols.saturating_sub(1)) {
            if sheet.formula_hidden(r, c) || !sheet.may_select(r, c) {
                row.push(GridCell { value: sheet.cell(r, c).to_string(), formula: None, num_format: None });
                continue;
            }
            let formula = engine.formula(r, c);
            let value = match &formula {
                Some(_) => sheet.cell(r, c).to_string(),
                None => engine.editable_input_at(engine.active_sheet(), r, c),
            };
            row.push(GridCell { value, formula, num_format: None });
        }
        rows.push(row);
    }
    Fragment::Grid(rows)
}

/// Paste a fragment at (row, col): grids land cell-by-cell (formulas
/// re-entered so they re-evaluate), text lands line-per-row. One
/// calculation boundary for the batch; a rejected write fails the paste
/// instead of landing half of it.
pub fn paste_at(
    engine: &mut TablesEngine,
    row: usize, col: usize,
    frag: &Fragment,
) -> Result<(), String> {
    let sheet = engine.active_sheet();
    let mut inputs = Vec::new();
    match frag {
        Fragment::Grid(rows) => {
            for (dr, r) in rows.iter().enumerate() {
                for (dc, cell) in r.iter().enumerate() {
                    let text = match &cell.formula {
                        Some(f) => format!("={f}"),
                        None => cell.value.clone(),
                    };
                    inputs.push((row + dr, col + dc, text));
                }
            }
        }
        Fragment::Text(paras) => {
            for (dr, p) in paras.iter().enumerate() {
                inputs.push((row + dr, col, p.text()));
            }
        }
    }
    engine.put_cells_on_sheet(sheet, inputs.iter().map(|(r, c, t)| (*r, *c, t.as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_paste_round_trip_keeps_formulas_live() {
        let mut e = TablesEngine::new(10, 10).unwrap();
        e.set_cell_text(0, 0, "2").unwrap();
        e.set_cell_text(0, 1, "3").unwrap();
        e.set_cell_text(1, 0, "=A1+B1").unwrap();
        e.evaluate();
        let mut sheet = SheetModel::new("S", 10, 10, 0);
        sheet.sync_from_engine(&e);

        let frag = copy_range(&sheet, &e, 0, 0, 1, 1);
        // Paste into a fresh engine at C1 (row 0, col 2)
        let mut e2 = TablesEngine::new(10, 10).unwrap();
        paste_at(&mut e2, 0, 2, &frag).unwrap();
        assert_eq!(e2.cell(0, 2), "2");
        // The formula was re-entered relative to its new home and evaluates.
        assert!(e2.has_formula(1, 2), "formula did not survive paste");
    }

    #[test]
    fn copy_carries_inputs_never_displayed_results() {
        let mut e = TablesEngine::new(10, 10).unwrap();
        e.set_cell_text(0, 0, "=1+1").unwrap();
        let mut sheet = SheetModel::new("S", 10, 10, 0);
        sheet.sync_from_engine(&e);
        assert_eq!(sheet.cell(0, 0), "2");
        let frag = copy_range(&sheet, &e, 0, 0, 0, 0);
        // The fragment pastes the formula, not the displayed "2": the
        // projection never becomes input.
        let mut e2 = TablesEngine::new(10, 10).unwrap();
        paste_at(&mut e2, 0, 0, &frag).unwrap();
        assert_eq!(e2.input_at(0, 0, 0), "=1+1");
        assert_eq!(e2.cell(0, 0), "2");
    }

    #[test]
    fn text_fragment_pastes_line_per_row() {
        use suite_common_core::document::Paragraph;
        let frag = Fragment::Text(vec![Paragraph::plain("alpha"), Paragraph::plain("beta")]);
        let mut e = TablesEngine::new(5, 5).unwrap();
        paste_at(&mut e, 0, 0, &frag).unwrap();
        assert_eq!(e.cell(0, 0), "alpha");
        assert_eq!(e.cell(1, 0), "beta");
    }
}
