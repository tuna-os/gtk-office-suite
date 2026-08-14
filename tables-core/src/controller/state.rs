use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashSet;
use std::rc::Rc;

use suite_common_core::undo::Command;

use crate::engine::TablesEngine;
use crate::sheet::SheetModel;

/// Canonical mutable state for a Tables window.
///
/// GTK views may observe this state, but edits must go through
/// [`crate::controller::WorkbookController`] so the calculation engine, rendered sheet, and undo
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

pub(super) struct CellInputCommand {
    pub(super) sheet_id: u32,
    pub(super) row: usize,
    pub(super) col: usize,
    pub(super) old_input: String,
    pub(super) new_input: String,
}

pub(super) struct SheetSnapshotCommand {
    pub(super) sheet_id: u32,
    pub(super) before: SheetModel,
    pub(super) after: SheetModel,
    pub(super) description: &'static str,
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

pub(super) struct CellInputChange {
    pub(super) row: usize,
    pub(super) col: usize,
    pub(super) old_input: String,
    pub(super) new_input: String,
}

pub(super) struct CellBatchCommand {
    pub(super) sheet_id: u32,
    pub(super) changes: Vec<CellInputChange>,
    pub(super) description: &'static str,
}

/// Create-a-named-range as one undo step (undo = delete it again). Errors
/// from a stale apply/undo — e.g. redoing after something else deleted
/// the name — are swallowed rather than panicking, same posture as
/// CellBatchCommand's sheet_index_for_id lookups: undo/redo commands
/// degrade gracefully when the state they target has moved on.
pub(super) struct DefinedNameCommand {
    pub(super) name: String,
    pub(super) formula: String,
}

impl Command<WorkbookState> for DefinedNameCommand {
    fn apply(&self, state: &mut WorkbookState) {
        let _ = state
            .engine
            .model
            .new_defined_name(&self.name, None, &self.formula);
    }
    fn undo(&self, state: &mut WorkbookState) {
        let _ = state.engine.model.delete_defined_name(&self.name, None);
    }
    fn description(&self) -> &str {
        "Define Name"
    }
}

/// Set which rows are hidden by a column-value filter, as one undo step.
/// Degrades gracefully (no-op) if the target sheet has since been
/// deleted — same posture as the other undo commands here.
pub(super) struct FilterCommand {
    pub(super) sheet_id: u32,
    pub(super) before: HashSet<usize>,
    pub(super) after: HashSet<usize>,
}

impl Command<WorkbookState> for FilterCommand {
    fn apply(&self, state: &mut WorkbookState) {
        if let Some(idx) = state.sheet_index_for_id(self.sheet_id) {
            state.sheets[idx].borrow_mut().hidden_rows = self.after.clone();
        }
    }
    fn undo(&self, state: &mut WorkbookState) {
        if let Some(idx) = state.sheet_index_for_id(self.sheet_id) {
            state.sheets[idx].borrow_mut().hidden_rows = self.before.clone();
        }
    }
    fn description(&self) -> &str {
        "Filter Rows"
    }
}

/// Manual row/column hiding (#113), independent of [`FilterCommand`] —
/// see `SheetModel::hidden_rows_manual`'s doc comment for why they're
/// kept separate.
pub(super) struct HideRowsCommand {
    pub(super) sheet_id: u32,
    pub(super) before: HashSet<usize>,
    pub(super) after: HashSet<usize>,
}

impl Command<WorkbookState> for HideRowsCommand {
    fn apply(&self, state: &mut WorkbookState) {
        if let Some(idx) = state.sheet_index_for_id(self.sheet_id) {
            state.sheets[idx].borrow_mut().hidden_rows_manual = self.after.clone();
        }
    }
    fn undo(&self, state: &mut WorkbookState) {
        if let Some(idx) = state.sheet_index_for_id(self.sheet_id) {
            state.sheets[idx].borrow_mut().hidden_rows_manual = self.before.clone();
        }
    }
    fn description(&self) -> &str {
        "Hide Rows"
    }
}

pub(super) struct HideColsCommand {
    pub(super) sheet_id: u32,
    pub(super) before: HashSet<usize>,
    pub(super) after: HashSet<usize>,
}

impl Command<WorkbookState> for HideColsCommand {
    fn apply(&self, state: &mut WorkbookState) {
        if let Some(idx) = state.sheet_index_for_id(self.sheet_id) {
            state.sheets[idx].borrow_mut().hidden_cols = self.after.clone();
        }
    }
    fn undo(&self, state: &mut WorkbookState) {
        if let Some(idx) = state.sheet_index_for_id(self.sheet_id) {
            state.sheets[idx].borrow_mut().hidden_cols = self.before.clone();
        }
    }
    fn description(&self) -> &str {
        "Hide Columns"
    }
}

pub(super) struct PrintAreaCommand {
    pub(super) sheet_id: u32,
    pub(super) before: Option<(usize, usize, usize, usize)>,
    pub(super) after: Option<(usize, usize, usize, usize)>,
}

impl Command<WorkbookState> for PrintAreaCommand {
    fn apply(&self, state: &mut WorkbookState) {
        if let Some(idx) = state.sheet_index_for_id(self.sheet_id) {
            state.sheets[idx].borrow_mut().print_area = self.after;
        }
    }
    fn undo(&self, state: &mut WorkbookState) {
        if let Some(idx) = state.sheet_index_for_id(self.sheet_id) {
            state.sheets[idx].borrow_mut().print_area = self.before;
        }
    }
    fn description(&self) -> &str {
        "Set Print Area"
    }
}

pub(super) struct PageSetupCommand {
    pub(super) sheet_id: u32,
    pub(super) before: suite_common_core::print::PageSetup,
    pub(super) after: suite_common_core::print::PageSetup,
}

impl Command<WorkbookState> for PageSetupCommand {
    fn apply(&self, state: &mut WorkbookState) {
        if let Some(idx) = state.sheet_index_for_id(self.sheet_id) {
            state.sheets[idx].borrow_mut().page_setup = self.after.clone();
        }
    }
    fn undo(&self, state: &mut WorkbookState) {
        if let Some(idx) = state.sheet_index_for_id(self.sheet_id) {
            state.sheets[idx].borrow_mut().page_setup = self.before.clone();
        }
    }
    fn description(&self) -> &str {
        "Page Setup"
    }
}

pub(super) struct SortCommand {
    pub(super) sheet_id: u32,
    pub(super) before_inputs: Vec<Vec<String>>,
    pub(super) after_inputs: Vec<Vec<String>>,
    pub(super) before_sheet: SheetModel,
    pub(super) after_sheet: SheetModel,
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
        self.description
    }
}
