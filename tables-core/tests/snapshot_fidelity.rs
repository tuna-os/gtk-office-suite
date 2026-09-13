// snapshot_fidelity.rs — what a crash-recovery snapshot keeps (#322).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// A Tables snapshot is an xlsx package built by `save_sheets_to_xlsx_bytes`,
// and recovery reads it back with `load_workbook` — the same pair a plain
// Save/Open uses. So "no recovery format silently strips supported content"
// is a claim about that round trip, and it was false: the writer emitted
// column widths, row heights, frozen panes and merged ranges, and the reader
// parsed none of them. Save a workbook, reopen it, and the layout was gone;
// crash and recover, and it was gone from the recovered copy too.
//
// These tests go through the byte path rather than through a file saved by
// some other route, because the bytes are what a snapshot actually holds.

use tables_core::io::{load_workbook, save_sheets_to_xlsx_bytes};
use tables_core::sheet::SheetModel;

/// Round-trip a model the way autosave does: to xlsx bytes, onto disk as the
/// recovery path's temp file, and back through the real open path.
fn through_a_snapshot(sheets: &[SheetModel], tag: &str) -> Vec<SheetModel> {
    let bytes = save_sheets_to_xlsx_bytes(sheets, None).expect("snapshot bytes");
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(format!("{tag}.xlsx"));
    std::fs::write(&path, &bytes).expect("write snapshot");
    let (_engine, back) = load_workbook(path.to_str().unwrap()).expect("recover");
    back
}

fn sheet_with_layout() -> SheetModel {
    let mut s = SheetModel::new("Layout", 20, 10, 0);
    s.data[0][0] = "kept".to_string();
    s.col_widths[1] = 222.0;
    s.col_widths[4] = 63.0;
    s.row_heights[2] = 44.0;
    s.frozen_rows = 2;
    s.frozen_cols = 1;
    // (row, col, rowspan, colspan) — the order save.rs writes.
    s.merges.push((4, 0, 2, 3));
    s
}

#[test]
fn column_widths_survive_a_snapshot() {
    let back = through_a_snapshot(&[sheet_with_layout()], "widths");
    let s = &back[0];
    // xlsx stores widths in character units, so a pixel width is quantised
    // rather than preserved bit-for-bit. A pixel of slack is invisible on
    // screen; losing the column back to its default is not, and that is what
    // this guards.
    assert!(
        (s.col_widths[1] - 222.0).abs() < 1.0,
        "column 1 came back {} px, not ~222 — width dropped to the default?",
        s.col_widths[1],
    );
    assert!(
        (s.col_widths[4] - 63.0).abs() < 1.0,
        "column 4 came back {} px, not ~63",
        s.col_widths[4],
    );
}

#[test]
fn row_heights_survive_a_snapshot() {
    let back = through_a_snapshot(&[sheet_with_layout()], "heights");
    let s = &back[0];
    assert!(
        (s.row_heights[2] - 44.0).abs() < 1.0,
        "row 2 came back {} px, not ~44",
        s.row_heights[2],
    );
}

#[test]
fn frozen_panes_survive_a_snapshot() {
    let back = through_a_snapshot(&[sheet_with_layout()], "frozen");
    let s = &back[0];
    // Asserted as a pair: xSplit counts columns and ySplit rows, so reading
    // them the wrong way round gives (1, 2) here and passes any check that
    // only asks whether *something* was frozen.
    assert_eq!(
        (s.frozen_rows, s.frozen_cols),
        (2, 1),
        "frozen panes came back as {:?}",
        (s.frozen_rows, s.frozen_cols),
    );
}

#[test]
fn merged_ranges_survive_a_snapshot() {
    let back = through_a_snapshot(&[sheet_with_layout()], "merges");
    let s = &back[0];
    assert_eq!(
        s.merges,
        vec![(4, 0, 2, 3)],
        "merged range came back as {:?}",
        s.merges,
    );
}

#[test]
fn a_default_sheet_gains_no_layout_it_never_had() {
    // The other direction, and the one a careless fix breaks: reading widths
    // for every `<col>` run rather than only the custom ones would hand back
    // a sheet full of "explicit" defaults, and a merge list that grows by one
    // empty entry per save.
    let plain = SheetModel::new("Plain", 12, 6, 0);
    let back = through_a_snapshot(std::slice::from_ref(&plain), "plain");
    let s = &back[0];
    assert_eq!(s.merges, Vec::new(), "a sheet with no merges gained {:?}", s.merges);
    assert_eq!((s.frozen_rows, s.frozen_cols), (0, 0), "a sheet gained a freeze");
    for c in 0..6 {
        assert!(
            (s.col_widths[c] - plain.col_width(c)).abs() < 1.0,
            "column {c} came back {} px, not its default {}",
            s.col_widths[c],
            plain.col_width(c),
        );
    }
}

#[test]
fn layout_is_kept_per_sheet_not_smeared_across_them() {
    // Sheet properties are resolved by name through workbook.xml's rels;
    // a lookup that fell back to "sheet1.xml" would give every sheet the
    // first one's layout and still satisfy every assertion above.
    let first = sheet_with_layout();
    let mut second = SheetModel::new("Bare", 20, 10, 1);
    second.data[0][0] = "other".to_string();
    let back = through_a_snapshot(&[first, second], "per-sheet");

    assert_eq!(back[1].merges, Vec::new(), "sheet 2 inherited sheet 1's merges");
    assert_eq!(
        (back[1].frozen_rows, back[1].frozen_cols),
        (0, 0),
        "sheet 2 inherited sheet 1's frozen panes",
    );
    assert!(
        (back[1].col_widths[1] - 222.0).abs() >= 1.0,
        "sheet 2 inherited sheet 1's 222px column width",
    );
}
