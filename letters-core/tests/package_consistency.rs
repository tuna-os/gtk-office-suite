//! The packages Letters writes are consistent (#1274): every part has a
//! content type or a manifest entry, and nothing relates to a part that
//! isn't there. `suite_common_core::carry::problems` is the check.

use letters_core::model::{Document, Paragraph, ParaStyle, Run, RunStyle, TableCell};
use letters_core::session::DocumentSession;
use std::collections::BTreeMap;
use std::path::Path;

fn rich() -> Document {
    let mut d = letters_core::comments::sample_document();
    for p in letters_core::track::sample_document().paragraphs {
        d.paragraphs.push(p);
    }
    d.paragraphs.push(Paragraph { style: ParaStyle { table_cell: Some(TableCell { table: 1, row: 0, col: 0 }), ..Default::default() }, runs: vec![Run::plain("cell")] });
    d.paragraphs.push(Paragraph { style: ParaStyle::default(), runs: vec![Run { text: "a link".into(), style: RunStyle { link: Some("https://example.com/a?b=1&c=2".into()), ..Default::default() } }] });
    d.footnotes.push("a note".into());
    d.paragraphs.push(Paragraph { style: ParaStyle::default(), runs: vec![Run { text: String::new(), style: RunStyle { footnote: Some(0), ..Default::default() } }] });
    d.header = Some("head".into());
    d
}

#[test]
fn docx_and_odt_packages_are_consistent() {
    let dir = tempfile::tempdir().unwrap();
    for ext in ["docx", "odt"] {
        let path = dir.path().join(format!("rich.{ext}"));
        letters_core::save::write(&rich(), &path).unwrap();
        assert_eq!(suite_common_core::carry::problems(&path), Vec::<String>::new(), "{ext}");
    }
}

// ── LET-002: opaque preservation round-trip discipline ────────────────────
// The admitted safe-part list per format (see `suite_common_core::carry`):
// DOCX carries `customXml/**`, `docProps/custom.xml` and
// `docProps/thumbnail.*`; ODT carries `Thumbnails/**` and `settings.xml`.
// Everything else the model does not hold (here a macro project) is never
// passed through: the save refuses it, and the LET-001 dialog names it from
// the session's stored findings.

fn read_members(path: &Path) -> Vec<(String, Vec<u8>)> {
    use std::io::Read;
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut members = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        members.push((name, bytes));
    }
    members
}

fn read_member(path: &Path, name: &str) -> Vec<u8> {
    use std::io::Read;
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut bytes = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
    bytes
}

fn rewrite_package(path: &Path, members: &[(String, Vec<u8>)], odf: bool) {
    use std::io::Write;
    let file = std::fs::File::create(path).unwrap();
    let mut out = zip::ZipWriter::new(file);
    for (name, bytes) in members {
        // ODF's mimetype stays first and uncompressed.
        let method = if odf && name == "mimetype" {
            zip::CompressionMethod::Stored
        } else {
            zip::CompressionMethod::Deflated
        };
        out.start_file(name, zip::write::SimpleFileOptions::default().compression_method(method))
            .unwrap();
        out.write_all(bytes).unwrap();
    }
    out.finish().unwrap();
}

fn patch_xml(members: &mut [(String, Vec<u8>)], name: &str, close: &str, extra: &str) {
    let body = members.iter_mut().find(|entry| entry.0.as_str() == name).unwrap_or_else(|| panic!("{name} the writer must emit"));
    let mut text = String::from_utf8(body.1.clone()).unwrap_or_else(|_| panic!("{name} must be UTF-8"));
    assert!(text.contains(close), "{name} must contain {close}");
    text = text.replace(close, &format!("{extra}{close}"));
    body.1 = text.into_bytes();
}

/// Add the admitted DOCX safe parts plus an unsafe macro project to the
/// package Letters wrote, keeping content types and relationships
/// consistent. Returns the safe parts' bytes for the byte-identity check.
fn inject_docx_parts(path: &Path) -> BTreeMap<String, Vec<u8>> {
    const THUMB_REL: &str =
        "http://schemas.openxmlformats.org/package/2006/relationships/metadata/thumbnail";
    const CUSTOM_REL: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties";
    const ITEM_REL: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXmlProps";
    let safe: &[(&str, &str, &str)] = &[
        ("customXml/item1.xml", "<data>kept</data>", "application/xml"),
        (
            "customXml/itemProps1.xml",
            "<ds:datastoreItem/>",
            "application/vnd.openxmlformats-officedocument.customXmlProperties+xml",
        ),
        (
            "docProps/custom.xml",
            "<Properties/>",
            "application/vnd.openxmlformats-officedocument.custom-properties+xml",
        ),
        ("docProps/thumbnail.jpeg", "JPEGBYTES", "image/jpeg"),
    ];
    let mut members = read_members(path);
    let types = String::from_utf8(
        members
            .iter()
            .find(|entry| entry.0.as_str() == "[Content_Types].xml")
            .expect("[Content_Types].xml the writer must emit")
            .1
            .clone(),
    )
    .expect("[Content_Types].xml must be UTF-8");
    let mut overrides = String::new();
    if !types.contains("Extension=\"rels\"") {
        overrides.push_str(
            "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
        );
    }
    for (name, _, media) in safe {
        overrides.push_str(&format!("<Override PartName=\"/{name}\" ContentType=\"{media}\"/>"));
    }
    overrides.push_str(
        "<Override PartName=\"/word/vbaProject.bin\" ContentType=\"application/vnd.ms-office.vbaProject\"/>",
    );
    patch_xml(&mut members, "[Content_Types].xml", "</Types>", &overrides);
    patch_xml(
        &mut members,
        "_rels/.rels",
        "</Relationships>",
        &format!(
            "<Relationship Id=\"rIdCarried1\" Type=\"{THUMB_REL}\" Target=\"docProps/thumbnail.jpeg\"/>\
             <Relationship Id=\"rIdCarried2\" Type=\"{CUSTOM_REL}\" Target=\"docProps/custom.xml\"/>"
        ),
    );
    let mut safe_bytes = BTreeMap::new();
    for (name, data, _) in safe {
        members.push((name.to_string(), data.as_bytes().to_vec()));
        safe_bytes.insert(name.to_string(), data.as_bytes().to_vec());
    }
    let item_rels = format!(
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rId1\" Type=\"{ITEM_REL}\" Target=\"itemProps1.xml\"/></Relationships>"
    )
    .into_bytes();
    members.push(("customXml/_rels/item1.xml.rels".to_string(), item_rels.clone()));
    safe_bytes.insert("customXml/_rels/item1.xml.rels".to_string(), item_rels);
    members.push(("word/vbaProject.bin".to_string(), b"MACROBYTES".to_vec()));
    rewrite_package(path, &members, false);
    safe_bytes
}

/// Add the admitted ODT safe parts plus an unsafe macro to the package
/// Letters wrote, keeping the manifest consistent. Returns the safe parts'
/// bytes for the byte-identity check.
fn inject_odt_parts(path: &Path) -> BTreeMap<String, Vec<u8>> {
    let safe: &[(&str, &[u8], &str)] = &[
        ("Thumbnails/thumbnail.png", b"PNGBYTES", "image/png"),
        ("settings.xml", b"<office:document-settings/>", "text/xml"),
    ];
    let mut members = read_members(path);
    let mut entries = String::new();
    for (name, _, media) in safe {
        entries.push_str(&format!(
            "<manifest:file-entry manifest:full-path=\"{name}\" manifest:media-type=\"{media}\"/>"
        ));
    }
    entries.push_str(
        "<manifest:file-entry manifest:full-path=\"Basic/script.xml\" manifest:media-type=\"text/xml\"/>",
    );
    patch_xml(&mut members, "META-INF/manifest.xml", "</manifest:manifest>", &entries);
    let mut safe_bytes = BTreeMap::new();
    for (name, data, _) in safe {
        members.push((name.to_string(), data.to_vec()));
        safe_bytes.insert(name.to_string(), data.to_vec());
    }
    members.push(("Basic/script.xml".to_string(), b"<macro/>".to_vec()));
    rewrite_package(path, &members, true);
    safe_bytes
}

fn table_doc(cell: &str) -> Document {
    let mut doc = Document::default();
    doc.paragraphs.push(Paragraph { style: ParaStyle::default(), runs: vec![Run::plain("intro")] });
    for col in 0..2 {
        doc.paragraphs.push(Paragraph {
            style: ParaStyle { table_cell: Some(TableCell { table: 1, row: 0, col }), ..Default::default() },
            runs: vec![Run::plain(if col == 0 { cell } else { "pad" })],
        });
    }
    doc
}

/// Edit → save → reopen through the session's import provenance (LET-001):
/// the admitted safe parts survive an unrelated table edit byte-identical,
/// the unsafe part is refused and disclosed, and the saved package stays
/// consistent.
fn round_trip_with_table_edit(
    ext: &str,
    inject: fn(&Path) -> BTreeMap<String, Vec<u8>>,
    admitted: &[&str],
    unsafe_marker: &str,
) {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join(format!("opaque.{ext}"));
    letters_core::save::write(&table_doc("before"), &source).unwrap();
    let safe_bytes = inject(&source);

    // The import provenance LET-001 pins: carried parts + loss findings come
    // from the imported revision, never a re-read of the live path.
    let slot = std::rc::Rc::new(suite_common_core::autosave::AutosaveSlot::new(dir.path(), "opaque"));
    let mut session = DocumentSession::new(slot);
    session.file = Some(source.clone());
    session.set_import_context(DocumentSession::capture_import_context(&source));

    // The admitted safe-part list, exactly: nothing more (the macro is not
    // carried), nothing less.
    let carried: Vec<String> = session.carried_for_save().names().map(|n| n.to_string()).collect();
    let mut expected: Vec<String> = admitted.iter().map(|n| n.to_string()).collect();
    expected.sort();
    assert_eq!(carried, expected, "only the admitted safe parts are carried");
    // The unsafe content routes to the LET-001 dialog, from stored findings.
    let loss: Vec<String> = session
        .stored_source_loss()
        .map(|r| r.destructive_features().iter().map(|f| f.id.clone()).collect())
        .unwrap_or_default();
    assert_eq!(loss, ["macros"], "the unsafe part must be disclosed, not passed through");

    // An unrelated table edit, saved over the source through one atomic
    // replace — the same `write_with` primitive Ctrl+S uses.
    let mut edited = table_doc("after");
    edited.paragraphs.push(Paragraph { style: ParaStyle::default(), runs: vec![Run::plain("added")] });
    let carried = session.carried_for_save();
    session
        .save_to(source.clone(), |p| {
            carried.write_with(p, |q| letters_core::save::write(&edited, q).map(|_| ()))
        })
        .unwrap();

    for (name, bytes) in &safe_bytes {
        assert_eq!(&read_member(&source, name), bytes, "{name} must survive the save byte-identical");
    }
    let names: Vec<String> = read_members(&source).into_iter().map(|(n, _)| n).collect();
    assert!(
        !names.iter().any(|n| n.contains(unsafe_marker)),
        "the unsafe part must not be passed through: {names:?}"
    );
    assert_eq!(
        suite_common_core::carry::problems(&source),
        Vec::<String>::new(),
        "the saved package must stay consistent"
    );

    let back = letters_core::save::read(&source).unwrap();
    let texts: Vec<String> =
        back.paragraphs.iter().flat_map(|p| p.runs.iter().map(|r| r.text.clone())).collect();
    assert!(texts.iter().any(|t| t.contains("after")), "the table edit must survive: {texts:?}");
    assert!(texts.iter().any(|t| t == "added"), "the added paragraph must survive: {texts:?}");
    assert!(texts.iter().any(|t| t == "intro"), "untouched text must survive: {texts:?}");
}

#[test]
fn opaque_parts_survive_unrelated_table_edit_byte_identical_docx() {
    round_trip_with_table_edit(
        "docx",
        inject_docx_parts,
        &[
            "customXml/_rels/item1.xml.rels",
            "customXml/item1.xml",
            "customXml/itemProps1.xml",
            "docProps/custom.xml",
            "docProps/thumbnail.jpeg",
        ],
        "vbaProject",
    );
}

#[test]
fn opaque_parts_survive_unrelated_table_edit_byte_identical_odt() {
    round_trip_with_table_edit(
        "odt",
        inject_odt_parts,
        &["Thumbnails/thumbnail.png", "settings.xml"],
        "Basic/",
    );
}
