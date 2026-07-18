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

/// Load a spreadsheet file into the engine. Returns (rows, cols).
pub fn load_file_into_engine(path: &str, engine: &mut TablesEngine) -> Result<(usize, usize), String> {
    let ext = std::path::Path::new(path)
        .extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    match ext.as_str() {
        "xlsx" | "xls" | "xlsm" | "xlsb" => {
            let mut wb: calamine::Xlsx<_> = open_workbook(path)
                .map_err(|e| format!("Cannot open file: {}", e))?;
            let sheet_names = wb.sheet_names().to_vec();
            if sheet_names.is_empty() { return Err("No sheets found".into()); }
            let range = wb.worksheet_range(&sheet_names[0])
                .map_err(|e| format!("Cannot read sheet: {}", e))?;
            Ok(load_range_into_engine(&range, engine))
        }
        "ods" => {
            let mut wb: calamine::Ods<_> = open_workbook(path)
                .map_err(|e| format!("Cannot open file: {}", e))?;
            let sheet_names = wb.sheet_names().to_vec();
            if sheet_names.is_empty() { return Err("No sheets found".into()); }
            let range = wb.worksheet_range(&sheet_names[0])
                .map_err(|e| format!("Cannot read sheet: {}", e))?;
            Ok(load_range_into_engine(&range, engine))
        }
        "csv" | "tsv" => {
            let content = std::fs::read_to_string(path)
                .map_err(|e| format!("Cannot read file: {}", e))?;
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

/// Save sheet data to an XLSX file. Numbers are written as numbers,
/// everything else as strings.
pub fn save_sheets_to_xlsx(path: &str, sheets: &[SheetModel]) -> Result<(), String> {
    use rust_xlsxwriter::Workbook;
    let mut workbook = Workbook::new();
    for sh in sheets {
        let sheet = workbook.add_worksheet();
        sheet.set_name(&sh.name).map_err(|e| format!("Sheet name: {}", e))?;
        for r in 0..sh.rows {
            for c in 0..sh.cols {
                let val = &sh.data[r][c];
                if val.is_empty() { continue; }
                if let Ok(n) = val.parse::<f64>() {
                    sheet.write_number(r as u32, c as u16, n)
                        .map_err(|e| format!("Write error: {}", e))?;
                } else {
                    sheet.write_string(r as u32, c as u16, val)
                        .map_err(|e| format!("Write error: {}", e))?;
                }
            }
        }
    }
    workbook.save(path).map_err(|e| format!("Save error: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> TablesEngine {
        TablesEngine::new(20, 10).expect("engine")
    }

    fn write_temp(dir: &tempfile::TempDir, name: &str, content: &str) -> String {
        let p = dir.path().join(name);
        std::fs::write(&p, content).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn csv_import_basic_grid() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_temp(&dir, "t.csv", "a,b,c\n1,2,3\n");
        let mut e = engine();
        let (rows, cols) = load_file_into_engine(&path, &mut e).unwrap();
        assert_eq!((rows, cols), (2, 3));
        assert_eq!(e.cell(0, 0), "a");
        assert_eq!(e.cell(1, 2), "3");
    }

    #[test]
    fn csv_import_strips_quotes_and_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_temp(&dir, "t.csv", "\"hello\" , world\n");
        let mut e = engine();
        load_file_into_engine(&path, &mut e).unwrap();
        assert_eq!(e.cell(0, 0), "hello");
        assert_eq!(e.cell(0, 1), "world");
    }

    #[test]
    fn tsv_import_uses_tab_delimiter() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_temp(&dir, "t.tsv", "x\ty\n");
        let mut e = engine();
        let (_, cols) = load_file_into_engine(&path, &mut e).unwrap();
        assert_eq!(cols, 2);
        assert_eq!(e.cell(0, 1), "y");
    }

    #[test]
    fn csv_formulas_evaluate_on_import() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_temp(&dir, "t.csv", "1,2,=A1+B1\n");
        let mut e = engine();
        load_file_into_engine(&path, &mut e).unwrap();
        assert_eq!(e.cell(0, 2), "3");
    }

    #[test]
    fn unsupported_extension_is_an_error() {
        let mut e = engine();
        let err = load_file_into_engine("/nonexistent/file.foo", &mut e).unwrap_err();
        assert!(err.contains("Unsupported format"));
    }

    #[test]
    fn missing_xlsx_is_an_error_not_a_panic() {
        let mut e = engine();
        assert!(load_file_into_engine("/nonexistent/file.xlsx", &mut e).is_err());
    }

    #[test]
    fn xlsx_round_trip_preserves_grid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rt.xlsx").to_string_lossy().into_owned();

        let mut sheet = SheetModel::new("Round Trip", 3, 3, 0);
        sheet.data[0][0] = "hello".into();
        sheet.data[0][1] = "42".into();
        sheet.data[2][2] = "3.5".into();
        save_sheets_to_xlsx(&path, &[sheet]).unwrap();

        let mut e = engine();
        let (rows, cols) = load_file_into_engine(&path, &mut e).unwrap();
        assert_eq!(e.cell(0, 0), "hello");
        assert_eq!(e.cell(0, 1), "42");
        assert_eq!(e.cell(2, 2), "3.50");
        assert!(rows >= 3 && cols >= 3);
    }
}
