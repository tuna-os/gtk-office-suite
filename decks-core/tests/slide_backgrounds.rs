// slide_backgrounds.rs — a slide filled with a picture keeps it.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// DCK-004, reduced from #1397: the pay centre impact assessment opens on a
// title slide whose background is a picture (`p:bg` → `a:blipFill`,
// stretched 1% past each side by `a:fillRect l="-1000" r="-1000"`) under a
// white title. The reader dropped the picture at import, the first wrong
// stage, so the title was white on white and every save lost it.

use std::io::{Read, Write};

use decks_core::engine::{read_pptx, write_pptx, BackgroundPicture, Deck, FillRect, SlideObject};

/// A 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00,
    0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00,
    0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x9E,
    0xDD, 0x22, 0x71, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// A copy of the package at `path` with `name` replaced by `f` of it.
fn rewrite(path: &std::path::Path, name: &str, f: impl Fn(&str) -> String) -> std::path::PathBuf {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let out = path.with_extension("patched.pptx");
    let mut w = zip::ZipWriter::new(std::fs::File::create(&out).unwrap());
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let entry_name = entry.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if entry_name == name {
            bytes = f(&String::from_utf8(bytes).unwrap()).into_bytes();
        }
        w.start_file(entry_name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(&bytes).unwrap();
    }
    w.finish().unwrap();
    out
}

fn part(path: &std::path::Path, name: &str) -> String {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut s = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

/// The pay centre title slide, reduced: a slide filled with a picture the
/// way PowerPoint writes it, and a title. The package comes from our writer
/// with a picture on the slide (so the media part and its relationship
/// exist); the slide part is then replaced by PowerPoint's markup, which
/// names that picture only as the background.
fn powerpoint_picture_slide(dir: &std::path::Path, bg: &str) -> std::path::PathBuf {
    let src = dir.join("navy.png");
    std::fs::write(&src, PNG).unwrap();
    let mut d = Deck::new();
    d.slides[0].objects.push(SlideObject::Image { path: src.to_string_lossy().into_owned(), x: 0.0, y: 0.0, w: 10.0, h: 10.0, rotation: 0.0, crop: Default::default() });
    let path = dir.join("ours.pptx");
    write_pptx(path.to_str().unwrap(), &d).unwrap();
    let rels = part(&path, "ppt/slides/_rels/slide1.xml.rels");
    assert!(rels.contains("Id=\"rId1\"") && rels.contains("media/image1"), "{rels}");
    let bg = bg.to_string();
    rewrite(&path, "ppt/slides/slide1.xml", move |_| {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld>{bg}<p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id="7" name="Title 1"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="549328" y="2534443"/><a:ext cx="9259690" cy="1789113"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr anchor="ctr"/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US" sz="4400"><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill></a:rPr><a:t>Pay Centre Impact Assessment</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#
        )
    })
}

/// PowerPoint's `p:bg` of the pay centre title slide: a `a:lum` child in
/// the blip, an empty `a:srcRect`, stretched 1% past left and right.
const PAY_CENTRE_BG: &str = r#"<p:bg><p:bgPr><a:blipFill dpi="0" rotWithShape="1"><a:blip r:embed="rId1"><a:lum/></a:blip><a:srcRect/><a:stretch><a:fillRect l="-1000" r="-1000"/></a:stretch></a:blipFill><a:effectLst/></p:bgPr></p:bg>"#;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn a_powerpoint_picture_background_is_the_slides_background() {
    let dir = tempfile::tempdir().unwrap();
    let path = powerpoint_picture_slide(dir.path(), PAY_CENTRE_BG);
    let d = read_pptx(path.to_str().unwrap()).unwrap();
    let s = &d.slides[0];
    let pic = s.background_picture.as_ref().expect("the slide's background picture");
    assert_eq!(std::fs::read(&pic.path).unwrap(), PNG);
    assert!(close(pic.fill.left, -0.01) && close(pic.fill.right, -0.01) && close(pic.fill.top, 0.0) && close(pic.fill.bottom, 0.0), "{:?}", pic.fill);
    // A background, not a picture on the slide, and no colour invented.
    assert!(s.objects.iter().all(|o| !matches!(o, SlideObject::Image { .. })), "{:?}", s.objects);
    assert_eq!(s.background, "", "a picture background states no colour");
    let title = s.objects.iter().find_map(|o| match o {
        SlideObject::TextBox { text, runs, .. } => Some((text.trim().to_string(), runs.iter().any(|r| r.style.color.as_deref() == Some("ffffff")))),
        _ => None,
    });
    assert_eq!(title, Some(("Pay Centre Impact Assessment".to_string(), true)), "{:?}", s.objects);
}

/// A colour inside the picture (a duotone recolour) belongs to the
/// picture: it is not the slide's background colour.
#[test]
fn a_colour_inside_the_background_picture_is_not_the_background_colour() {
    let dir = tempfile::tempdir().unwrap();
    let bg = r#"<p:bg><p:bgPr><a:blipFill><a:blip r:embed="rId1"><a:duotone><a:prstClr val="black"/><a:srgbClr val="FF0000"/></a:duotone></a:blip><a:stretch><a:fillRect/></a:stretch></a:blipFill></p:bgPr></p:bg>"#;
    let d = read_pptx(powerpoint_picture_slide(dir.path(), bg).to_str().unwrap()).unwrap();
    assert_eq!(d.slides[0].background, "");
    let pic = d.slides[0].background_picture.as_ref().expect("the picture");
    assert!(pic.fill.is_none(), "{:?}", pic.fill);
}

/// A background naming a relationship the slide does not have is no
/// picture, not an error: the save's loss report names the dangling one.
#[test]
fn a_background_naming_no_picture_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let bg = r#"<p:bg><p:bgPr><a:blipFill><a:blip r:embed="rId99"/><a:stretch><a:fillRect/></a:stretch></a:blipFill></p:bgPr></p:bg>"#;
    let d = read_pptx(powerpoint_picture_slide(dir.path(), bg).to_str().unwrap()).unwrap();
    assert!(d.slides[0].background_picture.is_none());
}

fn deck_with_background(dir: &std::path::Path, fill: FillRect) -> Deck {
    let src = dir.join("bg.png");
    std::fs::write(&src, PNG).unwrap();
    let mut d = Deck::new();
    d.slides[0].background_picture = Some(BackgroundPicture { path: src.to_string_lossy().into_owned(), fill });
    d
}

#[test]
fn a_background_picture_and_its_fill_rect_survive_a_pptx_save() {
    let dir = tempfile::tempdir().unwrap();
    let fill = FillRect { left: -0.01, right: -0.01, ..Default::default() };
    let d = deck_with_background(dir.path(), fill);
    let path = dir.path().join("bg.pptx");
    write_pptx(path.to_str().unwrap(), &d).unwrap();
    let slide = part(&path, "ppt/slides/slide1.xml");
    assert!(slide.contains(r#"<a:fillRect l="-1000" r="-1000"/>"#), "{slide}");
    let back = read_pptx(path.to_str().unwrap()).unwrap();
    let got = back.slides[0].background_picture.as_ref().expect("the background picture");
    assert_eq!(std::fs::read(&got.path).unwrap(), PNG);
    assert_eq!(got.fill, fill);
    assert!(back.slides[0].objects.is_empty(), "a background, not a picture on the slide: {:?}", back.slides[0].objects);
}

/// ODF has no `a:fillRect`: the picture fills the page, and comes back as
/// the page's background rather than a picture on it.
#[test]
fn a_background_picture_survives_an_odp_save() {
    let dir = tempfile::tempdir().unwrap();
    let d = deck_with_background(dir.path(), FillRect::default());
    let path = dir.path().join("bg.odp");
    decks_core::odp::write(&d, path.to_str().unwrap()).unwrap();
    let content = part(&path, "content.xml");
    assert!(content.contains(r#"draw:fill="bitmap""#), "{content}");
    let back = decks_core::odp::read(path.to_str().unwrap()).unwrap();
    let got = back.slides[0].background_picture.as_ref().expect("the background picture");
    assert_eq!(std::fs::read(&got.path).unwrap(), PNG);
    assert!(back.slides[0].objects.is_empty(), "{:?}", back.slides[0].objects);
}

/// An unrelated edit, undone and redone, leaves the background picture as
/// it was, and the next save keeps it.
#[test]
fn editing_the_title_keeps_the_background_picture() {
    let dir = tempfile::tempdir().unwrap();
    let path = powerpoint_picture_slide(dir.path(), PAY_CENTRE_BG);
    let d = read_pptx(path.to_str().unwrap()).unwrap();
    let before = d.slides[0].background_picture.clone();
    let ctl = decks_core::controller::DecksController::new(d.slides, d.masters);
    let deck = ctl.deck();
    let (slide, title) = (deck.slides[0].ids.slide, deck.slides[0].ids.objects[0]);
    ctl.change_text(slide, title, "Pay Centre Impact Assessment, revised".into());
    assert!(ctl.undo());
    assert!(ctl.redo());
    let edited = ctl.deck();
    assert_eq!(edited.slides[0].background_picture, before);
    let out = dir.path().join("edited.pptx");
    write_pptx(out.to_str().unwrap(), &edited).unwrap();
    let back = read_pptx(out.to_str().unwrap()).unwrap();
    let got = back.slides[0].background_picture.as_ref().expect("kept");
    assert_eq!(std::fs::read(&got.path).unwrap(), PNG);
    assert_eq!(Some(got.fill), before.map(|b| b.fill));
}
