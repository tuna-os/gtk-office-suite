//! loss.rs — what a save would drop from the file a document came from.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! `save::compatibility_report` says what a *format* can't hold of the
//! document Letters has. This is the other half (#1274): what the *file*
//! holds that Letters never read, so a save, into any format, leaves it
//! out. Macros, embedded objects, SmartArt and charts are referenced from
//! the body Letters rewrites, so they can't be passed through; the save
//! asks first. The parts that can be passed through safely (custom XML, a
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

fn record(report: &mut CompatibilityReport, (id, label): (&str, &str), location: &str) {
    report.record(UnsupportedFeature::new(id, label, location, FeatureDisposition::WarnOnLoss, "Letters doesn't read this part of the file"));
}

/// The content of the document at `path` that a save of the document read
/// from it would lose. Empty for a file Letters wrote, a file that isn't
/// there, and anything that isn't a docx or odt package.
pub fn content_a_save_drops(path: &std::path::Path) -> CompatibilityReport {
    let mut report = CompatibilityReport::new("source");
    let Ok(file) = std::fs::File::open(path) else { return report };
    let Ok(zip) = zip::ZipArchive::new(file) else { return report };
    if ZipBudget::default().check_entry_count(zip.len()).is_err() {
        return report;
    }
    let names: BTreeSet<&str> = zip.file_names().collect();
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
    } else if names.contains("META-INF/manifest.xml") {
        if let Some(n) = any("Basic/").or_else(|| any("Scripts/")) {
            record(&mut report, MACROS, &n);
        }
        // Charts and OLE objects are both `draw:object` sub-documents,
        // stored as `Object N/` directories.
        if let Some(n) = any("Object ").or_else(|| any("ObjectReplacements/")) {
            record(&mut report, OLE, &n);
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
