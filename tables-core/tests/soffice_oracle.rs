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
