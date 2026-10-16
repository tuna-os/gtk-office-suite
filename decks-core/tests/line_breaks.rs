// line_breaks.rs — a line break inside a paragraph stays one.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// DCK-004, reduced from #1397: the pay centre impact assessment's title is
// one paragraph broken over three lines with `a:br` ("Pay Centre Impact
// Assessment –" / "Unified Actions for Pay (UAP) and" / "Departmental
// Surcharge"). The reader dropped the breaks at import, so the words ran
// together ("andDepartmental") and the title wrapped elsewhere.

use std::io::{Read, Write};

use decks_core::engine::{read_pptx, write_pptx, Deck, Run, RunStyle, SlideObject, LINE_BREAK};

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

/// PowerPoint's markup for the title: breaks carrying their own `a:rPr`
/// (an element, not an empty one), and one empty break.
#[test]
fn a_powerpoint_line_break_is_a_break_inside_the_paragraph() {
    let dir = tempfile::tempdir().unwrap();
    let ours = dir.path().join("ours.pptx");
    write_pptx(ours.to_str().unwrap(), &Deck::new()).unwrap();
    let rpr = r#"<a:rPr lang="sv-SE" sz="4400"><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill></a:rPr>"#;
    let slide = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id="7" name="Title 1"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="549328" y="2534443"/><a:ext cx="9259690" cy="1789113"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r>{rpr}<a:t>Pay Centre Impact Assessment – </a:t></a:r><a:br>{rpr}</a:br><a:r>{rpr}<a:t>Unified Actions for Pay (UAP) and</a:t></a:r><a:br/><a:r>{rpr}<a:t>Departmental Surcharge</a:t></a:r></a:p><a:p><a:br/><a:r>{rpr}<a:t>November 2024</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
    );
    let path = replace(&ours, "ppt/slides/slide1.xml", &slide);
    let d = read_pptx(path.to_str().unwrap()).unwrap();
    assert_eq!(
        text_of(&d),
        format!("Pay Centre Impact Assessment – {LINE_BREAK}Unified Actions for Pay (UAP) and{LINE_BREAK}Departmental Surcharge\n{LINE_BREAK}November 2024"),
        "breaks inside the paragraph, one paragraph break, nothing welded"
    );
    let SlideObject::TextBox { runs, .. } = &d.slides[0].objects[0] else { unreachable!() };
    let joined: String = runs.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(joined, text_of(&d), "the runs are the text");
    // Written back as breaks, not characters, and read back the same.
    let out = dir.path().join("back.pptx");
    write_pptx(out.to_str().unwrap(), &d).unwrap();
    let xml = part(&out, "ppt/slides/slide1.xml");
    assert_eq!(xml.matches("<a:br/>").count(), 3, "{xml}");
    assert!(!xml.contains(LINE_BREAK), "no raw separator in a:t");
    assert_eq!(text_of(&read_pptx(out.to_str().unwrap()).unwrap()), text_of(&d));
}

fn deck() -> Deck {
    let mut d = Deck::new();
    let bold = RunStyle { bold: true, ..Default::default() };
    let first = format!("Unified Actions for Pay (UAP) and{LINE_BREAK}Departmental Surcharge");
    d.slides[0].objects = vec![SlideObject::TextBox {
        text: format!("{first}\nNovember 2024"),
        x: 50.0,
        y: 50.0,
        w: 600.0,
        h: 200.0,
        rotation: 0.0,
        runs: vec![Run { text: first, style: bold }, Run { text: "\nNovember 2024".into(), style: RunStyle::default() }],
        body: Default::default(),
    }];
    d
}

#[test]
fn a_line_break_survives_pptx_as_a_break_between_runs_of_one_look() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("br.pptx");
    write_pptx(path.to_str().unwrap(), &deck()).unwrap();
    let xml = part(&path, "ppt/slides/slide1.xml");
    assert!(xml.contains("and</a:t></a:r><a:br/><a:r>"), "an a:br between the runs: {xml}");
    let back = read_pptx(path.to_str().unwrap()).unwrap();
    assert_eq!(text_of(&back), text_of(&deck()));
    let SlideObject::TextBox { runs, .. } = &back.slides[0].objects[0] else { unreachable!() };
    assert!(runs.iter().any(|r| r.text.starts_with("Departmental") && r.style.bold), "the run after the break keeps its look: {runs:?}");
}

#[test]
fn a_line_break_survives_odp_as_a_line_break() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("br.odp");
    decks_core::odp::write(&deck(), path.to_str().unwrap()).unwrap();
    let xml = part(&path, "content.xml");
    assert!(xml.contains("<text:line-break/>"), "{xml}");
    let back = decks_core::odp::read(path.to_str().unwrap()).unwrap();
    assert_eq!(text_of(&back), text_of(&deck()));
}
