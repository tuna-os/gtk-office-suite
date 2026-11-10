// xlsx_typed_save.rs — an XLSX save must write each cell with the type
// the engine holds for it (#1980).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The writer used to re-infer types from cell text: a text cell holding
// "00123" came back as the number 123, "2024-01-15" as a date serial,
// and booleans were written as strings at all — merged anchors included,
// which `merge_range` always writes as strings. This is the round trip
// that proves the types survive: fixture types in, save, reload, compare
// types (not renderings — "true" the string and TRUE the boolean render
// alike, which is exactly how the corruption hid).

use tables_core::engine::TypedValue;
use tables_core::io::{load_workbook, save_sheets_to_xlsx_bytes};

#[test]
fn xlsx_save_preserves_engine_cell_types() {
    use rust_xlsxwriter::{Format, Workbook};

    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("typed.xlsx");
    let mut fixture = Workbook::new();
    let sheet = fixture.add_worksheet();
    sheet.write_string(0, 0, "00123").unwrap();
    sheet.write_string(1, 0, "2024-01-15").unwrap();
    sheet.write_boolean(2, 0, true).unwrap();
    sheet.write_number(3, 0, 42.5).unwrap();
    sheet.write_string(4, 0, "true").unwrap();
    // A merged range whose anchor is a number: merge_range writes its
    // value as a string, so the typed write has to happen over it.
    sheet.merge_range(0, 2, 0, 3, "", &Format::new()).unwrap();
    sheet.write_number(0, 2, 7.0).unwrap();
    fixture.save(&source).unwrap();

    let (engine, sheets) = load_workbook(source.to_str().unwrap()).unwrap();
    // The fixture's types are the engine's types before the save.
    assert_eq!(engine.typed_value_at(0, 0, 0), TypedValue::Text("00123".into()));
    assert_eq!(engine.typed_value_at(0, 1, 0), TypedValue::Text("2024-01-15".into()));
    assert_eq!(engine.typed_value_at(0, 2, 0), TypedValue::Boolean(true));
    assert_eq!(engine.typed_value_at(0, 3, 0), TypedValue::Number(42.5));
    assert_eq!(engine.typed_value_at(0, 4, 0), TypedValue::Text("true".into()));
    assert_eq!(engine.typed_value_at(0, 0, 2), TypedValue::Number(7.0));

    let bytes = save_sheets_to_xlsx_bytes(&sheets, Some(&engine)).unwrap();
    let saved = dir.path().join("saved.xlsx");
    std::fs::write(&saved, bytes).unwrap();
    let (reloaded, resheets) = load_workbook(saved.to_str().unwrap()).unwrap();

    assert_eq!(reloaded.typed_value_at(0, 0, 0), TypedValue::Text("00123".into()), "leading zeros are text");
    assert_eq!(reloaded.typed_value_at(0, 1, 0), TypedValue::Text("2024-01-15".into()), "a date-shaped string stays a string");
    assert_eq!(reloaded.typed_value_at(0, 2, 0), TypedValue::Boolean(true), "a boolean is written as a boolean");
    assert_eq!(reloaded.typed_value_at(0, 3, 0), TypedValue::Number(42.5));
    assert_eq!(reloaded.typed_value_at(0, 4, 0), TypedValue::Text("true".into()), "the string \"true\" is not the boolean");
    assert_eq!(reloaded.typed_value_at(0, 0, 2), TypedValue::Number(7.0), "the merged anchor keeps its number");
    assert!(!resheets[0].merges.is_empty(), "the merge itself survives");
}
