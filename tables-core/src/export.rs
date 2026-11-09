// export.rs — Typst export for Tables.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::engine::TablesEngine;
use crate::sheet::SheetModel;

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

/// The cells a sheet prints, `(top, left, bottom, right)` inclusive: its
/// print area, or else its used range, as Calc prints it. `None` for an
/// empty sheet with no print area.
pub fn printed_range(sheet: &SheetModel) -> Option<(usize, usize, usize, usize)> {
    sheet
        .print_area
        .or_else(|| sheet.used_extent().map(|(r, c)| (0, 0, r, c)))
}

/// Pixels on the grid (1/96 in) to millimetres on paper.
const MM_PER_PX: f64 = 25.4 / 96.0;
/// Points (1/72 in) per grid pixel.
const PT_PER_PX: f64 = 72.0 / 96.0;

/// The rows and columns of `range` that print: hidden ones don't.
fn printed_rows_cols(sheet: &SheetModel, range: (usize, usize, usize, usize)) -> (Vec<usize>, Vec<usize>) {
    let (top, left, bottom, right) = range;
    let rows = (top..=bottom.max(top)).filter(|&r| !sheet.is_row_hidden(r)).collect();
    let cols = (left..=right.max(left)).filter(|&c| !sheet.is_col_hidden(c)).collect();
    (rows, cols)
}

/// The size `range` prints at 100%, in millimetres: its visible columns'
/// widths by its visible rows' heights.
pub fn printed_size_mm(sheet: &SheetModel, range: (usize, usize, usize, usize)) -> (f64, f64) {
    let (rows, cols) = printed_rows_cols(sheet, range);
    let w: f64 = cols.iter().map(|&c| sheet.col_width(c)).sum();
    let h: f64 = rows.iter().map(|&r| sheet.row_height(r)).sum();
    (w * MM_PER_PX, h * MM_PER_PX)
}

/// The scale the sheet prints at: its page setup's scale, or the scale
/// that fits its printed range to the pages its fit-to-page asks for.
pub fn print_scale(sheet: &SheetModel) -> f64 {
    let (w, h) = printed_range(sheet).map_or((0.0, 0.0), |range| printed_size_mm(sheet, range));
    sheet.page_setup.print_scale_for(w, h)
}

/// Typst source for printing `sheet`, the engine's active sheet: the
/// printed range with its column widths, text and padding all at the
/// sheet's print scale, under its page setup. Hidden rows and columns are
/// left out, as a print leaves them out.
pub fn to_typst_for_sheet(ss: &TablesEngine, sheet: &SheetModel) -> String {
    let setup = &sheet.page_setup;
    let mut out = typst_page_directive(setup);
    let Some(range) = printed_range(sheet) else {
        return out;
    };
    let scale = print_scale(sheet);
    let (rows, cols) = printed_rows_cols(sheet, range);
    let widths: Vec<String> = cols
        .iter()
        .map(|&c| typst_pt(sheet.col_width(c) * PT_PER_PX * scale))
        .collect();
    out.push_str(&format!(
        "#set text(size: {})\n#set table(inset: {})\n#table(\n  columns: ({},),\n",
        typst_pt(sheet.default_font_size * scale),
        typst_pt(5.0 * scale),
        widths.join(", "),
    ));
    for &r in &rows {
        let cells: Vec<String> = cols
            .iter()
            .map(|&c| escape_typst_cell(&ss.cell(r, c)))
            .collect();
        out.push_str("  [");
        out.push_str(&cells.join("], ["));
        out.push_str("],\n");
    }
    out.push_str(")\n");
    out
}

/// Export `sheet` to PDF at its print scale and page setup — see
/// [`to_typst_for_sheet`].
pub fn to_pdf_for_sheet(ss: &TablesEngine, sheet: &SheetModel, output_path: &str) -> Result<(), String> {
    suite_export::compile_pdf_to_file(&to_typst_for_sheet(ss, sheet), output_path)
}

/// A length in points, as Typst writes one.
fn typst_pt(pt: f64) -> String {
    format!("{}pt", (pt * 100.0).round() / 100.0)
}

/// Escape Typst special characters (`\`, `[`, `]`, `#`) in a table cell
/// so that the cell content is treated as literal text.
pub fn escape_typst_cell(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '[' => out.push_str("\\["),
            ']' => out.push_str("\\]"),
            '#' => out.push_str("\\#"),
            _ => out.push(c),
        }
    }
    out
}

fn typst_table(grid: &[Vec<String>]) -> String {
    let columns = grid.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let mut out = format!("#table(\n  columns: {columns},\n");
    for row in grid {
        out.push_str("  [");
        let cells: Vec<String> = row.iter().map(|s| escape_typst_cell(s)).collect();
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
        e.set_cell_text(0, 0, "a").unwrap();
        e.set_cell_text(2, 2, "z").unwrap();
        assert_eq!(to_typst_range(&e, None), to_typst(&e));
    }

    #[test]
    fn to_typst_range_bounds_to_the_print_area() {
        let mut e = TablesEngine::new(4, 4).unwrap();
        e.set_cell_text(0, 0, "in").unwrap();
        e.set_cell_text(3, 3, "out").unwrap();
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
        e.set_cell_text(0, 0, "hi").unwrap();
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
        e.set_cell_text(0, 0, "r1c1").unwrap();
        e.set_cell_text(0, 1, "r1c2").unwrap();
        e.set_cell_text(1, 0, "r2c1").unwrap();
        e.set_cell_text(1, 1, "r2c2").unwrap();
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
        e.set_cell_text(0, 0, "solo").unwrap();
        let out = to_typst(&e);
        assert!(out.starts_with("#table("), "missing table header: {out}");
        assert!(out.contains("[solo]"), "cell missing: {out}");
        assert!(
            out.trim_end().ends_with(')'),
            "missing closing paren: {out}"
        );
    }

    /// #176: multi-column grids declare the real column count (it used to
    /// be hardcoded to 1, so every sheet exported as a single column).
    #[test]
    fn typst_table_multi_column_grid_declares_real_column_count() {
        let mut e = TablesEngine::new(2, 3).unwrap();
        e.set_cell_text(0, 0, "r1c1").unwrap();
        e.set_cell_text(0, 1, "r1c2").unwrap();
        e.set_cell_text(0, 2, "r1c3").unwrap();
        e.set_cell_text(1, 0, "r2c1").unwrap();
        let out = to_typst(&e);
        assert!(
            out.contains("columns: 3"),
            "typst export bug #176: multi-column grid emitted as single column — got {out:?}"
        );
    }

    #[test]
    fn escape_typst_cell_escapes_special_characters() {
        assert_eq!(escape_typst_cell("hello"), "hello");
        assert_eq!(escape_typst_cell(r"a\b"), r"a\\b");
        assert_eq!(escape_typst_cell("[brackets]"), r"\[brackets\]");
        assert_eq!(
            escape_typst_cell("#nonexistent_function()"),
            r"\#nonexistent_function()"
        );
        assert_eq!(escape_typst_cell(r"\\#test"), r"\\\\\#test");
    }

    #[test]
    fn typst_table_escapes_cell_specials_and_compiles_to_pdf() {
        let mut e = TablesEngine::new(3, 1).unwrap();
        e.set_cell_text(0, 0, "#nonexistent_function()").unwrap();
        e.set_cell_text(1, 0, "[brackets]").unwrap();
        e.set_cell_text(2, 0, r"\\#test").unwrap();

        let out = to_typst(&e);
        assert!(
            out.contains(r"[\#nonexistent_function()]"),
            "function call syntax not escaped: {out}"
        );
        assert!(
            out.contains(r"[\[brackets\]]"),
            "brackets not escaped: {out}"
        );
        assert!(
            out.contains(r"[\\\\\#test]"),
            "backslashes and hash not escaped: {out}"
        );

        let pdf = suite_export::compile_pdf(&out)
            .expect("Typst PDF compilation must succeed with escaped cell content");
        assert!(pdf.starts_with(b"%PDF-"), "output is not a valid PDF");
        assert!(pdf.len() > 500, "suspiciously small PDF");
    }

    use crate::sheet::{COL_WIDTH, ROW_HEIGHT};
    use suite_common_core::print::{Orientation, PageSetup};

    /// A 5 × 4 grid like the render lab's `tables/print-scale` fixture.
    fn values_sheet(setup: PageSetup) -> (TablesEngine, SheetModel) {
        let mut e = TablesEngine::new(5, 4).unwrap();
        for r in 0..5 {
            for c in 0..4 {
                e.set_cell_text(r, c, &format!("{}", (r + 1) * 10 + c + 1)).unwrap();
            }
        }
        let mut sheet = SheetModel::new("S", 30, 10, 0);
        sheet.sync_from_engine(&e);
        sheet.page_setup = setup;
        (e, sheet)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    /// The printed range is the used range unless a print area is set,
    /// as Calc prints: trailing empty rows and columns are not paper.
    #[test]
    fn the_printed_range_is_the_print_area_or_else_the_used_range() {
        let (_, mut sheet) = values_sheet(PageSetup::default());
        assert_eq!(printed_range(&sheet), Some((0, 0, 4, 3)));
        sheet.print_area = Some((1, 1, 2, 2));
        assert_eq!(printed_range(&sheet), Some((1, 1, 2, 2)));
        let empty = SheetModel::new("E", 10, 10, 0);
        assert_eq!(printed_range(&empty), None);
    }

    #[test]
    fn hidden_columns_take_no_width_on_paper() {
        let (_, mut sheet) = values_sheet(PageSetup::default());
        let (w, h) = printed_size_mm(&sheet, (0, 0, 4, 3));
        assert!(close(w, 4.0 * COL_WIDTH * 25.4 / 96.0), "{w}");
        assert!(close(h, 5.0 * ROW_HEIGHT * 25.4 / 96.0), "{h}");
        sheet.hidden_cols.insert(1);
        let (w, _) = printed_size_mm(&sheet, (0, 0, 4, 3));
        assert!(close(w, 3.0 * COL_WIDTH * 25.4 / 96.0), "{w}");
    }

    /// A sheet set to print at 46% (#1336) exports its columns, text and
    /// padding at 46%, not at 100%.
    #[test]
    fn a_sheet_exports_at_its_print_scale() {
        let (e, sheet) = values_sheet(PageSetup { scale: 0.46, ..PageSetup::default() });
        assert!(close(print_scale(&sheet), 0.46));
        let out = to_typst_for_sheet(&e, &sheet);
        let col = typst_pt(COL_WIDTH * 0.75 * 0.46);
        assert!(out.contains(&format!("columns: ({col}, {col}, {col}, {col},)")), "{out}");
        assert!(out.contains(&format!("#set text(size: {})", typst_pt(sheet.default_font_size * 0.46))), "{out}");
        assert!(out.contains(&format!("#set table(inset: {})", typst_pt(5.0 * 0.46))), "{out}");
        assert!(out.contains("[11], [12], [13], [14],"), "{out}");
        assert!(out.contains("[51], [52], [53], [54],"), "{out}");
    }

    /// Fit to one page wide shrinks a sheet wider than the page until its
    /// columns fill the printable width, whatever the stored scale says.
    #[test]
    fn a_sheet_fitted_to_one_page_wide_fills_the_printable_width() {
        let setup = PageSetup {
            orientation: Orientation::Landscape,
            scale: 0.62,
            fit_to_pages: Some((1, 0)),
            ..PageSetup::default()
        };
        let (e, mut sheet) = values_sheet(setup);
        for c in 0..4 {
            sheet.col_widths[c] = 400.0;
        }
        let scale = print_scale(&sheet);
        let (area_w, _) = sheet.page_setup.content_area_mm();
        assert!(close(4.0 * 400.0 * 25.4 / 96.0 * scale, area_w), "scale {scale}");
        assert!(scale < 0.62, "fit, not the stored scale: {scale}");
        let out = to_typst_for_sheet(&e, &sheet);
        assert!(out.contains(&format!("columns: ({},", typst_pt(400.0 * 0.75 * scale))), "{out}");
    }

    #[test]
    fn hidden_rows_and_columns_are_left_out_of_the_export() {
        let (e, mut sheet) = values_sheet(PageSetup::default());
        sheet.hidden_cols.insert(1);
        sheet.hidden_rows_manual.insert(0);
        let out = to_typst_for_sheet(&e, &sheet);
        assert!(!out.contains("[11]"), "hidden row printed: {out}");
        assert!(out.contains("[21], [23], [24],"), "hidden column printed: {out}");
    }

    #[test]
    fn a_scaled_sheet_compiles_to_a_pdf_and_an_empty_one_to_a_blank_page() {
        let (e, sheet) = values_sheet(PageSetup { scale: 0.46, ..PageSetup::default() });
        let pdf = suite_export::compile_pdf(&to_typst_for_sheet(&e, &sheet)).expect("scaled sheet compiles");
        assert!(pdf.starts_with(b"%PDF-"));
        let empty = SheetModel::new("E", 10, 10, 0);
        let pdf = suite_export::compile_pdf(&to_typst_for_sheet(&TablesEngine::new(1, 1).unwrap(), &empty))
            .expect("empty sheet compiles");
        assert!(pdf.starts_with(b"%PDF-"));
    }
}
