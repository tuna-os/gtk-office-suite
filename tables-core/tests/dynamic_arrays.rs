// dynamic_arrays.rs — the dynamic-array envelope Tables admits (#1659).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// IronCalc is the calculation authority: nothing here reimplements a
// function or a spill. Every expectation below was measured on the pinned
// engine (ironcalc_base 0.9.1) and is pinned, so an engine upgrade that
// changes a result fails here and the inventory in
// docs/TABLES-DYNAMIC-ARRAYS.md has to be updated with it. What a Calc
// rewrite does to the same workbook is pinned in soffice_oracle.rs.

mod support;

use support::*;
use tables_core::controller::WorkbookController;
use tables_core::engine::TablesEngine;

#[test]
fn every_admitted_function_is_exercised_and_every_case_names_an_admitted_one() {
    for name in ADMITTED.iter().filter(|n| **n != "RANDARRAY") {
        assert!(CASES.iter().any(|c| c.function == *name), "{name} is admitted but no case exercises it");
    }
    for case in CASES {
        assert!(ADMITTED.contains(&case.function), "{} is exercised but not admitted", case.function);
        assert!(case.formula.to_uppercase().contains(case.function), "{}: {}", case.function, case.formula);
    }
}

#[test]
fn each_admitted_function_spills_the_shape_and_values_its_row_says() {
    for case in CASES {
        let mut e = engine();
        e.set_cell_text(ANCHOR.0, ANCHOR.1, case.formula).unwrap();
        assert_eq!(spilled(&e, case, ANCHOR.0, ANCHOR.1), expected(case), "{}", case.formula);
        // Nothing is written outside the rectangle.
        let (h, w) = (case.spill.len(), case.spill[0].len());
        assert_eq!(rect(&e, ANCHOR.0, ANCHOR.1 + w, h, 1), vec![vec![String::new()]; h], "{}: spilled right of its shape", case.formula);
        assert_eq!(rect(&e, ANCHOR.0 + h, ANCHOR.1, 1, w), vec![vec![String::new(); w]], "{}: spilled below its shape", case.formula);
    }
}

#[test]
fn randarray_spills_numbers_in_the_shape_asked_for() {
    let mut e = engine();
    e.set_cell_text(0, 4, "=RANDARRAY(2,3)").unwrap();
    for row in rect(&e, 0, 4, 2, 3) {
        for cell in row {
            let n: f64 = cell.parse().unwrap_or_else(|_| panic!("{cell:?} is not a number"));
            assert!((0.0..1.0).contains(&n), "{n} is outside [0, 1)");
        }
    }
    assert_eq!(rect(&e, 0, 7, 1, 1), vec![vec![String::new()]], "spilled past its width");
}

#[test]
fn array_arithmetic_and_aggregates_over_an_array_spill_or_reduce() {
    let mut e = engine();
    e.set_cell_text(0, 4, "=A1:A3*2").unwrap();
    e.set_cell_text(0, 5, "=SUM(A1:A3*2)").unwrap();
    e.set_cell_text(0, 6, "=INDEX(SEQUENCE(3),2)").unwrap();
    assert_eq!(rect(&e, 0, 4, 3, 1), vec![vec!["2"], vec!["4"], vec!["6"]]);
    assert_eq!(e.cell(0, 5), "12");
    assert_eq!(e.cell(0, 6), "2");
}

#[test]
fn an_empty_result_is_a_calc_error_not_an_empty_spill() {
    let mut e = engine();
    e.set_cell_text(0, 4, "=FILTER(A1:A4,A1:A4>99)").unwrap();
    e.set_cell_text(0, 5, "=SEQUENCE(0)").unwrap();
    assert_eq!(e.cell(0, 4), "#CALC!");
    assert_eq!(e.cell(0, 5), "#CALC!");
}

/// A blocked destination shows #SPILL! on the anchor in every direction,
/// whether the blocker is a constant or a formula, and the spill appears
/// as soon as the blocker is cleared.
#[test]
fn a_blocked_spill_shows_spill_in_every_direction_and_clears_with_the_blocker() {
    for (name, formula, blocker, blocker_at) in [
        ("vertical", "=SEQUENCE(3)", "x", (2, 4)),
        ("horizontal", "=SEQUENCE(1,3)", "x", (0, 6)),
        ("2-D corner", "=SEQUENCE(2,2)", "x", (1, 5)),
        ("formula", "=SEQUENCE(3)", "=1+1", (1, 4)),
    ] {
        let mut e = engine();
        e.set_cell_text(blocker_at.0, blocker_at.1, blocker).unwrap();
        e.set_cell_text(0, 4, formula).unwrap();
        assert_eq!(e.cell(0, 4), "#SPILL!", "{name}");
        assert_ne!(e.cell(blocker_at.0, blocker_at.1), "", "{name}: the blocker was overwritten");
        e.set_cell_text(blocker_at.0, blocker_at.1, "").unwrap();
        assert_eq!(e.cell(0, 4), "1", "{name}: unblocked, it spills");
    }
}

/// The spill follows its sources: a FILTER over a range recomputes when a
/// source cell changes, and a SEQUENCE sized by a cell grows and shrinks.
#[test]
fn a_spill_recalculates_with_source_edits_and_resizes() {
    let mut e = engine();
    e.set_cell_text(0, 4, "=FILTER(A1:A4,A1:A4>2)").unwrap();
    assert_eq!(rect(&e, 0, 4, 3, 1), vec![vec!["3"], vec!["4"], vec![""]]);
    e.set_cell_text(0, 0, "9").unwrap();
    assert_eq!(rect(&e, 0, 4, 3, 1), vec![vec!["9"], vec!["3"], vec!["4"]]);
    e.set_cell_text(2, 0, "0").unwrap();
    assert_eq!(rect(&e, 0, 4, 3, 1), vec![vec!["9"], vec!["4"], vec![""]]);

    let mut e = engine();
    e.set_cell_text(6, 0, "3").unwrap();
    e.set_cell_text(0, 4, "=SEQUENCE(A7)").unwrap();
    assert_eq!(rect(&e, 0, 4, 4, 1), vec![vec!["1"], vec!["2"], vec!["3"], vec![""]]);
    e.set_cell_text(6, 0, "1").unwrap();
    assert_eq!(rect(&e, 0, 4, 4, 1), vec![vec!["1"], vec![""], vec![""], vec![""]]);
    e.set_cell_text(6, 0, "4").unwrap();
    assert_eq!(rect(&e, 0, 4, 4, 1), vec![vec!["1"], vec!["2"], vec!["3"], vec!["4"]]);
}

/// Other cells read a spilled cell like any value, and a source on another
/// sheet is followed.
#[test]
fn references_into_a_spill_and_a_spill_from_another_sheet_work() {
    let mut e = engine();
    e.set_cell_text(0, 4, "=SEQUENCE(3)").unwrap();
    e.set_cell_text(0, 6, "=E2*10").unwrap();
    e.set_cell_text(1, 6, "=SUM(E1:E3)").unwrap();
    assert_eq!((e.cell(0, 6), e.cell(1, 6)), ("20".to_string(), "6".to_string()));

    let mut e = engine();
    let data = e.add_sheet("Data").unwrap();
    for (r, v) in ["1", "5", "9"].into_iter().enumerate() {
        e.set_cell_text_on_sheet(data, r, 0, v).unwrap();
    }
    e.set_cell_text(0, 4, "=SORT(Data!A1:A3,1,-1)").unwrap();
    assert_eq!(rect(&e, 0, 4, 3, 1), vec![vec!["9"], vec!["5"], vec!["1"]]);
    e.set_cell_text_on_sheet(data, 0, 0, "100").unwrap();
    assert_eq!(rect(&e, 0, 4, 3, 1), vec![vec!["100"], vec!["9"], vec!["5"]]);
}

/// LIMIT, pinned: the grid shows the configured rows and columns. A spill
/// that runs past them is clipped at the edge with no #SPILL!, where Excel
/// would refuse the spill. The values past the edge exist in the engine
/// and are not shown or saved as inputs.
#[test]
fn a_spill_past_the_grid_is_clipped_not_refused() {
    let mut e = TablesEngine::new(6, 4).unwrap();
    e.set_cell_text(4, 0, "=SEQUENCE(3)").unwrap();
    assert_eq!(rect(&e, 4, 0, 2, 1), vec![vec!["1"], vec!["2"]], "the third value is past the last row");
    let mut e = TablesEngine::new(6, 4).unwrap();
    e.set_cell_text(0, 3, "=SEQUENCE(1,3)").unwrap();
    assert_eq!(e.cell(0, 3), "1", "the other two are past the last column");
}

/// LIMIT, pinned: the spill-reference operator (`A1#`, a reference to a
/// whole spill) is not implemented by the pinned engine: it yields the
/// anchor cell's value alone. A workbook using it computes something
/// different from Excel without an error.
#[test]
fn the_spill_reference_operator_is_not_supported() {
    let mut e = engine();
    e.set_cell_text(0, 4, "=SEQUENCE(3)").unwrap();
    e.set_cell_text(0, 6, "=E1#").unwrap();
    assert_eq!(e.cell(0, 6), "1", "E1# is the anchor alone, not the three values");
}

/// The controller owns the history: entering a spill is one undo step that
/// takes the whole projection out, and redo brings it back; a blocked
/// spill clears when the blocker's entry is undone.
#[test]
fn undo_and_redo_take_a_spill_out_and_back() {
    let mut c = WorkbookController::new(10, 8).unwrap();
    c.edit_cell(0, 0, "=SEQUENCE(3)");
    let column = |c: &WorkbookController| -> Vec<String> { (0..4).map(|r| c.state.borrow().sheet().cell(r, 0).to_string()).collect() };
    assert_eq!(column(&c), ["1", "2", "3", ""]);
    assert!(c.undo());
    assert_eq!(column(&c), ["", "", "", ""], "one undo takes the whole spill out");
    assert!(c.redo());
    assert_eq!(column(&c), ["1", "2", "3", ""], "and one redo brings it back");

    let mut c = WorkbookController::new(10, 8).unwrap();
    c.edit_cell(2, 0, "x");
    c.edit_cell(0, 0, "=SEQUENCE(3)");
    assert_eq!(c.state.borrow().sheet().cell(0, 0), "#SPILL!");
    assert!(c.undo(), "undo the formula");
    assert!(c.undo(), "undo the blocker");
    c.edit_cell(0, 0, "=SEQUENCE(3)");
    assert_eq!(c.state.borrow().sheet().cell(2, 0), "3", "with the blocker gone it spills");
}

// ── XLSX ──────────────────────────────────────────────────────────────

/// SAVE AND REOPEN. Every admitted function's formula is written with the
/// prefix Excel needs (_xlfn., and _xlws. for FILTER, SORT and the like) as
/// an array formula, its first value is cached, and the cells it spills
/// into are *not* stored as constants (a constant there would block the
/// spill on reopen). Reopened, every spill is recomputed in full.
#[test]
fn xlsx_keeps_each_formula_and_recomputes_each_spill_on_reopen() {
    let (c, placed) = workbook_with_every_case();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spills.xlsx");
    save(&c, &path);

    let xml = sheet_xml(&path);
    for (col, case) in &placed {
        let anchor = format!("{}1", tables_core::sheet::col_label(*col));
        let start = xml.find(&format!("<c r=\"{anchor}\"")).unwrap_or_else(|| panic!("{anchor} is not in the file"));
        let cell = &xml[start..xml[start..].find("</c>").map(|e| start + e).unwrap()];
        assert!(cell.contains("t=\"array\""), "{}: not an array formula: {cell}", case.formula);
        assert!(cell.contains(" cm=\"1\""), "{}: not marked a dynamic array: {cell}", case.formula);
        assert_eq!(cell.contains("_xlfn."), !CLASSIC.contains(&case.function), "{}: the _xlfn. prefix is for the newer functions only: {cell}", case.formula);
        assert!(cell.contains(&format!("{}(", case.function)), "{}: function lost: {cell}", case.formula);
        // The cell below the anchor holds no stored value when the spill is a column.
        if case.spill.len() > 1 {
            let below = format!("<c r=\"{}2\"", tables_core::sheet::col_label(*col));
            assert!(!xml.contains(&below), "{}: a spilled cell was written into the file", case.formula);
        }
    }

    let (reopened, _) = tables_core::io::load_xlsx_workbook(path.to_str().unwrap()).unwrap();
    for (col, case) in &placed {
        assert_eq!(spilled(&reopened, case, 0, *col), expected(case), "{} after reopening", case.formula);
        assert_eq!(reopened.editable_input_at(0, 1, *col), "", "{}: a spilled cell holds an input after reopening", case.formula);
    }
}
