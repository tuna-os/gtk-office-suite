// load.rs — xlsx/ods/csv/tsv import into the engine.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of io.rs (issue #247).

use super::props::{parse_sheet_qualified_range, read_sheet_props_from_xlsx};

// io.rs — Spreadsheet file I/O: xlsx/ods/csv/tsv import, xlsx export.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Extracted from tables/src/window.rs so it is unit-testable without GTK.

use crate::engine::TablesEngine;
use crate::sheet::SheetModel;
use calamine::{open_workbook, Data, Reader};

fn data_to_string(cell: &Data) -> String {
    match cell {
        Data::String(s) => s.clone(),
        Data::Float(f) => f.to_string(),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(d) => d.to_string(),
        Data::Error(e) => format!("#{}", e),
        _ => String::new(),
    }
}

/// Grid size needed to hold `range` *at its own coordinates*, as (rows, cols).
///
/// `height()`/`width()` describe how big the range is, not where it sits. A
/// sheet whose only content is D4 has height 1 and width 1, but needs a 4x4
/// grid — treating the size as the extent silently drops every cell that is
/// not in the top-left corner (#324, and the "flaky" unicode property test in
/// #377/#371/#358 which only ever failed on grids that left A1 empty).
fn range_extent<T: calamine::CellType>(range: &calamine::Range<T>) -> (usize, usize) {
    match range.end() {
        Some((row, col)) => (row as usize + 1, col as usize + 1),
        None => (0, 0),
    }
}

fn load_range_into_engine(
    range: &calamine::Range<Data>,
    engine: &mut TablesEngine,
) -> (usize, usize) {
    // `rows()` yields the range's own rows, so its indices are relative to
    // `start()`. Writing them straight to the engine would shift a sheet whose
    // content begins at B2 up and left into A1.
    let (row0, col0) = range.start().unwrap_or((0, 0));
    for (r, row) in range.rows().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            engine.set_cell_text(row0 as usize + r, col0 as usize + c, &data_to_string(cell));
        }
    }
    engine.evaluate();
    let (rows, cols) = range_extent(range);
    (rows.max(1), cols.max(1))
}

fn load_xlsx_ranges_into_engine(
    values: &calamine::Range<Data>,
    formulas: &calamine::Range<String>,
    engine: &mut TablesEngine,
) -> (usize, usize) {
    // Absolute extents, not range sizes — `get_value` below is addressed in
    // absolute sheet coordinates, so the bounds must be too.
    let (value_rows, value_cols) = range_extent(values);
    let (formula_rows, formula_cols) = range_extent(formulas);
    let rows = value_rows.max(formula_rows);
    let cols = value_cols.max(formula_cols);
    for row in 0..rows {
        for col in 0..cols {
            let formula = formulas
                .get_value((row as u32, col as u32))
                .map(String::as_str)
                .unwrap_or("");
            if formula.is_empty() {
                let value = values
                    .get_value((row as u32, col as u32))
                    .map(data_to_string)
                    .unwrap_or_default();
                engine.set_cell_text(row, col, &value);
            } else {
                let input = if formula.starts_with('=') {
                    formula.to_string()
                } else {
                    format!("={formula}")
                };
                engine.set_cell_text(row, col, &input);
            }
        }
    }
    engine.evaluate();
    (rows.max(1), cols.max(1))
}

/// Load a spreadsheet file into the engine. Returns (rows, cols).
pub fn load_file_into_engine(
    path: &str,
    engine: &mut TablesEngine,
) -> Result<(usize, usize), String> {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        // Legacy BIFF .xls is not an OOXML package; opening it as Xlsx fails
        // with a confusing "cannot open" rather than reading the file.
        "xls" => {
            let mut wb: calamine::Xls<_> =
                open_workbook(path).map_err(|e| format!("Cannot open file: {}", e))?;
            let sheet_names = wb.sheet_names().to_vec();
            if sheet_names.is_empty() {
                return Err("No sheets found".into());
            }
            let range = wb
                .worksheet_range(&sheet_names[0])
                .map_err(|e| format!("Cannot read sheet: {}", e))?;
            Ok(load_range_into_engine(&range, engine))
        }
        "xlsx" | "xlsm" | "xlsb" => {
            let mut wb: calamine::Xlsx<_> =
                open_workbook(path).map_err(|e| format!("Cannot open file: {}", e))?;
            let sheet_names = wb.sheet_names().to_vec();
            if sheet_names.is_empty() {
                return Err("No sheets found".into());
            }
            let formulas = wb
                .worksheet_formula(&sheet_names[0])
                .map_err(|e| format!("Cannot read formulas: {}", e))?;
            let range = wb
                .worksheet_range(&sheet_names[0])
                .map_err(|e| format!("Cannot read sheet: {}", e))?;
            Ok(load_xlsx_ranges_into_engine(&range, &formulas, engine))
        }
        "ods" => {
            let mut wb: calamine::Ods<_> =
                open_workbook(path).map_err(|e| format!("Cannot open file: {}", e))?;
            let sheet_names = wb.sheet_names().to_vec();
            if sheet_names.is_empty() {
                return Err("No sheets found".into());
            }
            let range = wb
                .worksheet_range(&sheet_names[0])
                .map_err(|e| format!("Cannot read sheet: {}", e))?;
            Ok(load_range_into_engine(&range, engine))
        }
        "csv" | "tsv" => {
            let content =
                std::fs::read_to_string(path).map_err(|e| format!("Cannot read file: {}", e))?;
            let delim = if ext == "tsv" { '\t' } else { ',' };
            let mut max_rows = 0usize;
            let mut max_cols = 0usize;
            for (r, line) in content.lines().enumerate() {
                let cols: Vec<&str> = line.split(delim).collect();
                max_cols = max_cols.max(cols.len());
                for (c, val) in cols.iter().enumerate() {
                    let trimmed = val.trim().trim_matches('"');
                    engine.set_cell_text(r, c, trimmed);
                }
                max_rows = r + 1;
            }
            engine.evaluate();
            Ok((max_rows.max(1), max_cols.max(1)))
        }
        _ => Err(format!("Unsupported format: .{}", ext)),
    }
}

/// Load every XLSX worksheet into one calculation engine, preserving sheet
/// names/order and formula inputs. CSV/ODS remain single-sheet imports.
pub fn load_xlsx_workbook(path: &str) -> Result<(TablesEngine, Vec<SheetModel>), String> {
    let mut book: calamine::Xlsx<_> =
        open_workbook(path).map_err(|e| format!("Cannot open file: {e}"))?;
    let names = book.sheet_names().to_vec();
    if names.is_empty() {
        return Err("No sheets found".into());
    }

    let mut source = Vec::with_capacity(names.len());
    let mut max_rows = 1usize;
    let mut max_cols = 1usize;
    for name in &names {
        let formulas = book
            .worksheet_formula(name)
            .map_err(|e| format!("Cannot read formulas: {e}"))?;
        let values = book
            .worksheet_range(name)
            .map_err(|e| format!("Cannot read sheet: {e}"))?;
        let (value_rows, value_cols) = range_extent(&values);
        let (formula_rows, formula_cols) = range_extent(&formulas);
        max_rows = max_rows.max(value_rows).max(formula_rows);
        max_cols = max_cols.max(value_cols).max(formula_cols);
        source.push((values, formulas));
    }

    // Same floor as every other load path: a small workbook must still open
    // onto a usable grid rather than one sized exactly to its content (#447).
    let max_rows = max_rows.max(crate::sheet::DEFAULT_ROWS);
    let max_cols = max_cols.max(crate::sheet::DEFAULT_COLS);
    let mut engine = TablesEngine::new(max_rows, max_cols)?;
    engine.rename_sheet(0, &names[0])?;
    for name in names.iter().skip(1) {
        engine.add_sheet(name)?;
    }

    let sheet_props = read_sheet_props_from_xlsx(path, &names);

    let mut sheets = Vec::with_capacity(names.len());
    for (index, ((values, formulas), name)) in source.into_iter().zip(names.iter()).enumerate() {
        engine.set_active_sheet(index)?;
        let (rows, cols) = load_xlsx_ranges_into_engine(&values, &formulas, &mut engine);
        let sheet_id = engine.sheet_id_at(index).unwrap_or(index as u32);
        let mut sheet = SheetModel::new(
            name,
            rows.max(crate::sheet::DEFAULT_ROWS),
            cols.max(crate::sheet::DEFAULT_COLS),
            sheet_id,
        );
        sheet.sync_from_engine(&engine);
        if let Some(props) = sheet_props.get(name) {
            // Written back out as manually-hidden rather than reproducing
            // filter/manual as separate concepts — a saved-then-reopened
            // xlsx has no filter state left to restore, only hidden rows,
            // same as real Excel/Sheets after a filter is "applied".
            sheet.hidden_rows_manual = props.hidden_rows.clone();
            sheet.hidden_cols = props.hidden_cols.clone();
            if let Some(setup) = &props.page_setup {
                sheet.page_setup = setup.clone();
            }
        }
        sheets.push(sheet);
    }
    engine.set_active_sheet(0)?;
    // Named ranges (#113): calamine reads each <definedName>'s raw text
    // content, which is already in the '='-free "Sheet1!$A$1:$A$3" form
    // this app's own defined_names formulas use — no reformatting needed.
    // `_xlnm.Print_Area` is a built-in defined name (not a user-facing
    // one) that OOXML uses to store each sheet's print area — routed to
    // `SheetModel::print_area` instead of `new_defined_name`.
    for (name, formula) in book.defined_names() {
        if name == "_xlnm.Print_Area" {
            if let Some((sheet_name, r0, c0, r1, c1)) = parse_sheet_qualified_range(formula) {
                if let Some(sheet) = sheets.iter_mut().find(|s| s.name == sheet_name) {
                    sheet.print_area = Some((r0, c0, r1, c1));
                }
            }
            continue;
        }
        let _ = engine.model.new_defined_name(name, None, formula);
    }
    Ok((engine, sheets))
}

/// Load any supported spreadsheet file into a fresh engine plus one
/// [`SheetModel`] per sheet.
///
/// This is the entry point widget code should use. CLI open, the open dialog,
/// and drag-and-drop all need identical format coverage, and duplicating the
/// extension dispatch at each call site is exactly how `.ods` and `.xls` ended
/// up openable from the command line but not from the file dialog.
///
/// Multi-sheet formats keep every sheet and its producer-supplied name;
/// single-table formats (csv/tsv) yield one sheet named `Sheet1`.
pub fn load_workbook(path: &str) -> Result<(TablesEngine, Vec<SheetModel>), String> {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "xlsx" | "xlsm" | "xlsb" => load_xlsx_workbook(path),
        "xls" => load_xls_workbook(path),
        "ods" => load_ods_workbook(path),
        _ => load_single_sheet(path),
    }
}

/// Fallback for formats that carry a single unnamed table (csv/tsv).
fn load_single_sheet(path: &str) -> Result<(TablesEngine, Vec<SheetModel>), String> {
    use crate::sheet::{DEFAULT_COLS, DEFAULT_ROWS};
    let mut engine = TablesEngine::new(DEFAULT_ROWS, DEFAULT_COLS)?;
    let (rows, cols) = load_file_into_engine(path, &mut engine)?;
    // `to_grid` only walks `engine.rows`/`engine.cols`, so a file taller or
    // wider than the default grid would otherwise be silently truncated.
    engine.rows = engine.rows.max(rows);
    engine.cols = engine.cols.max(cols);
    let sheet_id = engine.sheet_id_at(0).unwrap_or(0);
    let mut sheet = SheetModel::new("Sheet1", engine.rows, engine.cols, sheet_id);
    sheet.sync_from_engine(&engine);
    Ok((engine, vec![sheet]))
}

fn build_named_workbook(source: Vec<(String, calamine::Range<Data>)>)
    -> Result<(TablesEngine, Vec<SheetModel>), String>
{
    if source.is_empty() {
        return Err("No sheets found".into());
    }
    // Floor the grid at the default editing size. Sizing purely to content
    // gives a three-row sheet for a three-row file, with nowhere to type: the
    // user cannot click, navigate to, or scroll to a cell past the last one
    // that already holds data (#447). Every other load path has always applied
    // this floor; the workbook loaders did not.
    use crate::sheet::{DEFAULT_COLS, DEFAULT_ROWS};
    let max_rows = source.iter().map(|(_, range)| range_extent(range).0).max()
        .unwrap_or(0).max(DEFAULT_ROWS);
    let max_cols = source.iter().map(|(_, range)| range_extent(range).1).max()
        .unwrap_or(0).max(DEFAULT_COLS);
    let names: Vec<String> = source.iter().map(|(name, _)| name.clone()).collect();
    let mut engine = TablesEngine::new(max_rows.max(1), max_cols.max(1))?;
    engine.rename_sheet(0, &names[0])?;
    for name in names.iter().skip(1) {
        engine.add_sheet(name)?;
    }
    let mut sheets = Vec::with_capacity(source.len());
    for (index, (name, range)) in source.into_iter().enumerate() {
        engine.set_active_sheet(index)?;
        let (rows, cols) = load_range_into_engine(&range, &mut engine);
        let sheet_id = engine.sheet_id_at(index).unwrap_or(index as u32);
        let mut sheet = SheetModel::new(&name, rows.max(DEFAULT_ROWS), cols.max(DEFAULT_COLS), sheet_id);
        sheet.sync_from_engine(&engine);
        sheets.push(sheet);
    }
    engine.set_active_sheet(0)?;
    Ok((engine, sheets))
}

/// Load every ODS sheet while retaining the names supplied by the producer.
pub fn load_ods_workbook(path: &str) -> Result<(TablesEngine, Vec<SheetModel>), String> {
    let mut book: calamine::Ods<_> = open_workbook(path)
        .map_err(|e| format!("Cannot open file: {e}"))?;
    let names = book.sheet_names().to_vec();
    let mut source = Vec::with_capacity(names.len());
    for name in names {
        let range = book.worksheet_range(&name)
            .map_err(|e| format!("Cannot read sheet: {e}"))?;
        source.push((name, range));
    }
    build_named_workbook(source)
}

/// Load every legacy XLS sheet while retaining names and cell values.
pub fn load_xls_workbook(path: &str) -> Result<(TablesEngine, Vec<SheetModel>), String> {
    let mut book: calamine::Xls<_> = open_workbook(path)
        .map_err(|e| format!("Cannot open file: {e}"))?;
    let names = book.sheet_names().to_vec();
    let mut source = Vec::with_capacity(names.len());
    for name in names {
        let range = book.worksheet_range(&name)
            .map_err(|e| format!("Cannot read sheet: {e}"))?;
        source.push((name, range));
    }
    build_named_workbook(source)
}

/// Spreadsheet import with a structured compatibility report and opaque
/// package members retained for a subsequent safe save.
pub fn load_xlsx_workbook_with_report(path: &str) -> Result<(TablesEngine, Vec<SheetModel>, suite_common_core::interop::CompatibilityReport, suite_common_core::interop::OpaquePackage), String> {
    let (engine, sheets) = load_xlsx_workbook(path)?;
    let opaque = suite_common_core::interop::OpaquePackage::capture(path, &["[Content_Types].xml", "_rels/.rels", "xl/workbook.xml", "xl/_rels/workbook.xml.rels"])?;
    let mut report = suite_common_core::interop::CompatibilityReport::new("xlsx");
    for name in opaque.part_names() {
        report.record(suite_common_core::interop::UnsupportedFeature::new("uninterpreted-package-part", "Uninterpreted package part", name, suite_common_core::interop::FeatureDisposition::OpaquePassThrough, "will be copied through on an opaque save"));
    }
    Ok((engine, sheets, report, opaque))
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::Cell;

    // ── build_named_workbook unit tests ───────────────────────────────────

    /// Empty source must produce a clear error, not a panic.
    #[test]
    fn build_named_workbook_empty_returns_error() {
        match build_named_workbook(vec![]) {
            Err(e) => assert!(
                e.contains("No sheets"),
                "error message should mention 'No sheets', got: {e:?}"
            ),
            Ok(_) => panic!("expected Err for empty source, got Ok"),
        }
    }

    /// A single-sheet workbook retains the sheet name and all cell values.
    #[test]
    fn build_named_workbook_single_sheet_round_trips() {
        use calamine::Range;
        // Cell::new takes (u32 row, u32 col) position.
        let cells = vec![
            Cell::new((0, 0), Data::String("hello".into())),
            Cell::new((0, 1), Data::Float(42.0)),
            Cell::new((1, 0), Data::Bool(true)),
        ];
        let range = Range::from_sparse(cells);

        let result = build_named_workbook(vec![("Alpha".into(), range)]);
        assert!(result.is_ok(), "single-sheet workbook should succeed");
        let (mut engine, sheets) = result.unwrap();

        assert_eq!(sheets.len(), 1, "expected one sheet");
        assert_eq!(sheets[0].name, "Alpha");

        engine.set_active_sheet(0).unwrap();
        assert_eq!(engine.cell(0, 0), "hello");
        assert_eq!(engine.cell(0, 1), "42"); // Float(42.0) → "42"
        assert_eq!(engine.cell(1, 0), "true");
    }

    /// Two sheets must produce two independently accessible SheetModels with
    /// correct names and cell data — none of sheet-1's cells bleed into sheet-2.
    #[test]
    fn build_named_workbook_two_sheets_isolated() {
        use calamine::Range;

        let range1 = Range::from_sparse(vec![
            Cell::new((0, 0), Data::String("sheet1-cell".into())),
        ]);
        let range2 = Range::from_sparse(vec![
            Cell::new((0, 0), Data::String("sheet2-cell".into())),
        ]);

        let result = build_named_workbook(vec![
            ("First".into(), range1),
            ("Second".into(), range2),
        ]);
        assert!(result.is_ok(), "two-sheet build should succeed");
        let (mut engine, sheets) = result.unwrap();

        assert_eq!(sheets.len(), 2);
        assert_eq!(sheets[0].name, "First");
        assert_eq!(sheets[1].name, "Second");

        engine.set_active_sheet(0).unwrap();
        assert_eq!(engine.cell(0, 0), "sheet1-cell", "sheet-1 cell must be 'sheet1-cell'");

        engine.set_active_sheet(1).unwrap();
        assert_eq!(engine.cell(0, 0), "sheet2-cell", "sheet-2 cell must be 'sheet2-cell'");
        // No bleed-over from sheet 1's content.
        assert_eq!(engine.cell(0, 1), "", "sheet-2 col-1 must be empty");
    }

    /// Engine is positioned on sheet 0 after build — not an arbitrary sheet.
    /// This is load-bearing: window.rs's open path calls engine.cell(0,0)
    /// immediately after loading without first calling set_active_sheet.
    #[test]
    fn build_named_workbook_active_sheet_is_zero_after_build() {
        use calamine::Range;
        let r1 = Range::from_sparse(vec![Cell::new((0, 0), Data::Int(1))]);
        let r2 = Range::from_sparse(vec![Cell::new((0, 0), Data::Int(2))]);
        let (engine, _sheets) =
            build_named_workbook(vec![("S1".into(), r1), ("S2".into(), r2)]).unwrap();
        // Must read sheet 0's value without an explicit set_active_sheet call.
        assert_eq!(engine.cell(0, 0), "1", "default active sheet must be 0 (S1)");
    }

    // ── load_ods_workbook integration tests ───────────────────────────────
    // Uses tables-core/tests/fixtures/two_sheets.ods, a real ODS file
    // generated by odfpy with two named sheets: "Sales" and "Config".
    // Regenerate with `python3 scripts/gen_test_fixtures.py`.

    /// The fixture is committed, so a missing one means a broken checkout.
    /// Panicking beats skipping: a test that silently passes with zero
    /// assertions is the `|| true` failure mode AGENTS.md warns about.
    fn ods_fixture() -> std::path::PathBuf {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/two_sheets.ods");
        assert!(
            path.exists(),
            "missing committed fixture {}; regenerate with \
             `python3 scripts/gen_test_fixtures.py`",
            path.display()
        );
        path
    }

    #[test]
    fn load_ods_workbook_sheet_names_preserved() {
        let path = ods_fixture();
        let (_engine, sheets) =
            load_ods_workbook(path.to_str().unwrap()).expect("load_ods_workbook failed");
        let names: Vec<&str> = sheets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Sales", "Config"], "sheet names: {names:?}");
    }

    #[test]
    fn load_ods_workbook_first_sheet_cell_values() {
        let path = ods_fixture();
        let (mut engine, sheets) =
            load_ods_workbook(path.to_str().unwrap()).expect("load_ods_workbook failed");
        assert!(!sheets.is_empty());
        engine.set_active_sheet(0).unwrap();
        // Row 0: "product", "qty", "price"
        assert_eq!(engine.cell(0, 0), "product");
        assert_eq!(engine.cell(0, 1), "qty");
        assert_eq!(engine.cell(0, 2), "price");
        // Row 1: "widget", "10", "2.50"
        assert_eq!(engine.cell(1, 0), "widget");
    }

    #[test]
    fn load_ods_workbook_second_sheet_accessible() {
        let path = ods_fixture();
        let (mut engine, sheets) =
            load_ods_workbook(path.to_str().unwrap()).expect("load_ods_workbook failed");
        assert!(sheets.len() >= 2, "expected at least 2 sheets, got {}", sheets.len());
        engine.set_active_sheet(1).unwrap();
        // Row 0: "key", "value"
        assert_eq!(engine.cell(0, 0), "key");
        assert_eq!(engine.cell(0, 1), "value");
        // Row 1: "tax_rate", "0.08"
        assert_eq!(engine.cell(1, 0), "tax_rate");
    }

    // ── load_xlsx_workbook_with_report integration tests ──────────────────
    // Uses tempfile + save_sheets_to_xlsx to create a clean xlsx in-process,
    // then exercises load_xlsx_workbook_with_report on it.

    /// Helper: build a minimal two-row xlsx in a temp dir and return its path
    /// as an owned String so it outlives the caller's borrow of `dir`.
    fn make_test_xlsx(dir: &tempfile::TempDir, sheet_name: &str) -> String {
        use crate::io::save_sheets_to_xlsx;
        let path = dir.path().join("test.xlsx");
        let mut sheet = SheetModel::new(sheet_name, 2, 2, 0);
        sheet.data[0][0] = "hello".into();
        sheet.data[0][1] = "world".into();
        sheet.data[1][0] = "42".into();
        sheet.data[1][1] = "99".into();
        save_sheets_to_xlsx(path.to_str().unwrap(), &[sheet])
            .expect("save_sheets_to_xlsx failed in test fixture");
        path.to_str().unwrap().to_owned()
    }

    /// Test 1: engine cells and sheet name survive a write-then-load round trip.
    #[test]
    fn load_xlsx_workbook_with_report_returns_engine_and_sheets() {
        let dir = tempfile::tempdir().unwrap();
        let path = make_test_xlsx(&dir, "MySheet");

        let (mut engine, sheets, _report, _opaque) =
            load_xlsx_workbook_with_report(&path).expect("load_xlsx_workbook_with_report failed");

        assert_eq!(sheets.len(), 1, "expected exactly one sheet");
        assert_eq!(sheets[0].name, "MySheet", "sheet name must survive round-trip");

        engine.set_active_sheet(0).unwrap();
        assert_eq!(engine.cell(0, 0), "hello");
        assert_eq!(engine.cell(0, 1), "world");
        assert_eq!(engine.cell(1, 0), "42");
        assert_eq!(engine.cell(1, 1), "99");
    }

    /// Test 2: a file we write ourselves must have no HardError features —
    /// opaque parts recorded are OpaquePassThrough only.
    #[test]
    fn load_xlsx_workbook_with_report_clean_file_has_no_warn_features() {
        let dir = tempfile::tempdir().unwrap();
        let path = make_test_xlsx(&dir, "Sheet1");

        let (_engine, _sheets, report, _opaque) =
            load_xlsx_workbook_with_report(&path).expect("load_xlsx_workbook_with_report failed");

        // A file we generated ourselves must never produce a HardError.
        assert!(
            !report.has_hard_errors(),
            "clean xlsx must have no hard errors; report: {:?}",
            report
        );

        // All recorded features (if any) must be OpaquePassThrough — no WarnOnLoss.
        use suite_common_core::interop::FeatureDisposition;
        for feature in &report.features {
            assert_eq!(
                feature.disposition,
                FeatureDisposition::OpaquePassThrough,
                "unexpected disposition {:?} for feature '{}' in clean file",
                feature.disposition,
                feature.id
            );
        }
    }

    /// Test 3: the returned OpaquePackage is valid and its length is accessible
    /// without panicking (defensive smoke test).
    #[test]
    fn load_xlsx_workbook_with_report_opaque_package_is_capturable() {
        let dir = tempfile::tempdir().unwrap();
        let path = make_test_xlsx(&dir, "Sheet1");

        let (_engine, _sheets, _report, opaque) =
            load_xlsx_workbook_with_report(&path).expect("load_xlsx_workbook_with_report failed");

        // len() must be callable and return a sensible value (>= 0 is trivially
        // true for usize, but we want to confirm no panic occurs).
        let _ = opaque.len();
        // A file generated by save_sheets_to_xlsx has no unexpected extra parts
        // beyond what is recognized, so the opaque package should be empty or
        // very small. Either way it must not fail or panic.
        assert!(opaque.len() < 1000, "suspiciously large opaque package: {}", opaque.len());
    }

    // ── load_workbook dispatch ────────────────────────────────────────────
    // One entry point for every open route. These tests pin the routing, so a
    // format that works from the CLI cannot quietly stop working in the file
    // dialog.

    #[test]
    fn load_workbook_routes_ods_to_the_multi_sheet_loader() {
        let path = ods_fixture();
        let (mut engine, sheets) =
            load_workbook(path.to_str().unwrap()).expect("load_workbook failed on .ods");
        let names: Vec<&str> = sheets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Sales", "Config"], "ods must keep both sheet names");
        engine.set_active_sheet(1).unwrap();
        assert_eq!(engine.cell(0, 0), "key");
    }

    #[test]
    fn load_workbook_routes_xlsx_to_the_multi_sheet_loader() {
        let dir = tempfile::tempdir().unwrap();
        let path = make_test_xlsx(&dir, "MySheet");
        let (mut engine, sheets) =
            load_workbook(&path).expect("load_workbook failed on .xlsx");
        assert_eq!(sheets.len(), 1);
        assert_eq!(sheets[0].name, "MySheet", "xlsx must keep its sheet name");
        engine.set_active_sheet(0).unwrap();
        assert_eq!(engine.cell(0, 0), "hello");
    }

    #[test]
    fn load_workbook_routes_csv_to_a_single_named_sheet() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.csv");
        std::fs::write(&path, "a,b\n1,2\n").unwrap();
        let (engine, sheets) =
            load_workbook(path.to_str().unwrap()).expect("load_workbook failed on .csv");
        assert_eq!(sheets.len(), 1);
        assert_eq!(sheets[0].name, "Sheet1");
        assert_eq!(engine.cell(0, 0), "a");
        assert_eq!(engine.cell(1, 1), "2");
    }

    /// A csv taller than the default grid must not be truncated: `to_grid`
    /// only walks `engine.rows`/`engine.cols`, so the loader has to grow them.
    #[test]
    fn load_workbook_csv_taller_than_default_grid_is_not_truncated() {
        use crate::sheet::DEFAULT_ROWS;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tall.csv");
        let rows = DEFAULT_ROWS + 25;
        let body: String = (0..rows).map(|r| format!("row{r}\n")).collect();
        std::fs::write(&path, body).unwrap();

        let (engine, sheets) =
            load_workbook(path.to_str().unwrap()).expect("load_workbook failed on tall .csv");
        assert!(engine.rows >= rows, "engine grid must cover {rows} rows, got {}", engine.rows);
        assert_eq!(sheets[0].data[rows - 1][0], format!("row{}", rows - 1));
    }

    /// An unknown extension is an error the UI can show, never a panic.
    #[test]
    fn load_workbook_unknown_extension_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.bin");
        std::fs::write(&path, [0u8, 1, 2, 3]).unwrap();
        assert!(load_workbook(path.to_str().unwrap()).is_err());
    }

    /// Legacy .xls is BIFF, not an OOXML package. Handing an .ods to the xls
    /// loader must fail cleanly rather than panic — same as a corrupt file.
    #[test]
    fn load_xls_workbook_rejects_non_biff_input() {
        let path = ods_fixture();
        assert!(
            load_xls_workbook(path.to_str().unwrap()).is_err(),
            "an ODS file is not BIFF and must be rejected"
        );
    }

    // ── absolute-coordinate regressions (#324) ────────────────────────────
    // A range's height/width say how big it is, not where it sits. Both
    // loaders used to conflate the two: the xlsx path read from the wrong
    // offset and lost the data, the ods/xls path wrote range-relative indices
    // and shifted the sheet up and left. The "flaky unicode" property failure
    // in #377/#371/#358/#324 was this bug — it only ever tripped on a
    // generated grid that happened to leave A1 empty.

    /// The exact counterexample proptest minimised to in CI run 33713585584:
    /// a 4x4 grid whose only content is the bottom-right cell.
    #[test]
    fn xlsx_round_trip_keeps_a_lone_bottom_right_cell() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sparse.xlsx");
        let mut engine = TablesEngine::new(4, 4).unwrap();
        engine.set_cell_text(3, 3, "e");
        engine.evaluate();
        let mut sheet = SheetModel::new("Sheet1", 4, 4, 0);
        sheet.sync_from_engine(&engine);
        crate::io::save_sheets_to_xlsx_with_engine(
            path.to_str().unwrap(), &[sheet], Some(&engine)
        ).unwrap();

        let (mut loaded, sheets) = load_xlsx_workbook(path.to_str().unwrap()).unwrap();
        loaded.set_active_sheet(0).unwrap();
        assert_eq!(loaded.cell(3, 3), "e", "the only populated cell must survive");
        assert!(loaded.rows >= 4 && loaded.cols >= 4, "grid must reach the populated cell");
        assert_eq!(sheets[0].data[3][3], "e", "and reach the sheet model too");
    }

    /// Content that starts away from A1 keeps its position: it is not shifted
    /// into the corner, and the empty leading cells stay empty.
    #[test]
    fn xlsx_round_trip_keeps_content_at_its_own_coordinates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("offset.xlsx");
        let mut engine = TablesEngine::new(4, 4).unwrap();
        engine.set_cell_text(1, 1, "corner");
        engine.set_cell_text(1, 2, "right");
        engine.set_cell_text(2, 1, "below");
        engine.evaluate();
        let mut sheet = SheetModel::new("Sheet1", 4, 4, 0);
        sheet.sync_from_engine(&engine);
        crate::io::save_sheets_to_xlsx_with_engine(
            path.to_str().unwrap(), &[sheet], Some(&engine)
        ).unwrap();

        let (mut loaded, _sheets) = load_xlsx_workbook(path.to_str().unwrap()).unwrap();
        loaded.set_active_sheet(0).unwrap();
        assert_eq!(loaded.cell(1, 1), "corner");
        assert_eq!(loaded.cell(1, 2), "right");
        assert_eq!(loaded.cell(2, 1), "below");
        assert_eq!(loaded.cell(0, 0), "", "A1 was empty and must stay empty");
        assert_eq!(loaded.cell(1, 0), "", "column A was empty and must stay empty");
    }

    /// Same property for the ODS reader, which takes the other loader path.
    /// `offset_start.ods` has an empty first row and an empty column A.
    #[test]
    fn ods_content_is_not_shifted_into_the_corner() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/offset_start.ods");
        assert!(
            path.exists(),
            "missing committed fixture {}; regenerate with \
             `python3 scripts/gen_test_fixtures.py`",
            path.display()
        );
        let (mut engine, sheets) =
            load_ods_workbook(path.to_str().unwrap()).expect("offset_start.ods must open");
        assert_eq!(sheets[0].name, "Offset");
        engine.set_active_sheet(0).unwrap();
        assert_eq!(engine.cell(1, 1), "corner", "B2 must stay at B2");
        assert_eq!(engine.cell(1, 2), "right");
        assert_eq!(engine.cell(2, 1), "below");
        assert_eq!(engine.cell(0, 0), "", "A1 was empty and must stay empty");
    }

    /// `range_extent` reports where a range ends, not how big it is — the
    /// distinction the two loaders depend on.
    #[test]
    fn range_extent_measures_position_not_size() {
        use calamine::{Cell, Range};
        let lone_corner = Range::from_sparse(vec![Cell::new((3, 3), Data::Int(1))]);
        assert_eq!(lone_corner.height(), 1, "the range itself is 1x1 ...");
        assert_eq!(range_extent(&lone_corner), (4, 4), "... but needs a 4x4 grid");

        let from_origin = Range::from_sparse(vec![Cell::new((0, 0), Data::Int(1))]);
        assert_eq!(range_extent(&from_origin), (1, 1));

        let empty: Range<Data> = Range::from_sparse(vec![]);
        assert_eq!(range_extent(&empty), (0, 0), "an empty range needs no grid");
    }

    // ── a loaded workbook opens onto a usable grid (#447) ─────────────────
    // Sizing the sheet to its content exactly leaves nowhere to type: a
    // three-row file gave a three-row grid, so the user could not click,
    // navigate to, or scroll to any cell past the last one holding data. The
    // corpus journeys caught it as "timed out waiting for A10 selected for
    // edit" — A10 did not exist.

    #[test]
    fn a_small_xlsx_opens_onto_a_full_size_grid() {
        use crate::sheet::{DEFAULT_COLS, DEFAULT_ROWS};
        let dir = tempfile::tempdir().unwrap();
        let path = make_test_xlsx(&dir, "Small");

        let (engine, sheets) = load_xlsx_workbook(&path).expect("load");
        assert!(
            sheets[0].rows >= DEFAULT_ROWS && sheets[0].cols >= DEFAULT_COLS,
            "sheet is {}x{}, smaller than the {DEFAULT_ROWS}x{DEFAULT_COLS} editing grid",
            sheets[0].rows, sheets[0].cols
        );
        assert!(engine.rows >= DEFAULT_ROWS, "engine grid too small: {}", engine.rows);
        // The content is still where it was.
        assert_eq!(engine.cell(0, 0), "hello");
    }

    #[test]
    fn a_small_ods_opens_onto_a_full_size_grid() {
        use crate::sheet::{DEFAULT_COLS, DEFAULT_ROWS};
        let path = ods_fixture();
        let (engine, sheets) = load_ods_workbook(path.to_str().unwrap()).expect("load");
        assert!(
            sheets[0].rows >= DEFAULT_ROWS && sheets[0].cols >= DEFAULT_COLS,
            "sheet is {}x{}, smaller than the editing grid",
            sheets[0].rows, sheets[0].cols
        );
        assert!(engine.rows >= DEFAULT_ROWS);
    }

    /// Row 9 (A10) must be addressable after opening a small file — the exact
    /// cell the corpus journey navigates to.
    #[test]
    fn cell_a10_is_addressable_after_opening_a_small_workbook() {
        let dir = tempfile::tempdir().unwrap();
        let path = make_test_xlsx(&dir, "Small");
        let (mut engine, sheets) = load_xlsx_workbook(&path).expect("load");
        assert!(sheets[0].rows > 9, "row 10 does not exist: {} rows", sheets[0].rows);
        engine.set_active_sheet(0).unwrap();
        engine.set_cell_text(9, 0, "typed into A10");
        assert_eq!(engine.cell(9, 0), "typed into A10");
    }

    /// A workbook larger than the default grid keeps its own size rather than
    /// being clamped down to it.
    #[test]
    fn a_workbook_larger_than_the_default_grid_keeps_its_size() {
        use crate::sheet::DEFAULT_ROWS;
        use calamine::{Cell, Range};
        let tall = DEFAULT_ROWS + 40;
        let range = Range::from_sparse(vec![
            Cell::new((0, 0), Data::String("top".into())),
            Cell::new((tall as u32 - 1, 0), Data::String("bottom".into())),
        ]);
        let (engine, sheets) = build_named_workbook(vec![("Tall".into(), range)]).unwrap();
        assert!(sheets[0].rows >= tall, "clamped to {} rows, need {tall}", sheets[0].rows);
        assert_eq!(engine.cell(tall - 1, 0), "bottom");
    }
}
