// pptx_media.rs — how a saved deck stores its pictures (#1984).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The pptx writer named every picture part `imageN.png`, whatever the file
// was, and stored a picture once per use: a JPEG photo became a `.png` part
// holding JPEG bytes, and a logo on thirty slides was stored thirty times.

use std::io::Read;

use decks_core::engine::{read_pptx, write_pptx_bytes, BackgroundPicture, Deck, SlideObject};

/// The start of a JPEG: SOI, a JFIF APP0 and a 300x400 SOF0, then EOI.
const JPEG: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x01, 0x2C, 0x01, 0x90, 0x03, 0x01, 0x11, 0x00, 0x02,
    0x11, 0x00, 0x03, 0x11, 0x00, 0xFF, 0xD9,
];

fn image(path: &str) -> SlideObject {
    SlideObject::Image { path: path.to_string(), x: 10.0, y: 10.0, w: 100.0, h: 80.0, rotation: 0.0, crop: Default::default() }
}

fn parts(bytes: &[u8]) -> zip::ZipArchive<std::io::Cursor<Vec<u8>>> {
    zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap()
}

fn media_names(zip: &zip::ZipArchive<std::io::Cursor<Vec<u8>>>) -> Vec<String> {
    let mut names: Vec<String> = zip.file_names().filter_map(Result::ok).map(|n| n.to_string()).filter(|n| n.starts_with("ppt/media/")).collect();
    names.sort();
    names
}

fn read_part(zip: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>, name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut out).unwrap();
    out
}

#[test]
fn a_jpeg_is_saved_as_a_jpeg_part() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("photo.jpeg");
    std::fs::write(&src, JPEG).unwrap();
    let mut deck = Deck::new();
    deck.slides[0].objects.push(image(src.to_str().unwrap()));
    let bytes = write_pptx_bytes(&deck).unwrap();
    let mut zip = parts(&bytes);
    assert_eq!(media_names(&zip), vec!["ppt/media/image1.jpg"]);
    assert_eq!(read_part(&mut zip, "ppt/media/image1.jpg"), JPEG);
    let types = String::from_utf8(read_part(&mut zip, "[Content_Types].xml")).unwrap();
    assert!(types.contains("<Default Extension=\"jpg\" ContentType=\"image/jpeg\"/>"), "{types}");
    let rels = String::from_utf8(read_part(&mut zip, "ppt/slides/_rels/slide1.xml.rels")).unwrap();
    assert!(rels.contains("Target=\"../media/image1.jpg\""), "{rels}");
}

#[test]
fn a_picture_without_a_known_extension_is_named_by_its_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("download");
    std::fs::write(&src, JPEG).unwrap();
    let mut deck = Deck::new();
    deck.slides[0].objects.push(image(src.to_str().unwrap()));
    let zip = parts(&write_pptx_bytes(&deck).unwrap());
    assert_eq!(media_names(&zip), vec!["ppt/media/image1.jpg"]);
}

#[test]
fn a_picture_used_on_many_slides_is_stored_once() {
    let dir = tempfile::tempdir().unwrap();
    let logo = dir.path().join("logo.jpg");
    let other = dir.path().join("other.jpg");
    std::fs::write(&logo, JPEG).unwrap();
    std::fs::write(&other, JPEG).unwrap();
    let logo = logo.to_str().unwrap().to_string();
    let mut deck = Deck::new();
    let mut slide = deck.slides[0].clone();
    slide.objects = vec![image(&logo), image(&logo)];
    slide.background_picture = Some(BackgroundPicture { path: logo.clone(), fill: Default::default() });
    deck.slides = vec![slide.clone(), slide.clone(), slide];
    deck.slides[2].objects.push(image(other.to_str().unwrap()));
    deck.masters[0].shapes.push(image(&logo));
    let bytes = write_pptx_bytes(&deck).unwrap();
    let zip = parts(&bytes);
    assert_eq!(media_names(&zip), vec!["ppt/media/image1.jpg", "ppt/media/image2.jpg"], "one part per file, not per use");

    // Every use still shows the picture when the file is read back.
    let path = dir.path().join("deck.pptx");
    std::fs::write(&path, &bytes).unwrap();
    let back = read_pptx(path.to_str().unwrap()).unwrap();
    assert_eq!(back.slides.len(), 3);
    for (i, s) in back.slides.iter().enumerate() {
        let pictures: Vec<Vec<u8>> = s
            .objects
            .iter()
            .filter_map(|o| match o {
                SlideObject::Image { path, .. } => Some(std::fs::read(path).unwrap()),
                _ => None,
            })
            .collect();
        assert_eq!(pictures.len(), if i == 2 { 3 } else { 2 }, "slide {i}");
        assert!(pictures.iter().all(|p| p == JPEG), "slide {i}");
        let bg = s.background_picture.as_ref().unwrap_or_else(|| panic!("slide {i} keeps its background"));
        assert_eq!(std::fs::read(&bg.path).unwrap(), JPEG);
    }
}
