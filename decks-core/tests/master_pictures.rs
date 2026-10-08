// master_pictures.rs — a master's pictures are drawn and kept.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The delegated authorities reports' master runs a band of coloured lines
// along the foot of every slide: a picture (`p:pic`) on the master. The
// reader took only a master's shapes, so every slide lost the band.

use decks_core::engine::{read_pptx, write_pptx, Deck, SlideObject};

/// A 1x1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
    0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63,
    0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x9E, 0xDD, 0x22, 0x71, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// A picture's bytes and its box.
type Picture = (Vec<u8>, (f64, f64, f64, f64));

fn pictures(objects: &[SlideObject]) -> Vec<Picture> {
    objects
        .iter()
        .filter_map(|o| match o {
            SlideObject::Image { path, x, y, w, h, .. } => Some((std::fs::read(path).unwrap(), (*x, *y, *w, *h))),
            _ => None,
        })
        .collect()
}

#[test]
fn a_master_picture_survives_a_pptx_save() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("band.png");
    std::fs::write(&src, PNG).unwrap();
    let mut d = Deck::new();
    let band = SlideObject::Image { path: src.to_string_lossy().into_owned(), x: 0.0, y: 480.0, w: 960.0, h: 60.0, rotation: 0.0, crop: Default::default() };
    d.masters[0].shapes.push(band);
    let path = dir.path().join("m.pptx");
    write_pptx(path.to_str().unwrap(), &d).unwrap();
    let back = read_pptx(path.to_str().unwrap()).unwrap();
    let got = pictures(&back.masters[0].shapes);
    assert_eq!(got.len(), 1, "{:?}", back.masters[0].shapes);
    assert_eq!(got[0].0, PNG);
    let (x, y, w, h) = got[0].1;
    assert!((x - 0.0).abs() < 0.5 && (y - 480.0).abs() < 0.5 && (w - 960.0).abs() < 0.5 && (h - 60.0).abs() < 0.5, "{:?}", got[0].1);
}
