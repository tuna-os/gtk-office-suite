// save_capability.rs — the formats Tables opens vs. the formats it can write.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Tables reads xlsx/xls/ods/csv/tsv and writes only xlsx. That asymmetry is
// fine; what is not fine is a save path assuming otherwise, because saving an
// imported `.csv` in place writes an xlsx package under a `.csv` name and
// destroys the user's file (#439).
//
// These tests pin the pairing end to end: a format is opened for real, and the
// capability answer for that same path is asserted. Adding a reader without a
// writer therefore fails here rather than in someone's documents.

use tables_core::io::{is_writable_format, load_workbook, save_sheets_to_xlsx, xlsx_save_as_name};
use tables_core::sheet::SheetModel;

fn ods_fixture() -> std::path::PathBuf {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/two_sheets.ods");
    assert!(
        path.exists(),
        "missing committed fixture {}; regenerate with `python3 scripts/gen_test_fixtures.py`",
        path.display()
    );
    path
}

/// A real ODS opens, and the same path is reported unwritable. Both halves
/// matter: reporting it unwritable would be trivially satisfiable if it also
/// failed to open.
#[test]
fn ods_opens_but_is_not_writable() {
    let path = ods_fixture();
    let path_str = path.to_str().unwrap();

    let (_engine, sheets) = load_workbook(path_str).expect("the ODS fixture must open");
    assert_eq!(sheets.len(), 2, "fixture should carry both sheets");

    assert!(
        !is_writable_format(path_str),
        "Tables has no ODS writer; saving in place would replace the file with xlsx bytes"
    );
    assert_eq!(xlsx_save_as_name(path_str), "two_sheets.xlsx");
}

/// Same contract for csv, which is the format a user is most likely to open,
/// edit and reflexively Ctrl+S.
#[test]
fn csv_opens_but_is_not_writable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quarterly.csv");
    std::fs::write(&path, "region,total\nnorth,10\n").unwrap();
    let path_str = path.to_str().unwrap();

    let (engine, sheets) = load_workbook(path_str).expect("the csv must open");
    assert_eq!(sheets.len(), 1);
    assert_eq!(engine.cell(1, 0), "north");

    assert!(!is_writable_format(path_str), "Tables has no csv writer");
    assert_eq!(xlsx_save_as_name(path_str), "quarterly.xlsx");
}

/// The format Tables *can* write round-trips, and is reported writable — so
/// the gate does not simply refuse everything.
#[test]
fn xlsx_opens_and_is_writable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("book.xlsx");
    let path_str = path.to_str().unwrap();

    let mut sheet = SheetModel::new("Sheet1", 2, 2, 0);
    sheet.data[0][0] = "kept".into();
    save_sheets_to_xlsx(path_str, &[sheet]).expect("xlsx must be writable");

    assert!(is_writable_format(path_str));
    let (engine, _sheets) = load_workbook(path_str).expect("the xlsx must reopen");
    assert_eq!(engine.cell(0, 0), "kept");
}

/// Every extension the loader accepts has a decided capability, and the
/// read-only ones outnumber the writable one. This is the invariant that
/// breaks if someone adds a reader and forgets that saving now has a hole:
/// the new extension must be listed here deliberately, one way or the other.
#[test]
fn every_readable_extension_has_a_decided_capability() {
    let writable = ["xlsx"];
    let read_only = ["xls", "xlsm", "xlsb", "ods", "csv", "tsv"];

    for extension in writable {
        assert!(
            is_writable_format(&format!("book.{extension}")),
            ".{extension} is listed writable but the gate refuses it"
        );
    }
    for extension in read_only {
        let path = format!("book.{extension}");
        assert!(
            !is_writable_format(&path),
            ".{extension} is listed read-only but the gate would let a save overwrite it"
        );
        // The offered alternative must be something we can actually write,
        // or Save As would hand the user a target the save path then refuses.
        assert!(is_writable_format(&xlsx_save_as_name(&path)));
    }
}
