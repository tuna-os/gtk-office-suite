// loss.rs — what an xlsx save would drop (TBL-001 loss ledger).
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
//
// TBL-001 inventory. Each row names its disposition in the shared
// vocabulary, what proves it, and where the detector below looks. Plain
// words map as: kept means the reader and writer round-trip it (no
// warning, asserted per field by `tests/xlsx_loss_budget.rs`); WarnOnLoss
// names the affected sheet or package part before a destructive save;
// HardError blocks the save, and confirming cannot override it.
//
// | Content | Disposition | Proof | Detector |
// |---|---|---|
// | Cell values, formulas and their cached values | kept | loss budget | — |
// | Global defined names | kept | loss budget | — |
// | CellIs rules on any sheet (op, range, threshold, fill, priority) | kept | loss budget | — |
// | Filter ranges with admitted value criteria | kept | loss budget | — |
// | Charts on any sheet (kind, every series, names, sRGB colors, ranges, title, axis titles, legend, anchor, size) | kept (TBL-002) | loss budget | — |
// | Manual hidden rows/columns, column widths, row heights | kept | loss budget | — |
// | Print area and page setup | kept | loss budget | — |
// | Notes and writable validation rules | kept | loss budget | — |
// | Cell locks, hidden formulas, modeled protection flags | kept (#1552) | loss budget | — |
// | Safe opaque parts (thumbnail, custom XML/properties) | kept (#1274) | package_consistency, opaque-parts smoke | carry, not this file |
// | Session filter hides with no persisted range/value form | WarnOnLoss (`filter-state`) | loss budget | `model_drops` |
// | Unsupported filter constructs (top-N, dates, colors, customs, table filters) | WarnOnLoss (`filter-criteria`) | unit tests | `scan_sheet_content`, `inspect_package` |
// | Array formulas (`t="array"`) | WarnOnLoss (`array-formulas`) | unit tests | `scan_sheet_content` |
// | Sheet-scoped names (`localSheetId`) | WarnOnLoss (`scoped-names`) | unit tests | `inspect_package` |
// | Unsupported chart kinds, absolute anchors, unresolvable chart rels | WarnOnLoss (`unsupported-charts`) | unit tests | `scan_chart_xml`, drawing scan |
// | Unsupported rule shapes (non-cellIs, unknown operator, multi-range, stop-if-true, missing style) | WarnOnLoss (`unsupported-rules`) | unit tests | `scan_sheet_content` |
// | Pivot definitions, unwritable validation, passwords, inactive permissions | WarnOnLoss | unit tests | `model_drops`, `protection_drops` |
// | Macros, pictures, comments, Excel tables, slicers, links, connections, OLE, sparklines | WarnOnLoss | unit tests | `inspect_package`, `inspect_ods` |
// | A binary .xls source (cannot be enumerated) | WarnOnLoss (`legacy-xls-content`) | unit tests | `source_drops` |
// | An xlsx/xlsm/ods source that cannot be inspected | HardError (`source-inspection-failed`) | unit tests | `source_drops` |
// | A missing source, or a csv/tsv source | clean: nothing on disk to lose | unit tests | — |
//
// A drawing that holds only supported charts is not a loss; a drawing that
// holds pictures or shapes still is. Chart and rule scans are deliberately
// independent of the readers: the readers are best-effort and skip what
// they cannot model, so "the reader found nothing" must never read as "the
// file held nothing".

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
const PROTECTION_PASSWORD: (&str, &str) = ("sheet-protection-password", "Sheet protection passwords");
const PROTECTION_OPTIONS: (&str, &str) = ("sheet-protection-options", "Sheet protection permissions");
const FILTER_STATE: (&str, &str) = ("filter-state", "Filter state (which rows a filter hid)");
const FILTER_CRITERIA: (&str, &str) = ("filter-criteria", "Filter criteria");
const ARRAY_FORMULAS: (&str, &str) = ("array-formulas", "Array formulas");
const SCOPED_NAMES: (&str, &str) = ("scoped-names", "Sheet-scoped names");
const UNSUPPORTED_CHARTS: (&str, &str) = ("unsupported-charts", "Charts Tables can't read");
const UNSUPPORTED_RULES: (&str, &str) = ("unsupported-rules", "Conditional formatting Tables can't read");
const LEGACY_XLS: (&str, &str) = ("legacy-xls-content", "Original .xls content");
const INSPECTION_FAILED: (&str, &str) = ("source-inspection-failed", "Source content could not be checked");

fn record(report: &mut CompatibilityReport, (id, label): (&str, &str), location: &str, detail: &str) {
    report.record(UnsupportedFeature::new(id, label, location, FeatureDisposition::WarnOnLoss, detail));
}

/// Whether a `<definedName ...>` element names a scoped built-in the
/// reader preserves. `_xlnm.Print_Area` lands in `SheetModel::print_area`
/// and `_xlnm._FilterDatabase` duplicates the `<autoFilter>` element the
/// filter reader parses, so neither warns. Any other scoped name
/// downgrades to workbook-global on read. Matching is case-sensitive per
/// OOXML; either quote style is accepted.
fn is_preserved_builtin_name(element: &str) -> bool {
    element.contains("name=\"_xlnm.Print_Area\"")
        || element.contains("name='_xlnm.Print_Area'")
        || element.contains("name=\"_xlnm._FilterDatabase\"")
        || element.contains("name='_xlnm._FilterDatabase'")
}

/// The save cannot go ahead: the source promises inspectable content (an
/// xlsx/xlsm/ods package) but it cannot be read. Confirming cannot
/// override this — an unknown loss is not a consented one.
fn inspection_failed(report: &mut CompatibilityReport, path: &str, detail: String) {
    report.record(UnsupportedFeature::new(
        INSPECTION_FAILED.0,
        INSPECTION_FAILED.1,
        path,
        FeatureDisposition::HardError,
        detail,
    ));
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
///
/// The preflight never touches the source: it only reads. A missing source
/// reports nothing — a file that is not on disk has no content a save
/// could destroy; the model is all that remains to write.
pub fn content_a_save_drops(source: Option<&str>, sheets: &[SheetModel]) -> CompatibilityReport {
    let mut report = CompatibilityReport::new("xlsx");
    model_drops(sheets, &mut report);
    if let Some(path) = source {
        source_drops(path, &mut report);
    }
    report
}

/// What the model holds that an xlsx save does not keep, whether or not
/// the document came from a file. Protection IO itself is #1552's merged
/// work; this only inventories what that IO cannot reproduce.
fn model_drops(sheets: &[SheetModel], report: &mut CompatibilityReport) {
    for sheet in sheets.iter() {
        if !sheet.pivot_tables.is_empty() {
            record(report, PIVOTS, &sheet.name, "the pivot's definition isn't written; its cells are");
        }
        if sheet.validations.iter().flatten().flatten().any(|r| !writable_rule(r)) {
            record(report, VALIDATION, &sheet.name, "a pattern, an empty or over-long list, or an unbounded range");
        }
        if sheet.protection.password_hash.is_some() {
            record(report, PROTECTION_PASSWORD, &sheet.name, "the stored password hash cannot be written; sheet protection is saved without a password");
        }
        let protection = &sheet.protection;
        if !protection.protected && (!protection.allow_select_locked || !protection.allow_select_unlocked
            || protection.allow_format_cells || protection.allow_insert_rows || protection.allow_delete_rows)
        {
            record(report, PROTECTION_OPTIONS, &sheet.name, "nondefault permissions on an unprotected sheet cannot be written; they reset to defaults while cell locks and hidden formulas remain");
        }
        // Filter hides the writer reproduces are the ranged definition's
        // derived rows (written as an autoFilter); any other hidden row —
        // a rangeless session filter, the dialog filter — is written as a
        // plain hidden row, reopens as manually hidden, and no clearing
        // of a filter can bring it back. Rules and charts on any sheet
        // round-trip where they are (TBL-002, TBL-003), so they warn
        // about nothing here.
        let derived: std::collections::HashSet<usize> = sheet
            .filter
            .as_ref()
            .filter(|def| def.range.is_some() && !def.columns.is_empty())
            .map(|def| def.derive_hidden(sheet))
            .unwrap_or_default();
        let mut rows: Vec<usize> = sheet.hidden_rows.difference(&derived).map(|r| r + 1).collect();
        if !rows.is_empty() {
            rows.sort_unstable();
            let shown = rows.iter().take(8).map(usize::to_string).collect::<Vec<_>>().join(", ");
            let more = if rows.len() > 8 { format!(" and {} more", rows.len() - 8) } else { String::new() };
            record(report, FILTER_STATE, &sheet.name, &format!(
                "{} row(s) hidden by a filter (row(s) {}{}) are written as plain hidden rows; they reopen as manually hidden rows, and clearing the filter cannot bring them back",
                rows.len(), shown, more,
            ));
        }
    }
}

fn source_drops(path: &str, report: &mut CompatibilityReport) {
    let lower = path.to_lowercase();
    let ods = lower.ends_with(".ods");
    let package = ods || lower.ends_with(".xlsx") || lower.ends_with(".xlsm");
    if !package {
        // A binary .xls workbook opens but cannot be enumerated the way a
        // package can: warn generically rather than claim no loss. Anything
        // else Tables can open (csv, tsv) holds nothing beyond its cells.
        if lower.ends_with(".xls") && std::path::Path::new(path).exists() {
            record(report, LEGACY_XLS, path, "the .xls workbook is a binary format Tables cannot inspect; macros, settings or other content it holds do not come back");
        }
        return;
    }
    let Ok(file) = std::fs::File::open(path) else {
        // Missing: nothing on disk for the save to destroy.
        return;
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        inspection_failed(report, path, "the source is not a readable package; saving would replace bytes Tables cannot account for".into());
        return;
    };
    let mut budget = ZipBudget::default();
    if let Err(detail) = budget.check_entry_count(zip.len()) {
        inspection_failed(report, path, detail);
        return;
    }
    let names = match zip.file_names().map(|n| n.map(|n| n.into_owned())).collect::<Result<BTreeSet<String>, _>>() {
        Ok(names) => names,
        Err(e) => return inspection_failed(report, path, format!("a member name cannot be read: {e}")),
    };
    if let Err(detail) = inspect_package(&mut zip, &names, ods, &mut budget, report) {
        inspection_failed(report, path, detail);
    }
}

/// One bounded read of a present package part. A present part that cannot
/// be decoded fails the whole inspection: an unread part is not an empty
/// part, and treating it as empty would authorize the save blind.
fn read_part(
    zip: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
    budget: &mut ZipBudget,
) -> Result<String, String> {
    zip.part_to_string(name, budget)
        .map_err(|e| format!("inspect {name}: {e:?}"))
}

/// Whether every drawing in the package holds only charts, which Tables
/// reads (io/charts.rs) and writes back. Pictures and shapes are a loss;
/// what the charts themselves hold is scanned separately per part.
fn drawings_hold_only_charts(
    zip: &mut zip::ZipArchive<std::fs::File>,
    names: &BTreeSet<String>,
    budget: &mut ZipBudget,
) -> Result<bool, String> {
    let mut clean = true;
    for n in names.iter().filter(|n| n.starts_with("xl/drawings/drawing") && n.ends_with(".xml")) {
        let xml = read_part(zip, n, budget)?;
        if xml.contains("<xdr:pic") || xml.contains("<xdr:sp>") || xml.contains("<xdr:sp ") || xml.contains("<xdr:grpSp") {
            clean = false;
        }
    }
    Ok(clean)
}

fn inspect_package(
    zip: &mut zip::ZipArchive<std::fs::File>,
    names: &BTreeSet<String>,
    ods: bool,
    budget: &mut ZipBudget,
    report: &mut CompatibilityReport,
) -> Result<(), String> {
    if ods {
        return inspect_ods(zip, names, budget, report);
    }
    let any = |prefix: &str| names.iter().any(|n| n.starts_with(prefix));
    if names.contains("xl/vbaProject.bin") {
        record(report, MACROS, "xl/vbaProject.bin", "macros aren't kept");
    }
    if any("xl/media/") || any("xl/drawings/drawing") && !drawings_hold_only_charts(zip, names, budget)? {
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
    if names.contains("xl/workbook.xml") {
        let xml = read_part(zip, "xl/workbook.xml", budget)?;
        // `_xlnm.Print_Area` and `_xlnm._FilterDatabase` are sheet-scoped
        // by OOXML design, but the reader preserves both (into
        // `SheetModel::print_area` and the parsed `<autoFilter>`), so
        // neither warns. Any other scoped name downgrades to
        // workbook-global on read.
        let mut rest = xml.as_str();
        let mut scoped_user_name = false;
        while let Some(start) = rest.find("<definedName") {
            let tag: &str = &rest[start..];
            if tag.as_bytes().get("<definedName".len()) == Some(&b's') {
                // The `<definedNames>` container, not an entry.
                rest = &tag["<definedName".len()..];
                continue;
            }
            let end = tag.find('>').unwrap_or(tag.len());
            let element = &tag[..end];
            if element.contains("localSheetId") && !is_preserved_builtin_name(element) {
                scoped_user_name = true;
                break;
            }
            rest = &tag[end..];
        }
        if scoped_user_name {
            record(report, SCOPED_NAMES, "xl/workbook.xml", "sheet-scoped names are read as workbook-global; a same-named range on another sheet does not come back");
        }
    }
    for name in names.iter().filter(|n| n.starts_with("xl/charts/chart") && n.ends_with(".xml")) {
        let xml = read_part(zip, name, budget)?;
        scan_chart_xml(&xml, name, report);
    }
    for name in names.iter().filter(|n| n.starts_with("xl/drawings/drawing") && n.ends_with(".xml")) {
        let xml = read_part(zip, name, budget)?;
        if xml.contains("<xdr:absoluteAnchor") || xml.contains("<absoluteAnchor") {
            record(report, UNSUPPORTED_CHARTS, name, "an absolutely-anchored drawing has no cell to place it at; saving over this file drops its position");
        }
    }
    let sheets: Vec<String> = names.iter().filter(|n| n.starts_with("xl/worksheets/") && n.ends_with(".xml")).cloned().collect();
    for sheet in &sheets {
        let xml = read_part(zip, sheet, budget)?;
        protection_drops(&xml, sheet, report);
        if xml.contains("sparklineGroup") {
            record(report, SPARKLINES, sheet, "sparklines aren't kept");
        }
        scan_sheet_content(&xml, sheet, report);
    }
    // Excel-table filters live in table parts, not worksheet parts, and
    // are not read: their rows' values stay, but the filter itself does
    // not come back.
    for table in names.iter().filter(|n| n.starts_with("xl/tables/table") && n.ends_with(".xml")) {
        let xml = read_part(zip, table, budget)?;
        if xml.contains("autoFilter") {
            record(report, FILTER_CRITERIA, table, "an Excel-table filter isn't read; its cells stay, but reopening shows no filter to clear");
        }
    }
    Ok(())
}

/// What one worksheet `<autoFilter>` holds that the model cannot carry:
/// each unsupported column construct, specifically. Admitted value lists
/// are read (TBL-003), so a plain value filter warns about nothing.
fn scan_auto_filter(xml: &str, location: &str, report: &mut CompatibilityReport) {
    if !(xml.contains("<autoFilter") || xml.contains(":autoFilter")) {
        return;
    }
    // One entry names every unsupported construct: the report dedupes by
    // feature id, so separate records would collapse to the first.
    let mut found = Vec::new();
    for (needle, what) in [
        ("<customFilters", "custom filters"),
        ("<dynamicFilter", "dynamic filters"),
        ("<top10", "top-N filters"),
        ("<dateGroupItem", "date-group filters"),
        ("<colorFilter", "color filters"),
        ("<iconFilter", "icon filters"),
    ] {
        if xml.contains(needle) {
            found.push(what);
        }
    }
    if !found.is_empty() {
        record(report, FILTER_CRITERIA, location, &format!(
            "{} aren't read — only value lists are; reopening shows plain hidden rows with no {} to clear",
            found.join(", "),
            found.join(", "),
        ));
    }
}
/// What one worksheet part holds that the model cannot carry: filter
/// constructs, array formulas and rule shapes the reader skips.
fn scan_sheet_content(xml: &str, location: &str, report: &mut CompatibilityReport) {
    scan_auto_filter(xml, location, report);
    if xml.contains("t=\"array\"") || xml.contains("t='array'") {
        record(report, ARRAY_FORMULAS, location, "array-formula cells are read as plain values and formulas; the shared array identity does not come back");
    }
    for block in xml.split("<conditionalFormatting").skip(1) {
        let block = block.split("</conditionalFormatting>").next().unwrap_or(block);
        // One entry per block: the report dedupes by feature id, so
        // separate records would collapse to the first issue.
        let mut issues: Vec<String> = Vec::new();
        let sqref = block.split("sqref=\"").nth(1).and_then(|t| t.split('"').next()).unwrap_or("");
        if sqref.split_whitespace().count() > 1 {
            issues.push("a rule spans several ranges, of which only the first is read; the others lose their formatting".to_string());
        } else {
            for rule in block.split("<cfRule").skip(1) {
                let attr = |name: &str| -> Option<String> {
                    rule.split(&format!("{name}=\"")).nth(1).and_then(|t| t.split('"').next()).map(str::to_string)
                        .or_else(|| rule.split(&format!("{name}='")).nth(1).and_then(|t| t.split('\'').next()).map(str::to_string))
                };
                let kind = attr("type");
                if kind.as_deref() != Some("cellIs") {
                    issues.push(format!(
                        "a '{}' rule is not read (only value comparisons are); its cells keep their values but not the formatting",
                        kind.as_deref().unwrap_or("untyped"),
                    ));
                    continue;
                }
                if !matches!(attr("operator").as_deref(), Some("greaterThan" | "lessThan" | "equal" | "between")) {
                    issues.push(format!(
                        "a value rule with operator '{}' is not read; its cells keep their values but not the formatting",
                        attr("operator").as_deref().unwrap_or("missing"),
                    ));
                }
                if attr("dxfId").is_none() {
                    issues.push("a value rule names no style Tables resolves, so it paints Tables' default highlight rather than the file's".to_string());
                }
                if matches!(attr("stopIfTrue").as_deref(), Some("1") | Some("true")) {
                    issues.push("a rule stops lower-priority rules when it matches; Tables paints the first matching rule and does not model stop-if-true separately".to_string());
                }
            }
        }
        if !issues.is_empty() {
            record(report, UNSUPPORTED_RULES, location, &issues.join(" "));
        }
    }
}

/// What one chart part holds that the model cannot carry: a chart kind the
/// reader skips. Every admitted series now round-trips (TBL-002), so a part
/// with only supported kinds records nothing — no matter how many series it
/// holds.
fn scan_chart_xml(xml: &str, location: &str, report: &mut CompatibilityReport) {
    use std::sync::LazyLock;
    static KIND: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"<(?:[A-Za-z_][\w.-]*:)?(\w+Chart)\b").unwrap()
    });
    let mut unknown: Vec<String> = KIND.captures_iter(xml)
        .map(|c| c[1].to_string())
        .filter(|k| !matches!(k.as_str(), "barChart" | "lineChart" | "pieChart" | "scatterChart" | "areaChart"))
        .collect();
    unknown.sort();
    unknown.dedup();
    if !unknown.is_empty() {
        record(report, UNSUPPORTED_CHARTS, location, &format!(
            "{} ({}) are not read; their data cells stay, but the graphic does not come back",
            if unknown.len() == 1 { "this chart kind" } else { "these chart kinds" },
            unknown.join(", "),
        ));
    }
}

fn inspect_ods(
    zip: &mut zip::ZipArchive<std::fs::File>,
    names: &BTreeSet<String>,
    budget: &mut ZipBudget,
    report: &mut CompatibilityReport,
) -> Result<(), String> {
    let content = read_part(zip, "content.xml", budget)?;
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
    if content.contains("<table:table-source") {
        record(report, LINKS, "content.xml", "Calc's hidden copies of linked workbooks aren't kept");
    }
    Ok(())
}

/// The writer accepts plaintext passwords only, not imported legacy hashes or
/// modern salted credentials. Permissions outside SheetProtection also cannot
/// be reproduced from the model. Keep these losses in the existing save gate.
fn protection_drops(xml: &str, location: &str, report: &mut CompatibilityReport) {
    use std::sync::LazyLock;
    static TAG: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"<(?:[A-Za-z_][\w.-]*:)?sheetProtection\b[^>]*>").unwrap()
    });
    static ATTRIBUTE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"\s([A-Za-z][A-Za-z0-9]*)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap()
    });
    for tag in TAG.find_iter(xml) {
        let attrs: std::collections::BTreeMap<&str, &str> = ATTRIBUTE.captures_iter(tag.as_str())
            .map(|a| (a.get(1).unwrap().as_str(), a.get(2).or_else(|| a.get(3)).unwrap().as_str()))
            .collect();
        if attrs.contains_key("password") {
            record(report, PROTECTION_PASSWORD, location, "the imported legacy sheet password hash cannot be written; sheet protection is saved without a password");
        }
        if ["algorithmName", "hashValue", "saltValue", "spinCount"].iter().any(|key| attrs.contains_key(key)) {
            record(report, PROTECTION_PASSWORD, location, "the imported modern salted sheet password credentials cannot be written; sheet protection is saved without a password");
        }
        if !matches!(attrs.get("sheet"), Some(&"1") | Some(&"true")) {
            continue;
        }
        let unsupported: Vec<&str> = [
            ("objects", false), ("scenarios", false),
            ("formatColumns", true), ("formatRows", true), ("insertColumns", true),
            ("deleteColumns", true), ("insertHyperlinks", true), ("sort", true),
            ("autoFilter", true), ("pivotTables", true),
        ].into_iter().filter_map(|(name, default)| {
            let disallowed = attrs.get(name).map_or(default, |value| matches!(*value, "1" | "true"));
            (!disallowed).then_some(name)
        }).collect();
        if !unsupported.is_empty() {
            record(report, PROTECTION_OPTIONS, location, &format!(
                "these unmodeled permissions become disallowed on save: {}", unsupported.join(", ")
            ));
        }
    }
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

    /// Calc's hidden caches of linked workbooks aren't loaded (load.rs
    /// `is_link_cache`), so a save says it drops them.
    #[test]
    fn an_ods_lists_its_links_to_other_workbooks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("linked.ods");
        package(&path, &[
            ("mimetype", "application/vnd.oasis.opendocument.spreadsheet"),
            ("content.xml", "<office:document-content><table:table table:name=\"'file:///C:/b.xlsm'#S\"><table:table-source xlink:href=\"file:///C:/b.xlsm\"/></table:table></office:document-content>"),
        ]);
        let r = content_a_save_drops(Some(path.to_str().unwrap()), &[sheet()]);
        assert_eq!(ids(&r), ["external-links"]);
    }

    #[test]
    fn csv_and_tsv_have_nothing_beyond_their_cells() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.csv");
        std::fs::write(&path, "a,b\n1,2\n").unwrap();
        assert!(content_a_save_drops(Some(path.to_str().unwrap()), &[sheet()]).features.is_empty());
    }

    #[test]
    fn sheet_password_losses_require_confirmation_without_touching_the_source() {
        let dir = tempfile::tempdir().unwrap();
        for (name, attributes, detail) in [
            ("legacy", "password=\"ABCD\"", "legacy"),
            ("modern", "algorithmName='SHA-512' hashValue='digest' saltValue='salt' spinCount='100000'", "modern salted"),
        ] {
            let path = dir.path().join(format!("{name}.xlsx"));
            let xml = format!("<worksheet><sheetProtection sheet=\"1\" objects=\"1\" scenarios=\"1\" {attributes}/></worksheet>");
            package(&path, &[("xl/worksheets/later.xml", &xml)]);
            let before = std::fs::read(&path).unwrap();
            let report = content_a_save_drops(Some(path.to_str().unwrap()), &[sheet()]);
            assert_eq!(ids(&report), ["sheet-protection-password"]);
            assert!(report.requires_confirmation());
            assert!(report.features[0].detail.contains(detail));
            assert_eq!(std::fs::read(&path).unwrap(), before, "preflight must not modify the source");
        }
        let mut s = sheet();
        s.protection.protected = true;
        s.protection.password_hash = Some("ABCD".into());
        assert_eq!(ids(&content_a_save_drops(None, &[s])), ["sheet-protection-password"]);
    }

    #[test]
    fn unmodeled_sheet_permissions_warn_only_when_their_meaning_changes() {
        let mut report = CompatibilityReport::new("xlsx");
        protection_drops(
            r#"<worksheet><sheetProtection sheet="1" objects="1" scenarios="1" formatCells="0" insertRows="0" deleteRows="0" selectLockedCells="1" selectUnlockedCells="1"/></worksheet>"#,
            "Sheet1", &mut report,
        );
        assert!(report.features.is_empty(), "modeled permissions are retained");
        protection_drops(
            r#"<worksheet><sheetProtection sheet="true" objects="true" scenarios="true" formatColumns="false" sort="0" autoFilter="0"/></worksheet>"#,
            "Later", &mut report,
        );
        assert_eq!(ids(&report), ["sheet-protection-options"]);
        assert!(report.requires_confirmation());
        assert!(report.features[0].detail.contains("formatColumns, sort, autoFilter"));
    }

    #[test]
    fn inactive_nondefault_permissions_warn_without_enabling_protection() {
        let mut s = sheet();
        s.protection.allow_insert_rows = true;
        let report = content_a_save_drops(None, &[s]);
        assert_eq!(ids(&report), ["sheet-protection-options"]);
        assert!(report.requires_confirmation());
        assert!(report.features[0].detail.contains("unprotected sheet"));
    }

    #[test]
    fn filter_state_warns_while_manual_hides_stay_silent() {
        let mut s = sheet();
        s.hidden_rows_manual.insert(2);
        assert!(content_a_save_drops(None, &[s.clone()]).features.is_empty(), "a manual hide round-trips");
        s.hidden_rows.insert(5);
        let report = content_a_save_drops(None, &[s]);
        assert_eq!(ids(&report), ["filter-state"]);
        assert!(report.requires_confirmation());
        let feature = &report.features[0];
        assert_eq!(feature.location, "Sheet1");
        assert!(feature.detail.contains("(row(s) 6)"), "names the 1-based row: {}", feature.detail);
        assert!(feature.detail.contains("clearing the filter"), "names the consequence: {}", feature.detail);
    }

    #[test]
    fn later_sheet_charts_and_rules_are_kept() {
        // TBL-002: charts resolve to their owning sheet through
        // relationships, so a later sheet's chart warns about nothing.
        // TBL-003: rules resolve the same way, so a later sheet's admitted
        // rule warns about nothing either.
        use crate::sheet::{ChartKind, ChartSeries, ChartSpec, CondOp, CondRule, LegendPosition};
        let first = sheet();
        let later = SheetModel::new("Later", 4, 4, 2);
        let mut sheets = vec![first, later];
        sheets[1].charts.push(ChartSpec {
            kind: ChartKind::Bar, title: "Later chart".into(),
            x_axis_title: None, y_axis_title: None, legend_position: LegendPosition::Right,
            series: vec![ChartSeries { name: String::new(), cat: (0, 0, 1), val: (0, 1, 1), color: None }],
            cat: (0, 0, 1), val: (0, 1, 1), anchor: (0, 2),
            width_px: 480.0, height_px: 280.0,
        });
        assert!(content_a_save_drops(None, &sheets).features.is_empty(), "a later sheet's chart is kept");
        sheets[1].cond_rules.push(CondRule { range: (0, 0, 1, 0), op: CondOp::Less, value: 2.0, value2: 0.0, fill: "FFEB9C".into(), priority: 1 });
        assert!(content_a_save_drops(None, &sheets).features.is_empty(), "a later sheet's rule is kept: {:?}", content_a_save_drops(None, &sheets).features);
    }

    #[test]
    fn every_admitted_chart_field_is_kept() {
        // TBL-002: kind, every series, literal names, sRGB colors, ranges,
        // title, axis titles, legend and size all round-trip, so a rich
        // chart warns about nothing — and neither does the plain core.
        use crate::sheet::{ChartKind, ChartSeries, ChartSpec, LegendPosition};
        let mut s = sheet();
        s.charts.push(ChartSpec {
            kind: ChartKind::Bar, title: "Big".into(),
            x_axis_title: Some("X".into()), y_axis_title: Some("Y".into()), legend_position: LegendPosition::Bottom,
            series: vec![
                ChartSeries { name: "One".into(), cat: (0, 0, 1), val: (0, 1, 1), color: Some("4472C4".into()) },
                ChartSeries { name: "Two".into(), cat: (0, 0, 1), val: (0, 2, 1), color: None },
            ],
            cat: (0, 0, 1), val: (0, 1, 1), anchor: (0, 3),
            width_px: 320.0, height_px: 200.0,
        });
        assert!(content_a_save_drops(None, &[s]).features.is_empty(), "a rich chart is kept");
        let mut kept = sheet();
        kept.charts.push(ChartSpec {
            kind: ChartKind::Bar, title: "Plain".into(),
            x_axis_title: None, y_axis_title: None, legend_position: LegendPosition::Right,
            series: vec![ChartSeries { name: String::new(), cat: (0, 0, 1), val: (0, 1, 1), color: None }],
            cat: (0, 0, 1), val: (0, 1, 1), anchor: (0, 3),
            width_px: 480.0, height_px: 280.0,
        });
        assert!(content_a_save_drops(None, &[kept]).features.is_empty());
    }

    #[test]
    fn print_area_scoped_name_does_not_warn() {
        // `_xlnm.Print_Area` is sheet-scoped by OOXML design, but the
        // reader preserves it into `print_area` — only user scoped names
        // warn.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("print-area.xlsx");
        package(&path, &[
            ("xl/workbook.xml", "<workbook><definedNames><definedName name=\"_xlnm.Print_Area\" localSheetId=\"0\">Data!$A$1:$C$6</definedName><definedName name=\"_xlnm._FilterDatabase\" localSheetId=\"0\" hidden=\"1\">Data!$A$1:$A$5</definedName></definedNames></workbook>"),
            ("xl/worksheets/sheet1.xml", "<worksheet><sheetData/></worksheet>"),
        ]);
        let report = content_a_save_drops(Some(path.to_str().unwrap()), &[sheet()]);
        assert!(!ids(&report).contains(&"scoped-names"), "preserved builtins warn: {:?}", report.features);
    }

    #[test]
    fn model_insufficient_content_warns_per_part() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("foreign.xlsx");
        package(&path, &[
            ("xl/workbook.xml", "<workbook><definedNames><definedName name=\"Local\" localSheetId=\"1\">Sheet1!$A$1</definedName></definedNames></workbook>"),
            ("xl/worksheets/sheet1.xml", "<worksheet><sheetData/><autoFilter ref=\"A1:B3\"><filterColumn colId=\"0\"><filters blank=\"0\"><filter val=\"kept\"/></filters></filterColumn><filterColumn colId=\"1\"><customFilters><customFilter operator=\"greaterThan\" val=\"5\"/></customFilters></filterColumn></autoFilter><sheetData><row><c><f t=\"array\" ref=\"C1:C3\">SUM(A1:B3)</f><v>6</v></c></row></sheetData><conditionalFormatting sqref=\"A1:A3\"><cfRule type=\"colorScale\"/></conditionalFormatting></worksheet>"),
            ("xl/worksheets/sheet2.xml", "<worksheet><conditionalFormatting sqref=\"A1\"><cfRule type=\"cellIs\" operator=\"greaterThan\"><formula>1</formula></cfRule></conditionalFormatting></worksheet>"),
            ("xl/tables/table1.xml", "<table><autoFilter ref=\"A1:B3\"/></table>"),
            ("xl/charts/chart1.xml", "<chart><plotArea><stockChart><ser><val><numRef><f>Sheet1!$A$1:$A$3</f></numRef></val></ser><ser><val><numRef><f>Sheet1!$B$1:$B$3</f></numRef></val></ser></stockChart></plotArea></chart>"),
        ]);
        let before = std::fs::read(&path).unwrap();
        let report = content_a_save_drops(Some(path.to_str().unwrap()), &[sheet()]);
        assert_eq!(
            ids(&report),
            ["array-formulas", "excel-tables", "filter-criteria", "scoped-names", "unsupported-charts", "unsupported-rules"]
        );
        for feature in &report.features {
            assert!(!feature.location.is_empty(), "every loss names its location");
            assert!(!feature.detail.is_empty(), "every loss names its consequence");
        }
        // The admitted value column and the later sheet's admitted rule
        // warn about nothing; the custom column, the table filter, the
        // colorScale rule and the stock chart each warn specifically.
        let details: Vec<&str> = report.features.iter().map(|f| f.detail.as_str()).collect();
        assert!(details.iter().any(|d| d.contains("custom filters")), "custom filter loss: {details:?}");
        assert!(details.iter().any(|d| d.contains("Excel-table filter")), "table filter loss: {details:?}");
        assert!(report.requires_confirmation());
        assert_eq!(std::fs::read(&path).unwrap(), before, "preflight must not modify the source");
    }

    #[test]
    fn unsupported_rule_operators_and_ranges_warn() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.xlsx");
        package(&path, &[("xl/worksheets/sheet1.xml",
            "<worksheet><conditionalFormatting sqref=\"A1:A3 C1:C3\"><cfRule type=\"cellIs\" operator=\"greaterThan\"/></conditionalFormatting></worksheet>")]);
        let report = content_a_save_drops(Some(path.to_str().unwrap()), &[sheet()]);
        assert_eq!(ids(&report), ["unsupported-rules"]);
        assert!(report.features[0].detail.contains("several ranges"));

        let path2 = dir.path().join("op.xlsx");
        package(&path2, &[("xl/worksheets/sheet1.xml",
            "<worksheet><conditionalFormatting sqref=\"A1\"><cfRule type=\"cellIs\" operator=\"top10\"/></conditionalFormatting></worksheet>")]);
        let report2 = content_a_save_drops(Some(path2.to_str().unwrap()), &[sheet()]);
        assert_eq!(ids(&report2), ["unsupported-rules"]);
        assert!(report2.features[0].detail.contains("top10"));
    }

    #[test]
    fn admitted_value_filters_and_later_sheet_rules_stay_silent() {
        // TBL-003: a value-list autoFilter is read (range and criteria),
        // and admitted rules resolve on every sheet — neither warns.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("admitted.xlsx");
        package(&path, &[
            ("xl/workbook.xml", "<workbook><sheets><sheet name=\"One\" r:id=\"rId1\"/><sheet name=\"Two\" r:id=\"rId2\"/></sheets></workbook>"),
            ("xl/_rels/workbook.xml.rels", "<Relationships><Relationship Id=\"rId1\" Target=\"worksheets/sheet1.xml\"/><Relationship Id=\"rId2\" Target=\"worksheets/sheet2.xml\"/></Relationships>"),
            ("xl/worksheets/sheet1.xml", "<worksheet><autoFilter ref=\"A1:B3\"><filterColumn colId=\"0\"><filters blank=\"1\"><filter val=\"a\"/></filters></filterColumn></autoFilter></worksheet>"),
            ("xl/worksheets/sheet2.xml", "<worksheet><conditionalFormatting sqref=\"A1\"><cfRule type=\"cellIs\" operator=\"greaterThan\" priority=\"1\" dxfId=\"0\"><formula>1</formula></cfRule></conditionalFormatting></worksheet>"),
            ("xl/styles.xml", "<styleSheet><dxfs count=\"1\"><dxf><fill><fgColor rgb=\"FFFF00\"/></fill></dxf></dxfs></styleSheet>"),
        ]);
        let report = content_a_save_drops(Some(path.to_str().unwrap()), &[sheet()]);
        assert!(report.features.is_empty(), "admitted content warns: {:?}", report.features);
    }

    #[test]
    fn each_unsupported_filter_construct_warns_specifically() {
        // TBL-003: top-N, date, color and custom filters must not be
        // reported as preserved — each names itself in the report.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("filters.xlsx");
        package(&path, &[("xl/worksheets/sheet1.xml",
            "<worksheet><autoFilter ref=\"A1:F5\">\
             <filterColumn colId=\"0\"><customFilters><customFilter operator=\"greaterThan\" val=\"5\"/></customFilters></filterColumn>\
             <filterColumn colId=\"1\"><filters><top10 val=\"3\"/></filters></filterColumn>\
             <filterColumn colId=\"2\"><filters><dateGroupItem dateTimeGrouping=\"year\" year=\"2024\"/></filters></filterColumn>\
             <filterColumn colId=\"3\"><colorFilter dxfId=\"0\"/></filterColumn>\
             <filterColumn colId=\"4\"><dynamicFilter type=\"aboveAverage\"/></filterColumn>\
             <filterColumn colId=\"5\"><iconFilter iconSet=\"3Arrows\" iconId=\"0\"/></filterColumn>\
             </autoFilter></worksheet>")]);
        let report = content_a_save_drops(Some(path.to_str().unwrap()), &[sheet()]);
        assert_eq!(ids(&report), ["filter-criteria"]);
        let details: Vec<&str> = report.features.iter().map(|f| f.detail.as_str()).collect();
        for needle in ["custom filters", "top-N filters", "date-group filters", "color filters", "dynamic filters", "icon filters"] {
            assert!(details.iter().any(|d| d.contains(needle)), "no loss for {needle}: {details:?}");
        }
        assert!(report.requires_confirmation());
    }

    #[test]
    fn stop_if_true_and_missing_styles_warn() {
        // TBL-003: stop-if-true behavior and unresolvable styles are
        // reported, never silently approximated.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stop.xlsx");
        package(&path, &[("xl/worksheets/sheet1.xml",
            "<worksheet><conditionalFormatting sqref=\"A1:A2\">\
             <cfRule type=\"cellIs\" operator=\"greaterThan\" priority=\"1\" dxfId=\"0\" stopIfTrue=\"1\"><formula>1</formula></cfRule>\
             <cfRule type=\"cellIs\" operator=\"lessThan\" priority=\"2\"><formula>0</formula></cfRule>\
             </conditionalFormatting></worksheet>")]);
        let report = content_a_save_drops(Some(path.to_str().unwrap()), &[sheet()]);
        assert_eq!(ids(&report), ["unsupported-rules"]);
        let details: Vec<&str> = report.features.iter().map(|f| f.detail.as_str()).collect();
        assert!(details.iter().any(|d| d.contains("stop")), "no stop-if-true loss: {details:?}");
        assert!(details.iter().any(|d| d.contains("no style")), "no missing-style loss: {details:?}");
    }

    #[test]
    fn a_supported_chart_part_stays_silent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("supported.xlsx");
        package(&path, &[("xl/charts/chart1.xml",
            "<c:chart xmlns:c=\"x\"><c:plotArea><c:barChart><c:ser><c:cat><strRef><f>Sheet1!$A$1:$A$3</f></strRef></c:cat><c:val><numRef><f>Sheet1!$B$1:$B$3</f></numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart>")]);
        assert!(content_a_save_drops(Some(path.to_str().unwrap()), &[sheet()]).features.is_empty());
    }

    #[test]
    fn an_xls_source_warns_that_it_cannot_be_enumerated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.xls");
        std::fs::write(&path, "not really an OLE workbook, only the extension matters").unwrap();
        let report = content_a_save_drops(Some(path.to_str().unwrap()), &[sheet()]);
        assert_eq!(ids(&report), ["legacy-xls-content"]);
        assert!(report.requires_confirmation());
        assert_eq!(report.features[0].location, path.to_str().unwrap());
        assert!(!report.features[0].detail.is_empty());
        // A missing .xls has nothing on disk to lose.
        assert!(content_a_save_drops(Some(dir.path().join("gone.xls").to_str().unwrap()), &[sheet()]).features.is_empty());
    }

    #[test]
    fn tbl001_unreadable_source_is_not_loss_free() {
        let dir = tempfile::tempdir().unwrap();
        // Garbage under an xlsx name: the source promises inspectable
        // content Tables cannot read, so the save is blocked, not silent.
        let corrupt = dir.path().join("corrupt.xlsx");
        std::fs::write(&corrupt, "this is not a package").unwrap();
        let report = content_a_save_drops(Some(corrupt.to_str().unwrap()), &[sheet()]);
        assert_eq!(ids(&report), ["source-inspection-failed"]);
        assert!(report.has_hard_errors());
        assert!(report.validate_save(false).is_err(), "cancel blocks");
        assert!(report.validate_save(true).is_err(), "even an explicit confirmation cannot bless an unknown loss");
        let feature = &report.features[0];
        assert_eq!(feature.location, corrupt.to_str().unwrap());
        assert!(!feature.detail.is_empty());
        // A truncated package fails the same closed way.
        let truncated = dir.path().join("truncated.xlsx");
        std::fs::write(&truncated, b"PK\x03\x04half a zip").unwrap();
        assert_eq!(ids(&content_a_save_drops(Some(truncated.to_str().unwrap()), &[sheet()])), ["source-inspection-failed"]);
        // A missing source is not an uninspectable one: nothing on disk.
        assert!(content_a_save_drops(Some(dir.path().join("missing.xlsx").to_str().unwrap()), &[sheet()]).features.is_empty());
        // An ods without its content part cannot be inspected either.
        let broken_ods = dir.path().join("broken.ods");
        package(&broken_ods, &[("mimetype", "application/vnd.oasis.opendocument.spreadsheet")]);
        assert_eq!(ids(&content_a_save_drops(Some(broken_ods.to_str().unwrap()), &[sheet()])), ["source-inspection-failed"]);
    }

    #[test]
    fn tbl001_cancel_all_save_routes() {
        // Every save route funnels the same report through the same gate:
        // overwriting the source, saving a copy, or saving a new document.
        // Without confirmation nothing is authorized to write; the preflight
        // itself leaves the source bytes alone either way.
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("macros.xlsm");
        package(&source, &[("xl/vbaProject.bin", "x")]);
        let before = std::fs::read(&source).unwrap();
        let mut s = sheet();
        s.data[0][0] = "edit".into();
        for (label, origin) in [
            ("overwrite", Some(source.to_str().unwrap())),
            ("save-as-copy", Some(source.to_str().unwrap())),
            ("new-document", None),
        ] {
            let report = content_a_save_drops(origin, &[s.clone()]);
            if label == "new-document" {
                assert!(report.features.is_empty(), "a new document has no source content to lose");
            } else {
                assert!(report.requires_confirmation(), "{label} must ask before dropping the macro");
                assert!(report.validate_save(false).is_err(), "{label}: cancel blocks the save");
                assert!(report.validate_save(true).is_ok(), "{label}: an explicit choice authorizes the save");
                for feature in &report.features {
                    assert!(!feature.location.is_empty(), "{label}: every loss names its location");
                    assert!(!feature.detail.is_empty(), "{label}: every loss names its consequence");
                }
            }
        }
        assert_eq!(std::fs::read(&source).unwrap(), before, "the preflight writes nothing");
    }
}
