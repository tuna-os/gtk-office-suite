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
/// everything else as strings; formulas (from `engine`, first sheet)
/// are written as real formulas so they survive into other suites.
pub fn save_sheets_to_xlsx(path: &str, sheets: &[SheetModel]) -> Result<(), String> {
    save_sheets_to_xlsx_with_engine(path, sheets, None)
}

pub fn save_sheets_to_xlsx_with_engine(
    path: &str,
    sheets: &[SheetModel],
    engine: Option<&TablesEngine>,
) -> Result<(), String> {
    use rust_xlsxwriter::{Formula, Workbook};
    let mut workbook = Workbook::new();
    for (si, sh) in sheets.iter().enumerate() {
        let sheet = workbook.add_worksheet();
        sheet.set_name(&sh.name).map_err(|e| format!("Sheet name: {}", e))?;

        // Cells covered by a merge are written by merge_range below.
        let mut merged: std::collections::HashSet<(usize, usize)> =
            std::collections::HashSet::new();
        for (mr, mc, rows, cols) in &sh.merges {
            for r in *mr..mr + *rows {
                for c in *mc..mc + *cols {
                    merged.insert((r, c));
                }
            }
        }

        for r in 0..sh.rows {
            for c in 0..sh.cols {
                if merged.contains(&(r, c)) { continue; }
                // The engine backs the first sheet; formulas persist as
                // formulas there (recalculable in Calc/Excel), values
                // elsewhere.
                if si == 0 {
                    if let Some(eng) = engine {
                        if let Some(f) = eng.formula(r, c) {
                            // Cache the computed value alongside the formula:
                            // consumers that skip recalc-on-load (LibreOffice
                            // included) show the right result immediately.
                            let cached = eng.cell(r, c);
                            sheet.write_formula(r as u32, c as u16,
                                    Formula::new(&f).set_result(&cached))
                                .map_err(|e| format!("Write error: {}", e))?;
                            continue;
                        }
                    }
                }
                let val = &sh.data[r][c];
                if val.is_empty() { continue; }
                if let Ok(n) = val.parse::<f64>() {
                    match xlsx_num_format(&sh.formats[r][c]) {
                        Some(fmt) => {
                            let f = rust_xlsxwriter::Format::new().set_num_format(&fmt);
                            sheet.write_number_with_format(r as u32, c as u16, n, &f)
                                .map_err(|e| format!("Write error: {}", e))?;
                        }
                        None => {
                            sheet.write_number(r as u32, c as u16, n)
                                .map_err(|e| format!("Write error: {}", e))?;
                        }
                    }
                } else {
                    sheet.write_string(r as u32, c as u16, val)
                        .map_err(|e| format!("Write error: {}", e))?;
                }
            }
        }

        for (mr, mc, rows, cols) in &sh.merges {
            let (lr, lc) = (mr + (*rows).max(1) - 1, mc + (*cols).max(1) - 1);
            let val = sh.data[*mr][*mc].clone();
            sheet
                .merge_range(
                    *mr as u32, *mc as u16, lr as u32, lc as u16,
                    &val, &rust_xlsxwriter::Format::default(),
                )
                .map_err(|e| format!("Merge error: {}", e))?;
        }
        if sh.frozen_rows > 0 || sh.frozen_cols > 0 {
            sheet
                .set_freeze_panes(sh.frozen_rows as u32, sh.frozen_cols as u16)
                .map_err(|e| format!("Freeze error: {}", e))?;
        }
        for c in 0..sh.cols {
            let w = sh.col_width(c);
            if (w - tables_core_default_col_width()).abs() > 0.5 {
                // Pixels → Excel character width (~7px per character unit).
                sheet
                    .set_column_width(c as u16, w / 7.0)
                    .map_err(|e| format!("Width error: {}", e))?;
            }
        }

        for ch in &sh.charts {
            use crate::sheet::ChartKind;
            use rust_xlsxwriter::{Chart, ChartType as XType};
            let mut chart = Chart::new(match ch.kind {
                ChartKind::Bar => XType::Column,
                ChartKind::Line => XType::Line,
                ChartKind::Pie => XType::Pie,
            });
            chart
                .add_series()
                .set_categories((
                    sh.name.as_str(),
                    ch.cat.0 as u32, ch.cat.1 as u16,
                    ch.cat.2 as u32, ch.cat.1 as u16,
                ))
                .set_values((
                    sh.name.as_str(),
                    ch.val.0 as u32, ch.val.1 as u16,
                    ch.val.2 as u32, ch.val.1 as u16,
                ));
            if !ch.title.is_empty() {
                chart.title().set_name(&ch.title);
            }
            sheet
                .insert_chart(ch.anchor.0 as u32, ch.anchor.1 as u16, &chart)
                .map_err(|e| format!("Chart error: {}", e))?;
        }
    }
    workbook.save(path).map_err(|e| format!("Save error: {}", e))?;
    Ok(())
}


/// Map our NumberFormat onto an xlsx number-format code, if non-default.
fn xlsx_num_format(nf: &suite_common_core::format::NumberFormat) -> Option<String> {
    use suite_common_core::format::NumberFormatKind::*;
    match &nf.kind {
        General => None,
        Number(d) => Some(if *d == 0 {
            "#,##0".to_string()
        } else {
            format!("#,##0.{}", "0".repeat(*d as usize))
        }),
        Currency(sym, d) => Some(if *d == 0 {
            format!("\"{}\"#,##0", sym)
        } else {
            format!("\"{}\"#,##0.{}", sym, "0".repeat(*d as usize))
        }),
        Percent(d) => Some(if *d == 0 {
            "0%".to_string()
        } else {
            format!("0.{}%", "0".repeat(*d as usize))
        }),
        Date(_) => Some("yyyy-mm-dd".to_string()),
        _ => None,
    }
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

    #[test]
    fn formulas_survive_as_formulas() {
        use calamine::{open_workbook, Reader};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.xlsx");

        let mut e = engine();
        e.set_cell_text(0, 0, "2");
        e.set_cell_text(0, 1, "3");
        e.set_cell_text(1, 0, "=A1+B1");
        e.evaluate();
        let mut sheet = SheetModel::new("S", 3, 3, 0);
        sheet.data[0][0] = "2".into();
        sheet.data[0][1] = "3".into();
        sheet.data[1][0] = "5".into();
        save_sheets_to_xlsx_with_engine(path.to_str().unwrap(), &[sheet], Some(&e)).unwrap();

        let mut wb: calamine::Xlsx<_> = open_workbook(&path).unwrap();
        let names = wb.sheet_names().to_vec();
        let formulas = wb.worksheet_formula(&names[0]).unwrap();
        let has = formulas.rows().flatten().any(|f| f.contains("A1+B1"));
        assert!(has, "formula not written as formula");
    }


    #[test]
    fn number_formats_written_to_xlsx() {
        use suite_common_core::format::{NumberFormat, NumberFormatKind};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fmt.xlsx");
        let mut sheet = SheetModel::new("Fmt", 2, 2, 0);
        sheet.data[0][0] = "0.5".into();
        sheet.formats[0][0] = NumberFormat { kind: NumberFormatKind::Percent(1) };
        save_sheets_to_xlsx(path.to_str().unwrap(), &[sheet]).unwrap();
        // the format lives in styles.xml; presence check via zip
        let bytes = std::fs::read(&path).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut styles = String::new();
        use std::io::Read as _;
        zip.by_name("xl/styles.xml").unwrap().read_to_string(&mut styles).unwrap();
        assert!(styles.contains("0.0%"), "percent format missing from styles: {styles}");
    }

}

/// The SheetModel default column width in px (COL_WIDTH).
fn tables_core_default_col_width() -> f64 {
    crate::sheet::COL_WIDTH
}


/// Read embedded charts back from an xlsx (ours or a Calc rewrite).
/// Best-effort: unknown chart kinds and foreign anchoring are skipped.
pub fn read_charts_from_xlsx(path: &str) -> Vec<crate::sheet::ChartSpec> {
    use crate::sheet::{parse_cell_ref, ChartKind, ChartSpec};
    let Ok(f) = std::fs::File::open(path) else { return Vec::new() };
    let Ok(mut zip) = zip::ZipArchive::new(f) else { return Vec::new() };

    // A1-style absolute range "Sheet1!$A$2:$A$5" → (first_row, col, last_row).
    fn parse_range(r: &str) -> Option<(usize, usize, usize)> {
        let range = r.rsplit('!').next()?.replace('$', "");
        let (a, b) = range.split_once(':').unwrap_or((range.as_str(), range.as_str()));
        let (r0, c0) = parse_cell_ref(a)?;
        let (r1, c1) = parse_cell_ref(b)?;
        if c0 != c1 {
            return None; // single-column series only (what we write)
        }
        Some((r0.min(r1), c0, r0.max(r1)))
    }

    // Anchors from drawing parts, in document order.
    let mut anchors: Vec<(usize, usize)> = Vec::new();
    let drawing_names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .filter(|n| n.starts_with("xl/drawings/drawing") && n.ends_with(".xml"))
        .collect();
    for name in &drawing_names {
        let mut xml = String::new();
        use std::io::Read as _;
        if zip.by_name(name).map(|mut f| f.read_to_string(&mut xml)).is_err() {
            continue;
        }
        // First <xdr:from> per anchor: <xdr:col>N</xdr:col><xdr:row>N</xdr:row>
        for from in xml.split("<xdr:from>").skip(1) {
            let grab = |tag: &str| -> Option<usize> {
                let open = format!("<xdr:{tag}>");
                let close = format!("</xdr:{tag}>");
                from.split(&open).nth(1)?.split(&close).next()?.trim().parse().ok()
            };
            if let (Some(c), Some(r)) = (grab("col"), grab("row")) {
                anchors.push((r, c));
            }
        }
    }

    let chart_names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .filter(|n| n.starts_with("xl/charts/chart") && n.ends_with(".xml"))
        .collect();
    let mut out = Vec::new();
    for (ci, name) in chart_names.iter().enumerate() {
        let mut xml = String::new();
        use std::io::Read as _;
        if zip.by_name(name).map(|mut f| f.read_to_string(&mut xml)).is_err() {
            continue;
        }
        let kind = if xml.contains("<c:barChart") {
            ChartKind::Bar
        } else if xml.contains("<c:lineChart") {
            ChartKind::Line
        } else if xml.contains("<c:pieChart") {
            ChartKind::Pie
        } else {
            continue;
        };
        let grab_f = |section: &str| -> Option<String> {
            xml.split(section)
                .nth(1)?
                .split("<c:f>")
                .nth(1)?
                .split("</c:f>")
                .next()
                .map(str::to_string)
        };
        let cat = grab_f("<c:cat>").and_then(|r| parse_range(&r));
        let Some(val) = grab_f("<c:val>").and_then(|r| parse_range(&r)) else { continue };
        // Title: first a:t inside c:title.
        let title = xml
            .split("<c:title>")
            .nth(1)
            .and_then(|t| t.split("</c:title>").next())
            .and_then(|t| t.split("<a:t>").nth(1))
            .and_then(|t| t.split("</a:t>").next())
            .unwrap_or("")
            .to_string();
        out.push(ChartSpec {
            kind,
            title,
            cat: cat.unwrap_or(val),
            val,
            anchor: anchors.get(ci).copied().unwrap_or((0, 0)),
        });
    }
    out
}

#[cfg(test)]
mod chart_tests {
    use super::*;
    use crate::sheet::{ChartKind, ChartSpec};

    #[test]
    fn chart_round_trips_through_xlsx() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.xlsx");
        let mut sh = SheetModel::new("Sheet1", 10, 5, 0);
        for (i, (l, v)) in [("North", 12.0), ("South", 9.5), ("East", 14.0)].iter().enumerate() {
            sh.data[i + 1][0] = l.to_string();
            sh.data[i + 1][1] = v.to_string();
        }
        sh.charts.push(ChartSpec {
            kind: ChartKind::Bar,
            title: "Regions".into(),
            cat: (1, 0, 3),
            val: (1, 1, 3),
            anchor: (5, 3),
        });
        save_sheets_to_xlsx(path.to_str().unwrap(), &[sh]).unwrap();
        let charts = read_charts_from_xlsx(path.to_str().unwrap());
        assert_eq!(charts.len(), 1, "chart part missing");
        let c = &charts[0];
        assert_eq!(c.kind, ChartKind::Bar);
        assert_eq!(c.val, (1, 1, 3), "values range lost: {c:?}");
        assert_eq!(c.cat, (1, 0, 3), "categories range lost: {c:?}");
        assert_eq!(c.title, "Regions");
        assert_eq!(c.anchor, (5, 3));
    }

    #[test]
    fn line_and_pie_kinds_round_trip() {
        for kind in [ChartKind::Line, ChartKind::Pie] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("c.xlsx");
            let mut sh = SheetModel::new("Sheet1", 6, 3, 0);
            sh.data[0][0] = "A".into();
            sh.data[0][1] = "1".into();
            sh.charts.push(ChartSpec {
                kind,
                title: String::new(),
                cat: (0, 0, 0),
                val: (0, 1, 0),
                anchor: (2, 0),
            });
            save_sheets_to_xlsx(path.to_str().unwrap(), &[sh]).unwrap();
            let charts = read_charts_from_xlsx(path.to_str().unwrap());
            assert_eq!(charts.len(), 1);
            assert_eq!(charts[0].kind, kind);
        }
    }
}
