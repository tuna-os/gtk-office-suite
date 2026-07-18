// export.rs — Typst export for Tables.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::engine::TablesEngine;

/// Export spreadsheet to Typst source (table format).
pub fn to_typst(ss: &TablesEngine) -> String {
    let grid = ss.to_grid();
    let mut out = String::from("#table(\n  columns: 1,\n");
    for row in &grid {
        out.push_str("  [");
        let cells: Vec<&str> = row.iter().map(|s| s.as_str()).collect();
        out.push_str(&cells.join("], ["));
        out.push_str("],\n");
    }
    out.push_str(")\n");
    out
}

/// Export to PDF via the in-process Typst engine.
pub fn to_pdf(ss: &TablesEngine, output_path: &str) -> Result<(), String> {
    suite_export::compile_pdf_to_file(&to_typst(ss), output_path)
}
