// leading_blank_paragraphs.rs — empty paragraphs ahead of the first run are lines.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// DCK-004, reduced from #1397: the pay centre impact assessment's figure tiles
// are top-anchored text boxes that open with two empty 14pt paragraphs and
// then the figure ("6,046"). PowerPoint and LibreOffice set the figure two
// lines down; the reader dropped the leading paragraphs, so we drew it at the
// top of the tile.

use std::io::{Read, Write};

use decks_core::engine::{read_pptx, write_pptx, Deck, SlideObject};

fn part(path: &std::path::Path, name: &str) -> String {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut s = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

/// A copy of the package at `path` with `name` replaced by `content`.
fn replace(path: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let out = path.with_extension("patched.pptx");
    let mut w = zip::ZipWriter::new(std::fs::File::create(&out).unwrap());
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let entry_name = entry.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if entry_name == name {
            bytes = content.as_bytes().to_vec();
        }
        w.start_file(entry_name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(&bytes).unwrap();
    }
    w.finish().unwrap();
    out
}

fn text_of(d: &Deck) -> String {
    match &d.slides[0].objects[0] {
        SlideObject::TextBox { text, .. } => text.clone(),
        other => panic!("{other:?}"),
    }
}

/// The tile's markup as PowerPoint wrote it, trimmed to the text.
fn tile() -> Deck {
    let dir = tempfile::tempdir().unwrap();
    let ours = dir.path().join("ours.pptx");
    write_pptx(ours.to_str().unwrap(), &Deck::new()).unwrap();
    let blank = r#"<a:p><a:pPr algn="ctr"/><a:endParaRPr lang="en-US" sz="1400"/></a:p>"#;
    let slide = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id="5" name="Tile"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="609600" y="1600200"/><a:ext cx="2286000" cy="1371600"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr rtlCol="0" anchor="t"/><a:lstStyle/>{blank}{blank}<a:p><a:pPr algn="ctr"/><a:r><a:rPr lang="en-US" sz="1400" b="1"/><a:t>6,046</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
    );
    let path = replace(&ours, "ppt/slides/slide1.xml", &slide);
    read_pptx(path.to_str().unwrap()).unwrap()
}

#[test]
fn leading_blank_paragraphs_are_kept_as_lines() {
    let d = tile();
    assert_eq!(text_of(&d), "\n\n6,046", "two blank lines, then the figure");
    let SlideObject::TextBox { runs, .. } = &d.slides[0].objects[0] else { unreachable!() };
    let joined: String = runs.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(joined, text_of(&d), "the runs are the text");
    assert!(runs.iter().all(|r| r.style.font_size_hp == Some(28)), "the blank lines are sized like the figure: {runs:?}");
    let SlideObject::TextBox { body, .. } = &d.slides[0].objects[0] else { unreachable!() };
    let paras = &body.paras;
    assert_eq!(paras.len(), 3, "one style per paragraph: {paras:?}");
}

#[test]
fn leading_blank_paragraphs_survive_our_pptx() {
    let d = tile();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("back.pptx");
    write_pptx(out.to_str().unwrap(), &d).unwrap();
    let xml = part(&out, "ppt/slides/slide1.xml");
    assert_eq!(xml.matches("<a:p>").count() + xml.matches("<a:p ").count(), 3, "{xml}");
    assert_eq!(text_of(&read_pptx(out.to_str().unwrap()).unwrap()), "\n\n6,046");
}

#[test]
fn leading_blank_paragraphs_survive_odp() {
    let d = tile();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("back.odp");
    decks_core::odp::write(&d, out.to_str().unwrap()).unwrap();
    let back = decks_core::odp::read(out.to_str().unwrap()).unwrap();
    assert_eq!(text_of(&back), "\n\n6,046");
}
