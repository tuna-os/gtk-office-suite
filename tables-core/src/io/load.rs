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

fn load_range_into_engine(
    range: &calamine::Range<Data>,
    engine: &mut TablesEngine,
) -> (usize, usize) {
    let (rows, cols) = (range.height(), range.width());
    for (r, row) in range.rows().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            engine.set_cell_text(r, c, &data_to_string(cell));
        }
    }
    engine.evaluate();
    (rows.max(1), cols.max(1))
}

fn load_xlsx_ranges_into_engine(
    values: &calamine::Range<Data>,
    formulas: &calamine::Range<String>,
    engine: &mut TablesEngine,
) -> (usize, usize) {
    let rows = values.height().max(formulas.height());
    let cols = values.width().max(formulas.width());
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
        "xlsx" | "xls" | "xlsm" | "xlsb" => {
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
        max_rows = max_rows.max(values.height()).max(formulas.height());
        max_cols = max_cols.max(values.width()).max(formulas.width());
        source.push((values, formulas));
    }

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
        let mut sheet = SheetModel::new(name, rows.max(1), cols.max(1), sheet_id);
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
