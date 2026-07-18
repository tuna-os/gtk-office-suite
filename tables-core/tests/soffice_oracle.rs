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
