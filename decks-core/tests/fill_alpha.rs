// fill_alpha.rs — a translucent fill keeps its opacity.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// 79 shape fills across 12 real decks are translucent: the CNSC
// roundtable's SmartArt boxes are accent colours at 75% over a navy slide.
// The reader kept the colour and dropped its `a:alpha`, and the canvas
// painted them opaque.

use decks_core::engine::shape::{Color, ShapeKind, ShapeStyle};
use decks_core::engine::theme::{ColorRef, FillSrc};
use decks_core::engine::{read_pptx, write_pptx, Deck, SlideObject};

fn deck(fill_src: Option<FillSrc>) -> Deck {
    let mut d = Deck::new();
    d.slides[0].objects = vec![SlideObject::Shape {
        kind: ShapeKind::Rect,
        x: 100.0,
        y: 100.0,
        w: 200.0,
        h: 100.0,
        rotation: 0.0,
        style: ShapeStyle { glow: None, shadow: None, fill_alpha: Some(0.75), fill: Some(Color(0xC0, 0x50, 0x4D)), gradient: None, stroke: None, fill_src, stroke_src: None },
    }];
    d
}

fn style(d: &Deck) -> ShapeStyle {
    d.slides[0]
        .objects
        .iter()
        .find_map(|o| match o {
            SlideObject::Shape { style, .. } => Some(style.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no shape: {:?}", d.slides[0].objects))
}

#[test]
fn a_translucent_fill_survives_a_pptx_save() {
    let dir = tempfile::tempdir().unwrap();
    for src in [None, Some(FillSrc::Color(ColorRef::Rgb("C0504D".into())))] {
        let path = dir.path().join("a.pptx");
        write_pptx(path.to_str().unwrap(), &deck(src.clone())).unwrap();
        let back = style(&read_pptx(path.to_str().unwrap()).unwrap());
        assert_eq!(back.fill_alpha, Some(0.75), "{src:?}");
        assert_eq!(back.fill, Some(Color(0xC0, 0x50, 0x4D)));
    }
}

#[test]
fn a_translucent_fill_survives_an_odp_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.odp");
    decks_core::odp::write(&deck(None), path.to_str().unwrap()).unwrap();
    let back = style(&decks_core::odp::read(path.to_str().unwrap()).unwrap());
    assert_eq!(back.fill_alpha, Some(0.75));
}

/// The fill's opacity is the fill's: an outline beside it stays opaque,
/// literal or carried (review on #2350).
#[test]
fn a_translucent_fills_outline_stays_opaque() {
    use decks_core::engine::shape::Stroke;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.pptx");
    let mut d = deck(None);
    if let SlideObject::Shape { style, .. } = &mut d.slides[0].objects[0] {
        style.stroke = Some(Stroke { color: Color(0x20, 0x30, 0x40), width: 2.0 });
    }
    write_pptx(path.to_str().unwrap(), &d).unwrap();
    let slide = {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut s = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("ppt/slides/slide1.xml").unwrap(), &mut s).unwrap();
        s
    };
    let ln = &slide[slide.find("<a:ln").expect("an outline")..];
    let ln = &ln[..ln.find("</a:ln>").unwrap()];
    assert!(!ln.contains("a:alpha"), "{ln}");
    let back = style(&read_pptx(path.to_str().unwrap()).unwrap());
    assert_eq!(back.fill_alpha, Some(0.75));
    assert_eq!(back.stroke.map(|s| s.color), Some(Color(0x20, 0x30, 0x40)));
}

#[test]
fn an_opaque_fill_states_no_opacity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("o.pptx");
    let mut d = deck(None);
    if let SlideObject::Shape { style, .. } = &mut d.slides[0].objects[0] {
        style.fill_alpha = None;
    }
    write_pptx(path.to_str().unwrap(), &d).unwrap();
    assert_eq!(style(&read_pptx(path.to_str().unwrap()).unwrap()).fill_alpha, None);
}
