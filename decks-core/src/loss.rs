//! loss.rs — what a save would drop from the file a deck came from.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Decks writes its whole package from the model, so anything the readers
//! don't model is gone after a save over (or a Save As from) a file that
//! had it: comments, audio and video, embedded OLE objects, SmartArt,
//! emphasis and motion-path animations, and pictures the file names but
//! does not contain. decks-readiness.md asks that such content be
//! "preserved or blocked/warned by #374 before save"; this finds it, as a
//! `CompatibilityReport` of `WarnOnLoss` features, so the window can ask
//! before writing (`decks/src/loss_ui.rs`), as Letters does for a format
//! that can't hold what a document has.
//!
//! It reads the file on disk at save time, not a note taken at open: the
//! file a save replaces or copies from is the one that can lose something,
//! and once Decks has written it there is nothing left to warn about.

use std::collections::BTreeSet;
use std::io::Read;

use suite_common_core::interop::{CompatibilityReport, FeatureDisposition, UnsupportedFeature};
use suite_common_core::zip_guard::{BoundedArchive, ZipBudget};

/// One kind of content Decks drops: its id and the label the dialog shows.
const COMMENTS: (&str, &str) = ("comments", "Comments");
const MEDIA: (&str, &str) = ("media", "Audio and video");
const OLE: (&str, &str) = ("embedded-objects", "Embedded objects");
const SMARTART: (&str, &str) = ("smartart", "SmartArt graphics");
const ANIMATION: (&str, &str) = ("animations", "Emphasis and motion-path animations");
const MISSING: (&str, &str) = ("missing-pictures", "Pictures missing from the file");

fn record(report: &mut CompatibilityReport, (id, label): (&str, &str), location: &str, detail: &str) {
    report.record(UnsupportedFeature::new(id, label, location, FeatureDisposition::WarnOnLoss, detail));
}

/// The content of the presentation at `path` that a save of the deck read
/// from it would lose. Empty for a file Decks wrote, a file that isn't
/// there, and anything that isn't a pptx or odp package.
pub fn content_a_save_drops(path: &str) -> CompatibilityReport {
    let odp = path.to_lowercase().ends_with(".odp");
    let mut report = CompatibilityReport::new(if odp { "odp" } else { "pptx" });
    let Ok(file) = std::fs::File::open(path) else { return report };
    let Ok(mut zip) = zip::ZipArchive::new(file) else { return report };
    let mut budget = ZipBudget::default();
    if budget.check_entry_count(zip.len()).is_err() {
        return report;
    }
    let names: BTreeSet<String> = zip.file_names().map(str::to_string).collect();
    if odp {
        odp_drops(&mut zip, &names, &mut budget, &mut report);
    } else {
        pptx_drops(&mut zip, &names, &mut budget, &mut report);
    }
    report
}

fn pptx_drops<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, names: &BTreeSet<String>, budget: &mut ZipBudget, report: &mut CompatibilityReport) {
    // Comment parts themselves: PowerPoint writes an authors list
    // (ppt/authors.xml, ppt/commentAuthors.xml) into decks with no comments.
    if names.iter().any(|n| n.starts_with("ppt/comments/") && n.ends_with(".xml")) {
        record(report, COMMENTS, "ppt/comments", "comments on slides are not kept");
    }
    if names.iter().any(|n| n.starts_with("ppt/diagrams/")) {
        record(report, SMARTART, "ppt/diagrams", "SmartArt is not kept");
    }
    let slides: Vec<String> = names.iter().filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml")).cloned().collect();
    for slide in slides {
        let xml = zip.optional_part_to_string(&slide, budget);
        if xml.contains("<a:videoFile") || xml.contains("<a:audioFile") || xml.contains("<p14:media") {
            record(report, MEDIA, &slide, "audio and video are not kept");
        }
        if xml.contains("<p:oleObj") {
            record(report, OLE, &slide, "embedded objects are not kept");
        }
        if xml.contains("presetClass=\"emph\"") || xml.contains("presetClass=\"path\"") {
            record(report, ANIMATION, &slide, "only entrance and exit builds are kept");
        }
        let rels = slide.replacen("ppt/slides/", "ppt/slides/_rels/", 1) + ".rels";
        let rels_xml = zip.optional_part_to_string(&rels, budget);
        for target in internal_targets(&rels_xml, "/image") {
            if !names.contains(&resolve("ppt/slides", &target)) {
                record(report, MISSING, &slide, &format!("{target} is not in the file"));
            }
        }
    }
}

fn odp_drops<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, names: &BTreeSet<String>, budget: &mut ZipBudget, report: &mut CompatibilityReport) {
    let content = zip.optional_part_to_string("content.xml", budget);
    if content.contains("<officeooo:annotation") || content.contains("<office:annotation") {
        record(report, COMMENTS, "content.xml", "comments on slides are not kept");
    }
    if content.contains("<draw:plugin") {
        record(report, MEDIA, "content.xml", "audio and video are not kept");
    }
    if content.contains("<draw:object-ole") {
        record(report, OLE, "content.xml", "embedded objects are not kept");
    }
    if content.contains("presentation:preset-class=\"emphasis\"") || content.contains("presentation:preset-class=\"motion-path\"") {
        record(report, ANIMATION, "content.xml", "only entrance and exit builds are kept");
    }
    for href in attr_values(&content, "<draw:image", "xlink:href") {
        let internal = !href.contains(':') && !href.starts_with('/') && !href.starts_with("../");
        if internal && !names.contains(href.trim_start_matches("./")) {
            record(report, MISSING, "content.xml", &format!("{href} is not in the file"));
        }
    }
}

/// The values of `attr` on every `element` start tag in `xml`.
fn attr_values<'a>(xml: &'a str, element: &str, attr: &str) -> Vec<&'a str> {
    let needle = format!("{attr}=\"");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(element) {
        let tag = &rest[i..];
        let end = tag.find('>').unwrap_or(tag.len());
        let tag = &tag[..end];
        if let Some(j) = tag.find(&needle) {
            let v = &tag[j + needle.len()..];
            if let Some(k) = v.find('"') {
                out.push(&v[..k]);
            }
        }
        rest = &rest[i + element.len()..];
    }
    out
}

/// The targets of the relationships in `rels` whose type ends with
/// `type_suffix`, leaving out external ones (a linked picture is a URL,
/// not a part).
fn internal_targets(rels: &str, type_suffix: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = rels;
    while let Some(i) = rest.find("<Relationship ") {
        let tag = &rest[i..];
        let tag = &tag[..tag.find('>').unwrap_or(tag.len())];
        let get = |k: &str| attr_values(tag, "<Relationship", k).first().map(|s| s.to_string());
        let external = get("TargetMode").is_some_and(|m| m == "External");
        if !external && get("Type").is_some_and(|t| t.ends_with(type_suffix)) {
            if let Some(t) = get("Target") {
                out.push(t);
            }
        }
        rest = &rest[i + 1..];
    }
    out
}

/// `target`, relative to the part folder `base`, as a package path.
fn resolve(base: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
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

    #[test]
    fn a_deck_decks_wrote_loses_nothing() {
        let dir = tempfile::tempdir().unwrap();
        for ext in ["pptx", "odp"] {
            let p = dir.path().join(format!("ours.{ext}"));
            let mut deck = crate::Deck::new();
            deck.slides[0].objects.push(crate::SlideObject::Shape {
                kind: crate::engine::shape::ShapeKind::Ellipse,
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
                rotation: 0.0,
                style: Default::default(),
            });
            crate::write_deck(p.to_str().unwrap(), &deck).unwrap();
            let r = content_a_save_drops(p.to_str().unwrap());
            assert!(r.features.is_empty(), "{ext}: {:?}", r.features);
            assert!(!r.requires_confirmation());
        }
        assert!(content_a_save_drops(dir.path().join("none.pptx").to_str().unwrap()).features.is_empty());
    }

    #[test]
    fn a_pptx_lists_comments_media_ole_smartart_animations_and_missing_pictures() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("theirs.pptx");
        package(&p, &[
            ("ppt/authors.xml", "<p188:authorLst/>"),
            ("ppt/comments/comment1.xml", "<p:cmLst/>"),
            ("ppt/diagrams/data1.xml", "<dgm:dataModel/>"),
            ("ppt/media/image1.png", "png"),
            (
                "ppt/slides/slide1.xml",
                "<p:sld><a:videoFile r:link=\"rId3\"/><p:oleObj/><p:cTn presetClass=\"emph\"/></p:sld>",
            ),
            (
                "ppt/slides/_rels/slide1.xml.rels",
                "<Relationships>\
                 <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"../media/image1.png\"/>\
                 <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"../media/gone.png\"/>\
                 <Relationship Id=\"rId4\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"https://example.org/a.png\" TargetMode=\"External\"/>\
                 </Relationships>",
            ),
        ]);
        let r = content_a_save_drops(p.to_str().unwrap());
        assert_eq!(ids(&r), ["animations", "comments", "embedded-objects", "media", "missing-pictures", "smartart"]);
        let missing: Vec<&str> = r.features.iter().filter(|f| f.id == "missing-pictures").map(|f| f.detail.as_str()).collect();
        assert_eq!(missing, ["../media/gone.png is not in the file"], "the picture that is there and the linked one are fine");
        assert!(r.requires_confirmation());
        assert!(r.validate_save(false).is_err() && r.validate_save(true).is_ok());
    }

    #[test]
    fn an_odp_lists_comments_media_ole_animations_and_missing_pictures() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("theirs.odp");
        package(&p, &[
            ("mimetype", "application/vnd.oasis.opendocument.presentation"),
            ("Pictures/here.png", "png"),
            (
                "content.xml",
                "<office:document-content><draw:page>\
                 <officeooo:annotation/><draw:plugin xlink:href=\"v.mp4\"/><draw:object-ole/>\
                 <anim:par presentation:preset-class=\"motion-path\"/>\
                 <draw:frame><draw:image xlink:href=\"Pictures/here.png\"/></draw:frame>\
                 <draw:frame><draw:image xlink:href=\"Pictures/gone.png\"/></draw:frame>\
                 <draw:frame><draw:image xlink:href=\"https://example.org/a.png\"/></draw:frame>\
                 </draw:page></office:document-content>",
            ),
        ]);
        let r = content_a_save_drops(p.to_str().unwrap());
        assert_eq!(ids(&r), ["animations", "comments", "embedded-objects", "media", "missing-pictures"]);
        assert_eq!(r.features.iter().filter(|f| f.id == "missing-pictures").count(), 1);
    }

    #[test]
    fn entrance_and_exit_builds_and_charts_are_not_losses() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("builds.odp");
        package(&p, &[(
            "content.xml",
            "<office:document-content><anim:par presentation:preset-class=\"entrance\"/>\
             <draw:frame><draw:object xlink:href=\"./Object 1\"/></draw:frame></office:document-content>",
        )]);
        assert!(content_a_save_drops(p.to_str().unwrap()).features.is_empty());
    }

    #[test]
    fn relative_targets_resolve_against_the_part_folder() {
        assert_eq!(resolve("ppt/slides", "../media/a.png"), "ppt/media/a.png");
        assert_eq!(resolve("ppt/slides", "/ppt/media/a.png"), "ppt/media/a.png");
        assert_eq!(resolve("ppt/slides", "./b.png"), "ppt/slides/b.png");
    }
}
