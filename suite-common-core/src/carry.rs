//! Parts of a package a save carries over from the file it replaces (#1274).
//!
//! The apps write a document from their model, so a save drops every
//! member of the source package the model doesn't hold. Most of those are
//! content the loss question names before the save (macros, pivots,
//! comments a format can't keep). A few are safe to keep as they were
//! whatever the edit: nothing the app rewrites refers to them, and they
//! refer to nothing the app rewrites.
//!
//! | Family | Carried | Why it is safe |
//! |---|---|---|
//! | OOXML | `docProps/thumbnail.*` | the package's preview image, related from `_rels/.rels` only |
//! | OOXML | `docProps/custom.xml` | custom document properties, related from `_rels/.rels` only |
//! | OOXML | `customXml/**` | custom XML data parts and their own properties; their relationships stay inside `customXml/` |
//! | ODF | `Thumbnails/**` | the package's preview image |
//! | ODF | `settings.xml` | application settings (view, zoom), when the writer wrote none |
//!
//! Everything else the model doesn't hold is unsafe to pass through after
//! an edit: it is referenced by, or refers into, parts the writer
//! regenerates (a slide's comments, a sheet's drawing). Those are never
//! copied half-consistent; the loss question reports them.
//!
//! A carried part keeps the package consistent: OOXML gets the part's
//! content type in `[Content_Types].xml` and, for a package-level part,
//! its relationship in `_rels/.rels`; ODF gets its manifest entry.
//! [`problems`] checks exactly that, for tests and for anyone else.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use crate::atomic_save::atomic_write_bytes;
use crate::zip_guard::{BoundedArchive, ZipBudget};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Ooxml,
    Odf,
}

#[derive(Clone, Debug)]
struct Part {
    name: String,
    bytes: Vec<u8>,
    /// OOXML content type or ODF media type.
    media_type: String,
    /// The type of the package-level relationship that names this part
    /// (OOXML `_rels/.rels`), if one does.
    package_rel: Option<String>,
}

/// The safe parts of a source package, ready to be added to the package a
/// save writes in its place.
#[derive(Clone, Debug, Default)]
pub struct Carried {
    family: Option<Family>,
    parts: Vec<Part>,
}

fn is_safe(family: Family, name: &str) -> bool {
    match family {
        Family::Ooxml => {
            name.starts_with("customXml/") || name.starts_with("docProps/thumbnail") || name == "docProps/custom.xml"
        }
        Family::Odf => name.starts_with("Thumbnails/") || name == "settings.xml",
    }
}

fn family_of(names: &BTreeSet<String>) -> Option<Family> {
    if names.contains("[Content_Types].xml") {
        Some(Family::Ooxml)
    } else if names.contains("META-INF/manifest.xml") {
        Some(Family::Odf)
    } else {
        None
    }
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    Some(&tag[at..at + tag[at..].find('"')?])
}

/// Every `<tag .../>` (or open tag) named `element` in `xml`.
fn tags<'a>(xml: &'a str, element: &str) -> Vec<&'a str> {
    let open = format!("<{element} ");
    xml.match_indices(&open)
        .filter_map(|(at, _)| xml[at..].find('>').map(|end| &xml[at..at + end + 1]))
        .collect()
}

fn read_names<R: Read + std::io::Seek>(zip: &ZipArchive<R>) -> BTreeSet<String> {
    zip.file_names().filter(|n| !n.ends_with('/')).map(str::to_string).collect()
}

/// The content type `[Content_Types].xml` gives part `name`.
fn content_type(types: &str, name: &str) -> Option<String> {
    let part = format!("/{name}");
    for t in tags(types, "Override") {
        if attr(t, "PartName").is_some_and(|p| p.eq_ignore_ascii_case(&part)) {
            return attr(t, "ContentType").map(str::to_string);
        }
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    tags(types, "Default")
        .into_iter()
        .find(|t| attr(t, "Extension").is_some_and(|e| e.eq_ignore_ascii_case(&ext)))
        .and_then(|t| attr(t, "ContentType").map(str::to_string))
}

/// The safe parts of the package at `source`; none when it isn't a package
/// or can't be read.
pub fn capture(source: &Path) -> Carried {
    let Ok(file) = File::open(source) else { return Carried::default() };
    let Ok(mut zip) = ZipArchive::new(file) else { return Carried::default() };
    let mut budget = ZipBudget::default();
    if budget.check_entry_count(zip.len()).is_err() {
        return Carried::default();
    }
    let names = read_names(&zip);
    let Some(family) = family_of(&names) else { return Carried::default() };
    let (types, rels, manifest) = match family {
        Family::Ooxml => (
            zip.optional_part_to_string("[Content_Types].xml", &mut budget),
            zip.optional_part_to_string("_rels/.rels", &mut budget),
            String::new(),
        ),
        Family::Odf => (String::new(), String::new(), zip.optional_part_to_string("META-INF/manifest.xml", &mut budget)),
    };
    let mut parts = Vec::new();
    for name in names.iter().filter(|n| is_safe(family, n)) {
        let Ok(bytes) = zip.part_to_bytes(name, &mut budget) else { continue };
        let (media_type, package_rel) = match family {
            Family::Ooxml => {
                let rel = tags(&rels, "Relationship")
                    .into_iter()
                    .find(|r| attr(r, "Target").is_some_and(|t| t.trim_start_matches('/') == name))
                    .and_then(|r| attr(r, "Type").map(str::to_string));
                (content_type(&types, name).unwrap_or_default(), rel)
            }
            Family::Odf => {
                let mt = tags(&manifest, "manifest:file-entry")
                    .into_iter()
                    .find(|e| attr(e, "manifest:full-path") == Some(name.as_str()))
                    .and_then(|e| attr(e, "manifest:media-type").map(str::to_string))
                    .unwrap_or_default();
                (mt, None)
            }
        };
        parts.push(Part { name: name.clone(), bytes, media_type, package_rel });
    }
    Carried { family: Some(family), parts }
}

impl Carried {
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// The carried members' names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().map(|p| p.name.as_str())
    }

    /// Add the carried parts to the package just written at `target`,
    /// with their content types, package relationships or manifest
    /// entries. A part the writer wrote itself is left as written; a
    /// package of the other family (a docx saved as odt), or a file that
    /// isn't a package at all, carries nothing.
    pub fn append_to(&self, target: &Path) -> Result<(), String> {
        if self.is_empty() {
            return Ok(());
        }
        let written = std::fs::read(target).map_err(|e| format!("open saved package: {e}"))?;
        match self.apply(&written)? {
            Some(bytes) => atomic_write_bytes(target, &bytes),
            None => Ok(()),
        }
    }

    /// Save to `target` with the carried parts, in one atomic replace.
    ///
    /// `write` is the app's writer, which takes a path and picks the
    /// format from its extension. Writing to `target` and then adding the
    /// parts would replace the file twice, and a crash, or a reader,
    /// between the two would see the package without them. So the writer
    /// writes a staging file beside `target` with the same extension; the
    /// parts are added to its bytes; and those bytes replace `target` once,
    /// through `atomic_write_bytes`. The staging file carries the atomic
    /// save's temporary prefix, so one a crash strands is swept like any
    /// other. With nothing to carry, `write` writes `target` itself.
    pub fn write_with(&self, target: &Path, write: impl FnOnce(&Path) -> Result<(), String>) -> Result<(), String> {
        if self.is_empty() {
            return write(target);
        }
        let dir = match target.parent() {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => Path::new("."),
        };
        let suffix = target.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        let staging = tempfile::Builder::new()
            .prefix(crate::atomic_save::TEMP_PREFIX)
            .suffix(&suffix)
            .tempfile_in(dir)
            .map_err(|e| format!("Failed to create save file: {e}"))?
            .into_temp_path();
        write(&staging)?;
        let written = std::fs::read(&staging).map_err(|e| format!("read saved package: {e}"))?;
        let bytes = self.apply(&written)?.unwrap_or(written);
        atomic_write_bytes(target, &bytes)
        // `staging` is removed when it drops.
    }

    /// `written` with the carried parts added, or `None` when there is
    /// nothing to add to it.
    fn apply(&self, written: &[u8]) -> Result<Option<Vec<u8>>, String> {
        let Some(family) = self.family else { return Ok(None) };
        // A save into a format that isn't a package (a docx saved as .txt)
        // has nothing to carry into.
        let Ok(mut zip) = ZipArchive::new(Cursor::new(written)) else { return Ok(None) };
        let names = read_names(&zip);
        if family_of(&names) != Some(family) {
            return Ok(None);
        }
        let add: Vec<&Part> = self.parts.iter().filter(|p| !names.contains(&p.name)).collect();
        if add.is_empty() {
            return Ok(None);
        }
        let mut buffer = Vec::new();
        {
            let mut out = ZipWriter::new(Cursor::new(&mut buffer));
            let deflated = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            for i in 0..zip.len() {
                let mut entry = zip.by_index(i).map_err(|e| format!("read saved entry: {e}"))?;
                if entry.is_dir() {
                    continue;
                }
                let name = entry.name().to_string();
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).map_err(|e| format!("read saved {name}: {e}"))?;
                let bytes = match (family, name.as_str()) {
                    (Family::Ooxml, "[Content_Types].xml") => with_content_types(&bytes, &add),
                    (Family::Ooxml, "_rels/.rels") => with_package_rels(&bytes, &add),
                    (Family::Odf, "META-INF/manifest.xml") => with_manifest_entries(&bytes, &add),
                    _ => bytes,
                };
                // ODF's mimetype stays first and uncompressed.
                out.start_file(&name, if name == "mimetype" { stored } else { deflated }).map_err(|e| format!("write {name}: {e}"))?;
                out.write_all(&bytes).map_err(|e| format!("write {name}: {e}"))?;
            }
            for part in &add {
                out.start_file(&part.name, deflated).map_err(|e| format!("write {}: {e}", part.name))?;
                out.write_all(&part.bytes).map_err(|e| format!("write {}: {e}", part.name))?;
            }
            out.finish().map_err(|e| format!("finish package: {e}"))?;
        }
        Ok(Some(buffer))
    }
}

fn insert_before(xml: &[u8], close: &str, add: &str) -> Vec<u8> {
    let text = String::from_utf8_lossy(xml);
    match text.rfind(close) {
        Some(at) => format!("{}{add}{}", &text[..at], &text[at..]).into_bytes(),
        None => xml.to_vec(),
    }
}

fn xml_attr(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;")
}

fn with_content_types(xml: &[u8], add: &[&Part]) -> Vec<u8> {
    let text = String::from_utf8_lossy(xml).into_owned();
    let mut extra = String::new();
    for p in add {
        if p.name.ends_with(".rels") || p.media_type.is_empty() || content_type(&text, &p.name).as_deref() == Some(p.media_type.as_str()) {
            continue;
        }
        extra.push_str(&format!("<Override PartName=\"/{}\" ContentType=\"{}\"/>", xml_attr(&p.name), xml_attr(&p.media_type)));
    }
    insert_before(xml, "</Types>", &extra)
}

fn with_package_rels(xml: &[u8], add: &[&Part]) -> Vec<u8> {
    let text = String::from_utf8_lossy(xml).into_owned();
    let mut extra = String::new();
    for (i, p) in add.iter().enumerate() {
        let Some(kind) = &p.package_rel else { continue };
        if tags(&text, "Relationship").iter().any(|r| attr(r, "Type") == Some(kind.as_str())) {
            continue;
        }
        extra.push_str(&format!(
            "<Relationship Id=\"rIdCarried{}\" Type=\"{}\" Target=\"{}\"/>",
            i + 1,
            xml_attr(kind),
            xml_attr(&p.name)
        ));
    }
    insert_before(xml, "</Relationships>", &extra)
}

fn with_manifest_entries(xml: &[u8], add: &[&Part]) -> Vec<u8> {
    let mut extra = String::new();
    for p in add {
        extra.push_str(&format!(
            "<manifest:file-entry manifest:full-path=\"{}\" manifest:media-type=\"{}\"/>",
            xml_attr(&p.name),
            xml_attr(&p.media_type)
        ));
    }
    insert_before(xml, "</manifest:manifest>", &extra)
}

/// What is inconsistent in the package at `path`: for OOXML a part with
/// no content type, an override for a part that isn't there, or an
/// internal relationship whose target isn't there; for ODF a manifest
/// entry for a missing member, or a member the manifest doesn't list.
pub fn problems(path: &Path) -> Vec<String> {
    let Ok(file) = File::open(path) else { return vec![format!("can't open {}", path.display())] };
    let Ok(mut zip) = ZipArchive::new(file) else { return vec![format!("{} is not a zip package", path.display())] };
    let mut budget = ZipBudget::default();
    let names = read_names(&zip);
    let mut out = Vec::new();
    match family_of(&names) {
        Some(Family::Ooxml) => {
            let types = zip.optional_part_to_string("[Content_Types].xml", &mut budget);
            for name in names.iter().filter(|n| *n != "[Content_Types].xml") {
                if content_type(&types, name).is_none() {
                    out.push(format!("{name} has no content type"));
                }
            }
            for t in tags(&types, "Override") {
                if let Some(part) = attr(t, "PartName") {
                    if !names.contains(part.trim_start_matches('/')) {
                        out.push(format!("an override names {part}, which isn't in the package"));
                    }
                }
            }
            for rels in names.iter().filter(|n| n.ends_with(".rels")) {
                let xml = zip.optional_part_to_string(rels, &mut budget);
                // `word/_rels/document.xml.rels` relates from `word/`.
                let base = rels.rsplit_once("_rels/").map_or("", |(dir, _)| dir);
                for r in tags(&xml, "Relationship") {
                    if attr(r, "TargetMode") == Some("External") {
                        continue;
                    }
                    let Some(target) = attr(r, "Target") else { continue };
                    let resolved = resolve(base, target);
                    if !names.contains(&resolved) {
                        out.push(format!("{rels} relates to {target}, which isn't in the package"));
                    }
                }
            }
        }
        Some(Family::Odf) => {
            let manifest = zip.optional_part_to_string("META-INF/manifest.xml", &mut budget);
            let listed: BTreeSet<String> = tags(&manifest, "manifest:file-entry")
                .into_iter()
                .filter_map(|e| attr(e, "manifest:full-path").map(str::to_string))
                .collect();
            for entry in &listed {
                if entry != "/" && !entry.ends_with('/') && !names.contains(entry) {
                    out.push(format!("the manifest lists {entry}, which isn't in the package"));
                }
            }
            for name in &names {
                if name != "mimetype" && name != "META-INF/manifest.xml" && !listed.contains(name) {
                    out.push(format!("{name} isn't in the manifest"));
                }
            }
        }
        None => out.push(format!("{} is neither OOXML nor ODF", path.display())),
    }
    out
}

/// `target` relative to the directory `base` ("word/"), as a member name.
fn resolve(base: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
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

    fn package(path: &Path, parts: &[(&str, &str)]) {
        let mut w = ZipWriter::new(File::create(path).unwrap());
        for (name, data) in parts {
            let opt = SimpleFileOptions::default().compression_method(if *name == "mimetype" { zip::CompressionMethod::Stored } else { zip::CompressionMethod::Deflated });
            w.start_file(*name, opt).unwrap();
            w.write_all(data.as_bytes()).unwrap();
        }
        w.finish().unwrap();
    }

    const THUMB: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail";

    fn docx_source(path: &Path) {
        package(path, &[
            ("[Content_Types].xml", r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="jpeg" ContentType="image/jpeg"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/customXml/itemProps1.xml" ContentType="application/vnd.openxmlformats-officedocument.customXmlProperties+xml"/><Override PartName="/word/vbaProject.bin" ContentType="application/vnd.ms-office.vbaProject"/></Types>"#),
            ("_rels/.rels", &format!(r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="{THUMB}" Target="docProps/thumbnail.jpeg"/></Relationships>"#)),
            ("word/document.xml", "<w:document/>"),
            ("docProps/thumbnail.jpeg", "JPEGBYTES"),
            ("customXml/item1.xml", "<data>kept</data>"),
            ("customXml/itemProps1.xml", "<ds:datastoreItem/>"),
            ("customXml/_rels/item1.xml.rels", r#"<Relationships><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXmlProps" Target="itemProps1.xml"/></Relationships>"#),
            ("word/vbaProject.bin", "MACROS"),
        ]);
    }

    fn written_docx(path: &Path) {
        package(path, &[
            ("[Content_Types].xml", r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#),
            ("_rels/.rels", r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#),
            ("word/document.xml", "<w:document>edited</w:document>"),
        ]);
    }

    fn members(path: &Path) -> BTreeSet<String> {
        read_names(&ZipArchive::new(File::open(path).unwrap()).unwrap())
    }

    fn read(path: &Path, name: &str) -> String {
        let mut s = String::new();
        ZipArchive::new(File::open(path).unwrap()).unwrap().by_name(name).unwrap().read_to_string(&mut s).unwrap();
        s
    }

    #[test]
    fn safe_ooxml_parts_are_carried_with_their_types_and_relationships() {
        let dir = tempfile::tempdir().unwrap();
        let (src, out) = (dir.path().join("src.docx"), dir.path().join("out.docx"));
        docx_source(&src);
        written_docx(&out);
        let carried = capture(&src);
        assert_eq!(carried.names().collect::<Vec<_>>(), ["customXml/_rels/item1.xml.rels", "customXml/item1.xml", "customXml/itemProps1.xml", "docProps/thumbnail.jpeg"]);
        carried.append_to(&out).unwrap();
        let names = members(&out);
        assert!(names.contains("docProps/thumbnail.jpeg") && names.contains("customXml/item1.xml"));
        assert!(!names.contains("word/vbaProject.bin"), "an unsafe part is not passed through");
        assert_eq!(read(&out, "word/document.xml"), "<w:document>edited</w:document>", "the written parts are the written ones");
        assert!(read(&out, "_rels/.rels").contains(&format!("Type=\"{THUMB}\" Target=\"docProps/thumbnail.jpeg\"")));
        assert!(read(&out, "[Content_Types].xml").contains("PartName=\"/docProps/thumbnail.jpeg\" ContentType=\"image/jpeg\""));
        assert_eq!(problems(&out), Vec::<String>::new());
    }

    #[test]
    fn safe_odf_parts_are_carried_with_their_manifest_entries() {
        let dir = tempfile::tempdir().unwrap();
        let (src, out) = (dir.path().join("src.odt"), dir.path().join("out.odt"));
        let manifest = |extra: &str| format!(r#"<?xml version="1.0"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.text"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>{extra}</manifest:manifest>"#);
        package(&src, &[
            ("mimetype", "application/vnd.oasis.opendocument.text"),
            ("META-INF/manifest.xml", &manifest(r#"<manifest:file-entry manifest:full-path="Thumbnails/thumbnail.png" manifest:media-type="image/png"/><manifest:file-entry manifest:full-path="settings.xml" manifest:media-type="text/xml"/><manifest:file-entry manifest:full-path="Basic/script.xml" manifest:media-type="text/xml"/>"#)),
            ("content.xml", "<old/>"),
            ("Thumbnails/thumbnail.png", "PNGBYTES"),
            ("settings.xml", "<office:document-settings/>"),
            ("Basic/script.xml", "<macro/>"),
        ]);
        package(&out, &[("mimetype", "application/vnd.oasis.opendocument.text"), ("META-INF/manifest.xml", &manifest("")), ("content.xml", "<new/>")]);
        let carried = capture(&src);
        carried.append_to(&out).unwrap();
        let names = members(&out);
        assert!(names.contains("Thumbnails/thumbnail.png") && names.contains("settings.xml"));
        assert!(!names.contains("Basic/script.xml"), "macros are not passed through");
        assert_eq!(problems(&out), Vec::<String>::new());
        let mut zip = ZipArchive::new(File::open(&out).unwrap()).unwrap();
        let first = zip.by_index(0).unwrap();
        assert_eq!((first.name(), first.compression()), ("mimetype", zip::CompressionMethod::Stored), "mimetype stays first, stored");
    }

    #[test]
    fn nothing_crosses_families_and_a_written_part_wins() {
        let dir = tempfile::tempdir().unwrap();
        let (src, out) = (dir.path().join("src.docx"), dir.path().join("out.odt"));
        docx_source(&src);
        package(&out, &[("mimetype", "application/vnd.oasis.opendocument.text"), ("META-INF/manifest.xml", "<manifest:manifest></manifest:manifest>"), ("content.xml", "<x/>")]);
        let before = std::fs::read(&out).unwrap();
        capture(&src).append_to(&out).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), before, "a docx's parts don't go into an odt");

        let out = dir.path().join("own.docx");
        written_docx(&out);
        let mut z = ZipWriter::new_append(std::fs::OpenOptions::new().read(true).write(true).open(&out).unwrap()).unwrap();
        z.start_file("customXml/item1.xml", SimpleFileOptions::default()).unwrap();
        z.write_all(b"<data>written</data>").unwrap();
        z.finish().unwrap();
        capture(&src).append_to(&out).unwrap();
        assert_eq!(read(&out, "customXml/item1.xml"), "<data>written</data>");

        let out = dir.path().join("plain.txt");
        std::fs::write(&out, "just text").unwrap();
        capture(&src).append_to(&out).unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "just text", "a save as plain text carries nothing and doesn't fail");
    }

    #[test]
    fn write_with_replaces_the_target_once_with_the_parts_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let (src, out) = (dir.path().join("src.docx"), dir.path().join("out.docx"));
        docx_source(&src);
        std::fs::write(&out, "the old file").unwrap();
        let carried = capture(&src);

        let failed = carried.write_with(&out, |_| Err("disk full".to_string()));
        assert_eq!(failed, Err("disk full".to_string()));
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "the old file", "a failed write leaves the target alone");

        let mut staged = None;
        carried
            .write_with(&out, |p| {
                assert_ne!(p, out.as_path(), "the writer writes a staging file, not the target");
                assert_eq!(p.extension(), out.extension(), "with the target's extension, which picks the format");
                assert_eq!(std::fs::read_to_string(&out).unwrap(), "the old file", "the target is untouched while the writer runs");
                staged = Some(p.to_path_buf());
                written_docx(p);
                Ok(())
            })
            .unwrap();
        assert!(members(&out).contains("customXml/item1.xml"));
        assert_eq!(read(&out, "word/document.xml"), "<w:document>edited</w:document>");
        assert_eq!(problems(&out), Vec::<String>::new());
        assert!(!staged.unwrap().exists(), "the staging file is removed");
        let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left.len(), 2, "only the source and the target: {left:?}");

        // Nothing to carry: the writer writes the target itself.
        Carried::default().write_with(&out, |p| {
            assert_eq!(p, out.as_path());
            std::fs::write(p, "direct").map_err(|e| e.to_string())
        }).unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "direct");
    }

    #[test]
    fn problems_finds_dangling_relationships_and_missing_types() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bad.docx");
        package(&p, &[
            ("[Content_Types].xml", r#"<Types><Default Extension="rels" ContentType="r"/><Override PartName="/word/document.xml" ContentType="d"/><Override PartName="/word/gone.xml" ContentType="g"/></Types>"#),
            ("_rels/.rels", r#"<Relationships><Relationship Id="rId1" Type="t" Target="word/document.xml"/><Relationship Id="rId2" Type="t" Target="docProps/missing.xml"/><Relationship Id="rId3" Type="h" Target="https://example.com" TargetMode="External"/></Relationships>"#),
            ("word/document.xml", "<w/>"),
            ("word/media/image1.png", "PNG"),
        ]);
        let found = problems(&p);
        assert!(found.iter().any(|f| f.contains("image1.png has no content type")), "{found:?}");
        assert!(found.iter().any(|f| f.contains("/word/gone.xml")), "{found:?}");
        assert!(found.iter().any(|f| f.contains("docProps/missing.xml")), "{found:?}");
        assert_eq!(found.len(), 3, "{found:?}");
        assert_eq!(resolve("word/", "../customXml/item1.xml"), "customXml/item1.xml");
    }
}
