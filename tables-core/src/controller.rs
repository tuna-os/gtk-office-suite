// controller.rs — canonical workbook state and user-facing edit controller.
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

use suite_common_core::events::{Broadcaster, Hint, Listener};
use suite_common_core::undo::{Command, UndoManager};

use crate::engine::TablesEngine;
use crate::fragment::Fragment;
use crate::sheet::{SheetModel, SortDirection};

/// Canonical mutable state for a Tables window.
///
/// GTK views may observe this state, but edits must go through
/// [`WorkbookController`] so the calculation engine, rendered sheet, and undo
/// history cannot drift apart.
pub struct WorkbookState {
    pub sheets: Vec<Rc<RefCell<SheetModel>>>,
    pub active_sheet: usize,
    pub engine: TablesEngine,
}

impl WorkbookState {
    pub fn new(rows: usize, cols: usize) -> Result<Self, String> {
        let engine = TablesEngine::new(rows, cols)?;
        let sheet_id = engine.sheet_id_at(0).unwrap_or(0);
        let sheet = SheetModel::new("Sheet1", rows, cols, sheet_id);
        Ok(Self {
            sheets: vec![Rc::new(RefCell::new(sheet))],
            active_sheet: 0,
            engine,
        })
    }

    /// Resolve a stable sheet identity to its current position. Sheets are
    /// never deleted out from under a live GUI reference without also being
    /// dropped from `sheets`, so this stays in lockstep with the engine.
    pub fn sheet_index_for_id(&self, sheet_id: u32) -> Option<usize> {
        self.sheets
            .iter()
            .position(|sheet| sheet.borrow().sheet_id == sheet_id)
    }

    pub fn sheet(&self) -> Ref<'_, SheetModel> {
        self.sheets[self.active_sheet].borrow()
    }

    pub fn sheet_mut(&self) -> RefMut<'_, SheetModel> {
        self.sheets[self.active_sheet].borrow_mut()
    }

    /// The editable input for a cell, preserving formulas rather than their
    /// calculated display value.
    pub fn cell_input(&self, row: usize, col: usize) -> String {
        self.engine
            .formula(row, col)
            .map(|formula| format!("={formula}"))
            .unwrap_or_else(|| self.engine.cell(row, col))
    }

    fn set_cell_input(&mut self, row: usize, col: usize, input: &str) {
        self.engine.set_cell_text(row, col, input);
        self.sync_active_sheet();
    }

    fn set_cell_inputs<'a>(&mut self, inputs: impl IntoIterator<Item = (usize, usize, &'a str)>) {
        for (row, col, input) in inputs {
            self.engine.set_cell_text(row, col, input);
        }
        self.engine.evaluate();
        self.sync_active_sheet();
    }

    fn set_cell_input_on_sheet(&mut self, sheet: usize, row: usize, col: usize, input: &str) {
        let previous = self.active_sheet;
        self.engine
            .set_active_sheet(sheet)
            .expect("valid worksheet index");
        self.active_sheet = sheet;
        self.set_cell_input(row, col, input);
        self.engine
            .set_active_sheet(previous)
            .expect("valid worksheet index");
        self.active_sheet = previous;
    }

    fn set_cell_inputs_on_sheet<'a>(
        &mut self,
        sheet: usize,
        inputs: impl IntoIterator<Item = (usize, usize, &'a str)>,
    ) {
        let previous = self.active_sheet;
        self.engine
            .set_active_sheet(sheet)
            .expect("valid worksheet index");
        self.active_sheet = sheet;
        self.set_cell_inputs(inputs);
        self.engine
            .set_active_sheet(previous)
            .expect("valid worksheet index");
        self.active_sheet = previous;
    }

    pub fn add_sheet(&mut self, name: String, rows: usize, cols: usize) -> Result<usize, String> {
        let index = self.engine.add_sheet(&name)?;
        let sheet_id = self.engine.sheet_id_at(index).unwrap_or(index as u32);
        self.sheets.push(Rc::new(RefCell::new(SheetModel::new(
            &name, rows, cols, sheet_id,
        ))));
        Ok(index)
    }

    pub fn switch_sheet(&mut self, index: usize) -> Result<(), String> {
        if index >= self.sheets.len() {
            return Err(format!("Sheet index {index} does not exist"));
        }
        self.engine.set_active_sheet(index)?;
        self.active_sheet = index;
        self.sync_active_sheet();
        Ok(())
    }

    /// Rename a sheet in both the engine and its live presentation model.
    pub fn rename_sheet(&mut self, index: usize, name: &str) -> Result<(), String> {
        if index >= self.sheets.len() {
            return Err(format!("Sheet index {index} does not exist"));
        }
        self.engine.rename_sheet(index, name)?;
        self.sheets[index].borrow_mut().name = name.to_string();
        Ok(())
    }

    /// Delete a sheet. Fails if it is the only sheet. Undo history entries
    /// that targeted the deleted sheet become inert no-ops rather than
    /// corrupting a different sheet (they resolve by sheet_id, and the id
    /// no longer exists).
    pub fn delete_sheet(&mut self, index: usize) -> Result<(), String> {
        if index >= self.sheets.len() {
            return Err(format!("Sheet index {index} does not exist"));
        }
        self.engine.delete_sheet(index)?;
        self.sheets.remove(index);
        self.active_sheet = self.engine.active_sheet();
        self.sync_active_sheet();
        Ok(())
    }

    /// Reorder sheets. `new_order` must be a permutation of current indices.
    pub fn reorder_sheets(&mut self, new_order: &[usize]) -> Result<(), String> {
        self.engine.reorder_sheets(new_order)?;
        self.sheets = new_order.iter().map(|&i| self.sheets[i].clone()).collect();
        self.active_sheet = self.engine.active_sheet();
        Ok(())
    }

    fn sync_active_sheet(&mut self) {
        let active = self.active_sheet;
        self.sheets[active]
            .borrow_mut()
            .sync_from_engine(&self.engine);
    }
}

struct CellInputCommand {
    sheet_id: u32,
    row: usize,
    col: usize,
    old_input: String,
    new_input: String,
}

struct SheetSnapshotCommand {
    sheet_id: u32,
    before: SheetModel,
    after: SheetModel,
    description: &'static str,
}

impl Command<WorkbookState> for SheetSnapshotCommand {
    fn apply(&self, state: &mut WorkbookState) {
        if let Some(index) = state.sheet_index_for_id(self.sheet_id) {
            *state.sheets[index].borrow_mut() = self.after.clone();
        }
    }

    fn undo(&self, state: &mut WorkbookState) {
        if let Some(index) = state.sheet_index_for_id(self.sheet_id) {
            *state.sheets[index].borrow_mut() = self.before.clone();
        }
    }

    fn description(&self) -> &str {
        self.description
    }
}

impl Command<WorkbookState> for CellInputCommand {
    fn apply(&self, state: &mut WorkbookState) {
        if let Some(index) = state.sheet_index_for_id(self.sheet_id) {
            state.set_cell_input_on_sheet(index, self.row, self.col, &self.new_input);
        }
    }

    fn undo(&self, state: &mut WorkbookState) {
        if let Some(index) = state.sheet_index_for_id(self.sheet_id) {
            state.set_cell_input_on_sheet(index, self.row, self.col, &self.old_input);
        }
    }

    fn description(&self) -> &str {
        "Edit Cell"
    }
}

struct CellInputChange {
    row: usize,
    col: usize,
    old_input: String,
    new_input: String,
}

struct CellBatchCommand {
    sheet_id: u32,
    changes: Vec<CellInputChange>,
}

struct SortCommand {
    sheet_id: u32,
    before_inputs: Vec<Vec<String>>,
    after_inputs: Vec<Vec<String>>,
    before_sheet: SheetModel,
    after_sheet: SheetModel,
}

impl SortCommand {
    fn restore(
        state: &mut WorkbookState,
        sheet_index: usize,
        inputs: &[Vec<String>],
        sheet: &SheetModel,
    ) {
        *state.sheets[sheet_index].borrow_mut() = sheet.clone();
        state.set_cell_inputs_on_sheet(
            sheet_index,
            inputs.iter().enumerate().flat_map(|(row, values)| {
                values
                    .iter()
                    .enumerate()
                    .map(move |(col, input)| (row, col, input.as_str()))
            }),
        );
    }
}

impl Command<WorkbookState> for SortCommand {
    fn apply(&self, state: &mut WorkbookState) {
        if let Some(index) = state.sheet_index_for_id(self.sheet_id) {
            Self::restore(state, index, &self.after_inputs, &self.after_sheet);
        }
    }

    fn undo(&self, state: &mut WorkbookState) {
        if let Some(index) = state.sheet_index_for_id(self.sheet_id) {
            Self::restore(state, index, &self.before_inputs, &self.before_sheet);
        }
    }

    fn description(&self) -> &str {
        "Sort"
    }
}

impl Command<WorkbookState> for CellBatchCommand {
    fn apply(&self, state: &mut WorkbookState) {
        if let Some(index) = state.sheet_index_for_id(self.sheet_id) {
            state.set_cell_inputs_on_sheet(
                index,
                self.changes
                    .iter()
                    .map(|change| (change.row, change.col, change.new_input.as_str())),
            );
        }
    }

    fn undo(&self, state: &mut WorkbookState) {
        if let Some(index) = state.sheet_index_for_id(self.sheet_id) {
            state.set_cell_inputs_on_sheet(
                index,
                self.changes
                    .iter()
                    .map(|change| (change.row, change.col, change.old_input.as_str())),
            );
        }
    }

    fn description(&self) -> &str {
        "Paste Cells"
    }
}

/// Owns the undo history for the exact state observed by the GUI.
pub struct WorkbookController {
    pub state: Rc<RefCell<WorkbookState>>,
    undo: UndoManager<WorkbookState>,
}

impl WorkbookController {
    pub fn new(rows: usize, cols: usize) -> Result<Self, String> {
        let state = Rc::new(RefCell::new(WorkbookState::new(rows, cols)?));
        let mut undo = UndoManager::new(state.clone());
        undo.broadcaster = Some(Rc::new(Broadcaster::new()));
        Ok(Self { state, undo })
    }

    pub fn listen_history(&self, listener: Rc<dyn Listener<Hint>>) {
        if let Some(broadcaster) = &self.undo.broadcaster {
            broadcaster.listen(listener);
        }
    }

    pub fn edit_cell(&mut self, row: usize, col: usize, input: impl Into<String>) {
        let new_input = input.into();
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let old_input = state.cell_input(row, col);
        drop(state);
        if old_input == new_input {
            return;
        }
        self.undo.execute(Box::new(CellInputCommand {
            sheet_id,
            row,
            col,
            old_input,
            new_input,
        }));
    }

    /// Apply a reversible mutation to the active sheet model.
    ///
    /// This is for sheet presentation/metadata operations that do not alter
    /// calculation inputs. Cell and formula edits must use [`Self::edit_cell`]
    /// so the engine is recalculated as part of undo and redo.
    pub fn mutate_sheet(
        &mut self,
        description: &'static str,
        mutation: impl FnOnce(&mut SheetModel),
    ) {
        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let before = state.sheet().clone();
        drop(state);
        let mut after = before.clone();
        mutation(&mut after);
        self.undo.execute(Box::new(SheetSnapshotCommand {
            sheet_id,
            before,
            after,
            description,
        }));
    }

    /// Paste a suite fragment as one undoable calculation edit.
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
        let changes: Vec<CellInputChange> = new_inputs
            .into_iter()
            .filter_map(|(r, c, new_input)| {
                let old_input = state.cell_input(r, c);
                (old_input != new_input).then_some(CellInputChange {
                    row: r,
                    col: c,
                    old_input,
                    new_input,
                })
            })
            .collect();
        drop(state);
        if !changes.is_empty() {
            self.undo
                .execute(Box::new(CellBatchCommand { sheet_id, changes }));
        }
    }

    /// Cycle a whole-sheet column sort while keeping source formulas and
    /// row-level presentation metadata aligned with their records.
    pub fn toggle_sort(&mut self, col: usize) {
        use SortDirection::{Ascending, Descending};

        let state = self.state.borrow();
        let sheet_id = state.sheet().sheet_id;
        let before_sheet = state.sheet().clone();
        if col >= before_sheet.cols {
            return;
        }
        let new_direction = match before_sheet.sorted_col {
            Some((current, Ascending)) if current == col => Descending,
            Some((current, Descending)) if current == col => {
                drop(state);
                self.mutate_sheet("Clear Sort Indicator", |sheet| sheet.sorted_col = None);
                return;
            }
            _ => Ascending,
        };
        let before_inputs: Vec<Vec<String>> = (0..before_sheet.rows)
            .map(|row| {
                (0..before_sheet.cols)
                    .map(|column| state.cell_input(row, column))
                    .collect()
            })
            .collect();
        let mut order: Vec<usize> = (0..before_sheet.rows).collect();
        order.sort_by(|&left, &right| {
            let left = before_sheet.data[left][col]
                .parse::<f64>()
                .ok()
                .unwrap_or(0.0);
            let right = before_sheet.data[right][col]
                .parse::<f64>()
                .ok()
                .unwrap_or(0.0);
            if new_direction == Ascending {
                left.partial_cmp(&right)
                    .unwrap_or(std::cmp::Ordering::Equal)
            } else {
                right
                    .partial_cmp(&left)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }
        });
        drop(state);

        let mut after_inputs = vec![vec![String::new(); before_sheet.cols]; before_sheet.rows];
        {
            let mut state = self.state.borrow_mut();
            for (new_row, &old_row) in order.iter().enumerate() {
                for column in 0..before_sheet.cols {
                    after_inputs[new_row][column] = state.engine.move_input(
                        &before_inputs[old_row][column],
                        (old_row, column),
                        (new_row, column),
                    );
                }
            }
        }
        let mut after_sheet = before_sheet.clone();
        after_sheet.sorted_col = Some((col, new_direction));
        after_sheet.formats = order
            .iter()
            .map(|&row| before_sheet.formats[row].clone())
            .collect();
        after_sheet.borders = order
            .iter()
            .map(|&row| before_sheet.borders[row].clone())
            .collect();
        after_sheet.validations = order
            .iter()
            .map(|&row| before_sheet.validations[row].clone())
            .collect();

        self.undo.execute(Box::new(SortCommand {
            sheet_id,
            before_inputs,
            after_inputs,
            before_sheet,
            after_sheet,
        }));
    }

    /// Record a live-preview sheet mutation after it has already been applied.
    /// This is useful for pointer drags: the view updates continuously, but the
    /// completed gesture becomes one undo step.
    pub fn record_sheet_mutation(&mut self, description: &'static str, before: SheetModel) {
        let after = self.state.borrow().sheet().clone();
        let sheet_id = self.state.borrow().sheet().sheet_id;
        self.undo.execute(Box::new(SheetSnapshotCommand {
            sheet_id,
            before,
            after,
            description,
        }));
    }

    pub fn undo(&mut self) -> bool {
        self.undo.undo()
    }

    pub fn redo(&mut self) -> bool {
        self.undo.redo()
    }

    pub fn can_undo(&self) -> bool {
        self.undo.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.undo.can_redo()
    }

    pub fn undo_description(&self) -> Option<&str> {
        self.undo.undo_description()
    }

    pub fn redo_description(&self) -> Option<&str> {
        self.undo.redo_description()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_cell_edit_undo_and_redo_share_canonical_state() {
        let mut controller = WorkbookController::new(5, 5).unwrap();
        let observed = controller.state.clone();

        controller.edit_cell(0, 0, "41");
        assert_eq!(observed.borrow().sheet().cell(0, 0), "41");
        assert!(controller.can_undo());

        assert!(controller.undo());
        assert_eq!(observed.borrow().sheet().cell(0, 0), "");
        assert!(controller.can_redo());

        assert!(controller.redo());
        assert_eq!(observed.borrow().sheet().cell(0, 0), "41");
    }

    #[test]
    fn undo_recalculates_dependent_formulas_and_restores_formula_input() {
        let mut controller = WorkbookController::new(5, 5).unwrap();
        controller.edit_cell(0, 0, "10");
        controller.edit_cell(0, 1, "=A1*2");
        assert_eq!(controller.state.borrow().sheet().cell(0, 1), "20");

        controller.edit_cell(0, 0, "25");
        assert_eq!(controller.state.borrow().sheet().cell(0, 1), "50");
        assert!(controller.undo());
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "10");
        assert_eq!(controller.state.borrow().sheet().cell(0, 1), "20");
        assert_eq!(controller.state.borrow().cell_input(0, 1), "=A1*2");
    }

    #[test]
    fn unchanged_edit_does_not_create_undo_entry() {
        let mut controller = WorkbookController::new(2, 2).unwrap();
        controller.edit_cell(0, 0, "");
        assert!(!controller.can_undo());
    }

    #[test]
    fn sheet_metadata_mutation_uses_the_live_undo_stack() {
        let mut controller = WorkbookController::new(2, 2).unwrap();
        let observed = controller.state.clone();
        controller.mutate_sheet("Resize Column", |sheet| {
            sheet.set_col_width(0, 240.0);
        });
        assert_eq!(observed.borrow().sheet().col_width(0), 240.0);
        assert!(controller.undo());
        assert_eq!(observed.borrow().sheet().col_width(0), 90.0);
        assert!(controller.redo());
        assert_eq!(observed.borrow().sheet().col_width(0), 240.0);
    }

    #[test]
    fn paste_is_one_undo_step_and_keeps_formulas_live() {
        use crate::fragment::GridCell;

        let mut controller = WorkbookController::new(5, 5).unwrap();
        let fragment = Fragment::Grid(vec![vec![
            GridCell {
                value: "3".into(),
                ..Default::default()
            },
            GridCell {
                value: String::new(),
                formula: Some("A1*4".into()),
                num_format: None,
            },
        ]]);
        controller.paste_fragment(0, 0, &fragment);
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "3");
        assert_eq!(controller.state.borrow().sheet().cell(0, 1), "12");
        assert!(controller.undo());
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "");
        assert_eq!(controller.state.borrow().sheet().cell(0, 1), "");
        assert!(controller.redo());
        assert_eq!(controller.state.borrow().cell_input(0, 1), "=A1*4");
    }

    #[test]
    fn sort_moves_formula_inputs_and_row_metadata_as_one_undo_step() {
        use suite_common_core::format::{NumberFormat, NumberFormatKind};

        let mut controller = WorkbookController::new(3, 2).unwrap();
        for (row, value) in ["3", "1", "2"].into_iter().enumerate() {
            controller.edit_cell(row, 0, value);
            controller.edit_cell(row, 1, format!("=A{}*10", row + 1));
        }
        controller.mutate_sheet("Mark Row", |sheet| {
            sheet.formats[1][0] = NumberFormat::new(NumberFormatKind::Percent(1));
        });

        controller.toggle_sort(0);
        let state = controller.state.borrow();
        assert_eq!(state.sheet().cell(0, 0), "1");
        assert_eq!(state.sheet().cell(0, 1), "10");
        assert_eq!(state.cell_input(0, 1), "=A1*10");
        assert_eq!(
            state.sheet().formats[0][0].kind,
            NumberFormatKind::Percent(1)
        );
        drop(state);

        assert!(controller.undo());
        let state = controller.state.borrow();
        assert_eq!(state.sheet().cell(0, 0), "3");
        assert_eq!(state.cell_input(0, 1), "=A1*10");
        assert_eq!(
            state.sheet().formats[1][0].kind,
            NumberFormatKind::Percent(1)
        );
    }

    #[test]
    fn new_edit_clears_redo_and_reports_the_current_command() {
        let mut controller = WorkbookController::new(2, 2).unwrap();
        controller.edit_cell(0, 0, "first");
        assert_eq!(controller.undo_description(), Some("Edit Cell"));
        assert!(controller.undo());
        assert_eq!(controller.redo_description(), Some("Edit Cell"));

        controller.edit_cell(0, 1, "replacement");
        assert!(!controller.can_redo());
        assert_eq!(controller.undo_description(), Some("Edit Cell"));
    }

    #[test]
    fn edit_undo_redo_save_and_reopen_keeps_formula_source() {
        let mut controller = WorkbookController::new(3, 3).unwrap();
        controller.edit_cell(0, 0, "21");
        controller.edit_cell(0, 1, "=A1*2");
        assert!(controller.undo());
        assert!(controller.redo());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("undo-redo.xlsx");
        {
            let state = controller.state.borrow();
            let sheets = state
                .sheets
                .iter()
                .map(|sheet| sheet.borrow().clone())
                .collect::<Vec<_>>();
            crate::io::save_sheets_to_xlsx_with_engine(
                path.to_str().unwrap(),
                &sheets,
                Some(&state.engine),
            )
            .unwrap();
        }

        let mut reopened = TablesEngine::new(3, 3).unwrap();
        crate::io::load_file_into_engine(path.to_str().unwrap(), &mut reopened).unwrap();
        assert_eq!(reopened.cell(0, 0), "21");
        assert_eq!(reopened.cell(0, 1), "42");
        assert_eq!(reopened.formula(0, 1).as_deref(), Some("A1*2"));
    }

    #[test]
    fn sheets_keep_inputs_isolated_and_undo_targets_the_origin_sheet() {
        let mut controller = WorkbookController::new(3, 3).unwrap();
        {
            let state = controller.state.clone();
            let mut state = state.borrow_mut();
            state.add_sheet("Sheet2".into(), 3, 3).unwrap();
            state.switch_sheet(1).unwrap();
        }
        controller.edit_cell(0, 0, "second sheet");
        controller.state.borrow_mut().switch_sheet(0).unwrap();
        controller.edit_cell(0, 0, "first sheet");

        controller.state.borrow_mut().switch_sheet(1).unwrap();
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "second sheet");
        assert!(controller.undo());
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "second sheet");

        controller.state.borrow_mut().switch_sheet(0).unwrap();
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "");
        controller.state.borrow_mut().switch_sheet(1).unwrap();
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "second sheet");
    }

    #[test]
    fn rename_sheet_updates_name_and_keeps_cross_sheet_formulas_live() {
        let mut controller = WorkbookController::new(2, 2).unwrap();
        controller.edit_cell(0, 0, "5");
        {
            let mut state = controller.state.borrow_mut();
            state.add_sheet("Sheet2".into(), 2, 2).unwrap();
            state.switch_sheet(1).unwrap();
        }
        controller.edit_cell(0, 0, "=Sheet1!A1*2");
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "10");

        controller.state.borrow_mut().rename_sheet(0, "Inputs").unwrap();
        assert_eq!(controller.state.borrow().sheets[0].borrow().name, "Inputs");
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "10");

        // The formula on Sheet2 still resolves through the renamed sheet.
        controller.state.borrow_mut().switch_sheet(0).unwrap();
        controller.edit_cell(0, 0, "7");
        controller.state.borrow_mut().switch_sheet(1).unwrap();
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "14");
    }

    /// The adversarial case for sheet-identity undo: after a structural
    /// delete renumbers sheet positions, older undo commands (captured by
    /// sheet_id) must keep targeting their original sheet rather than
    /// whatever now sits at their old positional index.
    #[test]
    fn deleting_a_sheet_does_not_retarget_other_sheets_undo_history() {
        let mut controller = WorkbookController::new(2, 2).unwrap();
        controller.edit_cell(0, 0, "sheet1-a"); // undo #1, targets Sheet1

        {
            let mut state = controller.state.borrow_mut();
            state.add_sheet("Sheet2".into(), 2, 2).unwrap();
            state.add_sheet("Sheet3".into(), 2, 2).unwrap();
            state.switch_sheet(1).unwrap();
        }
        controller.edit_cell(0, 0, "sheet2-a");

        // Delete Sheet2 (positional index 1). Sheet3 shifts from index 2 to 1.
        controller.state.borrow_mut().switch_sheet(0).unwrap();
        controller.state.borrow_mut().delete_sheet(1).unwrap();
        assert_eq!(controller.state.borrow().sheets.len(), 2);

        // Undoing the Sheet2 edit is inert: Sheet2 no longer exists, and the
        // command must not silently retarget Sheet3, which is now at the old
        // Sheet2 position.
        controller.state.borrow_mut().switch_sheet(1).unwrap();
        let sheet3_before = controller.state.borrow().sheet().cell(0, 0).to_string();
        assert!(controller.undo()); // pops the Sheet2 edit off the stack
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), sheet3_before);

        // The Sheet1 edit underneath it still undoes correctly.
        controller.state.borrow_mut().switch_sheet(0).unwrap();
        assert!(controller.undo());
        assert_eq!(controller.state.borrow().sheet().cell(0, 0), "");
    }

    #[test]
    fn reorder_sheets_permutes_presentation_state_with_engine_content() {
        let mut controller = WorkbookController::new(2, 2).unwrap();
        {
            let mut state = controller.state.borrow_mut();
            state.sheets[0].borrow_mut().set_col_width(0, 200.0);
            state.add_sheet("Sheet2".into(), 2, 2).unwrap();
            state.switch_sheet(1).unwrap();
        }
        controller.edit_cell(0, 0, "on-sheet-2");

        controller.state.borrow_mut().reorder_sheets(&[1, 0]).unwrap();
        let state = controller.state.borrow();
        assert_eq!(state.sheets[0].borrow().name, "Sheet2");
        assert_eq!(state.sheets[0].borrow().cell(0, 0), "on-sheet-2");
        assert_eq!(state.sheets[1].borrow().col_width(0), 200.0);
    }
}
