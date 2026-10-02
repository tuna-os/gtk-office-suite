// loss.rs — what an xlsx save would drop (#1272).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Tables writes one format, xlsx, from its model, and the format gate
// (format.rs) already refuses to put those bytes under another extension.
// What a save can still lose is content: what the model holds and the xlsx
// writer has no form for, and what the source file held that Tables never
// read into the model at all (macros, pictures, Excel tables, slicers…).
// This lists both, as a `CompatibilityReport` of `WarnOnLoss` features, so
// the window can ask before writing — as Letters does for a format that
// can't hold a document, and Decks for content it can't keep (#1264).
//
// The source is read at save time, not remembered from open: the file a
// save replaces (or copies from) is the one that can lose something, and
// once Tables has written it there is nothing left in it to warn about.

use std::collections::BTreeSet;

use suite_common_core::interop::{CompatibilityReport, FeatureDisposition, UnsupportedFeature};
use suite_common_core::zip_guard::{BoundedArchive, ZipBudget};

use crate::sheet::{SheetModel, ValidationRule};

const PIVOTS: (&str, &str) = ("pivot-tables", "Pivot tables (their results stay, as values)");
const VALIDATION: (&str, &str) = ("validation-rules", "Validation rules xlsx can't express");
const MACROS: (&str, &str) = ("macros", "Macros");
const PICTURES: (&str, &str) = ("pictures", "Pictures and drawings");
const THREADED: (&str, &str) = ("threaded-comments", "Threaded comments");
const TABLES: (&str, &str) = ("excel-tables", "Formatted tables (structured ranges)");
const SLICERS: (&str, &str) = ("slicers", "Slicers and timelines");
const LINKS: (&str, &str) = ("external-links", "Links to other workbooks");
const SPARKLINES: (&str, &str) = ("sparklines", "Sparklines");
const CONNECTIONS: (&str, &str) = ("data-connections", "Data connections and queries");
const OLE: (&str, &str) = ("embedded-objects", "Embedded objects");

fn record(report: &mut CompatibilityReport, (id, label): (&str, &str), location: &str, detail: &str) {
    report.record(UnsupportedFeature::new(id, label, location, FeatureDisposition::WarnOnLoss, detail));
}

/// Whether the xlsx writer has a form for `rule` (save.rs `xlsx_validation`).
fn writable_rule(rule: &ValidationRule) -> bool {
    match rule {
        ValidationRule::Regex(_) => false,
        ValidationRule::List(items) => !items.is_empty() && items.join(",").chars().count() <= 255,
        ValidationRule::WholeNumber { min, max } => min.is_some() || max.is_some(),
        ValidationRule::Decimal { min, max } => min.is_some() || max.is_some(),
        ValidationRule::TextLength { min, max } => min.is_some() || max.is_some(),
    }
}

/// What saving `sheets` as xlsx over (or from) the file at `source` would
/// lose. `source` is the document's current file, if it has one.
pub fn content_a_save_drops(source: Option<&str>, sheets: &[SheetModel]) -> CompatibilityReport {
    let mut report = CompatibilityReport::new("xlsx");
    for sheet in sheets {
        if !sheet.pivot_tables.is_empty() {
            record(&mut report, PIVOTS, &sheet.name, "the pivot's definition isn't written; its cells are");
        }
        if sheet.validations.iter().flatten().flatten().any(|r| !writable_rule(r)) {
            record(&mut report, VALIDATION, &sheet.name, "a pattern, an empty or over-long list, or an unbounded range");
        }
    }
    if let Some(path) = source {
        source_drops(path, &mut report);
    }
    report
}

fn source_drops(path: &str, report: &mut CompatibilityReport) {
    let lower = path.to_lowercase();
    let ods = lower.ends_with(".ods");
    if !(ods || lower.ends_with(".xlsx") || lower.ends_with(".xlsm")) {
        return;
    }
    let Ok(file) = std::fs::File::open(path) else { return };
    let Ok(mut zip) = zip::ZipArchive::new(file) else { return };
    let mut budget = ZipBudget::default();
    if budget.check_entry_count(zip.len()).is_err() {
        return;
    }
    let names: BTreeSet<String> = zip.file_names().map(str::to_string).collect();
    if ods {
        let content = zip.optional_part_to_string("content.xml", &mut budget);
        if content.contains("<draw:image") {
            record(report, PICTURES, "content.xml", "pictures on the sheet aren't kept");
        }
        if content.contains("<table:data-pilot-table") {
            record(report, PIVOTS, "content.xml", "Calc's pivot tables aren't read; their cells are");
        }
        if names.iter().any(|n| n.starts_with("Basic/") || n.starts_with("Scripts/")) || content.contains("<office:scripts><office:script") {
            record(report, MACROS, "Basic", "macros aren't kept");
        }
        if content.contains("<draw:object-ole") {
            record(report, OLE, "content.xml", "embedded objects aren't kept");
        }
        return;
    }
    let any = |prefix: &str| names.iter().any(|n| n.starts_with(prefix));
    if names.contains("xl/vbaProject.bin") {
        record(report, MACROS, "xl/vbaProject.bin", "macros aren't kept");
    }
    if any("xl/media/") || any("xl/drawings/drawing") && !only_charts(&mut zip, &names, &mut budget) {
        record(report, PICTURES, "xl/drawings", "pictures and shapes on the sheet aren't kept");
    }
    if any("xl/threadedComments/") {
        record(report, THREADED, "xl/threadedComments", "a thread's replies aren't kept; its first note is");
    }
    if any("xl/pivotTables/") {
        record(report, PIVOTS, "xl/pivotTables", "the pivot's definition isn't read; its cells are");
    }
    if any("xl/tables/") {
        record(report, TABLES, "xl/tables", "the table's name, style and filter buttons aren't kept; its cells are");
    }
    if any("xl/slicers/") || any("xl/timelines/") || any("xl/slicerCaches/") {
        record(report, SLICERS, "xl/slicers", "slicers aren't kept");
    }
    if any("xl/externalLinks/") {
        record(report, LINKS, "xl/externalLinks", "formulas keep their last values, not the link");
    }
    if names.contains("xl/connections.xml") || any("customXml/item") && any("xl/queryTables/") {
        record(report, CONNECTIONS, "xl/connections.xml", "queries and connections aren't kept");
    }
    if any("xl/embeddings/oleObject") {
        record(report, OLE, "xl/embeddings", "embedded objects aren't kept");
    }
    let sheets: Vec<String> = names.iter().filter(|n| n.starts_with("xl/worksheets/sheet") && n.ends_with(".xml")).cloned().collect();
    for sheet in sheets {
        if zip.optional_part_to_string(&sheet, &mut budget).contains("sparklineGroup") {
            record(report, SPARKLINES, &sheet, "sparklines aren't kept");
        }
    }
}

/// Whether every drawing in the package holds only charts, which Tables
/// reads (io/charts.rs) and writes back.
fn only_charts<R: std::io::Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, names: &BTreeSet<String>, budget: &mut ZipBudget) -> bool {
    names
        .iter()
        .filter(|n| n.starts_with("xl/drawings/drawing") && n.ends_with(".xml"))
        .all(|n| {
            let xml = zip.optional_part_to_string(n, budget);
            !(xml.contains("<xdr:pic") || xml.contains("<xdr:sp>") || xml.contains("<xdr:sp ") || xml.contains("<xdr:grpSp"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn package(path: &std::path::Path, parts: &[(&str, &str)]) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, data) in parts {
            z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(data.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }

    fn ids(r: &CompatibilityReport) -> Vec<&str> {
        let mut v: Vec<&str> = r.features.iter().map(|f| f.id.as_str()).collect();
        v.sort();
        v.dedup();
        v
    }

    fn sheet() -> SheetModel {
        SheetModel::new("Sheet1", 4, 4, 1)
    }

    #[test]
    fn a_workbook_tables_wrote_loses_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ours.xlsx");
        let mut s = sheet();
        s.data[0][0] = "1".into();
        crate::io::save_sheets_to_xlsx(path.to_str().unwrap(), &[s.clone()]).unwrap();
        let r = content_a_save_drops(Some(path.to_str().unwrap()), &[s]);
        assert!(r.features.is_empty(), "{:?}", r.features);
        assert!(content_a_save_drops(None, &[sheet()]).features.is_empty());
        assert!(content_a_save_drops(Some(dir.path().join("none.xlsx").to_str().unwrap()), &[sheet()]).features.is_empty());
    }

    #[test]
    fn the_model_reports_pivots_and_rules_xlsx_has_no_form_for() {
        let mut s = sheet();
        s.validations[0][0] = Some(ValidationRule::Regex("^a+$".into()));
        let r = content_a_save_drops(None, &[s.clone()]);
        assert_eq!(ids(&r), ["validation-rules"]);
        s.validations[0][0] = Some(ValidationRule::WholeNumber { min: Some(1), max: None });
        assert!(content_a_save_drops(None, &[s]).features.is_empty(), "a bounded rule is written");
        let mut p = sheet();
        p.pivot_tables.push(crate::sheet::PivotTableSpec {
            name: "Pivot".into(),
            source_range: (0, 0, 3, 1),
            target_cell: (0, 3),
            row_fields: vec![],
            col_fields: vec![],
            data_fields: vec![],
        });
        assert_eq!(ids(&content_a_save_drops(None, &[p])), ["pivot-tables"]);
    }

    #[test]
    fn an_xlsx_lists_what_tables_never_reads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("theirs.xlsx");
        package(&path, &[
            ("xl/workbook.xml", "<workbook/>"),
            ("xl/vbaProject.bin", "x"),
            ("xl/media/image1.png", "png"),
            ("xl/drawings/drawing1.xml", "<xdr:wsDr><xdr:pic/></xdr:wsDr>"),
            ("xl/threadedComments/threadedComment1.xml", "<x/>"),
            ("xl/pivotTables/pivotTable1.xml", "<x/>"),
            ("xl/tables/table1.xml", "<x/>"),
            ("xl/slicers/slicer1.xml", "<x/>"),
            ("xl/externalLinks/externalLink1.xml", "<x/>"),
            ("xl/connections.xml", "<x/>"),
            ("xl/embeddings/oleObject1.bin", "x"),
            ("xl/worksheets/sheet1.xml", "<worksheet><extLst><x14:sparklineGroups><x14:sparklineGroup/></x14:sparklineGroups></extLst></worksheet>"),
        ]);
        let r = content_a_save_drops(Some(path.to_str().unwrap()), &[sheet()]);
        assert_eq!(
            ids(&r),
            ["data-connections", "embedded-objects", "excel-tables", "external-links", "macros", "pictures", "pivot-tables", "slicers", "sparklines", "threaded-comments"]
        );
        assert!(r.requires_confirmation());
    }

    #[test]
    fn a_drawing_with_only_charts_is_not_a_loss() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chart.xlsx");
        package(&path, &[("xl/drawings/drawing1.xml", "<xdr:wsDr><xdr:graphicFrame/></xdr:wsDr>"), ("xl/charts/chart1.xml", "<c/>")]);
        assert!(content_a_save_drops(Some(path.to_str().unwrap()), &[sheet()]).features.is_empty());
    }

    #[test]
    fn an_ods_lists_pictures_pivots_macros_and_objects() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("theirs.ods");
        package(&path, &[
            ("mimetype", "application/vnd.oasis.opendocument.spreadsheet"),
            ("Basic/Standard/Module1.xml", "<x/>"),
            ("content.xml", "<office:document-content><draw:frame><draw:image/></draw:frame><table:data-pilot-table/><draw:object-ole/></office:document-content>"),
        ]);
        let r = content_a_save_drops(Some(path.to_str().unwrap()), &[sheet()]);
        assert_eq!(ids(&r), ["embedded-objects", "macros", "pictures", "pivot-tables"]);
    }

    #[test]
    fn csv_and_tsv_have_nothing_beyond_their_cells() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.csv");
        std::fs::write(&path, "a,b\n1,2\n").unwrap();
        assert!(content_a_save_drops(Some(path.to_str().unwrap()), &[sheet()]).features.is_empty());
    }
}
