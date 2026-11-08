// support/mod.rs — the dynamic-array cases the tests share (#1659).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// `dynamic_arrays.rs` pins them against the engine and the XLSX writer;
// `soffice_oracle.rs` pins what a Calc rewrite does to the same workbook.
// tests/test_tables_dynamic_arrays.py holds docs/TABLES-DYNAMIC-ARRAYS.md
// to ADMITTED.
#![allow(dead_code)]

use std::io::Read;
use tables_core::controller::WorkbookController;
use tables_core::engine::TablesEngine;

/// The functions the suite admits as spilling, each exercised by CASES.
pub const ADMITTED: &[&str] = &[
    "SEQUENCE", "TRANSPOSE", "FILTER", "SORT", "UNIQUE", "HSTACK", "VSTACK", "TAKE", "DROP", "TOCOL", "TOROW", "MMULT", "TEXTSPLIT",
    "RANDARRAY",
];

/// One row per admitted function and shape: the formula, and the
/// rectangle it spills from its anchor, as the grid shows it.
pub struct Case {
    pub function: &'static str,
    pub formula: &'static str,
    pub spill: &'static [&'static [&'static str]],
}

pub const CASES: &[Case] = &[
    Case { function: "SEQUENCE", formula: "=SEQUENCE(3)", spill: &[&["1"], &["2"], &["3"]] },
    Case { function: "SEQUENCE", formula: "=SEQUENCE(2,3)", spill: &[&["1", "2", "3"], &["4", "5", "6"]] },
    Case { function: "SEQUENCE", formula: "=SEQUENCE(3,1,10,5)", spill: &[&["10"], &["15"], &["20"]] },
    Case { function: "TRANSPOSE", formula: "=TRANSPOSE(A1:B3)", spill: &[&["1", "2", "3"], &["5", "6", "7"]] },
    Case { function: "FILTER", formula: "=FILTER(A1:A4,A1:A4>2)", spill: &[&["3"], &["4"]] },
    Case { function: "SORT", formula: "=SORT(A1:A4,1,-1)", spill: &[&["4"], &["3"], &["2"], &["1"]] },
    Case { function: "UNIQUE", formula: "=UNIQUE(C1:C4)", spill: &[&["a"], &["b"], &["c"]] },
    Case { function: "HSTACK", formula: "=HSTACK(A1:A2,B1:B2)", spill: &[&["1", "5"], &["2", "6"]] },
    Case { function: "VSTACK", formula: "=VSTACK(A1:A2,B1:B2)", spill: &[&["1"], &["2"], &["5"], &["6"]] },
    Case { function: "TAKE", formula: "=TAKE(A1:A4,2)", spill: &[&["1"], &["2"]] },
    Case { function: "DROP", formula: "=DROP(A1:A4,2)", spill: &[&["3"], &["4"]] },
    Case { function: "TOCOL", formula: "=TOCOL(A1:B2)", spill: &[&["1"], &["5"], &["2"], &["6"]] },
    Case { function: "TOROW", formula: "=TOROW(A1:B2)", spill: &[&["1", "5", "2", "6"]] },
    Case { function: "MMULT", formula: "=MMULT(A1:B2,A1:B2)", spill: &[&["11", "35"], &["14", "46"]] },
    Case { function: "TEXTSPLIT", formula: "=TEXTSPLIT(\"a,b,c\",\",\")", spill: &[&["a", "b", "c"]] },
];

/// Classic Excel functions that return arrays. They need no `_xlfn.`
/// prefix, and Calc 24.2 implements them (but evaluates one over its
/// anchor cell alone, since the file records only the anchor).
pub const CLASSIC: &[&str] = &["TRANSPOSE", "MMULT"];

pub const ROWS: usize = 14;
pub const COLS: usize = 80;
/// Where a single formula goes: E1, clear of the source data in A:C.
pub const ANCHOR: (usize, usize) = (0, 4);

/// A1:A4 = 1..4, B1:B4 = 5..8, C1:C4 = a, b, a, c.
pub fn with_source_data(engine: &mut TablesEngine) {
    for (r, (n, m, t)) in [("1", "5", "a"), ("2", "6", "b"), ("3", "7", "a"), ("4", "8", "c")].into_iter().enumerate() {
        engine.set_cell_text(r, 0, n).unwrap();
        engine.set_cell_text(r, 1, m).unwrap();
        engine.set_cell_text(r, 2, t).unwrap();
    }
}

pub fn engine() -> TablesEngine {
    let mut e = TablesEngine::new(ROWS, COLS).unwrap();
    with_source_data(&mut e);
    e
}

/// The rectangle `rows` x `cols` of the rendered grid from (row, col).
pub fn rect(e: &TablesEngine, row: usize, col: usize, rows: usize, cols: usize) -> Vec<Vec<String>> {
    let grid = e.to_grid();
    (row..row + rows).map(|r| (col..col + cols).map(|c| grid[r][c].clone()).collect()).collect()
}

pub fn expected(case: &Case) -> Vec<Vec<String>> {
    case.spill.iter().map(|row| row.iter().map(|c| c.to_string()).collect()).collect()
}

pub fn spilled(e: &TablesEngine, case: &Case, row: usize, col: usize) -> Vec<Vec<String>> {
    rect(e, row, col, case.spill.len(), case.spill[0].len())
}

/// Each case in its own block of columns (a spill is at most four wide), at
/// row 1 from column E, in one workbook.
pub fn workbook_with_every_case() -> (WorkbookController, Vec<(usize, &'static Case)>) {
    let mut c = WorkbookController::new(ROWS, COLS).unwrap();
    for (r, (n, m, t)) in [("1", "5", "a"), ("2", "6", "b"), ("3", "7", "a"), ("4", "8", "c")].into_iter().enumerate() {
        c.edit_cell(r, 0, n);
        c.edit_cell(r, 1, m);
        c.edit_cell(r, 2, t);
    }
    // The file sizes the grid it reopens to its stored cells (or 100 x 26, if
    // larger): an end marker keeps the last spill inside it.
    c.edit_cell(ROWS - 1, COLS - 1, "end");
    let mut placed = Vec::new();
    for (k, case) in CASES.iter().enumerate() {
        let col = 4 + k * 5;
        c.edit_cell(0, col, case.formula);
        placed.push((col, case));
    }
    (c, placed)
}

pub fn save(c: &WorkbookController, path: &std::path::Path) {
    let state = c.state.borrow();
    let sheets: Vec<_> = state.sheets.iter().map(|s| s.borrow().clone()).collect();
    tables_core::io::save_sheets_to_xlsx_with_engine(path.to_str().unwrap(), &sheets, Some(&state.engine)).unwrap();
}

pub fn sheet_xml(path: &std::path::Path) -> String {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut xml = String::new();
    zip.by_name("xl/worksheets/sheet1.xml").unwrap().read_to_string(&mut xml).unwrap();
    xml
}

