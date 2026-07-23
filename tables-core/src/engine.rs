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
}
