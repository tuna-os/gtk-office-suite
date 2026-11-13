// condrules.rs — read conditional-formatting rules back from xlsx.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of io.rs (issue #247).

// io.rs — Spreadsheet file I/O: xlsx/ods/csv/tsv import, xlsx export.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Extracted from tables/src/window.rs so it is unit-testable without GTK.


/// What one rule read could not model exactly (TBL-003): skipped rule
/// shapes, defaulted styles and stop-if-true flags. The save-time loss
/// scan (io/loss.rs) surfaces these per part; the reader itself never
/// approximates silently.
#[derive(Clone, Debug, PartialEq)]
pub struct CondRuleIssue {
    pub location: String,
    pub detail: String,
}

/// `name="value"` inside one already-isolated tag.
fn attr(tag: &str, name: &str) -> Option<String> {
    tag.split(&format!("{name}=\""))
        .nth(1)?
        .split('"')
        .next()
        .map(str::to_string)
}

/// XML text to plain text, for sheet names in workbook.xml.
fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Join a relative rel target onto its part's directory (`xl` +
/// `worksheets/sheet2.xml` → `xl/worksheets/sheet2.xml`).
fn join_part(dir: &str, target: &str) -> String {
    if target.starts_with('/') {
        return target.trim_start_matches('/').to_string();
    }
    let mut parts: Vec<&str> = if dir.is_empty() { Vec::new() } else { dir.split('/').collect() };
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Sheet name → worksheet part, via workbook.xml and the workbook rels —
/// the same resolution the sheet-props reader uses, never ZIP order or
/// part filenames (the chart reader's TBL-002 fix for the same
/// first-sheet-only shortcut this replaces for rules).
fn resolve_sheet_parts(
    zip: &mut zip::ZipArchive<std::fs::File>,
    budget: &mut suite_common_core::zip_guard::ZipBudget,
) -> Vec<(String, String)> {
    use suite_common_core::zip_guard::BoundedArchive;
    let (Ok(workbook_xml), Ok(rels_xml)) = (
        zip.part_to_string("xl/workbook.xml", budget),
        zip.part_to_string("xl/_rels/workbook.xml.rels", budget),
    ) else {
        return Vec::new();
    };
    let mut rels = std::collections::HashMap::new();
    for (id, target) in super::props::parse_rels(&rels_xml) {
        rels.insert(id, join_part("xl", &target));
    }
    let Some(sheets_block) = workbook_xml.split("<sheets>").nth(1) else {
        return Vec::new();
    };
    let sheets_block = sheets_block.split("</sheets>").next().unwrap_or("");
    let mut out = Vec::new();
    for tag in sheets_block.split("<sheet ").skip(1) {
        let tag = tag.split('>').next().unwrap_or("");
        let (Some(name), Some(rid)) = (attr(tag, "name"), attr(tag, "r:id")) else {
            continue;
        };
        if let Some(target) = rels.get(&rid) {
            out.push((unescape(&name), target.clone()));
        }
    }
    out
}

/// dxf index → fill RGB, from styles.xml (order of `<dxf>` elements).
fn read_dxf_fills(
    zip: &mut zip::ZipArchive<std::fs::File>,
    budget: &mut suite_common_core::zip_guard::ZipBudget,
) -> Vec<Option<String>> {
    use suite_common_core::zip_guard::BoundedArchive;
    let mut dxf_fills: Vec<Option<String>> = Vec::new();
    let styles = zip.optional_part_to_string("xl/styles.xml", budget);
    if let Some(dxfs) = styles.split("<dxfs").nth(1) {
        let dxfs = dxfs.split("</dxfs>").next().unwrap_or("");
        for dxf in dxfs.split("<dxf>").skip(1) {
            let fill = dxf
                .split("rgb=\"")
                .nth(1)
                .and_then(|t| t.split('"').next())
                .map(|c| {
                    let c = c.to_uppercase();
                    // ARGB → RGB: drop the alpha byte only, and only
                    // for a genuine 8-digit hex value. `len()` counts
                    // bytes, so a value holding a multi-byte character
                    // can be 8 bytes long without being 8 characters —
                    // slicing it at byte 2 then lands inside a
                    // character and panics. Found by the nightly
                    // malformed-input campaign (seed 1015848, retained
                    // as tests/crashes/dxf-fill-rgb-multibyte-char.xlsx).
                    if c.len() == 8 && c.bytes().all(|b| b.is_ascii_hexdigit()) {
                        c[2..].to_string()
                    } else {
                        c
                    }
                });
            dxf_fills.push(fill);
        }
    }
    dxf_fills
}

/// The admitted rules of one worksheet part, sorted by priority (stable):
/// comparison, first range, threshold(s), fill and priority survive;
/// unknown expressions, styles and stop-if-true flags are reported, not
/// approximated.
fn parse_sheet_rules(
    location: &str,
    sheet_xml: &str,
    dxf_fills: &[Option<String>],
    issues: &mut Vec<CondRuleIssue>,
) -> Vec<crate::sheet::CondRule> {
    use crate::sheet::{parse_cell_ref, CondOp, CondRule};
    let mut out = Vec::new();
    let mut order: u32 = 0;
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
        if sqref.split_whitespace().count() > 1 {
            issues.push(CondRuleIssue {
                location: location.to_string(),
                detail: format!(
                    "a rule spans several ranges ({sqref}), of which only the first is read; the others lose their formatting"
                ),
            });
        }
        // First range in the sqref (we write exactly one).
        let range = sqref.split_whitespace().next().unwrap_or(sqref);
        let (a, b) = range.split_once(':').unwrap_or((range, range));
        let (Some((r0, c0)), Some((r1, c1))) = (
            parse_cell_ref(&a.replace('$', "")),
            parse_cell_ref(&b.replace('$', "")),
        ) else {
            continue;
        };
        for rule_xml in block.split("<cfRule").skip(1) {
            order += 1;
            let rule_attr = |name: &str| -> Option<String> {
                rule_xml
                    .split(&format!("{name}=\""))
                    .nth(1)
                    .and_then(|t| t.split('"').next())
                    .map(str::to_string)
            };
            if rule_attr("type").as_deref() != Some("cellIs") {
                issues.push(CondRuleIssue {
                    location: location.to_string(),
                    detail: format!(
                        "a '{}' rule is not read (only value comparisons are); its cells keep their values but not the formatting",
                        rule_attr("type").as_deref().unwrap_or("untyped"),
                    ),
                });
                continue;
            }
            let formulas: Vec<f64> = rule_xml
                .split("<formula>")
                .skip(1)
                .filter_map(|t| t.split("</formula>").next())
                .filter_map(|v| v.trim().parse().ok())
                .collect();
            let Some(v0) = formulas.first().copied() else {
                issues.push(CondRuleIssue {
                    location: location.to_string(),
                    detail: "a value rule names no threshold Tables can compare, so it is not read; its cells keep their values but not the formatting".to_string(),
                });
                continue;
            };
            let op = match rule_attr("operator").as_deref() {
                Some("greaterThan") => CondOp::Greater,
                Some("lessThan") => CondOp::Less,
                Some("equal") => CondOp::Equal,
                Some("between") => CondOp::Between,
                _ => {
                    issues.push(CondRuleIssue {
                        location: location.to_string(),
                        detail: format!(
                            "a value rule with operator '{}' is not read; its cells keep their values but not the formatting",
                            rule_attr("operator").as_deref().unwrap_or("missing"),
                        ),
                    });
                    continue;
                }
            };
            let fill = match rule_attr("dxfId")
                .and_then(|id| id.parse::<usize>().ok())
                .and_then(|id| dxf_fills.get(id).cloned().flatten())
            {
                Some(fill) => fill,
                None => {
                    issues.push(CondRuleIssue {
                        location: location.to_string(),
                        detail: "a value rule names no style Tables resolves, so it paints Tables' default highlight rather than the file's".to_string(),
                    });
                    "FFFF00".into()
                }
            };
            if matches!(rule_attr("stopIfTrue").as_deref(), Some("1") | Some("true")) {
                issues.push(CondRuleIssue {
                    location: location.to_string(),
                    detail: "a rule stops lower-priority rules when it matches; Tables paints the first matching rule and does not model stop-if-true separately".to_string(),
                });
            }
            out.push(CondRule {
                range: (r0.min(r1), c0.min(c1), r0.max(r1), c1.max(c0)),
                op,
                value: v0,
                value2: formulas.get(1).copied().unwrap_or(v0),
                fill,
                priority: rule_attr("priority").and_then(|p| p.parse().ok()).unwrap_or(order),
            });
        }
    }
    out.sort_by_key(|rule| rule.priority);
    out
}

/// Read cell-value conditional-formatting rules back from an xlsx, with
/// the owning sheet of each rule.
///
/// Best-effort: only the rule shapes we write (and Calc's rewrites of
/// them) are recognized. Unknown expressions, styles and stop-if-true
/// flags come back as issues, never as silent approximations.
pub fn read_cond_rules_detailed(path: &str) -> (Vec<(String, crate::sheet::CondRule)>, Vec<CondRuleIssue>) {
    let Ok(f) = std::fs::File::open(path) else {
        return (Vec::new(), Vec::new());
    };
    let Ok(mut zip) = zip::ZipArchive::new(f) else {
        return (Vec::new(), Vec::new());
    };
    use suite_common_core::zip_guard::{BoundedArchive, ZipBudget};

    // Bounded reads: this is a best-effort reader over an untrusted
    // package, and "best effort" must not extend to decompressing whatever
    // the file asks for (#442).
    let mut budget = ZipBudget::default();
    if budget.check_entry_count(zip.len()).is_err() {
        return (Vec::new(), Vec::new());
    }

    let dxf_fills = read_dxf_fills(&mut zip, &mut budget);
    // Fall back to the historical first-sheet part when the workbook
    // relationships are missing: a bare sheet1.xml still reads, rather
    // than resolving nothing.
    let mut parts = resolve_sheet_parts(&mut zip, &mut budget);
    if parts.is_empty() {
        parts.push((String::new(), "xl/worksheets/sheet1.xml".to_string()));
    }
    let mut out = Vec::new();
    let mut issues = Vec::new();
    for (name, part) in &parts {
        let location = if name.is_empty() { part.clone() } else { name.clone() };
        match zip.part_to_string(part, &mut budget) {
            Ok(sheet_xml) => {
                out.extend(parse_sheet_rules(&location, &sheet_xml, &dxf_fills, &mut issues).into_iter().map(|rule| (name.clone(), rule)));
            }
            Err(detail) => issues.push(CondRuleIssue {
                location: location.clone(),
                detail: format!("the worksheet part could not be read ({detail:?}); its rules are missing"),
            }),
        }
    }
    (out, issues)
}

/// Read cell-value conditional-formatting rules back from an xlsx.
/// Best-effort: only the rule shapes we write (and Calc's rewrites of
/// them) are recognized. Covers every worksheet, in workbook order;
/// callers that need ownership use [[read_cond_rules_detailed]].
pub fn read_cond_rules_from_xlsx(path: &str) -> Vec<crate::sheet::CondRule> {
    read_cond_rules_detailed(path).0.into_iter().map(|(_, rule)| rule).collect()
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
        #[allow(clippy::needless_range_loop)]
        for r in 0..5 {
            sh.data[r][1] = format!("{}", r * 10);
        }
        sh.cond_rules.push(CondRule {
            range: (0, 1, 4, 1),
            op: CondOp::Greater,
            value: 25.0,
            value2: 25.0,
            fill: "FFC0C0".into(),
            priority: 1,
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
            priority: 1,
        });
        save_sheets_to_xlsx(path.to_str().unwrap(), &[sh]).unwrap();
        let rules = read_cond_rules_from_xlsx(path.to_str().unwrap());
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].op, CondOp::Between);
        assert!((rules[0].value - 2.0).abs() < 1e-9 && (rules[0].value2 - 8.0).abs() < 1e-9);
        assert_eq!(rules[0].priority, 1, "priority survives: {:?}", rules[0]);
    }

    /// A dxf fill that is eight *bytes* without being eight characters used
    /// to be sliced at byte 2 — mid-character — and panic. Found by the
    /// nightly malformed-input campaign and retained as
    /// `tests/crashes/dxf-fill-rgb-multibyte-char.xlsx`; this is the same
    /// defect asserted at the reader's own level.
    #[test]
    fn a_dxf_fill_that_is_not_hex_is_left_alone_rather_than_sliced() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("multibyte.xlsx");
        let rgb = "\u{fffd}FFFF0";
        assert_eq!(rgb.len(), 8, "eight bytes");
        assert_eq!(rgb.chars().count(), 6, "but not eight characters");
        let styles = format!(
            "<styleSheet><dxfs count=\"1\"><dxf><fill>\
             <fgColor rgb=\"{rgb}\"/></fill></dxf></dxfs></styleSheet>"
        );

        let file = std::fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("xl/styles.xml", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(styles.as_bytes()).unwrap();
        writer.finish().unwrap();

        // Returning at all is the assertion: this reader has no `Result`,
        // so a panic is its only possible failure mode.
        assert!(read_cond_rules_from_xlsx(path.to_str().unwrap()).is_empty());
    }

    #[test]
    fn rule_matching_logic() {
        let r = CondRule {
            range: (0, 0, 2, 2),
            op: CondOp::Between,
            value: 10.0,
            value2: 20.0,
            fill: "FFFF00".into(),
            priority: 1,
        };
        assert!(r.matches(15.0) && !r.matches(25.0));
        assert!(r.contains(1, 1) && !r.contains(3, 0));
    }

    /// One package part per write call, in the given order.
    fn package(path: &std::path::Path, parts: &[(&str, &str)]) {
        use std::io::Write;
        let file = std::fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, body) in parts {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(body.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }

    fn workbook_xml(sheets: &[(&str, &str)]) -> String {
        let mut xml = "<workbook><sheets>".to_string();
        for (name, rid) in sheets {
            xml.push_str(&format!("<sheet name=\"{name}\" sheetId=\"1\" r:id=\"{rid}\"/>"));
        }
        xml.push_str("</sheets></workbook>");
        xml
    }

    fn workbook_rels(rels: &[(&str, &str)]) -> String {
        let mut xml = "<Relationships>".to_string();
        for (id, target) in rels {
            xml.push_str(&format!("<Relationship Id=\"{id}\" Target=\"{target}\"/>"));
        }
        xml.push_str("</Relationships>");
        xml
    }

    /// TBL-003: rules resolve to their owning worksheet through
    /// workbook.xml and the workbook rels — never ZIP order or part
    /// filenames — with comparison, range, threshold, fill and priority
    /// intact. Unknown expressions, styles and stop-if-true flags come
    /// back as issues, never as silent approximations.
    #[test]
    fn tbl003_rules_resolve_worksheet() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.xlsx");
        // Shuffled member order, nonstandard part names: Beta is listed
        // first in the workbook but its part sorts last on disk.
        package(
            &path,
            &[
                (
                    "xl/worksheets/sheetB.xml",
                    "<worksheet><conditionalFormatting sqref=\"A1:A3\">\
                     <cfRule type=\"cellIs\" operator=\"greaterThan\" priority=\"3\" dxfId=\"0\" stopIfTrue=\"1\"><formula>10</formula></cfRule>\
                     <cfRule type=\"cellIs\" operator=\"lessThan\" priority=\"2\" dxfId=\"0\"><formula>2</formula></cfRule>\
                     <cfRule type=\"colorScale\" priority=\"1\"/>\
                     </conditionalFormatting>\
                     <conditionalFormatting sqref=\"B1 B2\">\
                     <cfRule type=\"cellIs\" operator=\"equal\" priority=\"4\" dxfId=\"0\"><formula>7</formula></cfRule>\
                     </conditionalFormatting></worksheet>",
                ),
                ("xl/styles.xml", "<styleSheet><dxfs count=\"1\"><dxf><fill><fgColor rgb=\"FFC7CE\"/></fill></dxf></dxfs></styleSheet>"),
                ("xl/workbook.xml", &workbook_xml(&[("Beta", "rId2"), ("Alpha", "rId1")])),
                (
                    "xl/_rels/workbook.xml.rels",
                    &workbook_rels(&[("rId1", "worksheets/sheetA.xml"), ("rId2", "worksheets/sheetB.xml")]),
                ),
                (
                    "xl/worksheets/sheetA.xml",
                    "<worksheet><conditionalFormatting sqref=\"C1:C2\">\
                     <cfRule type=\"cellIs\" operator=\"notBetween\" priority=\"7\"><formula>1</formula><formula>5</formula></cfRule>\
                     <cfRule type=\"cellIs\" operator=\"lessThan\" priority=\"8\" dxfId=\"9\"><formula>5</formula></cfRule>\
                     </conditionalFormatting></worksheet>",
                ),
            ],
        );
        let (rules, issues) = read_cond_rules_detailed(path.to_str().unwrap());
        // Beta's two admitted rules (priority-sorted), its multi-range
        // rule on the first range only, then Alpha's default-filled rule.
        // The colorScale and notBetween shapes never come back.
        assert_eq!(rules.len(), 4, "owned rules: {rules:?}");
        assert_eq!(rules[0].0, "Beta");
        assert_eq!((rules[0].1.op, rules[0].1.priority), (CondOp::Less, 2));
        assert_eq!(rules[1].0, "Beta");
        assert_eq!((rules[1].1.op, rules[1].1.priority), (CondOp::Greater, 3));
        assert!((rules[1].1.value - 10.0).abs() < 1e-9);
        assert_eq!(rules[1].1.fill, "FFC7CE", "dxf fill resolves: {:?}", rules[1].1);
        assert_eq!(rules[1].1.range, (0, 0, 2, 0));
        assert_eq!(rules[2].0, "Beta");
        assert_eq!(rules[2].1.range, (0, 1, 0, 1), "multi-range keeps the first: {:?}", rules[2].1);
        assert_eq!(rules[3].0, "Alpha");
        assert_eq!((rules[3].1.op, rules[3].1.value), (CondOp::Less, 5.0));
        assert_eq!(rules[3].1.fill, "FFFF00", "unresolvable dxfId falls back: {:?}", rules[3].1);
        let details: Vec<&str> = issues.iter().map(|i| i.detail.as_str()).collect();
        for needle in ["colorScale", "notBetween", "stop", "no style", "several ranges"] {
            assert!(details.iter().any(|d| d.contains(needle)), "no issue about {needle}: {details:?}");
        }
        assert!(issues.iter().all(|i| !i.location.is_empty()), "every issue names its sheet: {issues:?}");
    }
}
