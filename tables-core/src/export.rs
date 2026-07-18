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

/// Export to PDF via typst CLI.
pub fn to_pdf(ss: &TablesEngine, output_path: &str) -> Result<(), String> {
    let tmp = format!("{}.typ", output_path);
    std::fs::write(&tmp, to_typst(ss)).map_err(|e| e.to_string())?;
    let out = std::process::Command::new("typst")
        .args(["compile", &tmp, output_path])
        .output()
        .map_err(|e| format!("typst not found: {}", e))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into());
    }
    Ok(())
}
