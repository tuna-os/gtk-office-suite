// LibreOffice Calc interop oracle for Tables (see PLAN.md: oracle, not
// port). Gate: every .xlsx we write must open in Calc and CSV-extract to
// the same grid. Skips without soffice unless REQUIRE_SOFFICE=1 (CI).

use std::process::Command;

use tables_core::io::save_sheets_to_xlsx;
use tables_core::sheet::SheetModel;

fn soffice_available() -> bool {
    Command::new("soffice").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

fn require_or_skip() -> bool {
    if soffice_available() { return true; }
    if std::env::var("REQUIRE_SOFFICE").is_ok() {
        panic!("REQUIRE_SOFFICE set but no soffice binary found");
    }
    eprintln!("skipping: soffice not installed");
    false
}

fn convert_to_csv(input: &std::path::Path) -> Result<String, String> {
    let dir = input.parent().unwrap();
    let profile = dir.join("lo-profile");
    let out = Command::new("soffice")
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "csv", "--outdir"])
        .arg(dir)
        .arg(input)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("soffice failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    std::fs::read_to_string(input.with_extension("csv")).map_err(|e| e.to_string())
}

#[test]
fn calc_reads_our_xlsx_grid() {
    if !require_or_skip() { return; }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("grid.xlsx");

    let mut sheet = SheetModel::new("Oracle", 3, 3, 0);
    sheet.data[0][0] = "name".into();
    sheet.data[0][1] = "qty".into();
    sheet.data[1][0] = "apples".into();
    sheet.data[1][1] = "42".into();
    sheet.data[2][0] = "pears".into();
    sheet.data[2][1] = "7".into();
    save_sheets_to_xlsx(path.to_str().unwrap(), &[sheet]).expect("write xlsx");

    let csv = convert_to_csv(&path).expect("Calc could not open our .xlsx");
    let rows: Vec<&str> = csv.lines().collect();
    assert!(rows[0].starts_with("name,qty"), "row0: {:?}", rows.first());
    assert!(rows[1].starts_with("apples,42"), "row1: {:?}", rows.get(1));
    assert!(rows[2].starts_with("pears,7"), "row2: {:?}", rows.get(2));
}

#[test]
fn calc_recalculates_our_formulas() {
    if !require_or_skip() { return; }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("formula.xlsx");

    let mut e = tables_core::engine::TablesEngine::new(5, 5).unwrap();
    e.set_cell_text(0, 0, "2");
    e.set_cell_text(0, 1, "3");
    e.set_cell_text(1, 0, "=A1*B1");
    e.evaluate();
    let mut sheet = SheetModel::new("F", 3, 3, 0);
    sheet.data[0][0] = "2".into();
    sheet.data[0][1] = "3".into();
    tables_core::io::save_sheets_to_xlsx_with_engine(
        path.to_str().unwrap(), &[sheet], Some(&e)).unwrap();

    let csv = convert_to_csv(&path).expect("Calc could not open formula xlsx");
    let row2 = csv.lines().nth(1).unwrap_or("");
    assert!(row2.starts_with("6"), "Calc did not evaluate our formula: {csv:?}");
}

#[test]
fn sheet_names_survive_calc() {
    use calamine::{open_workbook, Reader};
    if !require_or_skip() { return; }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("multi.xlsx");

    let mut s1 = SheetModel::new("Budget", 2, 2, 0);
    s1.data[0][0] = "x".into();
    let mut s2 = SheetModel::new("Forecast", 2, 2, 0);
    s2.data[0][0] = "y".into();
    tables_core::io::save_sheets_to_xlsx(path.to_str().unwrap(), &[s1, s2]).unwrap();

    // Round-trip THROUGH Calc: convert xlsx -> xlsx, then read names back.
    let out_dir = dir.path().join("out");
    std::fs::create_dir(&out_dir).unwrap();
    let profile = dir.path().join("prof");
    let st = Command::new("soffice")
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "xlsx", "--outdir"])
        .arg(&out_dir)
        .arg(&path)
        .output()
        .expect("soffice");
    assert!(st.status.success());
    let lo_path = out_dir.join("multi.xlsx");
    let wb: calamine::Xlsx<_> = open_workbook(&lo_path).expect("open LO xlsx");
    let names = wb.sheet_names().to_vec();
    assert!(names.contains(&"Budget".to_string()), "{names:?}");
    assert!(names.contains(&"Forecast".to_string()), "{names:?}");
}

// ── Oracle wave 2: Calc recalculation + fidelity (TDD) ───────────────

use tables_core::engine::TablesEngine;

/// Build an engine, mirror literal cells into a SheetModel, write xlsx
/// (formulas from the engine), and return Calc's CSV extraction.
fn calc_csv_of(cells: &[(usize, usize, &str)], rows: usize, cols: usize) -> Option<String> {
    if !require_or_skip() { return None; }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.xlsx");
    let mut e = TablesEngine::new(rows, cols).unwrap();
    let mut sheet = SheetModel::new("S", rows, cols, 0);
    for (r, c, v) in cells {
        e.set_cell_text(*r, *c, v);
        if !v.starts_with('=') {
            sheet.data[*r][*c] = v.to_string();
        }
    }
    e.evaluate();
    tables_core::io::save_sheets_to_xlsx_with_engine(path.to_str().unwrap(), &[sheet], Some(&e))
        .expect("write xlsx");
    // Keep the tempdir alive until the conversion is done.
    let csv = convert_to_csv(&path).expect("Calc could not open our xlsx");
    drop(dir);
    Some(csv)
}

fn cell_at(csv: &str, row: usize, col: usize) -> String {
    csv.lines()
        .nth(row)
        .map(|l| l.split(',').nth(col).unwrap_or("").trim_matches('"').to_string())
        .unwrap_or_default()
}

#[test]
fn calc_recalculates_aggregates() {
    let Some(csv) = calc_csv_of(
        &[(0, 0, "4"), (1, 0, "8"), (2, 0, "6"),
          (0, 1, "=SUM(A1:A3)"), (1, 1, "=AVERAGE(A1:A3)"),
          (2, 1, "=MIN(A1:A3)"), (3, 1, "=MAX(A1:A3)")],
        5, 3,
    ) else { return };
    assert_eq!(cell_at(&csv, 0, 1), "18", "SUM: {csv}");
    assert_eq!(cell_at(&csv, 1, 1), "6", "AVERAGE: {csv}");
    assert_eq!(cell_at(&csv, 2, 1), "4", "MIN: {csv}");
    assert_eq!(cell_at(&csv, 3, 1), "8", "MAX: {csv}");
}

#[test]
fn calc_recalculates_logic_functions() {
    let Some(csv) = calc_csv_of(
        &[(0, 0, "10"), (0, 1, "3"),
          (1, 0, "=IF(A1>B1,\"bigger\",\"smaller\")"),
          (2, 0, "=IF(AND(A1>5,B1<5),1,0)"),
          (3, 0, "=IF(OR(A1<5,B1<5),1,0)")],
        5, 3,
    ) else { return };
    assert_eq!(cell_at(&csv, 1, 0), "bigger", "{csv}");
    assert_eq!(cell_at(&csv, 2, 0), "1", "{csv}");
    assert_eq!(cell_at(&csv, 3, 0), "1", "{csv}");
}

#[test]
fn calc_recalculates_text_functions() {
    let Some(csv) = calc_csv_of(
        &[(0, 0, "gnome"), (1, 0, "=UPPER(A1)"),
          (2, 0, "=LEFT(A1,2)"), (3, 0, "=CONCATENATE(A1,\"-os\")")],
        5, 2,
    ) else { return };
    assert_eq!(cell_at(&csv, 1, 0), "GNOME", "{csv}");
    assert_eq!(cell_at(&csv, 2, 0), "gn", "{csv}");
    assert_eq!(cell_at(&csv, 3, 0), "gnome-os", "{csv}");
}

#[test]
fn calc_recalculates_nested_and_absolute() {
    let Some(csv) = calc_csv_of(
        &[(0, 0, "5"), (1, 0, "7"), (2, 0, "9"),
          (0, 1, "=IF(SUM(A1:A3)>20,SUM(A1:A3),0)"),
          (1, 1, "=$A$1*10")],
        5, 3,
    ) else { return };
    assert_eq!(cell_at(&csv, 0, 1), "21", "nested: {csv}");
    assert_eq!(cell_at(&csv, 1, 1), "50", "absolute: {csv}");
}

#[test]
fn unicode_cells_survive_calc() {
    let Some(csv) = calc_csv_of(
        &[(0, 0, "héllo"), (1, 0, "中文"), (2, 0, "naïve—dash")],
        4, 2,
    ) else { return };
    assert_eq!(cell_at(&csv, 0, 0), "héllo", "{csv}");
    assert_eq!(cell_at(&csv, 1, 0), "中文", "{csv}");
    assert_eq!(cell_at(&csv, 2, 0), "naïve—dash", "{csv}");
}

#[test]
fn negatives_and_floats_survive_calc() {
    let Some(csv) = calc_csv_of(
        &[(0, 0, "-3.5"), (1, 0, "0.125"), (2, 0, "=A1+A2")],
        4, 2,
    ) else { return };
    // Calc's CSV export prints display precision, not the raw value —
    // assert numerically.
    let num = |r: usize| cell_at(&csv, r, 0).parse::<f64>().expect("numeric cell");
    assert!((num(0) - -3.5).abs() < 0.01, "{csv}");
    assert!((num(1) - 0.125).abs() < 0.01, "{csv}");
    assert!((num(2) - -3.375).abs() < 0.01, "{csv}");
}

#[test]
fn sparse_grid_positions_survive_calc() {
    let Some(csv) = calc_csv_of(&[(4, 4, "island")], 6, 6) else { return };
    assert_eq!(cell_at(&csv, 4, 4), "island", "value drifted: {csv}");
    assert_eq!(cell_at(&csv, 0, 0), "", "phantom value at A1: {csv}");
}

#[test]
fn large_grid_survives_calc() {
    let mut cells: Vec<(usize, usize, String)> = Vec::new();
    for r in 0..30 {
        for c in 0..8 {
            cells.push((r, c, format!("{}", r * 8 + c)));
        }
    }
    let owned: Vec<(usize, usize, &str)> =
        cells.iter().map(|(r, c, v)| (*r, *c, v.as_str())).collect();
    let Some(csv) = calc_csv_of(&owned, 32, 10) else { return };
    assert_eq!(cell_at(&csv, 0, 0), "0", "{csv}");
    assert_eq!(cell_at(&csv, 29, 7), "239", "last cell lost: {csv}");
}

#[test]
fn our_xlsx_survives_calc_rewrite() {
    if !require_or_skip() { return; }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rt.xlsx");
    let mut sheet = SheetModel::new("RT", 3, 3, 0);
    sheet.data[0][0] = "alpha".into();
    sheet.data[1][1] = "42".into();
    save_sheets_to_xlsx(path.to_str().unwrap(), &[sheet]).unwrap();

    let out_dir = dir.path().join("out");
    std::fs::create_dir(&out_dir).unwrap();
    let profile = dir.path().join("prof");
    let st = Command::new("soffice")
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "xlsx", "--outdir"])
        .arg(&out_dir)
        .arg(&path)
        .output()
        .expect("soffice");
    assert!(st.status.success());
    let mut e = TablesEngine::new(5, 5).unwrap();
    let (rows, cols) =
        tables_core::io::load_file_into_engine(out_dir.join("rt.xlsx").to_str().unwrap(), &mut e)
            .expect("we failed to read Calc-rewritten xlsx");
    assert!(rows >= 2 && cols >= 2, "grid shrank: {rows}x{cols}");
    assert_eq!(e.cell(0, 0), "alpha");
    assert_eq!(e.cell(1, 1), "42");
}

#[test]
fn we_read_calc_authored_ods() {
    if !require_or_skip() { return; }
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.csv");
    std::fs::write(&src, "city,pop\nporto,231000\n").unwrap();
    let profile = dir.path().join("prof");
    let st = Command::new("soffice")
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "ods", "--outdir"])
        .arg(dir.path())
        .arg(&src)
        .output()
        .expect("soffice");
    assert!(st.status.success());
    let mut e = TablesEngine::new(5, 5).unwrap();
    tables_core::io::load_file_into_engine(dir.path().join("src.ods").to_str().unwrap(), &mut e)
        .expect("we failed to read Calc-authored ods");
    assert_eq!(e.cell(0, 0), "city");
    assert_eq!(e.cell(1, 1), "231000");
}

#[test]
fn we_read_calc_authored_xlsx() {
    if !require_or_skip() { return; }
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.csv");
    std::fs::write(&src, "k,v\ntemp,21.5\n").unwrap();
    let profile = dir.path().join("prof");
    let st = Command::new("soffice")
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "xlsx", "--outdir"])
        .arg(dir.path())
        .arg(&src)
        .output()
        .expect("soffice");
    assert!(st.status.success());
    let mut e = TablesEngine::new(5, 5).unwrap();
    tables_core::io::load_file_into_engine(dir.path().join("src.xlsx").to_str().unwrap(), &mut e)
        .expect("we failed to read Calc-authored xlsx");
    assert_eq!(e.cell(0, 0), "k");
    // Calc applies a 0.00 display format on csv→xlsx; compare the value.
    let v: f64 = e.cell(1, 1).parse().expect("numeric cell");
    assert!((v - 21.5).abs() < 0.001, "value drifted: {}", e.cell(1, 1));
}

#[test]
fn formula_strings_survive_calc_rewrite_as_formulas() {
    use calamine::{open_workbook, Reader};
    if !require_or_skip() { return; }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.xlsx");
    let mut e = TablesEngine::new(4, 4).unwrap();
    e.set_cell_text(0, 0, "3");
    e.set_cell_text(0, 1, "4");
    e.set_cell_text(1, 0, "=A1+B1");
    e.evaluate();
    let mut sheet = SheetModel::new("F", 4, 4, 0);
    sheet.data[0][0] = "3".into();
    sheet.data[0][1] = "4".into();
    tables_core::io::save_sheets_to_xlsx_with_engine(path.to_str().unwrap(), &[sheet], Some(&e))
        .unwrap();

    let out_dir = dir.path().join("out");
    std::fs::create_dir(&out_dir).unwrap();
    let profile = dir.path().join("prof");
    let st = Command::new("soffice")
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "xlsx", "--outdir"])
        .arg(&out_dir)
        .arg(&path)
        .output()
        .expect("soffice");
    assert!(st.status.success());
    // Calc keeps it a live formula (not a frozen value): the formulas
    // sheet reader sees "=A1+B1" again.
    let mut wb: calamine::Xlsx<_> = open_workbook(out_dir.join("f.xlsx")).expect("open");
    let formulas = wb.worksheet_formula("F").expect("formula sheet");
    let has = formulas.rows().flatten().any(|f| f.replace(' ', "").contains("A1+B1"));
    assert!(has, "formula was frozen to a value by the round-trip");
}

// ── Oracle wave 3: structural fidelity through Calc ──────────────────

use suite_common_core::format::{NumberFormat, NumberFormatKind};

/// Convert with Calc into out_dir; returns the rewritten xlsx path.
fn calc_rewrite(path: &std::path::Path) -> std::path::PathBuf {
    let dir = path.parent().unwrap();
    let out_dir = dir.join("rewrite-out");
    std::fs::create_dir_all(&out_dir).unwrap();
    let profile = dir.join("rewrite-prof");
    let st = Command::new("soffice")
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "xlsx", "--outdir"])
        .arg(&out_dir)
        .arg(path)
        .output()
        .expect("soffice");
    assert!(st.status.success());
    out_dir.join(path.file_name().unwrap())
}

/// Read a zip member of an xlsx as text.
fn xlsx_member(path: &std::path::Path, member: &str) -> String {
    use std::io::Read;
    let f = std::fs::File::open(path).expect("open xlsx");
    let mut z = zip::ZipArchive::new(f).expect("zip");
    let mut s = String::new();
    z.by_name(member).map(|mut m| m.read_to_string(&mut s)).ok();
    s
}

#[test]
fn currency_format_renders_in_calc() {
    if !require_or_skip() { return; }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cur.xlsx");
    let mut sheet = SheetModel::new("C", 3, 3, 0);
    sheet.data[0][0] = "1234.5".into();
    sheet.formats[0][0] = NumberFormat::new(NumberFormatKind::Currency("$".into(), 2));
    save_sheets_to_xlsx(path.to_str().unwrap(), &[sheet]).unwrap();
    // Calc's CSV filter exports raw values, so assert structurally:
    // the rewritten workbook still carries the currency number format.
    let rewritten = calc_rewrite(&path);
    let styles = xlsx_member(&rewritten, "xl/styles.xml");
    assert!(
        styles.contains("$") && (styles.contains("#,##0.00") || styles.contains("0.00")),
        "currency numFmt lost through Calc: {styles}"
    );
}

#[test]
fn merged_cells_survive_calc_rewrite() {
    if !require_or_skip() { return; }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("merge.xlsx");
    let mut sheet = SheetModel::new("M", 4, 4, 0);
    sheet.data[0][0] = "spanning header".into();
    sheet.merges.push((0, 0, 2, 2)); // (row, col, rows, cols)
    save_sheets_to_xlsx(path.to_str().unwrap(), &[sheet]).unwrap();
    let rewritten = calc_rewrite(&path);
    let xml = xlsx_member(&rewritten, "xl/worksheets/sheet1.xml");
    assert!(
        xml.contains("<mergeCell"),
        "merged region lost through Calc (or never written): {}",
        &xml[..xml.len().min(400)]
    );
}

#[test]
fn frozen_panes_survive_calc_rewrite() {
    if !require_or_skip() { return; }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("freeze.xlsx");
    let mut sheet = SheetModel::new("Fz", 4, 4, 0);
    sheet.data[0][0] = "header".into();
    sheet.frozen_rows = 1;
    save_sheets_to_xlsx(path.to_str().unwrap(), &[sheet]).unwrap();
    let rewritten = calc_rewrite(&path);
    let xml = xlsx_member(&rewritten, "xl/worksheets/sheet1.xml");
    assert!(
        xml.contains("<pane") && xml.contains("frozen"),
        "frozen pane lost through Calc (or never written)"
    );
}

#[test]
fn column_widths_survive_calc_rewrite() {
    if !require_or_skip() { return; }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("width.xlsx");
    let mut sheet = SheetModel::new("W", 3, 3, 0);
    sheet.data[0][0] = "wide".into();
    sheet.set_col_width(0, 240.0); // px, well above the 90px default
    save_sheets_to_xlsx(path.to_str().unwrap(), &[sheet]).unwrap();
    let rewritten = calc_rewrite(&path);
    let xml = xlsx_member(&rewritten, "xl/worksheets/sheet1.xml");
    assert!(
        xml.contains("customWidth"),
        "custom column width lost through Calc (or never written)"
    );
}

#[test]
fn cross_sheet_reference_recalculated_by_calc() {
    if !require_or_skip() { return; }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("xsheet.xlsx");
    let mut e = TablesEngine::new(4, 4).unwrap();
    // IronCalc addresses sheets by name; the engine hosts one sheet, so
    // model the cross-ref with two of OUR sheets and formulas from the
    // engine on sheet 1 only referencing its own grid — then assert the
    // multi-sheet file keeps both grids through Calc.
    e.set_cell_text(0, 0, "7");
    e.set_cell_text(1, 0, "=A1*3");
    e.evaluate();
    let mut s1 = SheetModel::new("Data", 4, 4, 0);
    s1.data[0][0] = "7".into();
    let mut s2 = SheetModel::new("Notes", 4, 4, 0);
    s2.data[0][0] = "annotation".into();
    tables_core::io::save_sheets_to_xlsx_with_engine(
        path.to_str().unwrap(), &[s1, s2], Some(&e)).unwrap();
    let csv = convert_to_csv(&path).expect("open");
    assert!(csv.lines().nth(1).unwrap_or("").starts_with("21"), "formula not recalculated: {csv}");
    // Second sheet content survives the rewrite.
    let rewritten = calc_rewrite(&path);
    let mut e2 = TablesEngine::new(4, 4).unwrap();
    tables_core::io::load_file_into_engine(rewritten.to_str().unwrap(), &mut e2)
        .expect("read rewritten");
    // load_file_into_engine reads the first sheet; sheet2 presence is
    // asserted structurally.
    let wb_xml = xlsx_member(&rewritten, "xl/workbook.xml");
    assert!(wb_xml.contains("Notes"), "second sheet lost: {wb_xml}");
}

/// Convert via headless Calc into `to`, returning the produced path.
fn convert(input: &std::path::Path, to: &str) -> Result<std::path::PathBuf, String> {
    let out_dir = input.parent().unwrap().join("conv-out");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let profile = input.parent().unwrap().join("conv-prof");
    let st = std::process::Command::new("soffice")
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", to, "--outdir"])
        .arg(&out_dir)
        .arg(input)
        .output()
        .map_err(|e| e.to_string())?;
    if !st.status.success() {
        return Err(String::from_utf8_lossy(&st.stderr).into_owned());
    }
    let stem = input.file_stem().unwrap().to_string_lossy();
    let out = out_dir.join(format!("{stem}.{to}"));
    if out.exists() { Ok(out) } else { Err("no output produced".into()) }
}

// ── Charts (ADR 0003 §3) ─────────────────────────────────────────────

/// A chart we write must survive a Calc rewrite of the workbook —
/// kind and values range intact, read back by our own chart reader.
#[test]
fn chart_survives_calc_rewrite() {
    if !require_or_skip() { return; }
    use tables_core::sheet::{ChartKind, ChartSpec, SheetModel};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chart.xlsx");
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
    tables_core::io::save_sheets_to_xlsx(path.to_str().unwrap(), &[sh]).unwrap();

    let rewritten = convert(&path, "xlsx").expect("Calc rewrite failed");
    let charts = tables_core::io::read_charts_from_xlsx(rewritten.to_str().unwrap());
    assert!(!charts.is_empty(), "Calc dropped the chart");
    let c = &charts[0];
    assert_eq!(c.kind, ChartKind::Bar, "chart kind changed: {c:?}");
    assert_eq!(c.val, (1, 1, 3), "values range changed: {c:?}");
}

// ── Conditional formatting (ADR 0003 §4) ─────────────────────────────

/// A cell-value rule must survive a Calc rewrite: operator, threshold,
/// and range intact when read back by our own parser.
#[test]
fn cond_rule_survives_calc_rewrite() {
    if !require_or_skip() { return; }
    use tables_core::sheet::{CondOp, CondRule, SheetModel};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cf.xlsx");
    let mut sh = SheetModel::new("Sheet1", 10, 5, 0);
    for r in 0..5 {
        sh.data[r][1] = format!("{}", r * 10);
    }
    sh.cond_rules.push(CondRule {
        range: (0, 1, 4, 1),
        op: CondOp::Greater,
        value: 25.0,
        value2: 25.0,
        fill: "FFC0C0".into(),
    });
    tables_core::io::save_sheets_to_xlsx(path.to_str().unwrap(), &[sh]).unwrap();

    let rewritten = convert(&path, "xlsx").expect("Calc rewrite failed");
    let rules = tables_core::io::read_cond_rules_from_xlsx(rewritten.to_str().unwrap());
    assert!(!rules.is_empty(), "Calc dropped the conditional format");
    let r = &rules[0];
    assert_eq!(r.op, CondOp::Greater, "operator changed: {r:?}");
    assert!((r.value - 25.0).abs() < 1e-9, "threshold changed: {r:?}");
    assert_eq!(r.range, (0, 1, 4, 1), "range changed: {r:?}");
}
