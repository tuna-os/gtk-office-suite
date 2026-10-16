//! loss.rs — what a save would drop from the file a deck came from.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Decks writes its whole package from the model, so anything the readers
//! don't model is gone after a save over (or a Save As from) a file that
//! had it. decks-readiness.md asks that such content be "preserved or
//! blocked/warned by #374 before save"; this finds it, as a
//! `CompatibilityReport` of `WarnOnLoss` features (or `HardError` when
//! inspection fails), so the window can ask before writing
//! (`decks/src/loss_ui.rs`), as Letters does for a format that can't hold
//! what a document has.
//!
//! It reads the file on disk at save time, not a note taken at open: the
//! file a save replaces or copies from is the one that can lose something,
//! and once Decks has written it there is nothing left to warn about.
//! Planning only reads; it never edits the deck or records history. Consent
//! given for the resulting report is scoped to that source revision, target
//! and feature set: `loss_ui` re-inspects before writing and blocks the save
//! instead of reusing consent when the source no longer matches.
//!
//! DCK-001 loss-matrix inventory. Each row names its per-format disposition
//! in the shared vocabulary (`FeatureDisposition`), the fixture that proves
//! it, and where the detector looks. Plain words map as: editable means the
//! reader models it (`MustPreserve`, so no warning); safe-opaque would mean
//! byte-preserved without interpretation (not claimed for any row yet —
//! DCK-002 owns that); explicit-loss is `WarnOnLoss`: the dialog names the
//! affected slide/object/part before a destructive save; blocked is
//! `HardError`: the save cannot go ahead, and confirming cannot override it.
//!
//! | # | Content | PPTX disposition | ODP disposition | Fixture | Detector |
//! |---|---|---|---|---|---|
//! | 1 | Comments | explicit-loss (`comments`): `ppt/comments/*.xml` | explicit-loss (`comments`): annotation elements in `content.xml`/`styles.xml` | `impress-loss-pptx-v1`, `impress-loss-odp-v1` | `pptx_drops`, `odp_drops` below |
//! | 2 | Audio and video | explicit-loss (`media`): `videoFile`/`audioFile`/prefixed `media` elements | explicit-loss (`media`): `plugin` elements | `impress-loss-pptx-v1`, `impress-loss-odp-v1` | `pptx_drops`, `odp_drops` below |
//! | 3 | Embedded OLE objects | explicit-loss (`embedded-objects`): `oleObj` elements | explicit-loss (`embedded-objects`): `object-ole` elements | `impress-loss-pptx-v1`, `impress-loss-odp-v1` | `pptx_drops`, `odp_drops` below |
//! | 4 | SmartArt diagrams | explicit-loss (`smartart`): `ppt/diagrams/` parts | not applicable: ODP has no diagram part; plain shapes import as shapes | `impress-loss-pptx-v1` | `pptx_drops` below |
//! | 5 | Emphasis and motion-path animation (entrance/exit stay editable) | explicit-loss (`animations`): `presetClass` `emph`/`path` | explicit-loss (`animations`): `preset-class` `emphasis`/`motion-path` | `impress-loss-pptx-v1`, `impress-loss-odp-v1` | `pptx_drops`, `odp_drops` below |
//! | 6 | Chart series past the first (the reader keeps the first series only) | explicit-loss (`multi-series-charts`): chart parts with two or more `ser` elements | explicit-loss (`multi-series-charts`): chart objects with two or more `series` elements | `impress-loss-pptx-v1`, `impress-loss-odp-v1` | `chart_parts`, `odp_chart_objects` below |
//! | 7 | Pictures the file names but does not contain | explicit-loss (`missing-pictures`): dangling `image` relationship targets per slide | explicit-loss (`missing-pictures`): dangling image `href`s per document part | loss unit tests (a dangling target cannot ship in `interop/corpus.json`, whose validator requires every relationship/manifest target to exist) | `pptx_drops`, `odp_drops` below |
//! | 8 | A source that cannot be inspected (missing, corrupt, over budget) | blocked (`source-inspection-failed`): no save, not even a confirmed one | blocked (`source-inspection-failed`) | loss unit tests, `hostile_packages.rs`, `malformed_inputs.rs` | `inspect_source`, `scan_part` below |
//! | 9 | Anything in a package Decks itself wrote | clean: no warning in either format | clean | `package_consistency.rs`, `a_deck_decks_wrote_loses_nothing` below | — |
//! | 10 | Slides on differently-sized pages (DCK-006) | not applicable: one `p:sldSz` per deck | explicit-loss (`mixed-page-sizes`): the first slide page's size is kept | loss unit test below | `mixed_page_sizes` in `odp_drops` |
//!
//! Detection is namespace/relationship-aware bounded parsing, not
//! prefix-sensitive substring search: element and attribute names compare by
//! local name (the part after any `:`), both quote styles and spacing around
//! `=` are accepted, comments and processing instructions are skipped, and
//! relationship targets resolve against their part folder with external
//! targets left out. A part that cannot be decoded under the zip budget is a
//! hard inspection error, never an empty scan that authorizes the save.

use std::collections::BTreeSet;
use std::io::Read;

use suite_common_core::interop::{CompatibilityReport, FeatureDisposition, UnsupportedFeature};
use suite_common_core::zip_guard::{BoundedArchive, ZipBudget};

/// One kind of content Decks drops: its stable id and the label the dialog shows.
///
/// Ids are stable across releases: the dialog, the corpus fixtures and the
/// tests below all name these, so renaming one is a migration, not a rename.
const COMMENTS: (&str, &str) = ("comments", "Comments");
const MEDIA: (&str, &str) = ("media", "Audio and video");
const OLE: (&str, &str) = ("embedded-objects", "Embedded objects");
const SMARTART: (&str, &str) = ("smartart", "SmartArt graphics");
const ANIMATION: (&str, &str) = ("animations", "Emphasis and motion-path animations");
const CHARTS: (&str, &str) = ("multi-series-charts", "Charts with more than one series");
const MISSING: (&str, &str) = ("missing-pictures", "Pictures missing from the file");
/// Slides on differently-sized pages (DCK-006): ODP lets each `draw:page`
/// use a page layout of its own, while the deck model (and pptx) keeps one
/// size — the reader normalises to the first slide page's. A save must warn
/// rather than silently keep that choice.
const MIXED_SIZES: (&str, &str) = ("mixed-page-sizes", "Slides on differently-sized pages");

fn record(report: &mut CompatibilityReport, (id, label): (&str, &str), location: &str, detail: &str) {
    // Deduplicate on the full triple, not just id and location: one slide
    // can name two missing pictures, and the shared `record` would keep only
    // the first. Every finding names its affected part in `location` or
    // `detail`, so dropping the second would hide a loss the dialog promised.
    if !report.features.iter().any(|existing| existing.id == id && existing.location == location && existing.detail == detail) {
        report.features.push(UnsupportedFeature::new(id, label, location, FeatureDisposition::WarnOnLoss, detail));
    }
}

/// The content of the presentation at `path` that a save of the deck read
/// from it would lose. A source that cannot be inspected blocks ordinary
/// saving: missing is not a new document, whose caller has no source.
/// A separately confirmed recovery copy does not claim to preserve source
/// content. This is namespace/relationship-aware bounded parsing, not full
/// package/XML validation.
pub fn content_a_save_drops(path: &str) -> CompatibilityReport {
    content_a_save_drops_with_budget(path, &mut ZipBudget::default())
}

/// Whether an explicitly unverified recovery copy has a fresh destination,
/// distinct from the source. Never replace an existing entry, including a
/// symlink; canonical parent directories also catch aliases of a missing
/// source. Call again after confirmation, before writing.
pub fn can_save_unverified_copy(source: &str, target: &str) -> bool {
    use std::path::Path;
    let source = Path::new(source);
    let target = Path::new(target);
    if !matches!(std::fs::symlink_metadata(target), Err(e) if e.kind() == std::io::ErrorKind::NotFound) {
        return false;
    }
    let location = |path: &Path| {
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
        let name = path.file_name().ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        std::fs::canonicalize(parent).map(|parent| parent.join(name))
    };
    let Ok(target) = location(target) else { return false };
    // A dangling source symlink could name the proposed destination.
    match std::fs::symlink_metadata(source) {
        Ok(metadata) if metadata.file_type().is_symlink() =>
            std::fs::canonicalize(source).is_ok_and(|source| source != target),
        Ok(_) => location(source).is_ok_and(|source| source != target),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => match location(source) {
            Ok(source) => source != target,
            // The source's parent has disappeared; it cannot be the fresh
            // target in the existing parent directory established above.
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        },
        Err(_) => false,
    }
}

fn content_a_save_drops_with_budget(path: &str, budget: &mut ZipBudget) -> CompatibilityReport {
    let odp = path.to_lowercase().ends_with(".odp");
    let mut report = CompatibilityReport::new(if odp { "odp" } else { "pptx" });
    if let Err(detail) = inspect_source(path, odp, budget, &mut report) {
        report.record(UnsupportedFeature::new(
            "source-inspection-failed",
            "Source content could not be checked",
            path,
            FeatureDisposition::HardError,
            detail,
        ));
    }
    report
}

fn inspect_source(path: &str, odp: bool, budget: &mut ZipBudget, report: &mut CompatibilityReport) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("open source: {e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("read source package: {e}"))?;
    budget.check_entry_count(zip.len())?;
    let names = zip
        .file_names()
        .map(|n| n.map(|n| n.into_owned()))
        .collect::<Result<BTreeSet<String>, _>>()
        .map_err(|e| format!("read source member name: {e}"))?;
    if odp {
        odp_drops(&mut zip, &names, budget, report)
    } else {
        pptx_drops(&mut zip, &names, budget, report)
    }
}

fn scan_part<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, name: &str, budget: &mut ZipBudget) -> Result<String, String> {
    zip.part_to_string(name, budget).map_err(|e| format!("inspect {name}: {e}"))
}

fn pptx_drops<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, names: &BTreeSet<String>, budget: &mut ZipBudget, report: &mut CompatibilityReport) -> Result<(), String> {
    // Comment parts themselves: PowerPoint writes an authors list
    // (ppt/authors.xml, ppt/commentAuthors.xml) into decks with no comments.
    if names.iter().any(|n| n.starts_with("ppt/comments/") && n.ends_with(".xml")) {
        record(report, COMMENTS, "ppt/comments", "comments on slides are not kept");
    }
    if names.iter().any(|n| n.starts_with("ppt/diagrams/")) {
        record(report, SMARTART, "ppt/diagrams", "SmartArt is not kept");
    }
    for slide in names.iter().filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml")) {
        let xml = scan_part(zip, slide, budget)?;
        let tags = start_tags(&xml);
        if tags.iter().any(|t| matches!(t.local, "videoFile" | "audioFile" | "media")) {
            record(report, MEDIA, slide, "audio and video are not kept");
        }
        if tags.iter().any(|t| t.local == "oleObj" || t.local.eq_ignore_ascii_case("oleobject")) {
            record(report, OLE, slide, "embedded objects are not kept");
        }
        if tags.iter().any(|t| matches!(attr_value(t.attrs, "presetClass"), Some("emph" | "path"))) {
            record(report, ANIMATION, slide, "only entrance and exit builds are kept");
        }
        let rels = slide.replacen("ppt/slides/", "ppt/slides/_rels/", 1) + ".rels";
        // A slide need not have relationships. If the part exists, failure
        // to inspect it is not evidence that all its pictures are present.
        if !names.contains(&rels) {
            continue;
        }
        let rels_xml = scan_part(zip, &rels, budget)?;
        for target in internal_targets(&rels_xml, "/image") {
            if !names.contains(&resolve("ppt/slides", &target)) {
                record(report, MISSING, slide, &format!("{target} is not in the file"));
            }
        }
    }
    for chart in chart_parts(zip, names, budget, report)? {
        let xml = scan_part(zip, &chart, budget)?;
        if start_tags(&xml).iter().filter(|t| t.local == "ser").count() > 1 {
            record(report, CHARTS, &chart, "only the first chart series is kept");
        }
    }
    Ok(())
}

/// Every DrawingML chart part the package's relationships name, resolved to
/// a part path. A failure to inspect a relationships part is a hard error,
/// not an empty chart list that would authorize the save.
fn chart_parts<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    names: &BTreeSet<String>,
    budget: &mut ZipBudget,
    report: &mut CompatibilityReport,
) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let rels_parts: Vec<&String> = names.iter().filter(|n| n.ends_with(".rels")).collect();
    for rels_name in rels_parts {
        let xml = scan_part(zip, rels_name, budget)?;
        let base = rels_base(rels_name);
        for target in internal_targets(&xml, "/chart") {
            let part = resolve(&base, &target);
            if names.contains(&part) {
                if !out.contains(&part) {
                    out.push(part);
                }
            } else {
                // A chart the relationships name but the package does not
                // contain: the save cannot keep what is not there to read.
                record(report, MISSING, rels_name.as_str(), &format!("{target} is not in the file"));
            }
        }
    }
    Ok(out)
}

/// The folder relationships in `rels_path` resolve against: the part folder
/// holding the relationships directory (`ppt/slides/_rels/slide1.xml.rels`
/// resolves against `ppt/slides`; the package root `_rels/.rels` against
/// the root).
fn rels_base(rels_path: &str) -> String {
    let mut parts: Vec<&str> = rels_path.split('/').filter(|p| !p.is_empty()).collect();
    parts.pop();
    if parts.last() == Some(&"_rels") {
        parts.pop();
    }
    parts.join("/")
}

fn odp_drops<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, names: &BTreeSet<String>, budget: &mut ZipBudget, report: &mut CompatibilityReport) -> Result<(), String> {
    // Master and layout artwork lives in styles.xml; content.xml alone would
    // miss a comment, a movie or a dangling picture the master names.
    // styles.xml is optional: its absence is not an inspection failure.
    let mut parts = vec!["content.xml".to_string()];
    if names.contains("styles.xml") {
        parts.push("styles.xml".to_string());
    }
    for part in &parts {
        let xml = scan_part(zip, part, budget)?;
        let tags = start_tags(&xml);
        if tags.iter().any(|t| t.local == "annotation") {
            record(report, COMMENTS, part, "comments on slides are not kept");
        }
        if tags.iter().any(|t| t.local == "plugin") {
            record(report, MEDIA, part, "audio and video are not kept");
        }
        if tags.iter().any(|t| t.local == "object-ole") {
            record(report, OLE, part, "embedded objects are not kept");
        }
        if tags.iter().any(|t| matches!(attr_value(t.attrs, "preset-class"), Some("emphasis" | "motion-path"))) {
            record(report, ANIMATION, part, "only entrance and exit builds are kept");
        }
        for tag in tags.iter().filter(|t| t.local == "image") {
            if let Some(href) = attr_value(tag.attrs, "href") {
                let internal = !href.contains(':') && !href.starts_with('/') && !href.starts_with("../");
                if internal && !names.contains(href.trim_start_matches("./")) {
                    record(report, MISSING, part, &format!("{href} is not in the file"));
                }
            }
        }
    }
    for object in odp_chart_objects(names) {
        let xml = scan_part(zip, &object, budget)?;
        let tags = start_tags(&xml);
        if tags.iter().any(|t| t.local == "chart") && tags.iter().filter(|t| t.local == "series").count() > 1 {
            record(report, CHARTS, &object, "only the first chart series is kept");
        }
    }
    mixed_page_sizes(zip, names, budget, report)?;
    Ok(())
}

/// Warn when an ODP's slides use page layouts of different sizes: the
/// reader keeps the first slide page's size on the deck and normalises the
/// rest onto it, so saving (either format) bakes that choice in.
/// Attribute names compare by local name, so writer-chosen prefixes never
/// matter; a `page-layout-properties` that is not its layout's first child
/// element is not that layout's and is ignored rather than misattributed.
fn mixed_page_sizes<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    names: &BTreeSet<String>,
    budget: &mut ZipBudget,
    report: &mut CompatibilityReport,
) -> Result<(), String> {
    use std::collections::HashMap;
    let content = scan_part(zip, "content.xml", budget)?;
    let styles = if names.contains("styles.xml") {
        scan_part(zip, "styles.xml", budget)?
    } else {
        String::new()
    };
    // Page-layout name to (width, height) in points.
    let mut layouts: HashMap<String, (f64, f64)> = HashMap::new();
    for xml in [&content, &styles] {
        let tags = start_tags(xml);
        for (j, t) in tags.iter().enumerate() {
            if t.local != "page-layout-properties" {
                continue;
            }
            let Some(prev) = j.checked_sub(1).and_then(|i| tags.get(i)) else { continue };
            if prev.local != "page-layout" {
                continue;
            }
            let (Some(name), Some(w), Some(h)) = (
                attr_value(prev.attrs, "name").map(str::to_string),
                attr_value(t.attrs, "page-width").and_then(crate::odp::parse_length_pt),
                attr_value(t.attrs, "page-height").and_then(crate::odp::parse_length_pt),
            ) else {
                continue;
            };
            if w > 0.0 && h > 0.0 {
                layouts.insert(name, (w, h));
            }
        }
    }
    // Master-page name to its page-layout name (empty elements in styles.xml).
    let mut master_layout: HashMap<String, String> = HashMap::new();
    for t in start_tags(&styles).iter().filter(|t| t.local == "master-page") {
        if let (Some(m), Some(l)) = (attr_value(t.attrs, "name"), attr_value(t.attrs, "page-layout-name")) {
            master_layout.insert(m.to_string(), l.to_string());
        }
    }
    // Each slide's page size, in hundredths of a point, in document order.
    let mut sizes: BTreeSet<(i64, i64)> = BTreeSet::new();
    for t in start_tags(&content).iter().filter(|t| t.local == "page") {
        let size = attr_value(t.attrs, "master-page-name")
            .and_then(|m| master_layout.get(m))
            .and_then(|l| layouts.get(l))
            .map(|&(w, h)| ((w * 100.0).round() as i64, (h * 100.0).round() as i64));
        if let Some(size) = size {
            sizes.insert(size);
        }
    }
    if sizes.len() > 1 {
        let mut list: Vec<String> =
            sizes.iter().map(|(w, h)| format!("{}x{}pt", *w as f64 / 100.0, *h as f64 / 100.0)).collect();
        list.sort();
        record(
            report,
            MIXED_SIZES,
            "content.xml",
            &format!("slides use differently-sized pages ({}); the save keeps the first page's size", list.join(", ")),
        );
    }
    Ok(())
}

/// Every embedded chart object's `content.xml` (`Object 1/content.xml`, as
/// Impress writes one). Only parts the package actually contains: a missing
/// object is the reader's error path, not this scanner's.
fn odp_chart_objects(names: &BTreeSet<String>) -> Vec<String> {
    names
        .iter()
        .filter(|n| {
            n.ends_with("/content.xml")
                && n.split('/').next().is_some_and(|first| first.starts_with("Object "))
        })
        .cloned()
        .collect()
}

/// A start tag (empty-element tags included) with its name split into the
/// qualified name as written (`p:cTn`) and the local name after any prefix
/// (`cTn`), plus the raw attribute text.
struct StartTag<'a> {
    #[allow(dead_code)]
    qualified: &'a str,
    local: &'a str,
    attrs: &'a str,
}

/// The local part of a qualified element or attribute name: `p:cTn` is
/// `cTn`, and an unprefixed name is itself. Namespace aliases are a writer's
/// choice, so detection must never depend on the prefix.
fn local_of(qualified: &str) -> &str {
    qualified.rsplit(':').next().unwrap_or(qualified)
}

/// The end of the tag `rest` starts, quote-aware: a `>` inside an attribute
/// value does not end the tag.
fn tag_end(rest: &str) -> Option<usize> {
    let mut quote = None;
    for (idx, ch) in rest.char_indices() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                } else if ch == '>' {
                    return Some(idx);
                }
            }
        }
    }
    None
}

/// Every start tag in `xml`, skipping comments, CDATA sections, processing
/// instructions, declarations and end tags. Bounded: each iteration consumes
/// at least one byte, and no allocation happens per tag beyond the output vector.
fn start_tags(xml: &str) -> Vec<StartTag<'_>> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find('<') {
        rest = &rest[i..];
        if rest.starts_with("<![CDATA[") {
            // Character data is content, not markup: an element-looking
            // string in here must not read as a loss.
            match rest.find("]]>") {
                Some(e) => rest = &rest[e + 3..],
                None => break,
            }
            continue;
        }
        if rest.starts_with("<!--") {
            match rest.find("-->") {
                Some(e) => rest = &rest[e + 3..],
                None => break,
            }
            continue;
        }
        if rest.starts_with("<?") {
            match rest.find("?>") {
                Some(e) => rest = &rest[e + 2..],
                None => break,
            }
            continue;
        }
        if rest.starts_with("<!") {
            match tag_end(rest) {
                Some(e) => rest = &rest[e + 1..],
                None => break,
            }
            continue;
        }
        let end = match tag_end(rest) {
            Some(e) => e,
            None => break,
        };
        let mut inner = rest[1..end].trim();
        rest = &rest[end + 1..];
        if inner.starts_with('/') {
            continue;
        }
        if inner.ends_with('/') {
            inner = inner[..inner.len() - 1].trim_end();
        }
        let split = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
        let qualified = &inner[..split];
        if qualified.is_empty() {
            continue;
        }
        out.push(StartTag { qualified, local: local_of(qualified), attrs: inner[split..].trim_start() });
    }
    out
}

/// The value of the attribute named `want` (by local name, so `xlink:href`
/// is `href` whatever prefix the writer chose), accepting both quote styles
/// and spacing around `=`. Valueless attributes are skipped, not misread.
fn attr_value<'a>(attrs: &'a str, want: &str) -> Option<&'a str> {
    let bytes = attrs.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
            i += 1;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' && bytes[i] != b'/' {
            i += 1;
        }
        if start == i {
            // A stray delimiter (never valid XML, often a truncated tag):
            // skip it rather than abandoning the attributes after it.
            i += 1;
            continue;
        }
        let name = &attrs[start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let value;
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            let value_start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            value = &attrs[value_start..i.min(bytes.len())];
            i += 1;
        } else {
            let value_start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            value = &attrs[value_start..i];
        }
        if local_of(name) == want {
            return Some(value);
        }
    }
    None
}

/// The targets of the relationships in `rels` whose type ends with
/// `type_suffix`, leaving out external ones (a linked picture is a URL,
/// not a part). Relationship elements and their attributes match by local
/// name: the prefix is the writer's choice, and values accept both quotes.
fn internal_targets(rels: &str, type_suffix: &str) -> Vec<String> {
    let mut out = Vec::new();
    for tag in start_tags(rels) {
        if tag.local != "Relationship" {
            continue;
        }
        if attr_value(tag.attrs, "TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("external")) {
            continue;
        }
        let matches = attr_value(tag.attrs, "Type").is_some_and(|t| t.ends_with(type_suffix));
        if matches {
            if let Some(target) = attr_value(tag.attrs, "Target") {
                out.push(target.to_string());
            }
        }
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

    fn assert_inspection_blocked(report: &CompatibilityReport, detail: &str) {
        let failure = report.features.iter().find(|f| f.id == "source-inspection-failed")
            .expect("a failed source scan must not claim the save is clean");
        assert_eq!(failure.disposition, FeatureDisposition::HardError);
        assert!(failure.detail.contains(detail), "{}", failure.detail);
        assert!(report.validate_save(false).is_err());
        assert!(report.validate_save(true).is_err(), "Save Anyway must not override a failed inspection");
    }

    #[test]
    fn missing_or_corrupt_sources_block_even_confirmed_saves() {
        let dir = tempfile::tempdir().unwrap();
        for ext in ["pptx", "odp"] {
            let path = dir.path().join(format!("source.{ext}"));
            assert_inspection_blocked(&content_a_save_drops(path.to_str().unwrap()), "open source");
            std::fs::write(&path, b"not a ZIP package").unwrap();
            assert_inspection_blocked(&content_a_save_drops(path.to_str().unwrap()), "read source package");
        }
    }

    #[test]
    fn recovery_copy_requires_a_fresh_distinct_destination() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.pptx");
        let copy = dir.path().join("copy.pptx");
        let allowed = |target: &std::path::Path| can_save_unverified_copy(source.to_str().unwrap(), target.to_str().unwrap());
        assert!(!allowed(&source), "a missing source is not a copy destination");
        assert!(!allowed(&dir.path().join(".").join("source.pptx")));
        assert!(allowed(&copy), "a missing source can be recovered to a fresh name");
        std::fs::write(&source, b"corrupt source").unwrap();
        assert!(allowed(&copy));
        std::fs::write(&copy, b"existing destination").unwrap();
        assert!(!allowed(&copy), "recovery must not overwrite another file");
    }

    #[test]
    fn recovery_copy_handles_relative_sources_and_deleted_parent_directories() {
        let dir = tempfile::tempdir_in(".").unwrap();
        let source = dir.path().join("source.pptx");
        let absolute_source = std::fs::canonicalize(dir.path()).unwrap().join("source.pptx");
        assert!(!can_save_unverified_copy(source.to_str().unwrap(), absolute_source.to_str().unwrap()));
        let missing_parent_source = dir.path().join("deleted-directory").join("source.pptx");
        let copy = dir.path().join("copy.pptx");
        assert!(can_save_unverified_copy(missing_parent_source.to_str().unwrap(), copy.to_str().unwrap()));
        assert!(!can_save_unverified_copy(source.to_str().unwrap(), missing_parent_source.to_str().unwrap()));
    }

    #[cfg(unix)]
    #[test]
    fn recovery_copy_rejects_symlink_and_hardlink_aliases() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.pptx");
        let alias_dir = dir.path().join("alias");
        symlink(dir.path(), &alias_dir).unwrap();
        assert!(!can_save_unverified_copy(source.to_str().unwrap(), alias_dir.join("source.pptx").to_str().unwrap()));
        let link = dir.path().join("link.pptx");
        symlink(&source, &link).unwrap();
        assert!(!can_save_unverified_copy(source.to_str().unwrap(), link.to_str().unwrap()));
        assert!(!can_save_unverified_copy(link.to_str().unwrap(), source.to_str().unwrap()));
        std::fs::write(&source, b"source").unwrap();
        let hardlink = dir.path().join("hardlink.pptx");
        std::fs::hard_link(&source, &hardlink).unwrap();
        assert!(!can_save_unverified_copy(source.to_str().unwrap(), hardlink.to_str().unwrap()));
    }

    #[test]
    fn missing_required_odp_content_blocks_saving() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing-content.odp");
        package(&path, &[("mimetype", "application/vnd.oasis.opendocument.presentation")]);
        assert_inspection_blocked(&content_a_save_drops(path.to_str().unwrap()), "content.xml: missing");
    }

    #[test]
    fn absent_optional_slide_relationships_are_not_a_scan_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no-relationships.pptx");
        package(&path, &[("ppt/slides/slide1.xml", "<p:sld/>")]);
        assert!(content_a_save_drops(path.to_str().unwrap()).validate_save(false).is_ok());
    }

    #[test]
    fn corrupt_relevant_parts_block_saving_instead_of_becoming_empty() {
        let dir = tempfile::tempdir().unwrap();
        for (ext, part) in [
            ("odp", "content.xml"),
            ("pptx", "ppt/slides/slide1.xml"),
            ("pptx", "ppt/slides/_rels/slide1.xml.rels"),
        ] {
            let path = dir.path().join(format!("corrupt.{ext}"));
            for invalid_utf8 in [false, true] {
                let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
                let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
                if part.ends_with(".rels") {
                    writer.start_file("ppt/slides/slide1.xml", options).unwrap();
                    writer.write_all(b"<p:sld/>").unwrap();
                }
                writer.start_file(part, options).unwrap();
                writer.write_all(if invalid_utf8 { b"\xff" } else { b"corrupt-me" }).unwrap();
                writer.finish().unwrap();
                if !invalid_utf8 {
                    let mut bytes = std::fs::read(&path).unwrap();
                    let offset = bytes.windows(b"corrupt-me".len()).position(|s| s == b"corrupt-me").unwrap();
                    bytes[offset] ^= 1; // Leave the stored CRC unchanged.
                    std::fs::write(&path, bytes).unwrap();
                }
                assert_inspection_blocked(&content_a_save_drops(path.to_str().unwrap()), part);
            }
        }
    }

    #[test]
    fn oversized_relevant_parts_block_saving() {
        use suite_common_core::zip_guard::ZipLimits;
        let dir = tempfile::tempdir().unwrap();
        for (ext, part) in [
            ("odp", "content.xml"),
            ("pptx", "ppt/slides/slide1.xml"),
            ("pptx", "ppt/slides/_rels/slide1.xml.rels"),
        ] {
            let path = dir.path().join(format!("oversized.{ext}"));
            let large = " ".repeat(65);
            let mut parts = vec![(part, large.as_str())];
            if part.ends_with(".rels") {
                parts.push(("ppt/slides/slide1.xml", "<p:sld/>"));
            }
            package(&path, &parts);
            let mut budget = ZipBudget::new(ZipLimits { max_entry_bytes: 64, ..ZipLimits::default() });
            let report = content_a_save_drops_with_budget(path.to_str().unwrap(), &mut budget);
            assert_inspection_blocked(&report, part);
            assert_inspection_blocked(&report, "byte limit");
        }
    }

    #[test]
    fn entry_count_and_total_scan_budgets_block_saving() {
        use suite_common_core::zip_guard::ZipLimits;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("over-budget.pptx");
        package(&path, &[
            ("ppt/slides/slide1.xml", "<p:sld/>"),
            ("ppt/slides/slide2.xml", "<p:sld/>"),
        ]);
        let mut count = ZipBudget::new(ZipLimits { max_entries: 1, ..ZipLimits::default() });
        assert_inspection_blocked(&content_a_save_drops_with_budget(path.to_str().unwrap(), &mut count), "members");
        let mut total = ZipBudget::new(ZipLimits { max_total_bytes: 12, ..ZipLimits::default() });
        assert_inspection_blocked(&content_a_save_drops_with_budget(path.to_str().unwrap(), &mut total), "byte total");
    }

    #[test]
    fn scan_failure_does_not_discard_losses_already_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("comments-and-broken-slide.pptx");
        package(&path, &[
            ("ppt/comments/comment1.xml", "<p:cmLst/>"),
            ("ppt/slides/slide1.xml", "<p:sld/>"),
        ]);
        let mut budget = ZipBudget::new(suite_common_core::zip_guard::ZipLimits {
            max_entry_bytes: 1,
            ..Default::default()
        });
        let report = content_a_save_drops_with_budget(path.to_str().unwrap(), &mut budget);
        assert_eq!(ids(&report), ["comments", "source-inspection-failed"]);
        assert_inspection_blocked(&report, "ppt/slides/slide1.xml");
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
    fn loss_matrix_names_slide_and_target() {
        // DCK-001: every finding names the affected slide/object/part and
        // the reason, so the dialog can offer a copy/cancel/repair choice
        // for that content instead of a bare feature label.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("matrix.pptx");
        package(&p, &[
            ("ppt/comments/comment1.xml", "<p:cmLst/>"),
            (
                "ppt/slides/slide2.xml",
                "<p:sld><a:videoFile r:link=\"rId3\"/><p:oleObj/><p:cTn presetClass=\"emph\"/></p:sld>",
            ),
            (
                "ppt/slides/_rels/slide2.xml.rels",
                "<Relationships>\
                 <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"../media/gone.png\"/>\
                 </Relationships>",
            ),
        ]);
        let r = content_a_save_drops(p.to_str().unwrap());
        let at = |id: &str| {
            r.features.iter().filter(|f| f.id == id).map(|f| f.location.clone()).collect::<Vec<_>>()
        };
        assert_eq!(at("comments"), ["ppt/comments"]);
        for id in ["media", "embedded-objects", "animations", "missing-pictures"] {
            assert_eq!(at(id), ["ppt/slides/slide2.xml"], "{id} must name its slide");
        }
        let missing = r.features.iter().find(|f| f.id == "missing-pictures").unwrap();
        assert!(missing.detail.contains("../media/gone.png"), "{}", missing.detail);
        assert!(r.requires_confirmation());
        assert!(r.validate_save(false).is_err() && r.validate_save(true).is_ok());
    }

    #[test]
    fn namespace_aliases_do_not_bypass_loss_detection() {
        // DCK-001: prefixes are the writer's choice. The same content under
        // unfamiliar aliases, single quotes and spaced attributes must warn
        // exactly like the canonical serialization.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("aliased.pptx");
        package(&p, &[
            (
                "ppt/slides/slide1.xml",
                "<q:sld xmlns:q=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
                 <d:videoFile xmlns:d=\"http://schemas.openxmlformats.org/drawingml/2006/main\"/>\
                 <q:oleObj/>\
                 <q:cTn  w:presetClass = 'emph' xmlns:w=\"urn:x\"/>\
                 </q:sld>",
            ),
            (
                "ppt/slides/_rels/slide1.xml.rels",
                "<pkg:Relationships xmlns:pkg=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
                 <pkg:Relationship Id='rId1' Type = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/image' Target = '../media/gone.png'/>\
                 </pkg:Relationships>",
            ),
        ]);
        let r = content_a_save_drops(p.to_str().unwrap());
        assert_eq!(ids(&r), ["animations", "embedded-objects", "media", "missing-pictures"]);
        let o = dir.path().join("aliased.odp");
        package(&o, &[(
            "content.xml",
            "<office:document-content><draw:page>\
             <x:annotation xmlns:x=\"urn:oasis:names:tc:opendocument:xmlns:office\"/>\
             <y:plugin xmlns:y=\"urn:oasis:names:tc:opendocument:xmlns:drawing\"/>\
             <y:object-ole/>\
             <anim:par z:preset-class='motion-path' xmlns:z=\"urn:oasis:names:tc:opendocument:xmlns:presentation\"/>\
             </draw:page></office:document-content>",
        )]);
        assert_eq!(ids(&content_a_save_drops(o.to_str().unwrap())), ["animations", "comments", "embedded-objects", "media"]);
    }

    #[test]
    fn markup_that_is_not_markup_is_not_a_loss() {
        // Comments, CDATA and quoted attribute values that merely mention an
        // unsupported element must not warn: the scanner reads tags, not text.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("quoted.pptx");
        package(&p, &[(
            "ppt/slides/slide1.xml",
            "<p:sld><!-- <p:oleObj/> --><![CDATA[<a:videoFile/>]]>\
             <p:sp><a:t>&lt;p:oleObj&gt; and presetClass=\"emph\" as text</a:t></p:sp></p:sld>",
        )]);
        assert!(content_a_save_drops(p.to_str().unwrap()).features.is_empty());
    }

    #[test]
    fn incomplete_analysis_never_authorizes_save() {
        // DCK-001: a report the scanner could not finish is a block, however
        // the user answers. `validate_save(true)` overrides warnings, never
        // a failed inspection.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("truncated.pptx");
        // Stored, so the slide bytes sit verbatim in the archive and flipping
        // one breaks the part's CRC while leaving the archive readable.
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        writer.start_file("ppt/comments/comment1.xml", stored).unwrap();
        writer.write_all(b"<p:cmLst/>").unwrap();
        writer.start_file("ppt/slides/slide1.xml", stored).unwrap();
        writer.write_all(b"<p:sld><p:oleObj/>").unwrap();
        writer.finish().unwrap();
        // Corrupt the slide part in place so its CRC fails: the scanner must
        // report a hard error, not an empty finding list.
        let mut bytes = std::fs::read(&p).unwrap();
        let needle = b"<p:oleObj/>";
        let offset = bytes.windows(needle.len()).position(|s| s == needle).unwrap();
        bytes[offset] ^= 1;
        std::fs::write(&p, bytes).unwrap();
        let r = content_a_save_drops(p.to_str().unwrap());
        assert!(r.has_hard_errors());
        assert!(r.validate_save(false).is_err());
        assert!(r.validate_save(true).is_err(), "confirmation must not override a failed inspection");
    }

    #[test]
    fn two_missing_pictures_on_one_slide_are_two_findings() {
        // One slide can lose two pictures; the dialog must be able to name
        // both, so per-target findings must not collapse onto each other.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("two-gone.pptx");
        package(&p, &[
            ("ppt/slides/slide1.xml", "<p:sld/>"),
            (
                "ppt/slides/_rels/slide1.xml.rels",
                "<Relationships>\
                 <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"../media/a.png\"/>\
                 <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"../media/b.png\"/>\
                 </Relationships>",
            ),
        ]);
        let r = content_a_save_drops(p.to_str().unwrap());
        let mut details: Vec<&str> =
            r.features.iter().filter(|f| f.id == "missing-pictures").map(|f| f.detail.as_str()).collect();
        details.sort();
        assert_eq!(details, ["../media/a.png is not in the file", "../media/b.png is not in the file"]);
    }

    #[test]
    fn multi_series_charts_warn_in_both_formats() {
        // The readers keep the first series only (`parse_chart_space`,
        // `parse_odf_chart`); a second series is an explicit loss with the
        // chart part as its location. One series stays silent.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("series.pptx");
        package(&p, &[
            ("ppt/slides/slide1.xml", "<p:sld/>"),
            (
                "ppt/slides/_rels/slide1.xml.rels",
                "<Relationships>\
                 <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart\" Target=\"../charts/chart1.xml\"/>\
                 <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart\" Target=\"../charts/chart2.xml\"/>\
                 </Relationships>",
            ),
            (
                "ppt/charts/chart1.xml",
                "<c:chartSpace><c:chart><c:plotArea><c:barChart>\
                 <c:ser><c:idx val=\"0\"/></c:ser><c:ser><c:idx val=\"1\"/></c:ser>\
                 </c:barChart></c:plotArea></c:chart></c:chartSpace>",
            ),
            (
                "ppt/charts/chart2.xml",
                "<c:chartSpace><c:chart><c:plotArea><c:barChart>\
                 <c:ser><c:idx val=\"0\"/></c:ser>\
                 </c:barChart></c:plotArea></c:chart></c:chartSpace>",
            ),
        ]);
        let r = content_a_save_drops(p.to_str().unwrap());
        let charts: Vec<&UnsupportedFeature> = r.features.iter().filter(|f| f.id == "multi-series-charts").collect();
        assert_eq!(charts.len(), 1);
        assert_eq!(charts[0].location, "ppt/charts/chart1.xml");
        let o = dir.path().join("series.odp");
        package(&o, &[
            (
                "content.xml",
                "<office:document-content><draw:page>\
                 <draw:frame><draw:object xlink:href=\"./Object 1\"/></draw:frame>\
                 <draw:frame><draw:object xlink:href=\"./Object 2\"/></draw:frame>\
                 </draw:page></office:document-content>",
            ),
            (
                "Object 1/content.xml",
                "<office:document-content><chart:chart><chart:series/><chart:series/></chart:chart>\
                 <table:table/></office:document-content>",
            ),
            (
                "Object 2/content.xml",
                "<office:document-content><chart:chart><chart:series/></chart:chart>\
                 <table:table/></office:document-content>",
            ),
        ]);
        let r = content_a_save_drops(o.to_str().unwrap());
        let charts: Vec<&UnsupportedFeature> = r.features.iter().filter(|f| f.id == "multi-series-charts").collect();
        assert_eq!(charts.len(), 1);
        assert_eq!(charts[0].location, "Object 1/content.xml");
    }

    #[test]
    fn odp_master_artwork_in_styles_xml_is_inspected() {
        // Referenced master/layout artwork belongs to the loss boundary
        // (PR #1550's preflight): a movie or dangling picture styles.xml
        // names must warn like content.xml's do.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("master.odp");
        package(&p, &[
            ("content.xml", "<office:document-content><draw:page/></office:document-content>"),
            (
                "styles.xml",
                "<office:document-styles><draw:page>\
                 <draw:plugin xlink:href=\"reel.mp4\"/>\
                 <draw:frame><draw:image xlink:href=\"Pictures/gone.png\"/></draw:frame>\
                 </draw:page></office:document-styles>",
            ),
        ]);
        let r = content_a_save_drops(p.to_str().unwrap());
        assert_eq!(ids(&r), ["media", "missing-pictures"]);
        assert!(r.features.iter().all(|f| f.location == "styles.xml"));
    }

    #[test]
    fn relative_targets_resolve_against_the_part_folder() {
        assert_eq!(resolve("ppt/slides", "../media/a.png"), "ppt/media/a.png");
        assert_eq!(resolve("ppt/slides", "/ppt/media/a.png"), "ppt/media/a.png");
        assert_eq!(resolve("ppt/slides", "./b.png"), "ppt/slides/b.png");
    }

    /// Differently-sized slide pages warn; uniform ones do not. Aliased
    /// namespace prefixes prove the scan compares local names: `d:page`
    /// is a slide whatever prefix the writer chose.
    #[test]
    fn mixed_odp_page_sizes_warn_and_uniform_ones_do_not() {
        let dir = tempfile::tempdir().unwrap();
        let styles = |second: &str| format!(
            "<office:document-styles>\
             <office:automatic-styles>\
             <style:page-layout style:name=\"PM1\">\
             <style:page-layout-properties fo:page-width=\"28cm\" fo:page-height=\"21cm\"/>\
             </style:page-layout>\
             <style:page-layout style:name=\"PM2\">\
             <style:page-layout-properties fo:page-width=\"25cm\" fo:page-height=\"{second}\"/>\
             </style:page-layout>\
             </office:automatic-styles>\
             <office:master-styles>\
             <s:master-page s:name=\"M1\" s:page-layout-name=\"PM1\"/>\
             <s:master-page s:name=\"M2\" s:page-layout-name=\"PM2\"/>\
             </office:master-styles>\
             </office:document-styles>"
        );
        let content = |second: &str| format!(
            "<office:document-content><office:body><office:presentation>\
             <d:page d:master-page-name=\"M1\"/>\
             <d:page d:master-page-name=\"{second}\"/>\
             </office:presentation></office:body></office:document-content>"
        );
        let mixed = dir.path().join("mixed.odp");
        package(&mixed, &[("content.xml", &content("M2")), ("styles.xml", &styles("25cm"))]);
        let r = content_a_save_drops(mixed.to_str().unwrap());
        let findings: Vec<&UnsupportedFeature> =
            r.features.iter().filter(|f| f.id == "mixed-page-sizes").collect();
        assert_eq!(findings.len(), 1, "{:?}", ids(&r));
        assert!(findings[0].detail.contains("first page's size"), "{}", findings[0].detail);
        let uniform = dir.path().join("uniform.odp");
        package(&uniform, &[("content.xml", &content("M1")), ("styles.xml", &styles("21cm"))]);
        let r = content_a_save_drops(uniform.to_str().unwrap());
        assert!(!ids(&r).contains(&"mixed-page-sizes"), "{:?}", ids(&r));
    }
}
