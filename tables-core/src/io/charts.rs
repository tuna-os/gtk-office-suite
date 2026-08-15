// charts.rs — read embedded charts back from xlsx.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of io.rs (issue #247).

// io.rs — Spreadsheet file I/O: xlsx/ods/csv/tsv import, xlsx export.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Extracted from tables/src/window.rs so it is unit-testable without GTK.


/// Read embedded charts back from an xlsx (ours or a Calc rewrite).
/// Best-effort: unknown chart kinds and foreign anchoring are skipped.
pub fn read_charts_from_xlsx(path: &str) -> Vec<crate::sheet::ChartSpec> {
    use crate::sheet::{parse_cell_ref, ChartKind, ChartSpec, LegendPosition};
    let Ok(f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let Ok(mut zip) = zip::ZipArchive::new(f) else {
        return Vec::new();
    };

    // A1-style absolute range "Sheet1!$A$2:$A$5" → (first_row, col, last_row).
    fn parse_range(r: &str) -> Option<(usize, usize, usize)> {
        let range = r.rsplit('!').next()?.replace('$', "");
        let (a, b) = range
            .split_once(':')
            .unwrap_or((range.as_str(), range.as_str()));
        let (r0, c0) = parse_cell_ref(a)?;
        let (r1, c1) = parse_cell_ref(b)?;
        if c0 != c1 {
            return None; // single-column series only (what we write)
        }
        Some((r0.min(r1), c0, r0.max(r1)))
    }

    // Anchors from drawing parts, in document order.
    let mut anchors: Vec<(usize, usize)> = Vec::new();
    let drawing_names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .filter(|n| n.starts_with("xl/drawings/drawing") && n.ends_with(".xml"))
        .collect();
    for name in &drawing_names {
        let mut xml = String::new();
        use std::io::Read as _;
        if zip
            .by_name(name)
            .map(|mut f| f.read_to_string(&mut xml))
            .is_err()
        {
            continue;
        }
        // First <xdr:from> per anchor: <xdr:col>N</xdr:col><xdr:row>N</xdr:row>
        for from in xml.split("<xdr:from>").skip(1) {
            let grab = |tag: &str| -> Option<usize> {
                let open = format!("<xdr:{tag}>");
                let close = format!("</xdr:{tag}>");
                from.split(&open)
                    .nth(1)?
                    .split(&close)
                    .next()?
                    .trim()
                    .parse()
                    .ok()
            };
            if let (Some(c), Some(r)) = (grab("col"), grab("row")) {
                anchors.push((r, c));
            }
        }
    }

    let chart_names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .filter(|n| n.starts_with("xl/charts/chart") && n.ends_with(".xml"))
        .collect();
    let mut out = Vec::new();
    for (ci, name) in chart_names.iter().enumerate() {
        let mut xml = String::new();
        use std::io::Read as _;
        if zip
            .by_name(name)
            .map(|mut f| f.read_to_string(&mut xml))
            .is_err()
        {
            continue;
        }
        let kind = if xml.contains("<c:barChart") {
            ChartKind::Bar
        } else if xml.contains("<c:lineChart") {
            ChartKind::Line
        } else if xml.contains("<c:pieChart") {
            ChartKind::Pie
        } else {
            continue;
        };
        let grab_f = |section: &str| -> Option<String> {
            xml.split(section)
                .nth(1)?
                .split("<c:f>")
                .nth(1)?
                .split("</c:f>")
                .next()
                .map(str::to_string)
        };
        let cat = grab_f("<c:cat>").and_then(|r| parse_range(&r));
        let Some(val) = grab_f("<c:val>").and_then(|r| parse_range(&r)) else {
            continue;
        };
        // Title: first a:t inside c:title.
        let title = xml
            .split("<c:title>")
            .nth(1)
            .and_then(|t| t.split("</c:title>").next())
            .and_then(|t| t.split("<a:t>").nth(1))
            .and_then(|t| t.split("</a:t>").next())
            .unwrap_or("")
            .to_string();
        out.push(ChartSpec {
            kind,
            title,
            x_axis_title: None,
            y_axis_title: None,
            legend_position: LegendPosition::Right,
            series: Vec::new(),
            cat: cat.unwrap_or(val),
            val,
            anchor: anchors.get(ci).copied().unwrap_or((0, 0)),
            width_px: 480.0,
            height_px: 280.0,
        });
    }
    out
}

#[cfg(test)]
mod chart_tests {
    use super::*;
    use crate::io::*;
    use crate::sheet::SheetModel;
    use crate::sheet::{ChartKind, ChartSpec, LegendPosition};

    #[test]
    fn chart_round_trips_through_xlsx() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.xlsx");
        let mut sh = SheetModel::new("Sheet1", 10, 5, 0);
        for (i, (l, v)) in [("North", 12.0), ("South", 9.5), ("East", 14.0)]
            .iter()
            .enumerate()
        {
            sh.data[i + 1][0] = l.to_string();
            sh.data[i + 1][1] = v.to_string();
        }
        sh.charts.push(ChartSpec {
            kind: ChartKind::Bar,
            title: "Regions".into(),
            x_axis_title: None,
            y_axis_title: None,
            legend_position: LegendPosition::Right,
            series: Vec::new(),
            cat: (1, 0, 3),
            val: (1, 1, 3),
            anchor: (5, 3),
            width_px: 480.0,
            height_px: 280.0,
        });
        save_sheets_to_xlsx(path.to_str().unwrap(), &[sh]).unwrap();
        let charts = read_charts_from_xlsx(path.to_str().unwrap());
        assert_eq!(charts.len(), 1, "chart part missing");
        let c = &charts[0];
        assert_eq!(c.kind, ChartKind::Bar);
        assert_eq!(c.val, (1, 1, 3), "values range lost: {c:?}");
        assert_eq!(c.cat, (1, 0, 3), "categories range lost: {c:?}");
        assert_eq!(c.title, "Regions");
        assert_eq!(c.anchor, (5, 3));
    }

    #[test]
    fn line_and_pie_kinds_round_trip() {
        for kind in [ChartKind::Line, ChartKind::Pie] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("c.xlsx");
            let mut sh = SheetModel::new("Sheet1", 6, 3, 0);
            sh.data[0][0] = "A".into();
            sh.data[0][1] = "1".into();
            sh.charts.push(ChartSpec {
                kind,
                title: String::new(),
                x_axis_title: None,
                y_axis_title: None,
                legend_position: LegendPosition::Right,
                series: Vec::new(),
                cat: (0, 0, 0),
                val: (0, 1, 0),
                anchor: (2, 0),
                width_px: 480.0,
                height_px: 280.0,
            });
            save_sheets_to_xlsx(path.to_str().unwrap(), &[sh]).unwrap();
            let charts = read_charts_from_xlsx(path.to_str().unwrap());
            assert_eq!(charts.len(), 1);
            assert_eq!(charts[0].kind, kind);
        }
    }
}
