// export.rs — Typst export for Tables.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::engine::TablesEngine;

/// Export spreadsheet to Typst source (table format).
pub fn to_typst(ss: &TablesEngine) -> String {
    typst_table(&ss.to_grid())
}

/// Like [`to_typst`], bounded to a print area (#113) — `None` exports
/// the whole grid, same as `to_typst`.
pub fn to_typst_range(ss: &TablesEngine, range: Option<(usize, usize, usize, usize)>) -> String {
    match range {
        Some(r) => typst_table(&ss.to_grid_range(r)),
        None => to_typst(ss),
    }
}

fn typst_table(grid: &[Vec<String>]) -> String {
    let mut out = String::from("#table(\n  columns: 1,\n");
    for row in grid {
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

/// Like [`to_pdf`], bounded to a print area (#113).
pub fn to_pdf_range(
    ss: &TablesEngine,
    range: Option<(usize, usize, usize, usize)>,
    output_path: &str,
) -> Result<(), String> {
    suite_export::compile_pdf_to_file(&to_typst_range(ss, range), output_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_typst_range_with_none_matches_the_whole_grid() {
        let mut e = TablesEngine::new(3, 3).unwrap();
        e.set_cell_text(0, 0, "a");
        e.set_cell_text(2, 2, "z");
        assert_eq!(to_typst_range(&e, None), to_typst(&e));
    }

    #[test]
    fn to_typst_range_bounds_to_the_print_area() {
        let mut e = TablesEngine::new(4, 4).unwrap();
        e.set_cell_text(0, 0, "in");
        e.set_cell_text(3, 3, "out");
        let bounded = to_typst_range(&e, Some((0, 0, 1, 1)));
        assert!(bounded.contains("in"));
        assert!(!bounded.contains("out"));
        // 2x2 range -> 2 table rows, not the full 4.
        assert_eq!(bounded.matches("],\n").count(), 2);
    }
}
