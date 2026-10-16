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

const PARA_BORDERS: (&str, &str) = ("para-border-styles", "Paragraph border styles");
const TAB_LEADERS: (&str, &str) = ("tab-leader-styles", "Tab leader styles");
const WIDOW_COUNTS: (&str, &str) = ("widow-orphan-counts", "Widow and orphan counts");

/// The start of `needle` in `hay`, if present.
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The values of attribute `attr` (spelled with its quotes, as in
/// `w:val="`) in `xml`, in order.
fn attr_values<'a>(xml: &'a [u8], attr: &[u8]) -> Vec<&'a [u8]> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = find(rest, attr) {
        rest = &rest[at + attr.len()..];
        let Some(end) = rest.iter().position(|&b| b == b'"') else { break };
        out.push(&rest[..end]);
        rest = &rest[end + 1..];
    }
    out
}

/// The `<w:pBdr ...>...</w:pBdr>` regions of a document part: the only
/// place paragraph border edges live.
fn pbdr_regions(xml: &[u8]) -> Vec<&[u8]> {
    const CLOSE: &[u8] = b"</w:pBdr>";
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = find(rest, b"<w:pBdr") {
        let Some(open) = rest[at..].iter().position(|&b| b == b'>').map(|e| at + e + 1) else { break };
        if rest[at..open].ends_with(b"/>") {
            rest = &rest[open..];
            continue;
        }
        let Some(close) = find(&rest[open..], CLOSE).map(|e| open + e) else { break };
        out.push(&rest[open..close]);
        rest = &rest[close + CLOSE.len()..];
    }
    out
}

/// The `<style:paragraph-properties ...>...</style:paragraph-properties>`
/// regions of a style part (self-closed ones included): the only place
/// paragraph widows, leaders and borders live.
fn para_prop_regions(xml: &[u8]) -> Vec<&[u8]> {
    const CLOSE: &[u8] = b"</style:paragraph-properties>";
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = find(rest, b"<style:paragraph-properties") {
        let Some(open) = rest[at..].iter().position(|&b| b == b'>').map(|e| at + e + 1) else { break };
        if rest[at..open].ends_with(b"/>") {
            out.push(&rest[at..open]);
            rest = &rest[open..];
            continue;
        }
        let Some(close) = find(&rest[open..], CLOSE).map(|e| open + e) else { break };
        out.push(&rest[at..close]);
        rest = &rest[close + CLOSE.len()..];
    }
    out
}

/// What the DOCX document part holds that the model drops: paragraph
/// border looks beyond single, dashed, dotted and double (wave, triple
/// and the rest, or a bar/between edge), and tab leaders beyond none,
/// dot, hyphen, underscore and middle dot (heavy draws as an
/// underscore).
fn docx_typography(xml: &[u8], member: &str, report: &mut CompatibilityReport) {
    const BORDERS: &[&[u8]] = &[b"nil", b"none", b"single", b"thick", b"double", b"dotted", b"dashed", b"dashSmallGap"];
    let mut borders = false;
    for region in pbdr_regions(xml) {
        if find(region, b"<w:bar").is_some() || find(region, b"<w:between").is_some() {
            borders = true;
            break;
        }
        if attr_values(region, b"w:val=\"").iter().any(|v| !BORDERS.contains(v)) {
            borders = true;
            break;
        }
    }
    if borders {
        record_why(report, PARA_BORDERS, member, "Letters keeps single, dashed, dotted and double paragraph borders; anything else is dropped");
    }
    const LEADERS: &[&[u8]] = &[b"none", b"dot", b"hyphen", b"underscore", b"middleDot"];
    if attr_values(xml, b"w:leader=\"").iter().any(|v| !LEADERS.contains(v)) {
        record_why(report, TAB_LEADERS, member, "Letters keeps dot, hyphen, underscore and middle dot tab leaders; anything else draws as an underscore");
    }
}

const SECTION_KINDS: (&str, &str) = ("section-kinds", "Section break kinds");
const SECTION_COLUMNS: (&str, &str) = ("section-unequal-columns", "Columns of different widths");
const PAGE_NUMBER_RESTART: (&str, &str) = ("page-number-restart", "Page numbering restarts");

/// The `<w:sectPr ...>...</w:sectPr>` regions of the document part, in
/// order (`w:sectPrChange` holds an old value and is not one).
fn sect_pr_regions(xml: &[u8]) -> Vec<&[u8]> {
    const CLOSE: &[u8] = b"</w:sectPr>";
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = find(rest, b"<w:sectPr") {
        if !matches!(rest.get(at + 9), Some(b'>' | b' ' | b'/')) {
            rest = &rest[at + 9..];
            continue;
        }
        let Some(open) = rest[at..].iter().position(|&b| b == b'>').map(|e| at + e + 1) else { break };
        if rest[at..open].ends_with(b"/>") {
            rest = &rest[open..];
            continue;
        }
        let Some(close) = find(&rest[open..], CLOSE).map(|e| open + e) else { break };
        out.push(&rest[open..close]);
        rest = &rest[close + CLOSE.len()..];
    }
    out
}

/// The open tags of each `tag` element in `xml` (`<w:type` matches
/// `<w:type …>`, not `<w:types`), attributes and all.
fn element_tags<'a>(xml: &'a [u8], tag: &[u8]) -> Vec<&'a [u8]> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = find(&xml[from..], tag).map(|a| from + a) {
        let end = xml[at..].iter().position(|&b| b == b'>').map_or(xml.len(), |e| at + e);
        if matches!(xml.get(at + tag.len()), Some(b' ' | b'/' | b'>')) {
            out.push(&xml[at..end]);
        }
        from = end;
    }
    out
}

/// What a DOCX of several sections holds that the section model
/// (`crate::sections`, LET-005S) draws otherwise: a later section that
/// starts on the same page, in the next column or on the next odd or
/// even page (each starts on the next page), columns of different widths
/// (drawn equal), and page numbering a later section restarts (numbering
/// continues).
fn docx_sections(xml: &[u8], member: &str, report: &mut CompatibilityReport) {
    let regions = sect_pr_regions(xml);
    if regions.len() < 2 {
        return;
    }
    let later = &regions[1..];
    if later.iter().flat_map(|r| element_tags(r, b"<w:type")).any(|t| attr_values(t, b"w:val=\"").first().is_some_and(|v| *v != b"nextPage")) {
        record_why(report, SECTION_KINDS, member, "every section starts on a new page; one meant to continue on the same page, in the next column, or on an odd or even page does not");
    }
    if regions.iter().any(|r| !element_tags(r, b"<w:col").is_empty() || find(r, b"w:equalWidth=\"0\"").is_some() || find(r, b"w:equalWidth=\"false\"").is_some()) {
        record_why(report, SECTION_COLUMNS, member, "a section's columns are drawn and saved equally wide");
    }
    if later.iter().flat_map(|r| element_tags(r, b"<w:pgNumType")).any(|t| !attr_values(t, b"w:start=\"").is_empty()) {
        record_why(report, PAGE_NUMBER_RESTART, member, "page numbers continue through every section");
    }
}

/// What an ODF document of several master pages holds that the section
/// model draws otherwise: page numbering restarted where a master page
/// starts (`style:page-number` with a number, not "auto"); numbering
/// continues.
fn odt_sections(xml: &[u8], member: &str, report: &mut CompatibilityReport) {
    let restarts = para_prop_regions(xml)
        .iter()
        .flat_map(|r| attr_values(r, b"style:page-number=\""))
        .any(|v| !v.is_empty() && v.iter().all(u8::is_ascii_digit));
    if restarts && find(xml, b"style:master-page-name=\"").is_some() {
        record_why(report, PAGE_NUMBER_RESTART, member, "page numbers continue through every section");
    }
}

/// What an ODF style part holds that the model drops: widow/orphan
/// counts past two (which collapse to controlled), leader styles past
/// dotted, dashed and solid (wave draws as its character, if any), and
/// border looks past solid, dashed, dotted and double.
fn odt_typography(xml: &[u8], member: &str, report: &mut CompatibilityReport) {
    let regions = para_prop_regions(xml);
    let counts = ["fo:widows=\"", "fo:orphans=\""].iter().any(|attr| {
        regions.iter().flat_map(|r| attr_values(r, attr.as_bytes())).any(|v| {
            v.iter().filter(|b| b.is_ascii_digit()).count() == v.len()
                && v.iter().fold(0u32, |n, b| n * 10 + u32::from(b - b'0')) > 2
        })
    });
    if counts {
        record_why(report, WIDOW_COUNTS, member, "Letters controls widows and orphans, or lets them strand; counts past two collapse to controlled");
    }
    const LEADERS: &[&[u8]] = &[b"none", b"dotted", b"dashed", b"solid"];
    if regions.iter().flat_map(|r| attr_values(r, b"style:leader-style=\"")).any(|v| !LEADERS.contains(&v)) {
        record_why(report, TAB_LEADERS, member, "Letters keeps dotted, dashed and solid tab leaders; anything else draws as its character, if any");
    }
    const BORDERS: &[&[u8]] = &[b"none", b"solid", b"dashed", b"dotted", b"double"];
    // The look is the second word ("0.5pt solid #000000"), or the only
    // word: Writer spells no border `fo:border="none"`, which drops
    // nothing and must stay quiet.
    let borders = ["fo:border=\"", "fo:border-top=\"", "fo:border-left=\"", "fo:border-bottom=\"", "fo:border-right=\""]
        .iter()
        .any(|attr| {
            regions.iter().flat_map(|r| attr_values(r, attr.as_bytes())).any(|v| {
                let mut words = v.split(|b| b.is_ascii_whitespace());
                match (words.next(), words.next()) {
                    (_, Some(look)) => !BORDERS.contains(&look),
                    (Some(look), None) => !BORDERS.contains(&look),
                    (None, _) => false,
                }
            })
        });
    if borders {
        record_why(report, PARA_BORDERS, member, "Letters keeps solid, dashed, dotted and double paragraph borders; anything else is dropped");
    }
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

/// The bytes of the package member `name`, read within budget, if the
/// package holds it.
fn member_bytes(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<Vec<u8>> {
    let mut entry = zip.by_name(name).ok()?;
    ZipBudget::default().read_entry(&mut entry, name).ok()
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
    let Ok(names) = zip.file_names().map(|n| n.map(|n| n.into_owned())).collect::<Result<BTreeSet<String>, _>>() else { return report };
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
        if let Some(xml) = member_bytes(&mut zip, "word/document.xml") {
            docx_typography(&xml, "word/document.xml", &mut report);
            docx_sections(&xml, "word/document.xml", &mut report);
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
        for member in ["content.xml", "styles.xml"] {
            if let Some(xml) = member_bytes(&mut zip, member) {
                odt_typography(&xml, member, &mut report);
                if member == "content.xml" {
                    odt_sections(&xml, member, &mut report);
                }
            }
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

    fn package_with(path: &std::path::Path, marker: &str, members: &[(&str, &str)]) {
        let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        w.start_file(marker, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(b"x").unwrap();
        for (n, body) in members {
            w.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(body.as_bytes()).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn unmodelled_paragraph_borders_and_leaders_are_named() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.docx");
        package_with(&p, "[Content_Types].xml", &[(
            "word/document.xml",
            "<w:body><w:p><w:pPr><w:pBdr><w:bottom w:val=\"wave\" w:sz=\"6\" w:color=\"FF0000\"/></w:pBdr>\
             <w:tabs><w:tab w:val=\"right\" w:pos=\"4320\" w:leader=\"heavy\"/></w:tabs></w:pPr>\
             <w:r><w:t>text</w:t></w:r></w:p></w:body>",
        )]);
        assert_eq!(ids(&content_a_save_drops(&p)), ["para-border-styles", "tab-leader-styles"]);
    }

    #[test]
    fn modelled_paragraph_borders_and_leaders_are_quiet() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.docx");
        package_with(&p, "[Content_Types].xml", &[(
            "word/document.xml",
            "<w:body><w:p><w:pPr><w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:color=\"000000\"/></w:pBdr>\
             <w:tabs><w:tab w:val=\"right\" w:pos=\"4320\" w:leader=\"dot\"/></w:tabs></w:pPr>\
             <w:r><w:t>text</w:t></w:r></w:p></w:body>",
        )]);
        assert!(ids(&content_a_save_drops(&p)).is_empty());
    }

    #[test]
    fn odt_widow_counts_leaders_and_borders_are_named() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.odt");
        package_with(&p, "META-INF/manifest.xml", &[(
            "content.xml",
            "<office:body><style:style style:name=\"P1\" style:family=\"paragraph\">\
             <style:paragraph-properties fo:widows=\"3\" fo:orphans=\"3\" fo:border-bottom=\"0.5pt groove #000000\">\
             <style:tab-stops><style:tab-stop style:position=\"10cm\" style:type=\"right\" \
             style:leader-style=\"wave\" style:leader-text=\"~\"/></style:tab-stops>\
             </style:paragraph-properties></style:style></office:body>",
        )]);
        assert_eq!(ids(&content_a_save_drops(&p)), ["widow-orphan-counts", "tab-leader-styles", "para-border-styles"]);
    }

    #[test]
    fn writer_spelling_no_border_stays_quiet() {
        // Writer spells no border `fo:border="none"`: one word, no
        // look to lose. Flagging it blocked every save of its own
        // files with a loss dialog about nothing (file-corpus odt).
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.odt");
        package_with(&p, "META-INF/manifest.xml", &[(
            "content.xml",
            "<office:body><style:style style:name=\"P1\" style:family=\"paragraph\">\
             <style:paragraph-properties fo:border=\"none\" fo:widows=\"2\" fo:orphans=\"2\"/>\
             </style:style></office:body>",
        )]);
        assert!(ids(&content_a_save_drops(&p)).is_empty());
    }

    /// A Word file of several sections says what the section model draws
    /// otherwise: a continuous break, unequal columns and a numbering
    /// restart in a later section. One section, or next-page breaks with
    /// equal columns (what Letters writes), loses nothing.
    #[test]
    fn unmodelled_section_kinds_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sections.docx");
        package_with(&p, "[Content_Types].xml", &[(
            "word/document.xml",
            "<w:body><w:p><w:pPr><w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/></w:sectPr></w:pPr></w:p>\
             <w:p/><w:sectPr><w:type w:val=\"continuous\"/><w:pgNumType w:start=\"1\"/>\
             <w:cols w:num=\"2\" w:equalWidth=\"0\"><w:col w:w=\"3000\"/><w:col w:w=\"5000\"/></w:cols></w:sectPr></w:body>",
        )]);
        assert_eq!(ids(&content_a_save_drops(&p)), ["section-kinds", "section-unequal-columns", "page-number-restart"]);

        let quiet = dir.path().join("quiet.docx");
        package_with(&quiet, "[Content_Types].xml", &[(
            "word/document.xml",
            "<w:body><w:p><w:pPr><w:sectPr><w:type w:val=\"nextPage\"/><w:cols w:num=\"2\" w:space=\"480\"/></w:sectPr></w:pPr></w:p>\
             <w:p/><w:sectPr><w:type w:val=\"nextPage\"/><w:pgNumType w:fmt=\"decimal\"/></w:sectPr></w:body>",
        )]);
        assert!(ids(&content_a_save_drops(&quiet)).is_empty());

        let mut doc = crate::model::Document::from_plain_text("one\ntwo");
        let ops = crate::sections::insert_break(&doc, crate::edit::paragraph_start(&doc, 1), crate::model::PageGeometry::default()).unwrap();
        crate::edit::apply_all(&mut doc, &ops).unwrap();
        let ours = dir.path().join("ours.docx");
        crate::save::write(&doc, &ours).unwrap();
        assert!(ids(&content_a_save_drops(&ours)).is_empty(), "a file Letters wrote loses nothing");
    }
}
