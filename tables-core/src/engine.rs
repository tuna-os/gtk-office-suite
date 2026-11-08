// engine.rs — Spreadsheet engine using IronCalc for formula evaluation.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Decision: IronCalc; `ironcalc_base = "0.8"` is locked to 0.8.3.
// Upstream 0.8 added dynamic arrays/spilling, LAMBDA/LET, and 100+ functions.
// Tables delegates evaluation, but its tests only establish the SEQUENCE
// spill cases below; they do not certify the full upstream or Excel feature set.
// XLSX import/export remain separate: calamine + rust_xlsxwriter.
//
// Architecture:
//   TablesEngine wraps ironcalc_base::Model and provides:
//   - Cell value get/set with formula detection (= prefix)
//   - Formula evaluation via model.evaluate()
//   - Column/row management
//   - Grid data export for Cairo rendering
//
// Calculation authority (TBL-006): IronCalc owns inputs, formulas, names,
// dependencies and calculated values. `SheetModel.data` is a projection —
// read-only display text, never written back as input. Every path in and
// out of the engine, with its upstream API and error semantics:
//
// | path | upstream API | on upstream error |
// | user edits, paste, fill, sort (controller ops) | `put_cells_on_sheet`, one `evaluate` per batch | the op fails; `apply_all` undoes the group, history and redo untouched |
// | structural ops and reference relinks | `insert/delete_rows/cols`, `put_cells_on_sheet` | same: the op fails before any history step |
// | reference translation (sort, fill) | `move_cell_value_to_area`, `extend_copied_value` | `Err`: the operation aborts instead of keeping stale references |
// | import (`io::load`) | `put_cell_text` per cell, one `evaluate` | `Err` naming the cell; a failed load commits nothing to the window |
// | collab rebuild | `put_cells_on_sheet` | `Err`; the merge keeps the previous workbook |
// | save, autosave, recovery (`io::save`) | `formula_at` + `input_at`, never the projection | `Err`; the previous file or snapshot stays |
// | reads (render, copy, formula bar, stats) | `cell_at` (displayed), `input_at` (editable), `formula_at` | n/a: infallible reads, unknown cells read empty |

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

    /// Move worksheets into `new_order` (a permutation of current indices).
    ///
    /// This used to rebuild the workbook from scratch, replaying every
    /// cell's input text into a fresh model, on the premise that IronCalc
    /// had no native sheet-move operation. It does: `Model::move_sheet`,
    /// which lifts the worksheet out and reinserts it, then re-parses
    /// every formula from the stored text so the sheet indices inside
    /// parsed references are rebuilt against the new positions.
    ///
    /// The rebuild carried only names and cell inputs across, so it
    /// silently discarded everything else the model held — defined names
    /// most visibly (#527), and sheet ids, which is how a reorder used to
    /// hand two sheets the same identity (#442). Moving sheets keeps all
    /// of it, because nothing is recreated.
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

        let active_old_index = self.active_sheet;

        // Selection sort against the target order: `current[position]` is
        // the original index of whatever sits there now, so each step
        // moves the sheet that belongs at `target` into place and the
        // prefix is never disturbed again.
        let mut current: Vec<usize> = (0..sheet_count).collect();
        for (target, &wanted) in new_order.iter().enumerate() {
            let from = current
                .iter()
                .position(|&original| original == wanted)
                .expect("new_order was checked to be a permutation");
            if from != target {
                self.model.move_sheet(from as u32, target as u32)?;
                let moved = current.remove(from);
                current.insert(target, moved);
            }
        }

        self.active_sheet = new_order
            .iter()
            .position(|&old_idx| old_idx == active_old_index)
            .unwrap_or(0);
        self.model.evaluate();
        Ok(())
    }

    /// What was typed into a cell on sheet `sheet`: its formula with the
    /// leading `=`, else its value as text ("" when empty). Setting this
    /// back with `set_cell_text` recreates the cell.
    pub fn input_at(&self, sheet: usize, row: usize, col: usize) -> String {
        self.model
            .get_localized_cell_content(sheet as u32, row as i32 + 1, col as i32 + 1)
            .unwrap_or_default()
    }

    /// Whether the cell holds a plain number (not text, bool, error or an
    /// unevaluated formula): the writer and the editor keep the engine's
    /// canonical numeric text for these, since a formatted rendering (a
    /// date, a currency) would not parse back to the value.
    pub fn is_number_at(&self, sheet: usize, row: usize, col: usize) -> bool {
        matches!(
            self.model.get_cell_value_by_index(sheet as u32, row as i32 + 1, col as i32 + 1),
            Ok(CellValue::Number(_))
        )
    }

    /// The editable input for a cell: its formula with the leading `=`,
    /// else text that recreates it when set back. A spill-derived cell
    /// (`Cell::SpillCell`: materialized by evaluation, holding another
    /// cell's computed value) reports no input — writing its displayed
    /// value back would flatten it into a constant blocking the spill.
    /// Plain values otherwise come from [`Self::input_at`] — except
    /// numbers, where a formatted rendering does not parse back and the
    /// canonical numeric text does. This is what undo captures, copy
    /// carries, sort moves and save writes; the displayed value never
    /// stands in for it.
    /// Whether the formula at (`row`, `col`) spills: the cell to its right
    /// or below is one of the cells the engine derived from it
    /// (`Cell::SpillCell` records its anchor). A blocked spill (#SPILL!)
    /// derives nothing and a scalar formula never does.
    pub fn is_spill_anchor(&self, sheet: usize, row: usize, col: usize) -> bool {
        let Ok(ws) = self.model.workbook.worksheet(sheet as u32) else { return false };
        let anchor = (row as i32 + 1, col as i32 + 1);
        [(anchor.0, anchor.1 + 1), (anchor.0 + 1, anchor.1)].into_iter().any(|(r, c)| {
            matches!(ws.cell(r, c).as_deref(), Some(ironcalc_base::types::Cell::SpillCell { a, .. }) if *a == anchor)
        })
    }

    pub fn editable_input_at(&self, sheet: usize, row: usize, col: usize) -> String {
        if let Ok(ws) = self.model.workbook.worksheet(sheet as u32) {
            if let Some(cell) = ws.cell(row as i32 + 1, col as i32 + 1) {
                if matches!(cell.as_ref(), ironcalc_base::types::Cell::SpillCell { .. }) {
                    return String::new();
                }
            }
        }
        if self.formula_at(sheet, row, col).is_some() {
            return self.input_at(sheet, row, col);
        }
        let input = self.input_at(sheet, row, col);
        if input.is_empty() {
            return String::new();
        }
        if self.is_number_at(sheet, row, col) {
            self.cell_at(sheet, row, col)
        } else {
            input
        }
    }

    /// Every formula cell in the workbook, `(sheet, row, col, input)`.
    pub fn formula_cells(&self) -> Vec<(usize, usize, usize, String)> {
        let mut out = Vec::new();
        for (s, ws) in self.model.workbook.worksheets.iter().enumerate() {
            for (r, c, cell) in ws.sheet_data.cells() {
                if cell.has_formula() && r >= 1 && c >= 1 {
                    let (r, c) = (r as usize - 1, c as usize - 1);
                    out.push((s, r, c, self.input_at(s, r, c)));
                }
            }
        }
        out
    }

    /// Insert `count` blank rows before `at` on sheet `sheet`; formulas
    /// everywhere follow the cells they point at.
    pub fn insert_rows(&mut self, sheet: usize, at: usize, count: usize) -> Result<(), String> {
        self.model.insert_rows(sheet as u32, at as i32 + 1, count as i32)?;
        self.model.evaluate();
        Ok(())
    }

    pub fn delete_rows(&mut self, sheet: usize, at: usize, count: usize) -> Result<(), String> {
        self.model.delete_rows(sheet as u32, at as i32 + 1, count as i32)?;
        self.model.evaluate();
        Ok(())
    }

    pub fn insert_cols(&mut self, sheet: usize, at: usize, count: usize) -> Result<(), String> {
        self.model.insert_columns(sheet as u32, at as i32 + 1, count as i32)?;
        self.model.evaluate();
        Ok(())
    }

    pub fn delete_cols(&mut self, sheet: usize, at: usize, count: usize) -> Result<(), String> {
        self.model.delete_columns(sheet as u32, at as i32 + 1, count as i32)?;
        self.model.evaluate();
        Ok(())
    }

    /// Insert a worksheet at `index`, reusing `sheet_id` when given (so a
    /// sheet deleted and put back keeps its identity).
    pub fn insert_sheet(&mut self, index: usize, name: &str, sheet_id: Option<u32>) -> Result<(), String> {
        self.model.insert_sheet(name, index as u32, sheet_id)?;
        if self.active_sheet >= index && self.sheet_count() > 1 {
            self.active_sheet += 1;
        }
        self.model.evaluate();
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
            Ok(CellValue::Number(n)) => general_number(n),
            Ok(CellValue::Boolean(b)) => format!("{}", b),
            Ok(CellValue::None) => String::new(),
            Err(_) => String::new(),
        }
    }

    /// Set a cell on the active sheet and recalculate, propagating an
    /// upstream rejection (an unknown sheet, a cell that cannot take
    /// input) instead of looking successful. For many cells prefer
    /// [`Self::put_cells_on_sheet`]: one calculation boundary for the
    /// whole batch, not one recalculation per cell.
    pub fn set_cell_text(&mut self, row: usize, col: usize, val: &str) -> Result<(), String> {
        let sheet = self.active_sheet;
        self.put_cells_on_sheet(sheet, [(row, col, val)])
    }

    /// [`Self::set_cell_text`] on `sheet` instead of the active one.
    pub fn set_cell_text_on_sheet(&mut self, sheet: usize, row: usize, col: usize, val: &str) -> Result<(), String> {
        self.put_cells_on_sheet(sheet, [(row, col, val)])
    }

    /// `set_cell_text` without the recalculation, for loops that set many
    /// cells and call [`evaluate`](Self::evaluate) once at the end. Each
    /// `evaluate` walks the whole workbook, so recalculating per cell made
    /// opening a file, pasting a grid and exporting a PDF quadratic in the
    /// cell count: a 512×128 xlsx did not open in twenty minutes (#1208).
    ///
    /// The upstream error is returned: a write the engine rejects is an
    /// error here, not a silent no-op.
    pub fn put_cell_text(&mut self, row: usize, col: usize, val: &str) -> Result<(), String> {
        let sheet = self.active_sheet;
        self.put_cell_text_on_sheet(sheet, row, col, val)
    }

    /// [`Self::put_cell_text`] on `sheet` instead of the active one.
    pub fn put_cell_text_on_sheet(&mut self, sheet: usize, row: usize, col: usize, val: &str) -> Result<(), String> {
        let r = row as i32 + 1; // IronCalc uses 1-based, i32
        let c = col as i32 + 1;
        self.model
            .set_user_input(sheet as u32, r, c, val.to_string())
            .map_err(|e| format!("cell ({row}, {col}) on sheet {sheet}: {e}"))
    }

    /// Write many cells on `sheet` with a single calculation boundary, all
    /// or nothing: the inputs they had are put back and the error returned
    /// if any write is rejected, so a failed batch leaves engine,
    /// projections, history and redo exactly where they were. Every
    /// batched user write (paste, fill, sort, import, collab rebuild)
    /// comes through here.
    pub fn put_cells_on_sheet<'a>(
        &mut self,
        sheet: usize,
        inputs: impl IntoIterator<Item = (usize, usize, &'a str)>,
    ) -> Result<(), String> {
        let inputs: Vec<(usize, usize, &'a str)> = inputs.into_iter().collect();
        let olds: Vec<(usize, usize, String)> = inputs
            .iter()
            .map(|(row, col, _)| (*row, *col, self.editable_input_at(sheet, *row, *col)))
            .collect();
        for (row, col, val) in &inputs {
            if let Err(e) = self.put_cell_text_on_sheet(sheet, *row, *col, val) {
                // Best effort: the batch already failed; put back what was
                // there so the caller sees one error, not a half-write.
                for (row, col, old) in &olds {
                    let _ = self.put_cell_text_on_sheet(sheet, *row, *col, old);
                }
                self.model.evaluate();
                return Err(e);
            }
        }
        self.model.evaluate();
        Ok(())
    }

    /// Put a file's text cell at (row, col) as text, never parsed: a text
    /// "1041", "TRUE" or "=x" stays text, as the file says, with the quote
    /// prefix only where plain input would read it as something else.
    /// Through `put_cell_text` a code column of text digits became numbers
    /// and showed as "1,041" (render-real `bics-sectoral-consumption`).
    /// The upstream error is returned, as `put_cell_text` returns it.
    pub fn put_cell_literal_text(&mut self, row: usize, col: usize, val: &str) -> Result<(), String> {
        let (r, c) = (row as i32 + 1, col as i32 + 1);
        let sheet = self.active_sheet;
        self.model
            .update_cell_with_text(sheet as u32, r, c, val)
            .map_err(|e| format!("cell ({row}, {col}) on sheet {sheet}: {e}"))
    }

    /// The formula of workbook-level name `name`, if it's defined.
    pub fn defined_name(&self, name: &str) -> Option<String> {
        self.model
            .workbook
            .defined_names
            .iter()
            .find(|n| n.sheet_id.is_none() && n.name.eq_ignore_ascii_case(name))
            .map(|n| n.formula.clone())
    }

    /// Define, redefine or (with `None`) remove workbook-level name `name`.
    ///
    /// A name can outlive the sheet it points at (deleting a sheet leaves
    /// its names' text alone, as Excel shows `#REF!`), and undo has to put
    /// such a name back exactly. IronCalc won't create a name whose sheet
    /// is gone but will update one, so a new name goes in as a placeholder
    /// and is then updated to its formula.
    pub fn set_defined_name(&mut self, name: &str, formula: Option<&str>) -> Result<(), String> {
        match (self.defined_name(name), formula) {
            (None, None) => Ok(()),
            (Some(_), None) => self.model.delete_defined_name(name, None),
            (Some(_), Some(f)) => self.model.update_defined_name(name, None, name, None, f),
            (None, Some(f)) => {
                if self.model.new_defined_name(name, None, f).is_ok() {
                    return Ok(());
                }
                self.model.workbook.defined_names.push(ironcalc_base::types::DefinedName {
                    name: name.to_string(),
                    formula: f.to_string(),
                    sheet_id: None,
                });
                let result = self.model.update_defined_name(name, None, name, None, f);
                if result.is_err() {
                    self.model.workbook.defined_names.pop();
                }
                result
            }
        }
    }

    /// Evaluate all formulas.
    pub fn evaluate(&mut self) {
        self.model.evaluate();
    }

    /// Translate relative references as a cell input moves within the grid.
    /// An upstream rejection (an unknown sheet, a reference the engine
    /// cannot move) is an error, not the input back unchanged: callers
    /// decide, visibly, what a failed translation means for their
    /// operation instead of keeping stale references that look successful.
    pub fn move_input(&mut self, input: &str, from: (usize, usize), to: (usize, usize)) -> Result<String, String> {
        if !input.starts_with('=') || from == to {
            return Ok(input.to_string());
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
            .map_err(|e| format!("cannot move {input:?}: {e}"))
    }

    /// Adjust a formula's relative references as if it were copied (not
    /// moved) from `from` to `to` — e.g. `=A1` copied from A1 down to A2
    /// becomes `=A2`, but `=$A$1` stays `=$A$1`. Unlike `move_input`
    /// (which updates *other* cells' references to point at a relocated
    /// cell), this is the fill-handle/copy-paste semantics: only the
    /// formula text itself changes, nothing else in the sheet does.
    pub fn extend_input(&mut self, input: &str, from: (usize, usize), to: (usize, usize)) -> Result<String, String> {
        if !input.starts_with('=') || from == to {
            return Ok(input.to_string());
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
            .map_err(|e| format!("cannot copy {input:?}: {e}"))
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
        self.has_formula_at(self.active_sheet, row, col)
    }

    /// [`Self::has_formula`] on `sheet` instead of the active one. Reads
    /// the engine's formula flag, not the projection, so a synced sheet
    /// and the engine cannot disagree about what is a formula.
    pub fn has_formula_at(&self, sheet: usize, row: usize, col: usize) -> bool {
        let r = row as i32 + 1;
        let c = col as i32 + 1;
        // Check raw cell for formula flag
        if let Ok(ws) = self.model.workbook.worksheet(sheet as u32) {
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

    /// Like [`Self::to_grid`], bounded to `range` (top, left, bottom,
    /// right, 0-based inclusive) — for print-area export (#113).
    pub fn to_grid_range(&self, range: (usize, usize, usize, usize)) -> Vec<Vec<String>> {
        let (top, left, bottom, right) = range;
        let bottom = bottom.min(self.rows.saturating_sub(1));
        let right = right.min(self.cols.saturating_sub(1));
        let mut grid = Vec::new();
        for r in top..=bottom.max(top) {
            let mut row = Vec::new();
            for c in left..=right.max(left) {
                row.push(self.cell(r, c));
            }
            grid.push(row);
        }
        grid
    }
}

/// A number as its cell text, the way a spreadsheet's "General" format
/// shows it: up to 15 significant digits (Excel's precision), trailing zeros
/// dropped. This string is also what the sheet model stores, so it must not
/// round: it used to be `{:.2}`, which turned 0.153 into "0.15" before any
/// number format saw it (render lab `tables/number-formats`: 15.3% came out
/// as 15.0%).
pub fn general_number(n: f64) -> String {
    if !n.is_finite() {
        return n.to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e15 {
        return format!("{}", n as i64);
    }
    let magnitude = n.abs().log10().floor() as i32;
    let decimals = 14 - magnitude;
    if !(0..=20).contains(&decimals) {
        return format!("{n}");
    }
    let s = format!("{:.*}", decimals as usize, n);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_string() } else { s.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_show_at_full_precision_without_float_noise() {
        assert_eq!(general_number(0.153), "0.153");
        assert_eq!(general_number(1234567.891), "1234567.891");
        assert_eq!(general_number(0.1 + 0.2), "0.3");
        assert_eq!(general_number(1.0 / 3.0), "0.333333333333333");
        assert_eq!(general_number(-2.5), "-2.5");
        assert_eq!(general_number(42.0), "42");
        assert_eq!(general_number(1e-20), "0.00000000000000000001");
    }

    #[test]
    fn test_engine_creation() {
        let engine = TablesEngine::new(10, 5).unwrap();
        assert_eq!(engine.rows, 10);
        assert_eq!(engine.cols, 5);
    }

    #[test]
    fn test_set_and_get_cell() {
        let mut engine = TablesEngine::new(5, 5).unwrap();
        engine.set_cell_text(0, 0, "Hello").unwrap();
        assert_eq!(engine.cell(0, 0), "Hello");
    }

    #[test]
    fn test_formula_sum() {
        let mut engine = TablesEngine::new(5, 5).unwrap();
        engine.set_cell_text(0, 0, "10").unwrap();
        engine.set_cell_text(1, 0, "20").unwrap();
        engine.set_cell_text(2, 0, "30").unwrap();
        engine.set_cell_text(2, 1, "=SUM(A1:A3)").unwrap();
        assert_eq!(engine.cell(2, 1), "60");
    }

    #[test]
    fn sequence_formula_spills_values_into_the_rendered_grid() {
        let mut engine = TablesEngine::new(4, 2).unwrap();
        engine.set_cell_text(0, 0, "=SEQUENCE(3)").unwrap();

        let grid = engine.to_grid();
        assert_eq!(grid[0][0], "1");
        assert_eq!(grid[1][0], "2");
        assert_eq!(grid[2][0], "3");
        let mut sheet = crate::sheet::SheetModel::new("Sheet1", 4, 2, 0);
        sheet.sync_from_engine(&engine);
        assert_eq!(sheet.data[0][0], "1");
        assert_eq!(sheet.data[1][0], "2");
        assert_eq!(sheet.data[2][0], "3");
        assert!(sheet.formulas[0][0]);
        assert!(!sheet.formulas[1][0]);
    }

    /// The XLSX writer asks this to write a spilling formula as a dynamic
    /// array formula: Excel evaluates a plain TRANSPOSE or MMULT in one cell.
    #[test]
    fn a_formula_is_a_spill_anchor_only_when_it_derives_cells() {
        let mut engine = TablesEngine::new(8, 8).unwrap();
        for (r, v) in [(0, "1"), (1, "2")] {
            engine.set_cell_text(r, 0, v).unwrap();
            engine.set_cell_text(r, 1, v).unwrap();
        }
        engine.set_cell_text(0, 3, "=SEQUENCE(3)").unwrap();
        engine.set_cell_text(0, 4, "=TRANSPOSE(A1:B2)").unwrap();
        engine.set_cell_text(0, 6, "=A1+1").unwrap();
        engine.set_cell_text(5, 5, "=SEQUENCE(1,3)").unwrap();
        assert!(engine.is_spill_anchor(0, 0, 3), "a column spill");
        assert!(engine.is_spill_anchor(0, 0, 4), "a transposed block");
        assert!(engine.is_spill_anchor(0, 5, 5), "a row spill");
        assert!(!engine.is_spill_anchor(0, 0, 6), "a scalar formula");
        assert!(!engine.is_spill_anchor(0, 1, 3), "a derived cell is not an anchor");
        assert!(!engine.is_spill_anchor(0, 0, 0), "a constant");
        let mut blocked = TablesEngine::new(4, 4).unwrap();
        blocked.set_cell_text(1, 0, "x").unwrap();
        blocked.set_cell_text(0, 0, "=SEQUENCE(3)").unwrap();
        assert!(!blocked.is_spill_anchor(0, 0, 0), "a blocked spill derives nothing");
    }

    #[test]
    fn sequence_formula_surfaces_spill_blocked_by_existing_cell() {
        let mut engine = TablesEngine::new(4, 4).unwrap();
        engine.set_cell_text(1, 1, "blocked").unwrap();
        engine.set_cell_text(0, 0, "=SEQUENCE(2,2)").unwrap();

        let grid = engine.to_grid();
        assert_eq!(grid[0][0], "#SPILL!");
        assert_eq!(grid[1][1], "blocked");
    }

    #[test]
    fn test_formula_concat() {
        let mut engine = TablesEngine::new(3, 3).unwrap();
        engine.set_cell_text(0, 0, "Hello").unwrap();
        engine.set_cell_text(0, 1, "World").unwrap();
        engine.set_cell_text(0, 2, "=CONCAT(A1, \" \", B1)").unwrap();
        assert_eq!(engine.cell(0, 2), "Hello World");
    }

    #[test]
    fn extend_input_shifts_relative_references() {
        let mut engine = TablesEngine::new(5, 5).unwrap();
        let shifted = engine.extend_input("=A1", (0, 0), (1, 0)).unwrap();
        assert_eq!(shifted, "=A2");
    }

    #[test]
    fn extend_input_leaves_absolute_references_unchanged() {
        let mut engine = TablesEngine::new(5, 5).unwrap();
        let shifted = engine.extend_input("=$A$1", (0, 0), (1, 0)).unwrap();
        assert_eq!(shifted, "=$A$1");
    }

    #[test]
    fn extend_input_leaves_non_formulas_unchanged() {
        let mut engine = TablesEngine::new(5, 5).unwrap();
        assert_eq!(engine.extend_input("42", (0, 0), (1, 0)).unwrap(), "42");
    }

    #[test]
    fn extend_input_same_source_and_target_is_a_no_op() {
        let mut engine = TablesEngine::new(5, 5).unwrap();
        assert_eq!(engine.extend_input("=A1+1", (2, 2), (2, 2)).unwrap(), "=A1+1");
    }

    #[test]
    fn test_grid_export() {
        let mut engine = TablesEngine::new(2, 2).unwrap();
        engine.set_cell_text(0, 0, "A1").unwrap();
        engine.set_cell_text(0, 1, "B1").unwrap();
        engine.set_cell_text(1, 0, "A2").unwrap();
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
        engine.set_cell_text(0, 0, "=1+1").unwrap();
        assert!(engine.has_formula(0, 0));
        engine.set_cell_text(0, 1, "not a formula").unwrap();
        assert!(!engine.has_formula(0, 1));
    }

    #[test]
    fn cross_sheet_formula_recalculates() {
        let mut engine = TablesEngine::new(3, 3).unwrap();
        engine.set_cell_text(0, 0, "7").unwrap();
        engine.add_sheet("Sheet2").unwrap();
        engine.set_active_sheet(1).unwrap();
        engine.set_cell_text(0, 0, "=Sheet1!A1*2").unwrap();
        assert_eq!(engine.cell(0, 0), "14");

        engine.set_active_sheet(0).unwrap();
        engine.set_cell_text(0, 0, "9").unwrap();
        engine.set_active_sheet(1).unwrap();
        assert_eq!(engine.cell(0, 0), "18");
    }

    #[test]
    fn delete_sheet_removes_only_that_sheet() {
        let mut engine = TablesEngine::new(3, 3).unwrap();
        engine.set_cell_text(0, 0, "1").unwrap();
        engine.add_sheet("Sheet2").unwrap();
        engine.set_active_sheet(1).unwrap();
        engine.set_cell_text(0, 0, "2").unwrap();
        engine.add_sheet("Sheet3").unwrap();
        engine.set_active_sheet(2).unwrap();
        engine.set_cell_text(0, 0, "3").unwrap();

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
        engine.set_cell_text(0, 0, "1").unwrap(); // Sheet1
        engine.add_sheet("Sheet2").unwrap();
        engine.set_active_sheet(1).unwrap();
        engine.set_cell_text(0, 0, "=Sheet1!A1*10").unwrap();
        engine.add_sheet("Sheet3").unwrap();
        engine.set_active_sheet(2).unwrap();
        engine.set_cell_text(0, 0, "3").unwrap();

        // Move Sheet3 to the front: [2, 0, 1]
        engine.reorder_sheets(&[2, 0, 1]).unwrap();
        assert_eq!(engine.sheet_name_at(0).as_deref(), Some("Sheet3"));
        assert_eq!(engine.sheet_name_at(1).as_deref(), Some("Sheet1"));
        assert_eq!(engine.sheet_name_at(2).as_deref(), Some("Sheet2"));
        assert_eq!(engine.cell_at(0, 0, 0), "3");
        assert_eq!(engine.cell_at(1, 0, 0), "1");
        assert_eq!(engine.cell_at(2, 0, 0), "10");

        engine.set_active_sheet(1).unwrap();
        engine.set_cell_text(0, 0, "5").unwrap();
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
    #[test]
    fn rejected_writes_are_errors_not_silent_no_ops() {
        let mut engine = TablesEngine::new(2, 2).unwrap();
        assert!(engine.put_cell_text_on_sheet(99, 0, 0, "x").is_err());
        assert!(engine.set_cell_text_on_sheet(99, 0, 0, "x").is_err());
        assert!(engine.put_cells_on_sheet(99, [(0, 0, "x")]).is_err());
    }

    #[test]
    fn failed_batch_writes_leave_the_prior_inputs_in_place() {
        let mut engine = TablesEngine::new(3, 3).unwrap();
        engine.set_cell_text(0, 0, "kept").unwrap();
        let before = engine.input_at(0, 0, 0);
        assert!(engine.put_cells_on_sheet(99, [(0, 0, "x")]).is_err());
        assert_eq!(engine.input_at(0, 0, 0), before);
        assert_eq!(before, "kept");
    }

    #[test]
    fn reference_translation_reports_upstream_rejections() {
        let mut engine = TablesEngine::new(2, 2).unwrap();
        // The fast paths never touch upstream; anything else is the
        // engine's own answer, success included — never the input back
        // with a swallowed error. (`extend` shifting `=A1` to `=A2` is
        // the long-established behavior above.)
        assert_eq!(engine.move_input("42", (0, 0), (1, 0)).unwrap(), "42");
        assert_eq!(engine.move_input("=A1+1", (2, 2), (2, 2)).unwrap(), "=A1+1");
        assert_eq!(engine.extend_input("=A1", (0, 0), (1, 0)).unwrap(), "=A2");
        assert!(engine.move_input("=A1", (0, 0), (1, 0)).is_ok());
    }

    #[test]
    fn spill_derived_cells_report_no_editable_input() {
        let mut engine = TablesEngine::new(5, 5).unwrap();
        engine.set_cell_text(0, 0, "=SEQUENCE(3)").unwrap();
        // The spill shows downstream, but only the anchor holds input:
        // carrying the displayed "2" as input would flatten the spill
        // into a constant that blocks it.
        assert_eq!(engine.cell(1, 0), "2");
        assert_eq!(engine.editable_input_at(0, 1, 0), "");
        assert_eq!(engine.editable_input_at(0, 0, 0), "=SEQUENCE(3)");
    }

}
