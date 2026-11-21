// misnamed_files.rs — a file is read as what it holds, not what it's called.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// GOV.UK's "Avian influenza in wild birds" download is served as .xlsx but
// is a Latin-1 CSV. Opened as a workbook it failed, and Tables showed
// nothing; LibreOffice reads it by its content, as a table of 7 pages.

fn write(dir: &std::path::Path, name: &str, bytes: &[u8]) -> String {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn a_latin1_csv_named_xlsx_opens_as_its_table() {
    let dir = tempfile::tempdir().unwrap();
    // "Pembrokeshire" and a Latin-1 "Ynys Môn" (ô is 0xF4).
    let mut bytes = b"Week (Year),County,Species\r\n1,Pembrokeshire,Mute Swan\r\n2,Ynys M".to_vec();
    bytes.push(0xF4);
    bytes.extend_from_slice(b"n,Herring Gull\r\n");
    let path = write(dir.path(), "birds.xlsx", &bytes);
    let (_, sheets) = tables_core::io::load_workbook(&path).expect("read as the CSV it is");
    assert_eq!(sheets.len(), 1);
    let s = &sheets[0];
    assert_eq!((s.cell(0, 0), s.cell(0, 2)), ("Week (Year)", "Species"));
    assert_eq!(s.cell(1, 1), "Pembrokeshire");
    assert_eq!(s.cell(2, 1), "Ynys Môn", "Latin-1 read as Latin-1");
}

#[test]
fn a_semicolon_table_named_ods_takes_its_delimiter() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "t.ods", b"a;b;c\n1;2;3\n");
    let (_, sheets) = tables_core::io::load_workbook(&path).expect("read as text");
    assert_eq!((sheets[0].cell(1, 0), sheets[0].cell(1, 2)), ("1", "3"));
}

#[test]
fn a_csv_keeps_its_commas_when_a_quoted_header_holds_semicolons() {
    let dir = tempfile::tempdir().unwrap();
    let body = b"\"Tags (a; b; c; d)\",Count\nx,1\n";
    for name in ["t.csv", "t.xlsx"] {
        let path = write(dir.path(), name, body);
        let (_, sheets) = tables_core::io::load_workbook(&path).expect("read");
        assert_eq!((sheets[0].cell(0, 0), sheets[0].cell(0, 1)), ("Tags (a; b; c; d)", "Count"), "{name}");
    }
}

#[test]
fn an_empty_or_html_file_named_xlsx_is_still_a_load_error() {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in [("empty.xlsx", &b""[..]), ("page.xlsx", &b"\n<!DOCTYPE html><html>Not found</html>"[..])] {
        let path = write(dir.path(), name, body);
        assert!(tables_core::io::load_workbook(&path).is_err(), "{name}");
    }
}
