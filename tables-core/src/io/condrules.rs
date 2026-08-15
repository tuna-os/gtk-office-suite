// condrules.rs — read conditional-formatting rules back from xlsx.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of io.rs (issue #247).

// io.rs — Spreadsheet file I/O: xlsx/ods/csv/tsv import, xlsx export.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Extracted from tables/src/window.rs so it is unit-testable without GTK.


/// Read cell-value conditional-formatting rules back from an xlsx.
/// Best-effort: only the rule shapes we write (and Calc's rewrites of
/// them) are recognized.
pub fn read_cond_rules_from_xlsx(path: &str) -> Vec<crate::sheet::CondRule> {
    use crate::sheet::{parse_cell_ref, CondOp, CondRule};
    let Ok(f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let Ok(mut zip) = zip::ZipArchive::new(f) else {
        return Vec::new();
    };
    use std::io::Read as _;

    // dxf index → fill RGB, from styles.xml (order of <dxf> elements).
    let mut dxf_fills: Vec<Option<String>> = Vec::new();
    {
        let mut styles = String::new();
        if zip
            .by_name("xl/styles.xml")
            .map(|mut f| f.read_to_string(&mut styles))
            .is_ok()
        {
            if let Some(dxfs) = styles.split("<dxfs").nth(1) {
                let dxfs = dxfs.split("</dxfs>").next().unwrap_or("");
                for dxf in dxfs.split("<dxf>").skip(1) {
                    let fill = dxf
                        .split("rgb=\"")
                        .nth(1)
                        .and_then(|t| t.split('"').next())
                        .map(|c| {
                            let c = c.to_uppercase();
                            // ARGB → RGB: drop the alpha byte only.
                            if c.len() == 8 {
                                c[2..].to_string()
                            } else {
                                c
                            }
                        });
                    dxf_fills.push(fill);
                }
            }
        }
    }

    let mut sheet_xml = String::new();
    if zip
        .by_name("xl/worksheets/sheet1.xml")
        .map(|mut f| f.read_to_string(&mut sheet_xml))
        .is_err()
    {
        return Vec::new();
    }

    let mut out = Vec::new();
    for block in sheet_xml.split("<conditionalFormatting").skip(1) {
        let block = block
            .split("</conditionalFormatting>")
            .next()
            .unwrap_or(block);
        let Some(sqref) = block
            .split("sqref=\"")
            .nth(1)
            .and_then(|t| t.split('"').next())
        else {
            continue;
        };
        // First range in the sqref (we write exactly one).
        let range = sqref.split(' ').next().unwrap_or(sqref);
        let (a, b) = range.split_once(':').unwrap_or((range, range));
        let (Some((r0, c0)), Some((r1, c1))) = (
            parse_cell_ref(&a.replace('$', "")),
            parse_cell_ref(&b.replace('$', "")),
        ) else {
            continue;
        };
        for rule_xml in block.split("<cfRule").skip(1) {
            if !rule_xml.contains("type=\"cellIs\"") {
                continue;
            }
            let attr = |name: &str| -> Option<String> {
                rule_xml
                    .split(&format!("{name}=\""))
                    .nth(1)
                    .and_then(|t| t.split('"').next())
                    .map(str::to_string)
            };
            let formulas: Vec<f64> = rule_xml
                .split("<formula>")
                .skip(1)
                .filter_map(|t| t.split("</formula>").next())
                .filter_map(|v| v.trim().parse().ok())
                .collect();
            let Some(v0) = formulas.first().copied() else {
                continue;
            };
            let op = match attr("operator").as_deref() {
                Some("greaterThan") => CondOp::Greater,
                Some("lessThan") => CondOp::Less,
                Some("equal") => CondOp::Equal,
                Some("between") => CondOp::Between,
                _ => continue,
            };
            let fill = attr("dxfId")
                .and_then(|id| id.parse::<usize>().ok())
                .and_then(|id| dxf_fills.get(id).cloned().flatten())
                .unwrap_or_else(|| "FFFF00".into());
            out.push(CondRule {
                range: (r0.min(r1), c0.min(c1), r0.max(r1), c1.max(c0)),
                op,
                value: v0,
                value2: formulas.get(1).copied().unwrap_or(v0),
                fill,
            });
        }
    }
    out
}

#[cfg(test)]
mod cond_tests {
    use super::*;
    use crate::io::*;
    use crate::sheet::SheetModel;
    use crate::sheet::{CondOp, CondRule};

    #[test]
    fn cond_rule_round_trips_through_xlsx() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cf.xlsx");
        let mut sh = SheetModel::new("Sheet1", 10, 5, 0);
        for r in 0..5 {
            sh.data[r][1] = format!("{}", r * 10);
        }
        sh.cond_rules.push(CondRule {
            range: (0, 1, 4, 1),
            op: CondOp::Greater,
            value: 25.0,
            value2: 25.0,
            fill: "FFC0C0".into(),
        });
        save_sheets_to_xlsx(path.to_str().unwrap(), &[sh]).unwrap();
        let rules = read_cond_rules_from_xlsx(path.to_str().unwrap());
        assert_eq!(rules.len(), 1, "rule missing");
        let r = &rules[0];
        assert_eq!(r.op, CondOp::Greater);
        assert_eq!(r.range, (0, 1, 4, 1), "range lost: {r:?}");
        assert!((r.value - 25.0).abs() < 1e-9);
        assert_eq!(r.fill, "FFC0C0", "fill lost: {r:?}");
    }

    #[test]
    fn between_rule_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cf2.xlsx");
        let mut sh = SheetModel::new("Sheet1", 5, 3, 0);
        sh.data[0][0] = "5".into();
        sh.cond_rules.push(CondRule {
            range: (0, 0, 3, 0),
            op: CondOp::Between,
            value: 2.0,
            value2: 8.0,
            fill: "C0FFC0".into(),
        });
        save_sheets_to_xlsx(path.to_str().unwrap(), &[sh]).unwrap();
        let rules = read_cond_rules_from_xlsx(path.to_str().unwrap());
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].op, CondOp::Between);
        assert!((rules[0].value - 2.0).abs() < 1e-9 && (rules[0].value2 - 8.0).abs() < 1e-9);
    }

    #[test]
    fn rule_matching_logic() {
        let r = CondRule {
            range: (0, 0, 2, 2),
            op: CondOp::Between,
            value: 10.0,
            value2: 20.0,
            fill: "FFFF00".into(),
        };
        assert!(r.matches(15.0) && !r.matches(25.0));
        assert!(r.contains(1, 1) && !r.contains(3, 0));
    }
}
