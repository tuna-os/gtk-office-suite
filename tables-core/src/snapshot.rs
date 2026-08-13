// snapshot.rs — test-only state introspection (#104).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// A normalized view of canonical workbook state for deterministic GUI
// journeys to assert against, instead of scraping AT-SPI tree text.
// Hand-written rather than a serde derive on the real document model:
// this type is purpose-built for tests and stays decoupled from
// SheetModel/CellValue's real (xlsx/ods) serialization concerns.
//
// This module ships in production builds (it's plain, inert data-building
// code — no I/O, no GTK), but nothing calls it unless the app crate
// explicitly wires up a test-only entry point gated behind an env var
// check that lives in the app, not here. See tables/src/window.rs.

use crate::controller::WorkbookController;

pub struct CellSnapshot {
    pub row: usize,
    pub col: usize,
    pub value: String,
    pub formula: Option<String>,
}

pub struct SheetSnapshot {
    pub name: String,
    pub cells: Vec<CellSnapshot>,
    /// Rows currently hidden — by a column filter or a manual hide
    /// (#113); tests generally care whether a row is hidden, not why,
    /// so this folds both together the same way `SheetModel::is_row_hidden`
    /// does. Sorted ascending.
    pub hidden_rows: Vec<usize>,
    /// Columns currently manually hidden (#113), sorted ascending.
    pub hidden_cols: Vec<usize>,
    /// Normalized selection rect (top, left, bottom, right), 0-based
    /// inclusive — same shape as `SheetModel::selection_rect`. Lets tests
    /// assert on a jump/selection outcome (e.g. a named-range jump)
    /// without walking the live AT-SPI tree, which this app's custom
    /// grid accessible can't yet survive after certain selection
    /// sequences (see the grow/shrink-extent crash noted alongside
    /// #113's named-ranges GUI test).
    pub selection: (usize, usize, usize, usize),
    /// Currently-sorted column and direction (#113's "visible criteria"),
    /// or `None` if unsorted. `true` = ascending.
    pub sorted_col: Option<(usize, bool)>,
}

pub struct WorkbookSnapshot {
    pub active_sheet_index: usize,
    pub sheet_names: Vec<String>,
    pub sheet: SheetSnapshot,
    /// Top-left of the grid canvas in window coordinates, filled in by the
    /// app layer (`tables/src/window.rs`) because only it can ask GTK where
    /// the drawing area landed. `None` when the canvas isn't realized yet.
    ///
    /// Mouse-driven GUI journeys (fill-handle drag, header click) need the
    /// canvas origin to turn a cell address into a screen pixel, and AT-SPI
    /// reports the wrong position for widgets nested in box containers
    /// (upstream GTK4 bridge gap, #132). They used to hardcode a measured
    /// chrome height, which silently breaks whenever toolbar metrics change
    /// (e.g. the 44sp touch targets in #118) — reporting it here keeps those
    /// tests pinned to the real layout instead.
    pub grid_origin: Option<(i32, i32)>,
}

/// Snapshot the active sheet's cells within `rows` x `cols`, skipping
/// empty, non-formula cells to keep output small.
pub fn snapshot(
    controller: &WorkbookController,
    rows: std::ops::Range<usize>,
    cols: std::ops::Range<usize>,
) -> WorkbookSnapshot {
    let state = controller.state.borrow();
    let sheet_names: Vec<String> = state.sheets.iter().map(|s| s.borrow().name.clone()).collect();
    let active_sheet_index = state.active_sheet;

    let mut cells = Vec::new();
    for row in rows.clone() {
        for col in cols.clone() {
            let value = state.sheet().cell(row, col).to_string();
            let input = state.cell_input(row, col);
            let formula = input.strip_prefix('=').map(|f| f.to_string());
            if value.is_empty() && formula.is_none() {
                continue;
            }
            cells.push(CellSnapshot { row, col, value, formula });
        }
    }
    let mut hidden_rows: Vec<usize> = (0..state.sheet().rows)
        .filter(|&r| state.sheet().is_row_hidden(r))
        .collect();
    hidden_rows.sort_unstable();
    let mut hidden_cols: Vec<usize> = state.sheet().hidden_cols.iter().copied().collect();
    hidden_cols.sort_unstable();
    let selection = state.sheet().selection_rect();
    let sorted_col = state
        .sheet()
        .sorted_col
        .map(|(c, dir)| (c, dir == crate::sheet::SortDirection::Ascending));
    drop(state);

    let sheet = SheetSnapshot {
        name: sheet_names[active_sheet_index].clone(),
        cells,
        hidden_rows,
        hidden_cols,
        selection,
        sorted_col,
    };

    WorkbookSnapshot { active_sheet_index, sheet_names, sheet, grid_origin: None }
}

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn json_str(s: &str) -> String {
    format!("\"{}\"", escape_json(s))
}

fn json_opt_str(s: &Option<String>) -> String {
    match s {
        Some(v) => json_str(v),
        None => "null".to_string(),
    }
}

impl WorkbookSnapshot {
    pub fn to_json(&self) -> String {
        let sheet_names = self
            .sheet_names
            .iter()
            .map(|n| json_str(n))
            .collect::<Vec<_>>()
            .join(",");
        let cells = self
            .sheet
            .cells
            .iter()
            .map(|c| {
                format!(
                    "{{\"row\":{},\"col\":{},\"value\":{},\"formula\":{}}}",
                    c.row,
                    c.col,
                    json_str(&c.value),
                    json_opt_str(&c.formula),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let hidden_rows = self
            .sheet
            .hidden_rows
            .iter()
            .map(|r| r.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let hidden_cols = self
            .sheet
            .hidden_cols
            .iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let (sr0, sc0, sr1, sc1) = self.sheet.selection;
        let sorted_col = match self.sheet.sorted_col {
            Some((c, asc)) => format!("{{\"col\":{c},\"ascending\":{asc}}}"),
            None => "null".to_string(),
        };
        let grid_origin = match self.grid_origin {
            Some((x, y)) => format!("{{\"x\":{x},\"y\":{y}}}"),
            None => "null".to_string(),
        };
        format!(
            "{{\"active_sheet_index\":{},\"sheet_names\":[{}],\"grid_origin\":{},\"sheet\":{{\"name\":{},\"cells\":[{}],\"hidden_rows\":[{}],\"hidden_cols\":[{}],\"selection\":[{},{},{},{}],\"sorted_col\":{}}}}}",
            self.active_sheet_index,
            sheet_names,
            grid_origin,
            json_str(&self.sheet.name),
            cells,
            hidden_rows,
            hidden_cols,
            sr0, sc0, sr1, sc1,
            sorted_col,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::WorkbookController;

    #[test]
    fn snapshot_skips_empty_cells_and_reports_formulas() {
        let mut c = WorkbookController::new(5, 5).unwrap();
        c.edit_cell(0, 0, "42");
        c.edit_cell(0, 1, "=A1*2");
        let snap = snapshot(&c, 0..2, 0..2);
        assert_eq!(snap.sheet.cells.len(), 2);
        assert_eq!(snap.sheet.cells[0].value, "42");
        assert_eq!(snap.sheet.cells[0].formula, None);
        assert_eq!(snap.sheet.cells[1].value, "84");
        assert_eq!(snap.sheet.cells[1].formula.as_deref(), Some("A1*2"));
    }

    #[test]
    fn to_json_round_trips_shape() {
        let mut c = WorkbookController::new(3, 3).unwrap();
        c.edit_cell(0, 0, "hello \"world\"");
        let snap = snapshot(&c, 0..1, 0..1);
        let json = snap.to_json();
        assert!(json.contains("\\\"world\\\""));
        assert!(json.contains("\"active_sheet_index\":0"));
        assert!(json.contains("\"sheet_names\":[\"Sheet1\"]"));
    }

    #[test]
    fn snapshot_reports_hidden_rows_sorted() {
        let mut c = WorkbookController::new(3, 2).unwrap();
        c.edit_cell(0, 0, "apple");
        c.edit_cell(1, 0, "banana");
        c.edit_cell(2, 0, "apple");
        c.filter_by_value(0, "apple");
        let snap = snapshot(&c, 0..3, 0..2);
        assert_eq!(snap.sheet.hidden_rows, vec![1]);
        assert!(snap.to_json().contains("\"hidden_rows\":[1]"));
    }

    #[test]
    fn snapshot_reports_manually_hidden_rows_and_cols() {
        let mut c = WorkbookController::new(3, 3).unwrap();
        c.state.borrow().sheet_mut().select_cell(1, 1);
        c.hide_selected_rows();
        c.hide_selected_cols();
        let snap = snapshot(&c, 0..3, 0..3);
        assert_eq!(snap.sheet.hidden_rows, vec![1]);
        assert_eq!(snap.sheet.hidden_cols, vec![1]);
        let json = snap.to_json();
        assert!(json.contains("\"hidden_rows\":[1]"));
        assert!(json.contains("\"hidden_cols\":[1]"));
    }

    #[test]
    fn snapshot_reports_the_current_selection() {
        let c = WorkbookController::new(5, 5).unwrap();
        c.state.borrow().sheet_mut().select_cell(0, 0);
        c.state.borrow().sheet_mut().extend_selection(2, 0);
        let snap = snapshot(&c, 0..5, 0..5);
        assert_eq!(snap.sheet.selection, (0, 0, 2, 0));
        assert!(snap.to_json().contains("\"selection\":[0,0,2,0]"));
    }

    #[test]
    fn snapshot_reports_sorted_col_and_direction() {
        let mut c = WorkbookController::new(3, 2).unwrap();
        c.edit_cell(0, 0, "b");
        c.edit_cell(1, 0, "a");
        let snap = snapshot(&c, 0..3, 0..2);
        assert_eq!(snap.sheet.sorted_col, None);
        assert!(snap.to_json().contains("\"sorted_col\":null"));

        c.toggle_sort(0);
        let snap = snapshot(&c, 0..3, 0..2);
        assert_eq!(snap.sheet.sorted_col, Some((0, true)));
        assert!(snap.to_json().contains("\"sorted_col\":{\"col\":0,\"ascending\":true}"));
    }

    #[test]
    fn snapshot_reports_grid_origin_when_the_app_layer_supplies_it() {
        let c = WorkbookController::new(2, 2).unwrap();
        let mut snap = snapshot(&c, 0..2, 0..2);
        assert_eq!(snap.grid_origin, None);
        assert!(snap.to_json().contains("\"grid_origin\":null"));

        snap.grid_origin = Some((0, 138));
        assert!(snap.to_json().contains("\"grid_origin\":{\"x\":0,\"y\":138}"));
    }
}
