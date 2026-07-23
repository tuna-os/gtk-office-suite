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
}

pub struct WorkbookSnapshot {
    pub active_sheet_index: usize,
    pub sheet_names: Vec<String>,
    pub sheet: SheetSnapshot,
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
    drop(state);

    let sheet = SheetSnapshot {
        name: sheet_names[active_sheet_index].clone(),
        cells,
    };

    WorkbookSnapshot { active_sheet_index, sheet_names, sheet }
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
        format!(
            "{{\"active_sheet_index\":{},\"sheet_names\":[{}],\"sheet\":{{\"name\":{},\"cells\":[{}]}}}}",
            self.active_sheet_index,
            sheet_names,
            json_str(&self.sheet.name),
            cells,
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
}
