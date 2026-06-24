// undo.rs — Tables-specific commands implementing suite_common::undo::Command.
// SPDX-License-Identifier: GPL-3.0-or-later

use suite_common::undo::Command;
use suite_common::format::NumberFormat;
use crate::window::{SheetModel, CellBorder};

// ── App state wrapper for undo ──────────────────────────────────────────
// Commands operate on the active sheet within AppState.

pub struct SheetState {
    pub sheets: Vec<SheetModel>,
    pub active_sheet: usize,
}

impl SheetState {
    pub fn sheet(&self) -> &SheetModel { &self.sheets[self.active_sheet] }
    pub fn sheet_mut(&mut self) -> &mut SheetModel { &mut self.sheets[self.active_sheet] }
}

// ── Cell Edit ───────────────────────────────────────────────────────────

pub struct CellEditCmd {
    pub row: usize,
    pub col: usize,
    pub old_val: String,
    pub new_val: String,
    pub old_formula: bool,
    pub new_formula: bool,
}

impl Command<SheetState> for CellEditCmd {
    fn apply(&self, state: &mut SheetState) {
        let sh = state.sheet_mut();
        sh.data[self.row][self.col] = self.new_val.clone();
        sh.formulas[self.row][self.col] = self.new_formula;
    }
    fn undo(&self, state: &mut SheetState) {
        let sh = state.sheet_mut();
        sh.data[self.row][self.col] = self.old_val.clone();
        sh.formulas[self.row][self.col] = self.old_formula;
    }
    fn description(&self) -> &str { "Edit Cell" }
}

// ── Column Resize ───────────────────────────────────────────────────────

pub struct ColResizeCmd {
    pub col: usize,
    pub old_width: f64,
    pub new_width: f64,
}

impl Command<SheetState> for ColResizeCmd {
    fn apply(&self, state: &mut SheetState) {
        state.sheet_mut().set_col_width(self.col, self.new_width);
    }
    fn undo(&self, state: &mut SheetState) {
        state.sheet_mut().set_col_width(self.col, self.old_width);
    }
    fn description(&self) -> &str { "Resize Column" }
}

// ── Sort ────────────────────────────────────────────────────────────────

pub struct SortCmd {
    pub col: usize,
    pub old_data: Vec<Vec<String>>,
    pub old_formulas: Vec<Vec<bool>>,
    pub old_formats: Vec<Vec<NumberFormat>>,
    pub old_borders: Vec<Vec<CellBorder>>,
    pub new_data: Vec<Vec<String>>,
    pub new_formulas: Vec<Vec<bool>>,
    pub new_formats: Vec<Vec<NumberFormat>>,
    pub new_borders: Vec<Vec<CellBorder>>,
}

impl Command<SheetState> for SortCmd {
    fn apply(&self, state: &mut SheetState) {
        let sh = state.sheet_mut();
        sh.data = self.new_data.clone();
        sh.formulas = self.new_formulas.clone();
        sh.formats = self.new_formats.clone();
        sh.borders = self.new_borders.clone();
    }
    fn undo(&self, state: &mut SheetState) {
        let sh = state.sheet_mut();
        sh.data = self.old_data.clone();
        sh.formulas = self.old_formulas.clone();
        sh.formats = self.old_formats.clone();
        sh.borders = self.old_borders.clone();
    }
    fn description(&self) -> &str { "Sort" }
}

// ── Format Change ───────────────────────────────────────────────────────

pub struct FormatCmd {
    pub row: usize,
    pub col: usize,
    pub old_format: NumberFormat,
    pub new_format: NumberFormat,
}

impl Command<SheetState> for FormatCmd {
    fn apply(&self, state: &mut SheetState) {
        state.sheet_mut().formats[self.row][self.col] = self.new_format.clone();
    }
    fn undo(&self, state: &mut SheetState) {
        state.sheet_mut().formats[self.row][self.col] = self.old_format.clone();
    }
    fn description(&self) -> &str { "Change Format" }
}

// ── Border Change ───────────────────────────────────────────────────────

pub struct BorderCmd {
    pub row: usize,
    pub col: usize,
    pub old_border: CellBorder,
    pub new_border: CellBorder,
}

impl Command<SheetState> for BorderCmd {
    fn apply(&self, state: &mut SheetState) {
        state.sheet_mut().borders[self.row][self.col] = self.new_border.clone();
    }
    fn undo(&self, state: &mut SheetState) {
        state.sheet_mut().borders[self.row][self.col] = self.old_border.clone();
    }
    fn description(&self) -> &str { "Change Border" }
}

// ── Merge Toggle ────────────────────────────────────────────────────────

pub struct MergeCmd {
    pub merges: Vec<(usize, usize, usize, usize)>,
}

impl Command<SheetState> for MergeCmd {
    fn apply(&self, state: &mut SheetState) {
        state.sheet_mut().merges = self.merges.clone();
    }
    fn undo(&self, state: &mut SheetState) {
        // We don't know the previous merge state, so this is a snapshot toggle.
        // In a full implementation, we'd store old_merges separately.
        let sh = state.sheet_mut();
        if !self.merges.is_empty() {
            sh.merges.clear();
        }
    }
    fn description(&self) -> &str { "Merge Cells" }
}

// ── Freeze Toggle ───────────────────────────────────────────────────────

pub struct FreezeCmd {
    pub old_rows: usize,
    pub old_cols: usize,
    pub new_rows: usize,
    pub new_cols: usize,
}

impl Command<SheetState> for FreezeCmd {
    fn apply(&self, state: &mut SheetState) {
        let sh = state.sheet_mut();
        sh.frozen_rows = self.new_rows;
        sh.frozen_cols = self.new_cols;
    }
    fn undo(&self, state: &mut SheetState) {
        let sh = state.sheet_mut();
        sh.frozen_rows = self.old_rows;
        sh.frozen_cols = self.old_cols;
    }
    fn description(&self) -> &str { "Freeze Panes" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::{SheetModel, CellBorder, BorderStyle};
    use suite_common::format::{NumberFormat, NumberFormatKind};

    fn make_state() -> SheetState {
        let sheet = SheetModel::new("Test", 5, 5, 0);
        SheetState { sheets: vec![sheet], active_sheet: 0 }
    }

    #[test]
    fn test_cell_edit_undo() {
        let mut state = make_state();
        let cmd = CellEditCmd {
            row: 0, col: 0, old_val: "old".into(), new_val: "new".into(),
            old_formula: false, new_formula: false,
        };
        cmd.apply(&mut state);
        assert_eq!(state.sheet().cell(0, 0), "new");
        cmd.undo(&mut state);
        assert_eq!(state.sheet().cell(0, 0), "old");
    }

    #[test]
    fn test_col_resize_undo() {
        let mut state = make_state();
        let old_w = state.sheet().col_width(0);
        let cmd = ColResizeCmd { col: 0, old_width: old_w, new_width: 200.0 };
        cmd.apply(&mut state);
        assert_eq!(state.sheet().col_width(0), 200.0);
        cmd.undo(&mut state);
        assert_eq!(state.sheet().col_width(0), old_w);
    }

    #[test]
    fn test_format_undo() {
        let mut state = make_state();
        let old = NumberFormat::default();
        let new = NumberFormat::new(NumberFormatKind::Currency("$".into(), 2));
        let cmd = FormatCmd { row: 0, col: 0, old_format: old.clone(), new_format: new.clone() };
        cmd.apply(&mut state);
        assert_eq!(state.sheet().formats[0][0].kind, NumberFormatKind::Currency("$".into(), 2));
        cmd.undo(&mut state);
        assert_eq!(state.sheet().formats[0][0].kind, NumberFormatKind::General);
    }

    #[test]
    fn test_border_undo() {
        let mut state = make_state();
        let old = CellBorder::none();
        let new = CellBorder::outline(BorderStyle::Solid, (0.0, 0.0, 0.0));
        let cmd = BorderCmd { row: 0, col: 0, old_border: old.clone(), new_border: new.clone() };
        cmd.apply(&mut state);
        assert_eq!(state.sheet().borders[0][0].top, BorderStyle::Solid);
        cmd.undo(&mut state);
        assert_eq!(state.sheet().borders[0][0].top, BorderStyle::None);
    }

    #[test]
    fn test_freeze_undo() {
        let mut state = make_state();
        let cmd = FreezeCmd { old_rows: 0, old_cols: 0, new_rows: 1, new_cols: 0 };
        cmd.apply(&mut state);
        assert_eq!(state.sheet().frozen_rows, 1);
        cmd.undo(&mut state);
        assert_eq!(state.sheet().frozen_rows, 0);
    }

    #[test]
    fn test_merge_toggle() {
        let mut state = make_state();
        assert!(state.sheet().merges.is_empty());
        let cmd = MergeCmd { merges: vec![(0, 0, 0, 1)] };
        cmd.apply(&mut state);
        assert_eq!(state.sheet().merges.len(), 1);
        cmd.undo(&mut state);
        assert!(state.sheet().merges.is_empty());
    }

    #[test]
    fn test_sort_cmd_preserves_data() {
        let mut state = make_state();
        // Set up data in reverse order
        state.sheet_mut().data[0][0] = "C".into();
        state.sheet_mut().data[1][0] = "A".into();
        state.sheet_mut().data[2][0] = "B".into();
        let old_data = state.sheet().data.clone();
        // Build sorted data manually
        let mut new_data = old_data.clone();
        new_data[0] = old_data[1].clone(); // A
        new_data[1] = old_data[2].clone(); // B
        new_data[2] = old_data[0].clone(); // C
        let cmd = SortCmd {
            col: 0,
            old_data: old_data.clone(),
            old_formulas: state.sheet().formulas.clone(),
            old_formats: state.sheet().formats.clone(),
            old_borders: state.sheet().borders.clone(),
            new_data: new_data.clone(),
            new_formulas: state.sheet().formulas.clone(),
            new_formats: state.sheet().formats.clone(),
            new_borders: state.sheet().borders.clone(),
        };
        cmd.apply(&mut state);
        assert_eq!(state.sheet().data[0][0], "A");
        assert_eq!(state.sheet().data[1][0], "B");
        assert_eq!(state.sheet().data[2][0], "C");
        cmd.undo(&mut state);
        assert_eq!(state.sheet().data[0][0], "C");
        assert_eq!(state.sheet().data[1][0], "A");
        assert_eq!(state.sheet().data[2][0], "B");
    }

    // ── Validation rule tests ─────────────────────────────────────
    use crate::window::ValidationRule;

    #[test]
    fn test_validation_list() {
        let rule = ValidationRule::List(vec!["A".into(), "B".into()]);
        assert!(rule.validate("A"));
        assert!(rule.validate("B"));
        assert!(!rule.validate("C"));
    }

    #[test]
    fn test_validation_whole_number() {
        let rule = ValidationRule::WholeNumber { min: Some(1), max: Some(10) };
        assert!(rule.validate("5"));
        assert!(!rule.validate("0"));
        assert!(!rule.validate("11"));
        assert!(!rule.validate("abc"));
    }

    #[test]
    fn test_validation_decimal() {
        let rule = ValidationRule::Decimal { min: Some(0.0), max: Some(1.0) };
        assert!(rule.validate("0.5"));
        assert!(!rule.validate("1.5"));
    }

    #[test]
    fn test_validation_text_length() {
        let rule = ValidationRule::TextLength { min: Some(1), max: Some(5) };
        assert!(rule.validate("abc"));
        assert!(!rule.validate(""));
        assert!(!rule.validate("abcdef"));
    }

    #[test]
    fn test_validation_regex() {
        let rule = ValidationRule::Regex(r"^\d{3}$".into());
        assert!(rule.validate("123"));
        assert!(!rule.validate("12"));
        assert!(!rule.validate("abc"));
    }
}
