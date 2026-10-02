//! loss.rs — what a save would drop from the file a document came from.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! `save::compatibility_report` says what a *format* can't hold of the
//! document Letters has. This is the other half (#1274): what the *file*
//! holds that Letters never read, so a save, into any format, leaves it
//! out. Macros, embedded objects, SmartArt and charts are referenced from
//! the body Letters rewrites, so they can't be passed through; the save
//! asks first. So does a table nested in a table cell: Letters keeps its
//! text in the outer cell, but its rows and columns are flattened (#1419). The parts that can be passed through safely (custom XML, a
//! thumbnail, settings) are carried instead (`suite_common_core::carry`).
//!
//! It reads the file on disk at save time, like Decks' and Tables' scans:
//! once Letters has written the file, there is nothing left to warn about.

use std::collections::BTreeSet;

use suite_common_core::interop::{CompatibilityReport, FeatureDisposition, UnsupportedFeature};
use suite_common_core::zip_guard::ZipBudget;

const MACROS: (&str, &str) = ("macros", "Macros");
const OLE: (&str, &str) = ("embedded-objects", "Embedded objects");
const SMARTART: (&str, &str) = ("smartart", "SmartArt graphics");
const CHARTS: (&str, &str) = ("charts", "Charts");
const NESTED_TABLES: (&str, &str) = ("nested-tables", "Tables inside table cells");

fn record(report: &mut CompatibilityReport, (id, label): (&str, &str), location: &str) {
    record_why(report, (id, label), location, "Letters doesn't read this part of the file");
}

fn record_why(report: &mut CompatibilityReport, (id, label): (&str, &str), location: &str, why: &str) {
    report.record(UnsupportedFeature::new(id, label, location, FeatureDisposition::WarnOnLoss, why));
}

/// Whether `xml` opens a `tag` element inside another one: a table in a
/// table cell. A scan of the bytes like `zip_guard::xml_depth`, so it
/// cannot recurse; `tag` is matched whole, so `<w:tblPr` is not `<w:tbl`.
fn nests(xml: &[u8], tag: &str) -> bool {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let (open, close) = (open.as_bytes(), close.as_bytes());
    let mut depth = 0usize;
    let mut i = 0;
    while let Some(at) = xml[i..].iter().position(|&b| b == b'<') {
        i += at;
        let rest = &xml[i..];
        if rest.starts_with(close) {
            depth = depth.saturating_sub(1);
        } else if rest.starts_with(open) && matches!(rest.get(open.len()), Some(b'>' | b' ' | b'\t' | b'\n' | b'\r')) {
            depth += 1;
            if depth > 1 {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Whether the package member `name` holds a `tag` inside another.
fn member_nests(zip: &mut zip::ZipArchive<std::fs::File>, name: &str, tag: &str) -> bool {
    let Ok(mut entry) = zip.by_name(name) else { return false };
    ZipBudget::default().read_entry(&mut entry, name).is_ok_and(|xml| nests(&xml, tag))
}

/// The content of the document at `path` that a save of the document read
/// from it would lose. Empty for a file Letters wrote, a file that isn't
/// there, and anything that isn't a docx or odt package.
pub fn content_a_save_drops(path: &std::path::Path) -> CompatibilityReport {
    let mut report = CompatibilityReport::new("source");
    let Ok(file) = std::fs::File::open(path) else { return report };
    let Ok(mut zip) = zip::ZipArchive::new(file) else { return report };
    if ZipBudget::default().check_entry_count(zip.len()).is_err() {
        return report;
    }
    let names: BTreeSet<String> = zip.file_names().map(str::to_string).collect();
    let flattened = "kept as text in the outer cell, without the inner table's rows and columns";
    let any = |prefix: &str| names.iter().find(|n| n.starts_with(prefix)).map(|n| n.to_string());
    if names.contains("[Content_Types].xml") {
        if let Some(n) = any("word/vbaProject") {
            record(&mut report, MACROS, &n);
        }
        if let Some(n) = any("word/embeddings/") {
            record(&mut report, OLE, &n);
        }
        if let Some(n) = any("word/diagrams/") {
            record(&mut report, SMARTART, &n);
        }
        if let Some(n) = any("word/charts/") {
            record(&mut report, CHARTS, &n);
        }
        if member_nests(&mut zip, "word/document.xml", "w:tbl") {
            record_why(&mut report, NESTED_TABLES, "word/document.xml", flattened);
        }
    } else if names.contains("META-INF/manifest.xml") {
        if let Some(n) = any("Basic/").or_else(|| any("Scripts/")) {
            record(&mut report, MACROS, &n);
        }
        // Charts and OLE objects are both `draw:object` sub-documents,
        // stored as `Object N/` directories.
        if let Some(n) = any("Object ").or_else(|| any("ObjectReplacements/")) {
            record(&mut report, OLE, &n);
        }
        if member_nests(&mut zip, "content.xml", "table:table") {
            record_why(&mut report, NESTED_TABLES, "content.xml", flattened);
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn package(path: &std::path::Path, names: &[&str]) {
        let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for n in names {
            w.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(b"x").unwrap();
        }
        w.finish().unwrap();
    }

    fn ids(r: &CompatibilityReport) -> Vec<String> {
        r.destructive_features().iter().map(|f| f.id.clone()).collect()
    }

    #[test]
    fn a_docx_lists_macros_objects_smartart_and_charts_but_not_carried_parts() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.docx");
        package(&p, &[
            "[Content_Types].xml", "word/document.xml", "word/vbaProject.bin", "word/embeddings/oleObject1.bin",
            "word/diagrams/data1.xml", "word/charts/chart1.xml", "customXml/item1.xml", "docProps/thumbnail.jpeg",
        ]);
        assert_eq!(ids(&content_a_save_drops(&p)), ["macros", "embedded-objects", "smartart", "charts"]);
    }

    #[test]
    fn an_odt_lists_macros_and_objects() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.odt");
        package(&p, &["mimetype", "META-INF/manifest.xml", "content.xml", "Basic/Standard/Module1.xml", "Object 1/content.xml", "Thumbnails/thumbnail.png"]);
        assert_eq!(ids(&content_a_save_drops(&p)), ["macros", "embedded-objects"]);
    }

    #[test]
    fn a_table_inside_a_table_cell_is_found_and_lookalike_tags_are_not() {
        assert!(nests(b"<w:tbl><w:tblPr/><w:tr><w:tc><w:tbl><w:tr/></w:tbl></w:tc></w:tr></w:tbl>", "w:tbl"));
        assert!(!nests(b"<w:tbl><w:tblPr/><w:tblGrid/></w:tbl><w:p/><w:tbl>\n</w:tbl>", "w:tbl"), "two tables side by side");
        assert!(nests(b"<table:table table:name=\"A\"><table:table-row><table:table-cell><table:table table:name=\"B\">", "table:table"));
        assert!(!nests(b"<table:table table:name=\"A\"><table:table-column/><table:table-row><table:table-cell/></table:table-row></table:table>", "table:table"));

        let dir = tempfile::tempdir().unwrap();
        for (name, member, xml) in [
            ("a.docx", "word/document.xml", "<w:body><w:tbl><w:tr><w:tc><w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl></w:body>"),
            ("a.odt", "content.xml", "<office:text><table:table><table:table-row><table:table-cell><table:table></table:table></table:table-cell></table:table-row></table:table></office:text>"),
        ] {
            let p = dir.path().join(name);
            let mut w = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
            let marker = if name.ends_with(".docx") { "[Content_Types].xml" } else { "META-INF/manifest.xml" };
            for (n, body) in [(marker, "x"), (member, xml)] {
                w.start_file(n, zip::write::SimpleFileOptions::default()).unwrap();
                w.write_all(body.as_bytes()).unwrap();
            }
            w.finish().unwrap();
            assert_eq!(ids(&content_a_save_drops(&p)), ["nested-tables"], "{name}");
        }
    }

    #[test]
    fn a_file_letters_wrote_or_plain_text_loses_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.docx");
        let doc = crate::model::Document::default();
        crate::save::write(&doc, &p).unwrap();
        assert!(ids(&content_a_save_drops(&p)).is_empty());
        let t = dir.path().join("a.md");
        std::fs::write(&t, "# hi").unwrap();
        assert!(ids(&content_a_save_drops(&t)).is_empty());
        assert!(ids(&content_a_save_drops(&dir.path().join("missing.docx"))).is_empty());
    }
}
