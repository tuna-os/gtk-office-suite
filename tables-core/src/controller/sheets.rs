// SPDX-License-Identifier: GPL-3.0-or-later
//! The sheet bar's edits (add, rename, move, delete) as ops, so each is one
//! undo step like any other edit.

use super::core::WorkbookController;
use super::ops::Op;

impl WorkbookController {
    fn active_sheet_id(&self, index: usize) -> Option<u32> {
        self.state.borrow().sheets.get(index).map(|s| s.borrow().sheet_id)
    }

    /// Add a blank sheet named `name` after the last one and show it.
    pub fn add_sheet(&mut self, name: &str) -> bool {
        let index = self.state.borrow().sheets.len();
        let op = Op::AddSheet { index, name: name.to_string(), sheet_id: None, content: None, relinks: Vec::new() };
        self.apply_ops("Add Sheet", vec![op])
    }

    /// Rename the sheet at `index`.
    pub fn rename_sheet(&mut self, index: usize, name: &str) -> bool {
        let Some(sheet) = self.active_sheet_id(index) else { return false };
        self.apply_ops("Rename Sheet", vec![Op::RenameSheet { sheet, name: name.to_string() }])
    }

    /// Move the sheet at `index` to position `to`.
    pub fn move_sheet(&mut self, index: usize, to: usize) -> bool {
        let Some(sheet) = self.active_sheet_id(index) else { return false };
        if to >= self.state.borrow().sheets.len() || to == index {
            return false;
        }
        self.apply_ops("Move Sheet", vec![Op::MoveSheet { sheet, to }])
    }

    /// Delete the sheet at `index` (never the only one). Undo brings it
    /// back whole, and the formulas that pointed at it.
    pub fn delete_sheet(&mut self, index: usize) -> bool {
        let Some(sheet) = self.active_sheet_id(index) else { return false };
        self.apply_ops("Delete Sheet", vec![Op::DeleteSheet { sheet }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(c: &WorkbookController) -> Vec<String> {
        c.state.borrow().sheets.iter().map(|s| s.borrow().name.clone()).collect()
    }

    #[test]
    fn sheet_bar_edits_undo_one_at_a_time_and_the_view_follows() {
        let mut c = WorkbookController::new(10, 5).unwrap();
        c.edit_cell(0, 0, "7");
        assert!(c.add_sheet("Sheet2"));
        assert_eq!(c.state.borrow().active_sheet, 1, "a new sheet is shown");
        c.edit_cell(0, 0, "=Sheet1!A1*2");
        assert!(c.rename_sheet(0, "Data"));
        assert!(c.move_sheet(1, 0));
        assert_eq!(names(&c), ["Sheet2", "Data"]);
        assert_eq!(c.state.borrow().active_sheet, 0, "the moved sheet stays shown");
        assert!(c.delete_sheet(1));
        assert_eq!(names(&c), ["Sheet2"]);

        assert!(c.undo());
        assert_eq!(names(&c), ["Sheet2", "Data"]);
        assert_eq!(c.state.borrow().active_sheet, 1, "undoing a delete shows the sheet again");
        assert_eq!(c.state.borrow().engine.cell_at(0, 0, 0), "14", "the formula finds its sheet again");
        assert!(c.undo());
        assert_eq!(names(&c), ["Data", "Sheet2"]);
        assert!(c.undo());
        assert_eq!(names(&c), ["Sheet1", "Sheet2"]);
        assert_eq!(c.state.borrow().active_sheet, 0, "undoing a rename shows the renamed sheet");
        assert!(c.undo()); // the formula
        assert!(c.undo()); // the new sheet
        assert_eq!(names(&c), ["Sheet1"]);
        for _ in 0..5 {
            assert!(c.redo());
        }
        assert_eq!(names(&c), ["Sheet2"]);
        assert!(!c.delete_sheet(0), "the only sheet stays");
    }
}
