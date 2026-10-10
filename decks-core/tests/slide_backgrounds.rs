// slide_backgrounds.rs — a slide filled with a picture keeps it.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The CCRM and pay centre decks open on a title slide whose background is
// a picture (`p:bg` `a:blipFill`): the reader dropped it, and the slide's
// white title was drawn on white.

use decks_core::engine::{read_pptx, write_pptx, Deck};

/// A 1x1 PNG.
fn png() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41,
        0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x9E, 0xDD, 0x22, 0x71, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

fn deck_with_background(dir: &std::path::Path) -> (Deck, Vec<u8>) {
    let bytes = png();
    let src = dir.join("bg.png");
    std::fs::write(&src, &bytes).unwrap();
    let mut d = Deck::new();
    d.slides[0].background_image = Some(src.to_string_lossy().into_owned());
    (d, bytes)
}

#[test]
fn a_background_picture_survives_a_pptx_save() {
    let dir = tempfile::tempdir().unwrap();
    let (d, bytes) = deck_with_background(dir.path());
    let path = dir.path().join("bg.pptx");
    write_pptx(path.to_str().unwrap(), &d).unwrap();
    let back = read_pptx(path.to_str().unwrap()).unwrap();
    let got = back.slides[0].background_image.as_deref().expect("the background picture");
    assert_eq!(std::fs::read(got).unwrap(), bytes);
    assert!(back.slides[0].objects.is_empty(), "a background, not a picture on the slide: {:?}", back.slides[0].objects);
}

#[test]
fn a_background_picture_survives_an_odp_save() {
    let dir = tempfile::tempdir().unwrap();
    let (d, bytes) = deck_with_background(dir.path());
    let path = dir.path().join("bg.odp");
    decks_core::odp::write(&d, path.to_str().unwrap()).unwrap();
    let back = decks_core::odp::read(path.to_str().unwrap()).unwrap();
    let got = back.slides[0].background_image.as_deref().expect("the background picture");
    assert_eq!(std::fs::read(got).unwrap(), bytes);
}
