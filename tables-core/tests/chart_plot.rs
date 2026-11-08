// chart_plot.rs — what a chart plots, read from the sheet (#1658).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The canvas used to draw a chart's first series alone, so a chart of Sales
// and Plan showed Sales. `ChartSpec::plot` reads every series the way the
// file names them; a single series agrees with `points`.

use tables_core::sheet::{ChartKind, ChartSeries, ChartSpec, LegendPosition, SheetModel};

fn sheet() -> SheetModel {
    let mut sh = SheetModel::new("Sheet1", 8, 4, 0);
    let rows = [("Q1", "3", "2"), ("Q2", "7", "5"), ("Q3", "5", "n/a"), ("Q4", "9", "4")];
    for (r, (q, sales, plan)) in rows.iter().enumerate() {
        sh.data[r + 1][0] = q.to_string();
        sh.data[r + 1][1] = sales.to_string();
        sh.data[r + 1][2] = plan.to_string();
    }
    sh
}

fn chart(series: Vec<ChartSeries>) -> ChartSpec {
    let first = series.first().cloned().unwrap();
    ChartSpec {
        kind: ChartKind::Bar,
        title: String::new(),
        x_axis_title: None,
        y_axis_title: None,
        legend_position: LegendPosition::Bottom,
        cat: first.cat,
        val: first.val,
        series,
        anchor: (1, 4),
        width_px: 480.0,
        height_px: 280.0,
    }
}

#[test]
fn every_series_is_plotted_under_its_name_over_the_first_series_categories() {
    let sh = sheet();
    let c = chart(vec![
        ChartSeries { name: "Sales".into(), cat: (1, 0, 4), val: (1, 1, 4), color: None },
        ChartSeries { name: "Plan".into(), cat: (1, 0, 4), val: (1, 2, 4), color: None },
    ]);
    let (categories, series) = c.plot(&sh);
    assert_eq!(categories, ["Q1", "Q2", "Q3", "Q4"]);
    assert_eq!(series.len(), 2, "a chart of two series draws two");
    assert_eq!(series[0], ("Sales".to_string(), vec![3.0, 7.0, 5.0, 9.0]));
    // A cell that isn't a number plots as 0, as in Excel.
    assert_eq!(series[1], ("Plan".to_string(), vec![2.0, 5.0, 0.0, 4.0]));
}

#[test]
fn an_unnamed_series_is_called_by_its_value_column_as_calc_does() {
    let sh = sheet();
    let c = chart(vec![
        ChartSeries { name: "  ".into(), cat: (1, 0, 4), val: (1, 1, 4), color: None },
        ChartSeries { name: String::new(), cat: (1, 0, 4), val: (1, 2, 4), color: None },
    ]);
    let names: Vec<String> = c.plot(&sh).1.into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["Column B", "Column C"]);
}

#[test]
fn a_single_series_plots_what_points_reads() {
    let sh = sheet();
    let c = chart(vec![ChartSeries { name: String::new(), cat: (1, 0, 4), val: (1, 1, 4), color: None }]);
    let (categories, series) = c.plot(&sh);
    let points = c.points(&sh);
    assert_eq!(series.len(), 1);
    assert_eq!(categories, points.iter().map(|p| p.0.clone()).collect::<Vec<_>>());
    assert_eq!(series[0].1, points.iter().map(|p| p.1).collect::<Vec<_>>());
    assert_eq!(series[0].0, c.series_name());
}

#[test]
fn a_chart_built_before_the_series_vector_plots_through_its_legacy_ranges() {
    let sh = sheet();
    let mut c = chart(vec![ChartSeries { name: String::new(), cat: (1, 0, 4), val: (1, 1, 4), color: None }]);
    c.series.clear();
    let (categories, series) = c.plot(&sh);
    assert_eq!((categories.len(), series.len()), (4, 1));
    assert_eq!(series[0].1, vec![3.0, 7.0, 5.0, 9.0]);
}

// ── a chart from a selection (#1658) ────────────────────────────────────

fn table() -> SheetModel {
    let mut sh = SheetModel::new("Sheet1", 8, 5, 0);
    let rows = [
        ["", "Sales", "Plan", "Notes"],
        ["Q1", "3", "2", "a"],
        ["Q2", "7", "5", "b"],
        ["Q3", "5", "6", "c"],
    ];
    for (r, row) in rows.iter().enumerate() {
        for (c, v) in row.iter().enumerate() {
            sh.data[r][c] = v.to_string();
        }
    }
    sh
}

#[test]
fn a_selected_table_is_one_series_per_value_column_named_by_its_header() {
    let sh = table();
    let spec = ChartSpec::from_selection(&sh, (0, 0, 3, 2), ChartKind::Bar, (6, 0), 480.0, 280.0).unwrap();
    assert_eq!(spec.series.len(), 2);
    assert_eq!(spec.series[0].name, "Sales");
    assert_eq!(spec.series[1].name, "Plan");
    assert_eq!(spec.series[0].val, (1, 1, 3));
    assert_eq!(spec.series[1].cat, (1, 0, 3));
    let (cats, plotted) = spec.plot(&sh);
    assert_eq!(cats, ["Q1", "Q2", "Q3"]);
    assert_eq!(plotted, vec![("Sales".to_string(), vec![3.0, 7.0, 5.0]), ("Plan".to_string(), vec![2.0, 5.0, 6.0])]);
    // The legacy ranges mirror the first series.
    assert_eq!((spec.cat, spec.val), (spec.series[0].cat, spec.series[0].val));
}

#[test]
fn without_a_header_row_every_row_is_data_and_the_series_are_unnamed() {
    let sh = table();
    let spec = ChartSpec::from_selection(&sh, (1, 0, 3, 2), ChartKind::Line, (6, 0), 480.0, 280.0).unwrap();
    assert_eq!(spec.series.len(), 2);
    assert_eq!(spec.series[0].val, (1, 1, 3));
    assert!(spec.series.iter().all(|s| s.name.is_empty()));
}

#[test]
fn a_column_with_no_number_is_not_a_series() {
    let sh = table();
    let spec = ChartSpec::from_selection(&sh, (0, 0, 3, 3), ChartKind::Bar, (6, 0), 480.0, 280.0).unwrap();
    assert_eq!(spec.series.len(), 2, "the Notes column holds text only");
}

#[test]
fn one_selected_column_plots_over_the_first_columns_labels_as_before() {
    let sh = table();
    let spec = ChartSpec::from_selection(&sh, (2, 1, 2, 1), ChartKind::Pie, (6, 0), 480.0, 280.0).unwrap();
    assert_eq!(spec.series.len(), 1);
    assert_eq!(spec.series[0].cat, (1, 0, 3));
    assert_eq!(spec.series[0].val, (1, 1, 3));
}

#[test]
fn a_selection_with_nothing_to_plot_makes_no_chart() {
    let sh = table();
    assert!(ChartSpec::from_selection(&sh, (0, 0, 0, 2), ChartKind::Bar, (6, 0), 480.0, 280.0).is_none(), "a header row alone");
    assert!(ChartSpec::from_selection(&sh, (1, 3, 1, 3), ChartKind::Bar, (6, 0), 480.0, 280.0).is_none(), "a text column");
}
