// xlsx_loss_budget.rs — what an XLSX save keeps, declared and tested (#1204).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// tables-readiness.md: "Test formulas, cached values, styles, charts,
// rules, names, protection and hidden/filter state against a declared XLSX
// loss budget." The budget is BUDGET below: every feature Tables models,
// and whether a save to .xlsx followed by Tables' own open brings it back.
// One workbook carries all of them at once, through the real byte path
// (save_sheets_to_xlsx_bytes with the engine, then load_workbook).
//
// The test fails both ways. A feature declared Kept that comes back wrong
// is a regression. A feature declared Lost that comes back intact means
// the budget is stale: tighten it, so the declared losses stay exactly the
// real ones.

use std::collections::HashSet;
use suite_common_core::charts::ChartKind;
use suite_common_core::format::{NumberFormat, NumberFormatKind};
use tables_core::io::{load_workbook, save_sheets_to_xlsx_bytes};
use tables_core::sheet::{ChartSeries, ChartSpec, CondOp, CondRule, LegendPosition, SheetModel, ValidationRule};
use tables_core::style::Rgb;
use tables_core::TablesEngine;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Budget {
    Kept,
    /// Lost on an xlsx round trip through Tables, with the reason.
    Lost(&'static str),
}
use Budget::*;

const ROWS: usize = 12;
const COLS: usize = 6;

fn workbook() -> (TablesEngine, Vec<SheetModel>) {
    let mut engine = TablesEngine::new(ROWS, COLS).expect("engine");
    let mut s = SheetModel::new("Data", ROWS, COLS, engine.sheet_id_at(0).unwrap());
    for (r, (label, n)) in [("North", "10"), ("South", "20"), ("East", "30")].iter().enumerate() {
        s.data[r + 1][0] = label.to_string();
        s.data[r + 1][1] = n.to_string();
        engine.set_cell_text(r + 1, 0, label);
        engine.set_cell_text(r + 1, 1, n);
    }
    s.data[0][0] = "Region".into();
    engine.set_cell_text(0, 0, "Region");
    // A formula and its cached value.
    s.data[5][1] = "=SUM(B2:B4)".into();
    s.formulas[5][1] = true;
    engine.set_cell_text(5, 1, "=SUM(B2:B4)");
    engine.set_defined_name("Total", Some("Data!$B$6")).unwrap();
    engine.evaluate();
    // Styles and number format.
    s.styles[0][0].bold = true;
    s.styles[0][0].fill = Some(Rgb(0xFF, 0xC7, 0xCE));
    s.styles[0][0].font_family = Some("Liberation Serif".into());
    s.formats[1][1] = NumberFormat::new(NumberFormatKind::Currency("$".into(), 2));
    // Conditional formatting.
    s.cond_rules.push(CondRule { range: (1, 1, 3, 1), op: CondOp::Greater, value: 15.0, value2: 0.0, fill: "C6EFCE".into() });
    // A chart over the data.
    let series = ChartSeries { name: "Sales".into(), cat: (1, 0, 3), val: (1, 1, 3), color: None };
    s.charts.push(ChartSpec {
        kind: ChartKind::Bar,
        title: "Sales by region".into(),
        x_axis_title: None,
        y_axis_title: None,
        legend_position: LegendPosition::Right,
        series: vec![series],
        cat: (1, 0, 3),
        val: (1, 1, 3),
        anchor: (1, 3),
        width_px: 320.0,
        height_px: 200.0,
    });
    // Protection, validation, notes, hidden and filtered rows, print area.
    s.protection.protected = true;
    s.validations[1][2] = Some(ValidationRule::WholeNumber { min: Some(0), max: Some(100) });
    s.notes[1][0] = Some("Largest region".into());
    s.hidden_rows_manual.insert(8);
    s.hidden_cols.insert(4);
    s.hidden_rows.insert(9);
    s.print_area = Some((0, 0, 5, 2));
    // A second sheet with a chart and a rule of its own.
    engine.add_sheet("Later").unwrap();
    let mut t = SheetModel::new("Later", ROWS, COLS, engine.sheet_id_at(1).unwrap());
    t.data[0][0] = "1".into();
    t.data[1][0] = "2".into();
    t.cond_rules.push(CondRule { range: (0, 0, 1, 0), op: CondOp::Less, value: 2.0, value2: 0.0, fill: "FFEB9C".into() });
    let mut chart = s.charts[0].clone();
    chart.title = "Later chart".into();
    t.charts.push(chart);
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
    ("number formats", Kept, |_, s| matches!(s[0].formats[1][1].kind, NumberFormatKind::Currency(ref c, 2) if c == "$")),
    ("conditional formatting", Kept, |_, s| s[0].cond_rules.iter().any(|r| r.range == (1, 1, 3, 1) && r.op == CondOp::Greater && r.fill.eq_ignore_ascii_case("C6EFCE"))),
    ("charts", Kept, |_, s| s[0].charts.iter().any(|c| c.kind == ChartKind::Bar && c.title == "Sales by region")),
    ("sheet protection", Kept, |_, s| s[0].protection.protected),
    ("data validation", Kept, |_, s| matches!(s[0].validations[1][2], Some(ValidationRule::WholeNumber { min: Some(0), max: Some(100) }))),
    ("cell notes", Kept, |_, s| s[0].notes[1][0].as_deref() == Some("Largest region")),
    ("hidden rows", Kept, |_, s| s[0].hidden_rows_manual.contains(&8)),
    ("hidden columns", Kept, |_, s| s[0].hidden_cols.contains(&4)),
    ("rows a filter hid stay hidden", Kept, |_, s| s[0].is_row_hidden(9)),
    ("print area", Kept, |_, s| s[0].print_area == Some((0, 0, 5, 2))),
    ("second sheet", Kept, |_, s| s.len() == 2 && s[1].name == "Later" && s[1].data[1][0] == "2"),
    ("a filter's hide, as a filter", Lost("xlsx has no filter state here: a filtered-out row is written hidden and reopens as a manually hidden row"),
        |_, s| s[0].hidden_rows.contains(&9)),
    ("conditional formatting on a later sheet", Lost("the rule reader resolves the first worksheet only"),
        |_, s| s.get(1).is_some_and(|t| !t.cond_rules.is_empty())),
    ("charts on a later sheet", Lost("the chart reader resolves the first worksheet's drawing only"),
        |_, s| s.get(1).is_some_and(|t| !t.charts.is_empty())),
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
