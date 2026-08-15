// props.rs — per-sheet xlsx properties: hidden rows/cols, page setup.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of io.rs (issue #247).

// io.rs — Spreadsheet file I/O: xlsx/ods/csv/tsv import, xlsx export.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Extracted from tables/src/window.rs so it is unit-testable without GTK.


/// Per-sheet presentation state (#113) that calamine doesn't expose:
/// hidden rows/columns and page setup. Read directly from each sheet's
/// own XML part, resolved by name through workbook.xml + the workbook's
/// rels (not just "sheet1.xml" — unlike [[read_charts_from_xlsx]]/
/// [[read_cond_rules_from_xlsx]]'s existing single-sheet shortcut, this
/// is used by the real multi-sheet "Open File" path, where getting the
/// wrong sheet's hidden rows would be a correctness bug, not just a
/// missing-feature gap).
#[derive(Default, Debug, PartialEq)]
pub struct SheetXlsxProps {
    pub hidden_rows: std::collections::HashSet<usize>,
    pub hidden_cols: std::collections::HashSet<usize>,
    pub page_setup: Option<suite_common_core::print::PageSetup>,
}

fn xml_attr<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!("{attr}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

/// OOXML boolean attributes are legal as either "1"/"0" or "true"/"false"
/// — our own writer emits "1", but LibreOffice's re-saved xlsx uses
/// "true", so a strict `== Some("1")` check silently drops hidden
/// rows/cols on a Calc round trip.
fn xml_bool_attr(tag: &str, attr: &str) -> bool {
    matches!(xml_attr(tag, attr), Some("1") | Some("true"))
}

/// name → target for each `<Relationship Id="rIdN" Target="...">` in a
/// `.rels` part.
fn parse_rels(xml: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for tag in xml.split("<Relationship ").skip(1) {
        let tag = tag.split('>').next().unwrap_or("");
        if let (Some(id), Some(target)) = (xml_attr(tag, "Id"), xml_attr(tag, "Target")) {
            out.insert(id.to_string(), target.to_string());
        }
    }
    out
}

/// sheet name → XML part path (e.g. "xl/worksheets/sheet2.xml"), via
/// workbook.xml's `<sheet name=".." r:id="rIdN">` and the workbook's own
/// rels part.
fn resolve_sheet_parts(
    zip: &mut zip::ZipArchive<std::fs::File>,
) -> std::collections::HashMap<String, String> {
    use std::io::Read as _;
    let mut workbook_xml = String::new();
    if zip
        .by_name("xl/workbook.xml")
        .and_then(|mut f| f.read_to_string(&mut workbook_xml).map_err(Into::into))
        .is_err()
    {
        return std::collections::HashMap::new();
    }
    let mut rels_xml = String::new();
    if zip
        .by_name("xl/_rels/workbook.xml.rels")
        .and_then(|mut f| f.read_to_string(&mut rels_xml).map_err(Into::into))
        .is_err()
    {
        return std::collections::HashMap::new();
    }
    let rels = parse_rels(&rels_xml);

    let mut out = std::collections::HashMap::new();
    let Some(sheets_block) = workbook_xml.split("<sheets>").nth(1) else {
        return out;
    };
    let sheets_block = sheets_block.split("</sheets>").next().unwrap_or("");
    for tag in sheets_block.split("<sheet ").skip(1) {
        let tag = tag.split('>').next().unwrap_or("");
        let Some(name) = xml_attr(tag, "name") else { continue };
        // The r:id attribute's literal text is "r:id" regardless of
        // namespace prefix binding — real-world xlsx writers don't vary
        // this in practice.
        let Some(rid) = xml_attr(tag, "r:id") else { continue };
        if let Some(target) = rels.get(rid) {
            let target = target.trim_start_matches("/xl/").trim_start_matches("xl/");
            out.insert(name.to_string(), format!("xl/{target}"));
        }
    }
    out
}

/// Read hidden rows/columns and page setup for every sheet in `names`,
/// keyed by sheet name.
pub fn read_sheet_props_from_xlsx(
    path: &str,
    names: &[String],
) -> std::collections::HashMap<String, SheetXlsxProps> {
    use std::io::Read as _;
    let mut out = std::collections::HashMap::new();
    let Ok(f) = std::fs::File::open(path) else {
        return out;
    };
    let Ok(mut zip) = zip::ZipArchive::new(f) else {
        return out;
    };
    let parts = resolve_sheet_parts(&mut zip);

    for name in names {
        let Some(part) = parts.get(name) else { continue };
        let mut xml = String::new();
        if zip
            .by_name(part)
            .and_then(|mut f| f.read_to_string(&mut xml).map_err(Into::into))
            .is_err()
        {
            continue;
        }

        let mut props = SheetXlsxProps::default();

        if let Some(cols_block) = xml.split("<cols>").nth(1) {
            let cols_block = cols_block.split("</cols>").next().unwrap_or("");
            for tag in cols_block.split("<col ").skip(1) {
                let tag = tag.split('>').next().unwrap_or("").trim_end_matches('/');
                if !xml_bool_attr(tag, "hidden") {
                    continue;
                }
                let min: Option<usize> = xml_attr(tag, "min").and_then(|v| v.parse().ok());
                let max: Option<usize> = xml_attr(tag, "max").and_then(|v| v.parse().ok());
                if let (Some(min), Some(max)) = (min, max) {
                    for c in min..=max {
                        props.hidden_cols.insert(c - 1); // 1-based → 0-based
                    }
                }
            }
        }

        if let Some(data_block) = xml.split("<sheetData>").nth(1) {
            let data_block = data_block.split("</sheetData>").next().unwrap_or("");
            for tag in data_block.split("<row ").skip(1) {
                let tag = tag.split('>').next().unwrap_or("").trim_end_matches('/');
                if !xml_bool_attr(tag, "hidden") {
                    continue;
                }
                if let Some(r) = xml_attr(tag, "r").and_then(|v| v.parse::<usize>().ok()) {
                    props.hidden_rows.insert(r - 1); // 1-based → 0-based
                }
            }
        }

        if let Some(tag) = xml.split("<pageSetup ").nth(1) {
            let tag = tag.split('>').next().unwrap_or("");
            use suite_common_core::print::{Orientation, PageSetup, PageSize};
            let mut setup = PageSetup {
                size: match xml_attr(tag, "paperSize") {
                    Some("1") => PageSize::Letter,
                    Some("5") => PageSize::Legal,
                    Some("8") => PageSize::A3,
                    _ => PageSize::A4,
                },
                orientation: match xml_attr(tag, "orientation") {
                    Some("landscape") => Orientation::Landscape,
                    _ => Orientation::Portrait,
                },
                ..PageSetup::default()
            };
            if let Some(m) = xml.split("<pageMargins ").nth(1) {
                let m = m.split('>').next().unwrap_or("");
                let in_to_mm = |v: &str| v.parse::<f64>().ok().map(|inches| inches * 25.4);
                if let Some(mm) = xml_attr(m, "left").and_then(in_to_mm) {
                    setup.margin_left_mm = mm;
                }
                if let Some(mm) = xml_attr(m, "right").and_then(in_to_mm) {
                    setup.margin_right_mm = mm;
                }
                if let Some(mm) = xml_attr(m, "top").and_then(in_to_mm) {
                    setup.margin_top_mm = mm;
                }
                if let Some(mm) = xml_attr(m, "bottom").and_then(in_to_mm) {
                    setup.margin_bottom_mm = mm;
                }
            }
            props.page_setup = Some(setup);
        }

        out.insert(name.clone(), props);
    }
    out
}

/// A cell/range reference of the form `'Sheet Name'!$A$1:$C$5` or
/// `Sheet1!$A$1:$C$5`, as written into the `_xlnm.Print_Area` built-in
/// defined name — sheet name plus a 0-based inclusive rect.
pub(super) fn parse_sheet_qualified_range(s: &str) -> Option<(String, usize, usize, usize, usize)> {
    use crate::sheet::parse_cell_ref;
    let (sheet_part, range_part) = s.rsplit_once('!')?;
    let sheet_name = sheet_part.trim_matches('\'').replace("''", "'");
    let range = range_part.replace('$', "");
    let (a, b) = range.split_once(':').unwrap_or((range.as_str(), range.as_str()));
    let (r0, c0) = parse_cell_ref(a)?;
    let (r1, c1) = parse_cell_ref(b)?;
    Some((sheet_name, r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1)))
}

#[cfg(test)]
mod sheet_props_tests {
    use super::*;
    use crate::io::*;
    use crate::sheet::SheetModel;
    
    #[test]
    fn hidden_rows_and_cols_round_trip_per_sheet() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hide.xlsx");
        let mut s1 = SheetModel::new("Sheet1", 5, 5, 0);
        s1.hidden_rows_manual.insert(1);
        s1.hidden_cols.insert(2);
        let s2 = SheetModel::new("Sheet2", 5, 5, 1);
        save_sheets_to_xlsx(path.to_str().unwrap(), &[s1, s2]).unwrap();

        let names = vec!["Sheet1".to_string(), "Sheet2".to_string()];
        let props = read_sheet_props_from_xlsx(path.to_str().unwrap(), &names);
        assert!(props["Sheet1"].hidden_rows.contains(&1));
        assert!(props["Sheet1"].hidden_cols.contains(&2));
        assert!(props["Sheet2"].hidden_rows.is_empty(), "Sheet2 must not inherit Sheet1's hides");
        assert!(props["Sheet2"].hidden_cols.is_empty());
    }

    #[test]
    fn xml_bool_attr_accepts_true_as_well_as_1() {
        // LibreOffice's own xlsx export writes hidden="true" (a legal
        // OOXML boolean per the xsd:boolean spec) rather than our
        // writer's hidden="1" — a naive `== Some("1")` check silently
        // drops every hidden row/col on a Calc round trip (caught by
        // soffice_oracle.rs's hidden_rows_and_cols_survive_calc_rewrite).
        assert!(xml_bool_attr("r=\"3\" hidden=\"true\"", "hidden"));
        assert!(xml_bool_attr("r=\"3\" hidden=\"1\"", "hidden"));
        assert!(!xml_bool_attr("r=\"3\" hidden=\"false\"", "hidden"));
        assert!(!xml_bool_attr("r=\"3\"", "hidden"));
    }

    #[test]
    fn page_setup_round_trips() {
        use suite_common_core::print::{Orientation, PageSize};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("setup.xlsx");
        let mut s = SheetModel::new("Sheet1", 3, 3, 0);
        s.page_setup.size = PageSize::Legal;
        s.page_setup.orientation = Orientation::Landscape;
        s.page_setup.margin_left_mm = 10.0;
        save_sheets_to_xlsx(path.to_str().unwrap(), &[s]).unwrap();

        let names = vec!["Sheet1".to_string()];
        let props = read_sheet_props_from_xlsx(path.to_str().unwrap(), &names);
        let setup = props["Sheet1"].page_setup.as_ref().unwrap();
        assert_eq!(setup.size, PageSize::Legal);
        assert_eq!(setup.orientation, Orientation::Landscape);
        assert!((setup.margin_left_mm - 10.0).abs() < 0.1, "{}", setup.margin_left_mm);
    }

    #[test]
    fn print_area_string_parses_sheet_and_range() {
        assert_eq!(
            parse_sheet_qualified_range("'Sheet2'!$A$1:$C$5"),
            Some(("Sheet2".to_string(), 0, 0, 4, 2))
        );
        assert_eq!(
            parse_sheet_qualified_range("Sheet1!$B$2:$B$2"),
            Some(("Sheet1".to_string(), 1, 1, 1, 1))
        );
    }
}
