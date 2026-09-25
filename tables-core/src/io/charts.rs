// charts.rs — read embedded charts back from xlsx.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of io.rs (issue #247).

// io.rs — Spreadsheet file I/O: xlsx/ods/csv/tsv import, xlsx export.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Extracted from tables/src/window.rs so it is unit-testable without GTK.


/// `xml` with every element name's namespace prefix removed (`<c:f>` →
/// `<f>`, `</xdr:from>` → `</from>`); attributes are left alone. Prefixes
/// are the producer's choice: we and Calc write `c:`/`xdr:`, openpyxl writes
/// a default namespace and no prefixes at all, and a reader matching literal
/// prefixed tags found no chart in its files (render lab `tables/chart`).
fn unprefix(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..=i]);
        rest = &rest[i + 1..];
        if let Some(r) = rest.strip_prefix('/') {
            out.push('/');
            rest = r;
        }
        let name_end = rest
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(rest.len());
        if let Some(colon) = rest[..name_end].find(':') {
            rest = &rest[colon + 1..];
        }
    }
    out.push_str(rest);
    out
}

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
    // Bounded reads: best-effort must not extend to decompressing whatever
    // an untrusted package asks for (#442).
    use suite_common_core::zip_guard::{BoundedArchive, ZipBudget};
    let mut budget = ZipBudget::default();
    if budget.check_entry_count(zip.len()).is_err() {
        return Vec::new();
    }

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

    // Anchors from drawing parts, in document order, with the size a
    // one-cell anchor states (`<xdr:ext cx cy>`, EMU; 9525 EMU = 1 px).
    type Anchor = ((usize, usize), Option<(f64, f64)>);
    let mut anchors: Vec<Anchor> = Vec::new();
    let drawing_names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .filter(|n| n.starts_with("xl/drawings/drawing") && n.ends_with(".xml"))
        .collect();
    for name in &drawing_names {
        let Ok(xml) = zip.part_to_string(name, &mut budget) else {
            continue;
        };
        let xml = unprefix(&xml);
        // First <from> per anchor: <xdr:col>N</xdr:col><xdr:row>N</xdr:row>
        for from in xml.split("<from>").skip(1) {
            let grab = |tag: &str| -> Option<usize> {
                let open = format!("<{tag}>");
                let close = format!("</{tag}>");
                from.split(&open)
                    .nth(1)?
                    .split(&close)
                    .next()?
                    .trim()
                    .parse()
                    .ok()
            };
            if let (Some(c), Some(r)) = (grab("col"), grab("row")) {
                // The anchor's own <xdr:ext>, before the next anchor starts.
                let size = from
                    .split("</oneCellAnchor>")
                    .next()
                    .filter(|a| !a.contains("<to>"))
                    .and_then(|a| a.split("<ext ").nth(1))
                    .and_then(|ext| {
                        let attr = |k: &str| -> Option<f64> {
                            ext.split(&format!("{k}=\"")).nth(1)?.split('"').next()?.parse().ok()
                        };
                        Some((attr("cx")? / 9525.0, attr("cy")? / 9525.0))
                    })
                    .filter(|(w, h)| *w >= 16.0 && *h >= 16.0 && *w <= 20_000.0 && *h <= 20_000.0);
                anchors.push(((r, c), size));
            }
        }
    }

    let chart_names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .filter(|n| n.starts_with("xl/charts/chart") && n.ends_with(".xml"))
        .collect();
    let mut out = Vec::new();
    for (ci, name) in chart_names.iter().enumerate() {
        let Ok(xml) = zip.part_to_string(name, &mut budget) else {
            continue;
        };
        let xml = unprefix(&xml);
        let kind = if xml.contains("<barChart") {
            ChartKind::Bar
        } else if xml.contains("<lineChart") {
            ChartKind::Line
        } else if xml.contains("<pieChart") {
            ChartKind::Pie
        } else if xml.contains("<scatterChart") {
            ChartKind::Scatter
        } else if xml.contains("<areaChart") {
            ChartKind::Area
        } else {
            continue;
        };
        let grab_f = |section: &str| -> Option<String> {
            xml.split(section)
                .nth(1)?
                .split("<f>")
                .nth(1)?
                .split("</f>")
                .next()
                .map(str::to_string)
        };
        // An XY chart's series has x and y values where the others have
        // categories and values.
        let cat = grab_f("<cat>").or_else(|| grab_f("<xVal>")).and_then(|r| parse_range(&r));
        let Some(val) = grab_f("<val>").or_else(|| grab_f("<yVal>")).and_then(|r| parse_range(&r)) else {
            continue;
        };
        // Title: first a:t inside c:title.
        let title = xml
            .split("<title>")
            .nth(1)
            .and_then(|t| t.split("</title>").next())
            .and_then(|t| t.split("<t>").nth(1))
            .and_then(|t| t.split("</t>").next())
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
            anchor: anchors.get(ci).map(|a| a.0).unwrap_or((0, 0)),
            width_px: anchors.get(ci).and_then(|a| a.1).map_or(480.0, |s| s.0),
            height_px: anchors.get(ci).and_then(|a| a.1).map_or(280.0, |s| s.1),
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
    fn element_prefixes_are_removed_and_attributes_kept() {
        assert_eq!(
            unprefix(r#"<c:chart r:id="rId1"><c:f>'S'!$A$1</c:f></c:chart><xdr:ext cx="1"/>"#),
            r#"<chart r:id="rId1"><f>'S'!$A$1</f></chart><ext cx="1"/>"#
        );
        assert_eq!(unprefix("<?xml v?><a b='c:d'/>"), "<?xml v?><a b='c:d'/>");
    }

    /// openpyxl writes chart and drawing parts with a default namespace
    /// (`<barChart>`, `<from>`), not `c:`/`xdr:` prefixes.
    #[test]
    fn an_unprefixed_chart_and_its_one_cell_anchor_size_are_read() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.xlsx");
        {
            let f = std::fs::File::create(&path).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("xl/drawings/drawing1.xml", o).unwrap();
            z.write_all(br#"<wsDr xmlns="x"><oneCellAnchor><from><col>3</col><colOff>0</colOff><row>1</row><rowOff>0</rowOff></from><ext cx="3240000" cy="2160000"/></oneCellAnchor></wsDr>"#).unwrap();
            z.start_file("xl/charts/chart1.xml", o).unwrap();
            z.write_all(br#"<chartSpace xmlns="c"><chart><plotArea><barChart><ser><cat><numRef><f>'Sheet'!$A$1:$A$4</f></numRef></cat><val><numRef><f>'Sheet'!$B$1:$B$4</f></numRef></val></ser></barChart></plotArea></chart></chartSpace>"#).unwrap();
            z.finish().unwrap();
        }
        let charts = read_charts_from_xlsx(path.to_str().unwrap());
        assert_eq!(charts.len(), 1);
        let c = &charts[0];
        assert_eq!((c.kind, c.cat, c.val, c.anchor), (ChartKind::Bar, (0, 0, 3), (0, 1, 3), (1, 3)));
        assert_eq!((c.width_px.round(), c.height_px.round()), (340.0, 227.0), "9 x 6 cm");
    }

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
    fn every_kind_round_trips_with_its_ranges() {
        for kind in [ChartKind::Line, ChartKind::Pie, ChartKind::Scatter, ChartKind::Area] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("c.xlsx");
            let mut sh = SheetModel::new("Sheet1", 6, 3, 0);
            for r in 0..4 {
                sh.data[r][0] = format!("{}", r + 1);
                sh.data[r][1] = format!("{}", (r * 7) % 5);
            }
            sh.charts.push(ChartSpec {
                kind,
                title: String::new(),
                x_axis_title: None,
                y_axis_title: None,
                legend_position: LegendPosition::Right,
                series: Vec::new(),
                cat: (0, 0, 3),
                val: (0, 1, 3),
                anchor: (2, 2),
                width_px: 480.0,
                height_px: 280.0,
            });
            save_sheets_to_xlsx(path.to_str().unwrap(), &[sh]).unwrap();
            let charts = read_charts_from_xlsx(path.to_str().unwrap());
            assert_eq!(charts.len(), 1, "{kind:?}");
            let c = &charts[0];
            assert_eq!((c.kind, c.cat, c.val, c.anchor), (kind, (0, 0, 3), (0, 1, 3), (2, 2)), "{kind:?}");
        }
    }

    /// A scatter chart's series names x and y values (`<c:xVal>`,
    /// `<c:yVal>`), not categories and values, as openpyxl and Calc write it.
    #[test]
    fn a_scatter_chart_reads_its_x_and_y_ranges() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.xlsx");
        {
            let f = std::fs::File::create(&path).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("xl/charts/chart1.xml", o).unwrap();
            z.write_all(
                br#"<c:chartSpace xmlns:c="c"><c:chart><c:plotArea><c:scatterChart><c:ser>
                <c:xVal><c:numRef><c:f>'Sheet'!$A$1:$A$4</c:f></c:numRef></c:xVal>
                <c:yVal><c:numRef><c:f>'Sheet'!$B$1:$B$4</c:f></c:numRef></c:yVal>
                </c:ser></c:scatterChart></c:plotArea></c:chart></c:chartSpace>"#,
            )
            .unwrap();
            z.finish().unwrap();
        }
        let charts = read_charts_from_xlsx(path.to_str().unwrap());
        assert_eq!(charts.len(), 1);
        assert_eq!((charts[0].kind, charts[0].cat, charts[0].val), (ChartKind::Scatter, (0, 0, 3), (0, 1, 3)));
    }
}
