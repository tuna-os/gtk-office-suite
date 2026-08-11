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

/// Typst `#set page(...)` directive for a page setup (#113) — width/
/// height already account for orientation via `page_dimensions_mm`.
fn typst_page_directive(setup: &suite_common_core::print::PageSetup) -> String {
    let (w, h) = setup.page_dimensions_mm();
    format!(
        "#set page(width: {w}mm, height: {h}mm, margin: (top: {top}mm, bottom: {bottom}mm, left: {left}mm, right: {right}mm))\n",
        top = setup.margin_top_mm,
        bottom = setup.margin_bottom_mm,
        left = setup.margin_left_mm,
        right = setup.margin_right_mm,
    )
}

/// Like [`to_typst_range`], with an explicit page setup (#113) prepended
/// as a Typst `#set page(...)` directive.
pub fn to_typst_with_setup(
    ss: &TablesEngine,
    range: Option<(usize, usize, usize, usize)>,
    setup: &suite_common_core::print::PageSetup,
) -> String {
    format!("{}{}", typst_page_directive(setup), to_typst_range(ss, range))
}

/// Like [`to_pdf_range`], with an explicit page setup (#113).
pub fn to_pdf_with_setup(
    ss: &TablesEngine,
    range: Option<(usize, usize, usize, usize)>,
    setup: &suite_common_core::print::PageSetup,
    output_path: &str,
) -> Result<(), String> {
    suite_export::compile_pdf_to_file(&to_typst_with_setup(ss, range, setup), output_path)
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

    #[test]
    fn to_typst_with_setup_prepends_a_page_directive() {
        use suite_common_core::print::{Orientation, PageSetup};
        let mut e = TablesEngine::new(2, 2).unwrap();
        e.set_cell_text(0, 0, "hi");
        let landscape = PageSetup { orientation: Orientation::Landscape, ..PageSetup::default() };
        let out = to_typst_with_setup(&e, None, &landscape);
        assert!(out.starts_with("#set page("));
        // Landscape A4: 297mm wide, 210mm tall (swapped from portrait).
        assert!(out.contains("width: 297"));
        assert!(out.contains("height: 210"));
        assert!(out.contains("#table("), "table body still present after the directive");
    }

    #[test]
    fn typst_table_emits_cells_in_row_major_order() {
        let mut e = TablesEngine::new(2, 2).unwrap();
        e.set_cell_text(0, 0, "r1c1");
        e.set_cell_text(0, 1, "r1c2");
        e.set_cell_text(1, 0, "r2c1");
        e.set_cell_text(1, 1, "r2c2");
        let out = to_typst(&e);
        // Cells stay in row-major order even though the column count is
        // currently wrong (see issue #176).
        assert!(
            out.contains("[r1c1], [r1c2],"),
            "row 1 not row-major: {out}"
        );
        assert!(
            out.contains("[r2c1], [r2c2],"),
            "row 2 not row-major: {out}"
        );
    }

    #[test]
    fn typst_table_single_cell_grid_is_a_valid_table() {
        let mut e = TablesEngine::new(1, 1).unwrap();
        e.set_cell_text(0, 0, "solo");
        let out = to_typst(&e);
        assert!(out.starts_with("#table("), "missing table header: {out}");
        assert!(out.contains("[solo]"), "cell missing: {out}");
        assert!(
            out.trim_end().ends_with(')'),
            "missing closing paren: {out}"
        );
    }

    /// Regression pin for #176: multi-column grids must declare the real
    /// column count. Currently \`typst_table\` hardcodes \`columns: 1\`, so
    /// this assertion fails — the \`#[should_panic]\` marker keeps the suite
    /// green and must be removed when the one-line fix lands (same pattern
    /// as the Tavern#73 brew_env regression test).
    #[test]
    #[should_panic(expected = "typst export bug #176: multi-column grid emitted as single column")]
    fn typst_table_multi_column_grid_declares_real_column_count() {
        let mut e = TablesEngine::new(2, 3).unwrap();
        e.set_cell_text(0, 0, "r1c1");
        e.set_cell_text(0, 1, "r1c2");
        e.set_cell_text(0, 2, "r1c3");
        e.set_cell_text(1, 0, "r2c1");
        let out = to_typst(&e);
        assert!(
            out.contains("columns: 3"),
            "typst export bug #176: multi-column grid emitted as single column — got {out:?}"
        );
    }
}
