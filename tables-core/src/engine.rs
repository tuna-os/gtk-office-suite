// engine.rs — Spreadsheet engine using IronCalc for formula evaluation.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Decision: IronCalc (v0.7.1) over Formualizer.
// Rationale:
//   - IronCalc is MIT/Apache 2.0 licensed (Formualizer license unclear)
//   - 83 functions covering all common spreadsheet operations
//   - Pure Rust, minimal dependencies
//   - Active development, v0.7.1 stable
//   - Simpler API surface for our needs
//   - XLSX import/export via calamine + rust_xlsxwriter (existing deps)
//
// Architecture:
//   TablesEngine wraps ironcalc_base::Model and provides:
//   - Cell value get/set with formula detection (= prefix)
//   - Formula evaluation via model.evaluate()
//   - Column/row management
//   - Grid data export for Cairo rendering

use ironcalc_base::expressions::types::{Area, CellReferenceIndex};
use ironcalc_base::{cell::CellValue, Model};

/// The Tables spreadsheet engine.
pub struct TablesEngine {
    pub model: Model<'static>,
    pub rows: usize,
    pub cols: usize,
    active_sheet: usize,
}

impl TablesEngine {
    /// Create a new empty spreadsheet.
    pub fn new(rows: usize, cols: usize) -> Result<Self, String> {
        let model = Model::new_empty("Sheet1", "en", "UTC", "en")
            .map_err(|e| format!("Failed to create engine: {}", e))?;
        Ok(TablesEngine {
            model,
            rows,
            cols,
            active_sheet: 0,
        })
    }

    /// Add a worksheet to the workbook and return its engine index.
    pub fn add_sheet(&mut self, name: &str) -> Result<usize, String> {
        self.model.add_sheet(name)?;
        Ok(self.model.workbook.worksheets.len() - 1)
    }

    pub fn rename_sheet(&mut self, index: usize, name: &str) -> Result<(), String> {
        self.model.rename_sheet_by_index(index as u32, name)
    }

    pub fn set_active_sheet(&mut self, index: usize) -> Result<(), String> {
        if index >= self.model.workbook.worksheets.len() {
            return Err(format!("Sheet index {index} does not exist"));
        }
        self.active_sheet = index;
        Ok(())
    }

    pub fn active_sheet(&self) -> usize {
        self.active_sheet
    }

    pub fn sheet_count(&self) -> usize {
        self.model.workbook.worksheets.len()
    }

    /// The IronCalc worksheet identity at a position. Stable across add,
    /// delete, and reorder — unlike the position itself.
    pub fn sheet_id_at(&self, index: usize) -> Option<u32> {
        self.model.workbook.worksheets.get(index).map(|w| w.sheet_id)
    }

    pub fn index_for_sheet_id(&self, id: u32) -> Option<usize> {
        self.model
            .workbook
            .worksheets
            .iter()
            .position(|w| w.sheet_id == id)
    }

    /// Delete a worksheet. Fails if it is the only sheet.
    pub fn delete_sheet(&mut self, index: usize) -> Result<(), String> {
        self.model.delete_sheet(index as u32)?;
        let count = self.sheet_count();
        if self.active_sheet >= count {
            self.active_sheet = count - 1;
        } else if self.active_sheet > index {
            self.active_sheet -= 1;
        }
        self.model.evaluate();
        Ok(())
    }

    /// Rebuild the workbook with worksheets in `new_order` (a permutation of
    /// current indices). IronCalc has no native sheet-move operation, so this
    /// replays every cell's input text into a fresh model in the new order.
    /// Cross-sheet formulas reference sheets by name, not position, so they
    /// keep resolving correctly across the rebuild.
    pub fn reorder_sheets(&mut self, new_order: &[usize]) -> Result<(), String> {
        let sheet_count = self.sheet_count();
        if new_order.len() != sheet_count {
            return Err("new_order must cover every sheet exactly once".into());
        }
        let mut seen = vec![false; sheet_count];
        for &i in new_order {
            if i >= sheet_count || std::mem::replace(&mut seen[i], true) {
                return Err("new_order must be a permutation of existing sheet indices".into());
            }
        }

        let names: Vec<String> = (0..sheet_count)
            .map(|i| self.sheet_name_at(i).unwrap_or_default())
            .collect();
        let inputs: Vec<Vec<(usize, usize, String)>> = (0..sheet_count)
            .map(|i| {
                let mut cells = Vec::new();
                for r in 0..self.rows {
                    for c in 0..self.cols {
                        let input = self
                            .formula_at(i, r, c)
                            .map(|f| format!("={f}"))
                            .unwrap_or_else(|| self.cell_at(i, r, c));
                        if !input.is_empty() {
                            cells.push((r, c, input));
                        }
                    }
                }
                cells
            })
            .collect();
        // Rebuilding assigns fresh IronCalc sheet_ids, so the old active
        // sheet's id will not exist in the new model. Track identity by
        // position within `new_order` instead.
        let active_old_index = self.active_sheet;

        // The placeholder name only needs to be valid and 'static; it is
        // immediately overwritten with the real (non-'static) target name,
        // which `rename_sheet_by_index` copies into an owned `String`.
        let mut model = Model::new_empty("Sheet1", "en", "UTC", "en")
            .map_err(|e| format!("Failed to rebuild workbook: {e}"))?;
        model.rename_sheet_by_index(0, &names[new_order[0]])?;
        for &old_idx in &new_order[1..] {
            model.add_sheet(&names[old_idx])?;
        }
        self.model = model;

        for (new_idx, &old_idx) in new_order.iter().enumerate() {
            for (r, c, input) in &inputs[old_idx] {
                let rr = *r as i32 + 1;
                let cc = *c as i32 + 1;
                let _ = self.model.set_user_input(new_idx as u32, rr, cc, input.clone());
            }
        }
        self.model.evaluate();
        self.active_sheet = new_order
            .iter()
            .position(|&old_idx| old_idx == active_old_index)
            .unwrap_or(0);
        Ok(())
    }

    pub fn sheet_name_at(&self, index: usize) -> Option<String> {
        self.model.workbook.worksheets.get(index).map(|w| w.name.clone())
    }

    /// Get cell value as a display string.
    pub fn cell(&self, row: usize, col: usize) -> String {
        self.cell_at(self.active_sheet, row, col)
    }

    pub fn cell_at(&self, sheet: usize, row: usize, col: usize) -> String {
        let r = row as i32 + 1; // IronCalc uses 1-based, i32
        let c = col as i32 + 1;
        match self.model.get_cell_value_by_index(sheet as u32, r, c) {
            Ok(CellValue::String(s)) => s,
            Ok(CellValue::Number(n)) => {
                if n.fract() == 0.0 && (n.abs() < 1e15) {
                    format!("{}", n as i64)
                } else {
                    format!("{:.2}", n)
                }
            }
            Ok(CellValue::Boolean(b)) => format!("{}", b),
            Ok(CellValue::None) => String::new(),
            Err(_) => String::new(),
        }
    }

    /// Set cell value or formula (if starts with '=').
    pub fn set_cell_text(&mut self, row: usize, col: usize, val: &str) {
        let r = row as i32 + 1; // IronCalc uses 1-based, i32
        let c = col as i32 + 1;
        let _ = self
            .model
            .set_user_input(self.active_sheet as u32, r, c, val.to_string());
        // Re-evaluate formulas
        self.model.evaluate();
    }

    /// Evaluate all formulas.
    pub fn evaluate(&mut self) {
        self.model.evaluate();
    }

    /// Translate relative references as a cell input moves within the grid.
    pub fn move_input(&mut self, input: &str, from: (usize, usize), to: (usize, usize)) -> String {
        if !input.starts_with('=') || from == to {
            return input.to_string();
        }
        let source = CellReferenceIndex {
            sheet: self.active_sheet as u32,
            row: from.0 as i32 + 1,
            column: from.1 as i32 + 1,
        };
        let target = CellReferenceIndex {
            sheet: self.active_sheet as u32,
            row: to.0 as i32 + 1,
            column: to.1 as i32 + 1,
        };
        let area = Area {
            sheet: self.active_sheet as u32,
            row: 1,
            column: 1,
            width: self.cols as i32,
            height: self.rows as i32,
        };
        self.model
            .move_cell_value_to_area(input, &source, &target, &area)
            .unwrap_or_else(|_| input.to_string())
    }

    /// Adjust a formula's relative references as if it were copied (not
    /// moved) from `from` to `to` — e.g. `=A1` copied from A1 down to A2
    /// becomes `=A2`, but `=$A$1` stays `=$A$1`. Unlike `move_input`
    /// (which updates *other* cells' references to point at a relocated
    /// cell), this is the fill-handle/copy-paste semantics: only the
    /// formula text itself changes, nothing else in the sheet does.
    pub fn extend_input(&mut self, input: &str, from: (usize, usize), to: (usize, usize)) -> String {
        if !input.starts_with('=') || from == to {
            return input.to_string();
        }
        let source = CellReferenceIndex {
            sheet: self.active_sheet as u32,
            row: from.0 as i32 + 1,
            column: from.1 as i32 + 1,
        };
        let target = CellReferenceIndex {
            sheet: self.active_sheet as u32,
            row: to.0 as i32 + 1,
            column: to.1 as i32 + 1,
        };
        self.model
            .extend_copied_value(input, &source, &target)
            .unwrap_or_else(|_| input.to_string())
    }

    /// Check if cell contains a formula (starts with '=').
    /// The formula for a cell (without leading '='), if it has one.
    pub fn formula(&self, row: usize, col: usize) -> Option<String> {
        self.formula_at(self.active_sheet, row, col)
    }

    pub fn formula_at(&self, sheet: usize, row: usize, col: usize) -> Option<String> {
        let r = row as i32 + 1;
        let c = col as i32 + 1;
        self.model
            .get_cell_formula(sheet as u32, r, c)
            .ok()
            .flatten()
            .map(|f| f.trim_start_matches('=').to_string())
    }

    pub fn has_formula(&self, row: usize, col: usize) -> bool {
        let r = row as i32 + 1;
        let c = col as i32 + 1;
        // Check raw cell for formula flag
        if let Ok(ws) = self.model.workbook.worksheet(self.active_sheet as u32) {
            if let Some(cell) = ws.cell(r, c) {
                return cell.has_formula();
            }
        }
        false
    }

    /// Export grid data as Vec<Vec<String>> for Cairo rendering.
    pub fn to_grid(&self) -> Vec<Vec<String>> {
        let mut grid = Vec::with_capacity(self.rows);
        for r in 0..self.rows {
            let mut row = Vec::with_capacity(self.cols);
            for c in 0..self.cols {
                row.push(self.cell(r, c));
            }
            grid.push(row);
        }
        grid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_creation() {
        let engine = TablesEngine::new(10, 5).unwrap();
        assert_eq!(engine.rows, 10);
        assert_eq!(engine.cols, 5);
    }

    #[test]
    fn test_set_and_get_cell() {
        let mut engine = TablesEngine::new(5, 5).unwrap();
        engine.set_cell_text(0, 0, "Hello");
        assert_eq!(engine.cell(0, 0), "Hello");
    }

    #[test]
    fn test_formula_sum() {
        let mut engine = TablesEngine::new(5, 5).unwrap();
        engine.set_cell_text(0, 0, "10");
        engine.set_cell_text(1, 0, "20");
        engine.set_cell_text(2, 0, "30");
        engine.set_cell_text(2, 1, "=SUM(A1:A3)");
        assert_eq!(engine.cell(2, 1), "60");
    }

    #[test]
    fn test_formula_concat() {
        let mut engine = TablesEngine::new(3, 3).unwrap();
        engine.set_cell_text(0, 0, "Hello");
        engine.set_cell_text(0, 1, "World");
        engine.set_cell_text(0, 2, "=CONCAT(A1, \" \", B1)");
        assert_eq!(engine.cell(0, 2), "Hello World");
    }

    #[test]
    fn extend_input_shifts_relative_references() {
        let mut engine = TablesEngine::new(5, 5).unwrap();
        let shifted = engine.extend_input("=A1", (0, 0), (1, 0));
        assert_eq!(shifted, "=A2");
    }

    #[test]
    fn extend_input_leaves_absolute_references_unchanged() {
        let mut engine = TablesEngine::new(5, 5).unwrap();
        let shifted = engine.extend_input("=$A$1", (0, 0), (1, 0));
        assert_eq!(shifted, "=$A$1");
    }

    #[test]
    fn extend_input_leaves_non_formulas_unchanged() {
        let mut engine = TablesEngine::new(5, 5).unwrap();
        assert_eq!(engine.extend_input("42", (0, 0), (1, 0)), "42");
    }

    #[test]
    fn extend_input_same_source_and_target_is_a_no_op() {
        let mut engine = TablesEngine::new(5, 5).unwrap();
        assert_eq!(engine.extend_input("=A1+1", (2, 2), (2, 2)), "=A1+1");
    }

    #[test]
    fn test_grid_export() {
        let mut engine = TablesEngine::new(2, 2).unwrap();
        engine.set_cell_text(0, 0, "A1");
        engine.set_cell_text(0, 1, "B1");
        engine.set_cell_text(1, 0, "A2");
        let grid = engine.to_grid();
        assert_eq!(grid[0][0], "A1");
        assert_eq!(grid[0][1], "B1");
        assert_eq!(grid[1][0], "A2");
    }

    #[test]
    fn test_empty_cell() {
        let engine = TablesEngine::new(3, 3).unwrap();
        assert_eq!(engine.cell(0, 0), "");
    }

    #[test]
    fn test_has_formula_detection() {
        let mut engine = TablesEngine::new(3, 3).unwrap();
        engine.set_cell_text(0, 0, "=1+1");
        assert!(engine.has_formula(0, 0));
        engine.set_cell_text(0, 1, "not a formula");
        assert!(!engine.has_formula(0, 1));
    }

    #[test]
    fn cross_sheet_formula_recalculates() {
        let mut engine = TablesEngine::new(3, 3).unwrap();
        engine.set_cell_text(0, 0, "7");
        engine.add_sheet("Sheet2").unwrap();
        engine.set_active_sheet(1).unwrap();
        engine.set_cell_text(0, 0, "=Sheet1!A1*2");
        assert_eq!(engine.cell(0, 0), "14");

        engine.set_active_sheet(0).unwrap();
        engine.set_cell_text(0, 0, "9");
        engine.set_active_sheet(1).unwrap();
        assert_eq!(engine.cell(0, 0), "18");
    }

    #[test]
    fn delete_sheet_removes_only_that_sheet() {
        let mut engine = TablesEngine::new(3, 3).unwrap();
        engine.set_cell_text(0, 0, "1");
        engine.add_sheet("Sheet2").unwrap();
        engine.set_active_sheet(1).unwrap();
        engine.set_cell_text(0, 0, "2");
        engine.add_sheet("Sheet3").unwrap();
        engine.set_active_sheet(2).unwrap();
        engine.set_cell_text(0, 0, "3");

        engine.delete_sheet(1).unwrap();
        assert_eq!(engine.sheet_count(), 2);
        assert_eq!(engine.sheet_name_at(0).as_deref(), Some("Sheet1"));
        assert_eq!(engine.sheet_name_at(1).as_deref(), Some("Sheet3"));
        assert_eq!(engine.cell_at(0, 0, 0), "1");
        assert_eq!(engine.cell_at(1, 0, 0), "3");
    }

    #[test]
    fn delete_sheet_rejects_the_only_sheet() {
        let mut engine = TablesEngine::new(2, 2).unwrap();
        assert!(engine.delete_sheet(0).is_err());
    }

    #[test]
    fn reorder_sheets_moves_content_and_keeps_cross_sheet_formulas_live() {
        let mut engine = TablesEngine::new(3, 3).unwrap();
        engine.set_cell_text(0, 0, "1"); // Sheet1
        engine.add_sheet("Sheet2").unwrap();
        engine.set_active_sheet(1).unwrap();
        engine.set_cell_text(0, 0, "=Sheet1!A1*10");
        engine.add_sheet("Sheet3").unwrap();
        engine.set_active_sheet(2).unwrap();
        engine.set_cell_text(0, 0, "3");

        // Move Sheet3 to the front: [2, 0, 1]
        engine.reorder_sheets(&[2, 0, 1]).unwrap();
        assert_eq!(engine.sheet_name_at(0).as_deref(), Some("Sheet3"));
        assert_eq!(engine.sheet_name_at(1).as_deref(), Some("Sheet1"));
        assert_eq!(engine.sheet_name_at(2).as_deref(), Some("Sheet2"));
        assert_eq!(engine.cell_at(0, 0, 0), "3");
        assert_eq!(engine.cell_at(1, 0, 0), "1");
        assert_eq!(engine.cell_at(2, 0, 0), "10");

        engine.set_active_sheet(1).unwrap();
        engine.set_cell_text(0, 0, "5");
        assert_eq!(engine.cell_at(2, 0, 0), "50");
    }

    #[test]
    fn reorder_sheets_preserves_active_sheet_identity() {
        let mut engine = TablesEngine::new(2, 2).unwrap();
        engine.add_sheet("Sheet2").unwrap();
        engine.set_active_sheet(1).unwrap();
        engine.reorder_sheets(&[1, 0]).unwrap();
        assert_eq!(engine.active_sheet(), 0);
        assert_eq!(engine.sheet_name_at(engine.active_sheet()).as_deref(), Some("Sheet2"));
    }
}
