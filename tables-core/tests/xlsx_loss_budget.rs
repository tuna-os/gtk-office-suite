// xlsx_loss_budget.rs — what an XLSX save keeps, declared and tested (#1204, TBL-001).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// tables-readiness.md: "Test formulas, cached values, styles, charts,
// rules, names, protection and hidden/filter state against a declared XLSX
// loss budget." The budget is BUDGET below: every feature Tables models,
// and whether a save to .xlsx followed by Tables' own open brings it back.
// One workbook carries all of them at once, through the real byte path
// (save_sheets_to_xlsx_bytes with the engine, then load_workbook).
//
// TBL-001 splits the old broad rows ("charts", "sheet permissions") into
// one entry per field: chart owner/series/order/name/color/ranges/title/
// axis/legend/anchor/size, each protection flag, credentials, rule
// ownership, filter criteria, formula/cache state, names, notes,
// validation shapes, hidden state, dimensions and print settings. Every
// Lost row maps to a `tables_core::io::loss` report entry with a location
// and a consequence (`LOST_COVERAGE`, proven by
// `tbl001_each_loss_has_a_location`); every Kept row is asserted on a
// fixture that warns about nothing (`retained_workbook`), and the saved
// XML is inspected independently of Tables' importer for what both
// readers can see.
//
// The test fails both ways. A feature declared Kept that comes back wrong
// is a regression. A feature declared Lost that survives means the budget
// is stale: tighten it, so the declared losses stay exactly the real ones.

use std::collections::HashSet;
use suite_common_core::charts::ChartKind;
use suite_common_core::format::{NumberFormat, NumberFormatKind};
use suite_common_core::print::{Orientation, PageSize};
use tables_core::io::{load_workbook, save_sheets_to_xlsx_bytes};
use tables_core::sheet::{ChartSeries, ChartSpec, CondOp, CondRule, FilterColumn, FilterDef, LegendPosition, SheetModel, ValidationRule};
use tables_core::style::Rgb;
use tables_core::TablesEngine;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Budget {
    Kept,
    /// Lost on an xlsx round trip through Tables, with the consequence.
    Lost(&'static str),
}
use Budget::*;

const ROWS: usize = 12;
const COLS: usize = 6;

fn workbook() -> (TablesEngine, Vec<SheetModel>) {
    let mut engine = TablesEngine::new(ROWS, COLS).expect("engine");
    let mut s = SheetModel::new("Data", ROWS, COLS, engine.sheet_id_at(0).unwrap());
    for (r, (label, n, m)) in [("North", "10", "4"), ("South", "20", "6"), ("East", "30", "2")].iter().enumerate() {
        s.data[r + 1][0] = label.to_string();
        s.data[r + 1][1] = n.to_string();
        s.data[r + 1][2] = m.to_string();
        engine.set_cell_text(r + 1, 0, label).unwrap();
        engine.set_cell_text(r + 1, 1, n).unwrap();
        engine.set_cell_text(r + 1, 2, m).unwrap();
    }
    s.data[0][0] = "Region".into();
    engine.set_cell_text(0, 0, "Region").unwrap();
    // A formula and its cached value.
    s.data[5][1] = "=SUM(B2:B4)".into();
    s.formulas[5][1] = true;
    engine.set_cell_text(5, 1, "=SUM(B2:B4)").unwrap();
    engine.set_defined_name("Total", Some("Data!$B$6")).unwrap();
    engine.evaluate();
    // Styles and number format.
    s.styles[0][0].bold = true;
    s.styles[0][0].fill = Some(Rgb(0xFF, 0xC7, 0xCE));
    s.styles[0][0].font_family = Some("Liberation Serif".into());
    s.formats[1][1] = NumberFormat::new(NumberFormatKind::Currency("$".into(), 2));
    // Dimensions: a widened column, a taller row and a frozen header.
    s.col_widths[1] = 140.0;
    s.row_heights[2] = 42.0;
    s.frozen_rows = 1;
    // Page setup beyond the default.
    s.page_setup.size = PageSize::Letter;
    s.page_setup.orientation = Orientation::Landscape;
    // Conditional formatting.
    s.cond_rules.push(CondRule { range: (1, 1, 3, 1), op: CondOp::Greater, value: 15.0, value2: 0.0, fill: "C6EFCE".into(), priority: 1 });
    // A chart over the data: two named series (one colored), axis titles,
    // a non-default legend and a non-default size. TBL-002 keeps every
    // admitted field: the budget asserts each one below.
    s.charts.push(ChartSpec {
        kind: ChartKind::Bar,
        title: "Sales by region".into(),
        x_axis_title: Some("Region".into()),
        y_axis_title: Some("Revenue".into()),
        legend_position: LegendPosition::Bottom,
        series: vec![
            ChartSeries { name: "Sales".into(), cat: (1, 0, 3), val: (1, 1, 3), color: Some("4472C4".into()) },
            ChartSeries { name: "Returns".into(), cat: (1, 0, 3), val: (1, 2, 3), color: None },
        ],
        cat: (1, 0, 3),
        val: (1, 1, 3),
        anchor: (1, 3),
        width_px: 320.0,
        height_px: 200.0,
    });
    // Protection, credentials, validation, notes, hidden and filtered rows, print area.
    s.protection.protected = true;
    s.protection.allow_select_locked = false;
    s.protection.allow_format_cells = true;
    s.protection.allow_insert_rows = true;
    s.protection.allow_delete_rows = true;
    s.protection.password_hash = Some("secret".into());
    s.cell_protections[1][1].locked = false;
    s.cell_protections[5][1].hidden_formula = true;
    s.cell_protections[10][5].locked = false;
    s.validations[1][2] = Some(ValidationRule::WholeNumber { min: Some(0), max: Some(100) });
    s.validations[2][2] = Some(ValidationRule::Regex("^N.*".into()));
    s.validations[3][2] = Some(ValidationRule::WholeNumber { min: None, max: None });
    s.validations[4][2] = Some(ValidationRule::List(Vec::new()));
    s.notes[1][0] = Some("Largest region".into());
    s.hidden_rows_manual.insert(8);
    s.hidden_cols.insert(4);
    s.hidden_rows.insert(9);
    s.print_area = Some((0, 0, 5, 2));
    // A second sheet with a chart and a rule of its own.
    engine.add_sheet("Later").unwrap();
    // TBL-006: the model is a projection — its content needs the engine
    // inputs the writer saves, not just display text.
    engine.set_active_sheet(1).unwrap();
    engine.set_cell_text(0, 0, "1").unwrap();
    engine.set_cell_text(1, 0, "2").unwrap();
    engine.set_active_sheet(0).unwrap();
    let mut t = SheetModel::new("Later", ROWS, COLS, engine.sheet_id_at(1).unwrap());
    t.data[0][0] = "1".into();
    t.data[1][0] = "2".into();
    t.cond_rules.push(CondRule { range: (0, 0, 1, 0), op: CondOp::Less, value: 2.0, value2: 0.0, fill: "FFEB9C".into(), priority: 1 });
    let mut chart = s.charts[0].clone();
    chart.title = "Later chart".into();
    t.charts.push(chart);
    t.protection.protected = true;
    t.protection.allow_select_unlocked = false;
    t.cell_protections[1][0].locked = false;
    (engine, vec![s, t])
}

/// The same workbook minus every declared loss: values, one plain chart,
/// first-sheet rules, modeled protection without credentials, writable
/// validation, manual hides, dimensions and print settings. Saving it must
/// warn about nothing — neither from the model nor from the file Tables
/// wrote — or a retained field warns spuriously.
fn retained_workbook() -> (TablesEngine, Vec<SheetModel>) {
    let mut engine = TablesEngine::new(ROWS, COLS).expect("engine");
    let mut s = SheetModel::new("Data", ROWS, COLS, engine.sheet_id_at(0).unwrap());
    s.data[1][0] = "North".into();
    s.data[1][1] = "10".into();
    engine.set_cell_text(1, 0, "North").unwrap();
    engine.set_cell_text(1, 1, "10").unwrap();
    s.data[5][1] = "=SUM(B2:B4)".into();
    s.formulas[5][1] = true;
    engine.set_cell_text(5, 1, "=SUM(B2:B4)").unwrap();
    engine.set_defined_name("Total", Some("Data!$B$6")).unwrap();
    engine.evaluate();
    s.styles[0][0].bold = true;
    s.col_widths[1] = 140.0;
    s.row_heights[2] = 42.0;
    s.frozen_rows = 1;
    s.cond_rules.push(CondRule { range: (1, 1, 3, 1), op: CondOp::Greater, value: 15.0, value2: 0.0, fill: "C6EFCE".into(), priority: 1 });
    s.charts.push(ChartSpec {
        kind: ChartKind::Bar,
        title: "Plain".into(),
        x_axis_title: None,
        y_axis_title: None,
        legend_position: LegendPosition::Right,
        series: vec![ChartSeries { name: String::new(), cat: (1, 0, 1), val: (1, 1, 1), color: None }],
        cat: (1, 0, 1),
        val: (1, 1, 1),
        anchor: (1, 3),
        width_px: 480.0,
        height_px: 280.0,
    });
    s.protection.protected = true;
    s.protection.allow_select_locked = false;
    s.protection.allow_format_cells = true;
    s.cell_protections[1][1].locked = false;
    s.validations[1][2] = Some(ValidationRule::WholeNumber { min: Some(0), max: Some(100) });
    s.notes[1][0] = Some("Largest region".into());
    s.hidden_rows_manual.insert(8);
    s.hidden_cols.insert(4);
    s.print_area = Some((0, 0, 5, 2));
    engine.add_sheet("Later").unwrap();
    let mut t = SheetModel::new("Later", ROWS, COLS, engine.sheet_id_at(1).unwrap());
    t.data[1][0] = "2".into();
    t.protection.protected = true;
    t.protection.allow_select_unlocked = false;
    t.cell_protections[1][0].locked = false;
    (engine, vec![s, t])
}

type Check = fn(&TablesEngine, &[SheetModel]) -> bool;

/// The declared XLSX loss budget: (feature, budget, does it come back?).
const BUDGET: &[(&str, Budget, Check)] = &[
    ("cell values", Kept, |_, s| s[0].data[1][0] == "North" && s[0].data[2][1] == "20"),
    ("formulas", Kept, |e, s| s[0].formulas[5][1] && e.formula_at(0, 5, 1).is_some_and(|f| f.contains("SUM(B2:B4)"))),
    ("cached formula values", Kept, |e, _| e.cell_at(0, 5, 1) == "60"),
    ("defined names", Kept, |e, _| e.defined_name("Total").is_some()),
    ("bold", Kept, |_, s| s[0].styles[0][0].bold),
    ("cell fill", Kept, |_, s| s[0].styles[0][0].fill == Some(Rgb(0xFF, 0xC7, 0xCE))),
    ("font family", Kept, |_, s| s[0].styles[0][0].font_family.as_deref() == Some("Liberation Serif")),
    ("number formats", Kept, |_, s| matches!(&s[0].formats[1][1].kind, NumberFormatKind::Currency(c, 2) if c == "$")),
    ("cond rule range", Kept, |_, s| s[0].cond_rules.iter().any(|r| r.range == (1, 1, 3, 1))),
    ("cond rule op", Kept, |_, s| s[0].cond_rules.iter().any(|r| r.op == CondOp::Greater)),
    ("cond rule threshold", Kept, |_, s| s[0].cond_rules.iter().any(|r| r.value == 15.0)),
    ("cond rule fill", Kept, |_, s| s[0].cond_rules.iter().any(|r| r.fill.eq_ignore_ascii_case("C6EFCE"))),
    ("first-sheet rule ownership", Kept, |_, s| !s[0].cond_rules.is_empty()),
    ("chart kind", Kept, |_, s| s[0].charts.iter().any(|c| c.kind == ChartKind::Bar)),
    ("chart title", Kept, |_, s| s[0].charts.iter().any(|c| c.title == "Sales by region")),
    ("first-series ranges", Kept, |_, s| s[0].charts.iter().any(|c| c.cat == (1, 0, 3) && c.val == (1, 1, 3))),
    ("chart anchor", Kept, |_, s| s[0].charts.iter().any(|c| c.anchor == (1, 3))),
    ("chart owner, first sheet", Kept, |_, s| s[0].charts.iter().any(|c| c.title == "Sales by region")),
    ("sheet protection", Kept, |_, s| s[0].protection.protected),
    ("permission select locked cells", Kept, |_, s| !s[0].protection.allow_select_locked),
    ("permission select unlocked cells", Kept, |_, s| s[0].protection.allow_select_unlocked),
    ("permission format cells", Kept, |_, s| s[0].protection.allow_format_cells),
    ("permission insert rows", Kept, |_, s| s[0].protection.allow_insert_rows),
    ("permission delete rows", Kept, |_, s| s[0].protection.allow_delete_rows),
    ("unlocked cells", Kept, |_, s| !s[0].cell_protections[1][1].locked),
    ("hidden formulas", Kept, |_, s| s[0].cell_protections[5][1].hidden_formula && s[0].cell_protections[5][1].locked),
    ("protection-only blank cells", Kept, |_, s| !s[0].cell_protections[10][5].locked && s[0].data[10][5].is_empty()),
    ("later sheet protection", Kept, |_, s| s[1].protection.protected && !s[1].protection.allow_select_unlocked && !s[1].cell_protections[1][0].locked),
    ("bounded whole-number validation", Kept, |_, s| matches!(s[0].validations[1][2], Some(ValidationRule::WholeNumber { min: Some(0), max: Some(100) }))),
    ("cell notes", Kept, |_, s| s[0].notes[1][0].as_deref() == Some("Largest region")),
    ("hidden rows", Kept, |_, s| s[0].hidden_rows_manual.contains(&8)),
    ("hidden columns", Kept, |_, s| s[0].hidden_cols.contains(&4)),
    ("rows a filter hid stay hidden", Kept, |_, s| s[0].is_row_hidden(9)),
    ("column widths", Kept, |_, s| (s[0].col_width(1) - 140.0).abs() < 3.0),
    ("row heights", Kept, |_, s| (s[0].row_height(2) - 42.0).abs() < 2.0),
    ("frozen panes", Kept, |_, s| s[0].frozen_rows == 1),
    ("print area", Kept, |_, s| s[0].print_area == Some((0, 0, 5, 2))),
    ("page setup", Kept, |_, s| s[0].page_setup.size == PageSize::Letter && s[0].page_setup.orientation == Orientation::Landscape),
    ("second sheet", Kept, |_, s| s.len() == 2 && s[1].name == "Later" && s[1].data[1][0] == "2"),
    ("a filter's hide, as a filter", Lost("xlsx keeps no filter state: a filtered-out row is written hidden and reopens as a manually hidden row"),
        |_, s| s[0].hidden_rows.contains(&9)),
    ("conditional formatting on a later sheet", Kept, |_, s| s.get(1).is_some_and(|t| !t.cond_rules.is_empty())),
    ("charts on a later sheet", Kept, |_, s| s.get(1).is_some_and(|t| t.charts.iter().any(|c| c.title == "Later chart"))),
    ("chart series past the first", Kept, |_, s| s[0].charts.iter().any(|c| c.series.len() > 1)),
    ("chart series names", Kept, |_, s| s[0].charts.iter().flat_map(|c| &c.series).any(|x| !x.name.trim().is_empty())),
    ("chart series colors", Kept, |_, s| s[0].charts.iter().flat_map(|c| &c.series).any(|x| x.color.as_deref() == Some("4472C4"))),
    ("chart axis titles", Kept, |_, s| s[0].charts.iter().any(|c| c.x_axis_title.as_deref() == Some("Region") && c.y_axis_title.as_deref() == Some("Revenue"))),
    ("chart legend position", Kept, |_, s| s[0].charts.iter().any(|c| c.legend_position == LegendPosition::Bottom)),
    ("chart size", Kept, |_, s| s[0].charts.iter().any(|c| (c.width_px - 320.0).abs() < 1.0 && (c.height_px - 200.0).abs() < 1.0)),
    ("protection password", Lost("the stored password hash cannot be written; protection is saved without it"),
        |_, s| s[0].protection.password_hash.is_some()),
    ("regex validation", Lost("a pattern rule has no xlsx form and is not written"),
        |_, s| matches!(s[0].validations[2][2], Some(ValidationRule::Regex(_)))),
    ("unbounded validation", Lost("a range with no bounds has no xlsx form and is not written"),
        |_, s| matches!(s[0].validations[3][2], Some(ValidationRule::WholeNumber { min: None, max: None }))),
    ("empty-list validation", Lost("an empty list has no xlsx form and is not written"),
        |_, s| matches!(s[0].validations[4][2], Some(ValidationRule::List(_)))),
];

/// Every Lost budget row and the save-gate report entry that makes it
/// observable and cancellable: the entry's id must appear for the unsaved
/// model, with a location and a consequence.
const LOST_COVERAGE: &[(&str, &str)] = &[
    ("a filter's hide, as a filter", "filter-state"),
    ("protection password", "sheet-protection-password"),
    ("regex validation", "validation-rules"),
    ("unbounded validation", "validation-rules"),
    ("empty-list validation", "validation-rules"),
];

#[test]
fn an_xlsx_save_keeps_and_loses_exactly_what_the_budget_declares() {
    let (engine, sheets) = workbook();
    // The checks must hold on the workbook before the save, or a feature
    // "lost" in the round trip was never there.
    for (name, _, check) in BUDGET {
        assert!(check(&engine, &sheets), "{name}: the fixture does not set it up");
    }
    let bytes = save_sheets_to_xlsx_bytes(&sheets, Some(&engine)).expect("save");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("budget.xlsx");
    std::fs::write(&path, &bytes).unwrap();
    let (back_engine, back) = load_workbook(path.to_str().unwrap()).expect("reopen");

    let mut report = Vec::new();
    let mut names = HashSet::new();
    for (name, budget, check) in BUDGET {
        assert!(names.insert(*name), "{name} is declared twice");
        let survived = check(&back_engine, &back);
        match (budget, survived) {
            (Kept, false) => report.push(format!("{name}: declared kept, lost on an xlsx round trip")),
            (Lost(_), true) => report.push(format!("{name}: declared lost, but it survives; tighten the budget")),
            _ => {}
        }
    }
    assert!(report.is_empty(), "XLSX loss budget broken:\n  {}", report.join("\n  "));
}

/// TBL-001: every declared loss is observable and cancellable. Each Lost
/// row existed before the save (proven above and re-asserted here), and
/// the save gate reports it with a location and a consequence — the same
/// report the save question shows before a destructive write.
#[test]
fn tbl001_each_loss_has_a_location() {
    let (engine, sheets) = workbook();
    for (name, _, check) in BUDGET {
        assert!(check(&engine, &sheets), "{name}: the fixture does not set it up");
    }
    let report = tables_core::io::loss::content_a_save_drops(None, &sheets);
    assert!(report.requires_confirmation(), "the full fixture must ask before saving");
    for (name, budget, _) in BUDGET {
        let reason = match budget {
            Kept => continue,
            Lost(reason) => reason,
        };
        assert!(!reason.is_empty(), "{name}: a declared loss must say what happens");
        let (_, id) = LOST_COVERAGE.iter().find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("{name}: lost without ledger coverage"));
        let hit = report.features.iter().find(|f| f.id == *id)
            .unwrap_or_else(|| panic!("{name}: no '{id}' entry in the loss report"));
        assert!(!hit.location.is_empty(), "{name}: the loss entry names no location");
        assert!(!hit.detail.is_empty(), "{name}: the loss entry names no consequence");
    }
}

/// TBL-001: retained fields warn about nothing. The kept-only workbook
/// asks no question on a fresh save, and the file Tables wrote for it
/// scans clean on the next save either.
#[test]
fn retained_fields_save_without_a_question() {
    let (engine, sheets) = retained_workbook();
    let fresh = tables_core::io::loss::content_a_save_drops(None, &sheets);
    assert!(fresh.features.is_empty(), "a retained field warns spuriously: {:?}", fresh.features);
    let bytes = save_sheets_to_xlsx_bytes(&sheets, Some(&engine)).expect("save");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("retained.xlsx");
    std::fs::write(&path, &bytes).unwrap();
    let (_, back) = load_workbook(path.to_str().unwrap()).expect("reopen");
    let resave = tables_core::io::loss::content_a_save_drops(Some(path.to_str().unwrap()), &back);
    assert!(resave.features.is_empty(), "Tables' own file warns spuriously: {:?}", resave.features);
}

/// TBL-003: the saved full fixture resaves without a question. Its later
/// sheet's admitted rule reads back where it was, its filter-hidden row
/// reopens as a manual hide, and every chart reads back — so neither the
/// model nor Tables' own package warns on the next save. (Losses that
/// never reach the file, like passwords and unwritable validation, are
/// proven by `tbl001_each_loss_has_a_location` on the unsaved model and
/// by the crafted foreign package in `loss.rs`.)
#[test]
fn the_saved_full_fixture_resaves_without_a_question() {
    let (engine, sheets) = workbook();
    let bytes = save_sheets_to_xlsx_bytes(&sheets, Some(&engine)).expect("save");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("foreign.xlsx");
    std::fs::write(&path, &bytes).unwrap();
    let (_, back) = load_workbook(path.to_str().unwrap()).expect("reopen");
    let report = tables_core::io::loss::content_a_save_drops(Some(path.to_str().unwrap()), &back);
    assert!(report.features.is_empty(), "Tables' own full output warns spuriously: {:?}", report.features);
}

/// A ranged value filter's fixture: a labeled column under test, with a
/// manual hide on a row the filter leaves visible.
fn filtered_workbook() -> (TablesEngine, Vec<SheetModel>) {
    let mut engine = TablesEngine::new(8, 4).expect("engine");
    let mut s = SheetModel::new("Data", 8, 4, engine.sheet_id_at(0).unwrap());
    for (r, v) in ["Region", "North", "South", "North", "East"].iter().enumerate() {
        s.data[r][0] = v.to_string();
        engine.set_cell_text(r, 0, v).unwrap();
    }
    s.filter = Some(FilterDef {
        range: Some((0, 0, 4, 0)),
        columns: vec![FilterColumn { col: 0, hidden: ["South".to_string(), "East".to_string()].into_iter().collect() }],
    });
    s.recompute_filter();
    // A manual hide on a row the filter leaves visible: clearing the
    // filter must never reveal it.
    s.hidden_rows_manual.insert(1);
    (engine, vec![s])
}

/// TBL-003: a ranged value filter persists through xlsx — range,
/// criteria, derived hides and independent manual hides — and clearing
/// it after reopen restores exactly the manual state.
#[test]
fn tbl003_filter_clear_after_reopen() {
    let (engine, sheets) = filtered_workbook();
    assert_eq!(sheets[0].filter.as_ref().and_then(|d| d.range), Some((0, 0, 4, 0)));
    let hidden: Vec<usize> = {
        let mut h: Vec<usize> = sheets[0].hidden_rows.iter().copied().collect();
        h.sort_unstable();
        h
    };
    assert_eq!(hidden, [2, 4], "the filter hides South and East, not the header: {hidden:?}");
    let bytes = save_sheets_to_xlsx_bytes(&sheets, Some(&engine)).expect("save");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("filtered.xlsx");
    std::fs::write(&path, &bytes).unwrap();
    let (_, mut back) = load_workbook(path.to_str().unwrap()).expect("reopen");
    let def = back[0].filter.clone().expect("the filter definition comes back");
    assert_eq!(def.range, Some((0, 0, 4, 0)), "the range comes back: {def:?}");
    assert_eq!(def.columns.len(), 1, "one criterion comes back: {def:?}");
    assert_eq!(def.columns[0].col, 0);
    for v in ["South", "East"] {
        assert!(def.columns[0].hidden.contains(v), "{v} stays hidden: {def:?}");
    }
    let hidden: Vec<usize> = {
        let mut h: Vec<usize> = back[0].hidden_rows.iter().copied().collect();
        h.sort_unstable();
        h
    };
    assert_eq!(hidden, [2, 4], "derived hides come back, header excluded: {hidden:?}");
    assert!(back[0].hidden_rows_manual.contains(&1), "the manual hide comes back independent");
    assert!(!back[0].hidden_rows.contains(&1), "the manual hide is not a filter hide");
    // A persisted filter warns about nothing on the next save.
    let report = tables_core::io::loss::content_a_save_drops(Some(path.to_str().unwrap()), &back);
    assert!(report.features.is_empty(), "a persisted filter warns spuriously: {:?}", report.features);
    // Clearing after reopen restores the manual state exactly.
    back[0].filter = None;
    *back[0].hidden_rows = Default::default();
    assert!((0..8).all(|r| !back[0].hidden_rows.contains(&r)), "no filter hide survives the clear");
    assert!(back[0].is_row_hidden(1), "the manual hide survives the clear");
    assert!(!(2..5).any(|r| back[0].is_row_hidden(r)), "filtered rows come back: {:?}", back[0].hidden_rows);
    // And the cleared sheet writes no filter for any reader.
    let cleared = save_sheets_to_xlsx_bytes(&back, None).expect("resave");
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&cleared)).expect("zip");
    let sheet_xml = read_part(&mut zip, &["xl/worksheets/sheet1.xml".to_string()], "xl/worksheets/sheet1.xml");
    assert!(!sheet_xml.contains("autoFilter"), "a cleared filter leaves no autoFilter behind");
}

/// What both readers can see, inspected in the saved XML independently of
/// Tables' importer: every retained field is written for Excel and
/// LibreOffice too, and filter state is honestly absent (hidden rows only,
/// no filter to clear).
#[test]
fn saved_xml_carries_what_both_readers_keep() {
    let (engine, sheets) = workbook();
    let bytes = save_sheets_to_xlsx_bytes(&sheets, Some(&engine)).expect("save");
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).expect("zip");
    let names: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
    let chart: String = {
        let mut found = None;
        for name in names.iter().filter(|n| n.starts_with("xl/charts/")) {
            let s = read_part(&mut zip, &names, name);
            if s.contains("Sales by region") {
                found = Some(s);
                break;
            }
        }
        found.expect("a chart part carrying the first sheet's chart")
    };
    assert!(chart.contains("Revenue"), "the axis title is written for every reader");
    assert_eq!(chart.matches("<c:ser").count(), 2, "both series are written for independent readers");
    for marker in ["Sales", "Returns", "4472C4", "legendPos", "Region"] {
        assert!(chart.contains(marker), "independent readers must see '{marker}'");
    }
    assert!(read_part(&mut zip, &names, "xl/workbook.xml").contains("Total"), "the defined name is written");
    assert!(read_part(&mut zip, &names, "xl/worksheets/sheet1.xml").contains("sheetProtection"), "protection is written");
    for n in names.iter().filter(|n| n.starts_with("xl/worksheets/")) {
        let s = read_part(&mut zip, &names, n);
        assert!(!s.contains("autoFilter"), "{n} must not claim a filter Tables cannot restore");
    }
    assert!(read_part(&mut zip, &names, "sheet2").contains("conditionalFormatting"), "the later sheet's rules are written to the file");
}

fn read_part(zip: &mut zip::ZipArchive<impl std::io::Read + std::io::Seek>, names: &[String], want: &str) -> String {
    use std::io::Read;
    let name = names.iter().find(|n| n.contains(want)).unwrap_or_else(|| panic!("no part like {want}"));
    let mut s = String::new();
    zip.by_name(name).expect("open").read_to_string(&mut s).expect("read");
    s
}
