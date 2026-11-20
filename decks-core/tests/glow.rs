// glow.rs — a shape's glow survives both formats (#2332).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The aiva progress dashboard's rings are circles filled with the slide's
// own colour: all that shows is each one's `a:glow`. The reader dropped it,
// so the rings were not there at all.

use decks_core::engine::shape::{Color, Glow, ShapeKind, ShapeStyle, Stroke};
use decks_core::engine::{Deck, SlideObject};

fn glow() -> Glow {
    Glow { radius: 10.0, color: Color(0x00, 0xC2, 0xF3), alpha: 0.25 }
}

fn deck() -> Deck {
    let mut d = Deck::new();
    let navy = Color(0x0E, 0x28, 0x41);
    d.slides[0].objects = vec![SlideObject::Shape {
        kind: ShapeKind::Ellipse,
        x: 100.0,
        y: 100.0,
        w: 120.0,
        h: 120.0,
        rotation: 0.0,
        style: ShapeStyle {
            glow: Some(glow()),
            fill: Some(navy),
            gradient: None,
            stroke: Some(Stroke { color: navy, width: 1.0 }),
            fill_src: None,
            stroke_src: None,
        },
    }];
    d
}

fn glow_of(d: &Deck) -> Option<Glow> {
    d.slides[0].objects.iter().find_map(|o| match o {
        SlideObject::Shape { style, .. } => style.glow,
        _ => None,
    })
}

fn close(a: Glow, b: Glow) -> bool {
    (a.radius - b.radius).abs() < 0.05 && a.color == b.color && (a.alpha - b.alpha).abs() < 0.01
}

#[test]
fn a_glow_survives_both_formats() {
    let dir = tempfile::tempdir().unwrap();
    for ext in ["pptx", "odp"] {
        let path = dir.path().join(format!("g.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &deck()).unwrap();
        let back = glow_of(&decks_core::read_deck(path.to_str().unwrap()).unwrap()).unwrap_or_else(|| panic!("{ext}: no glow"));
        assert!(close(back, glow()), "{ext}: {back:?}");
    }
}

#[test]
fn the_glow_is_written_as_powerpoint_writes_it() {
    // As the aiva deck writes it: rad in EMU (a model unit is 9525), the
    // colour's alpha in thousandths of a percent.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("g.pptx");
    decks_core::write_deck(path.to_str().unwrap(), &deck()).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("ppt/slides/slide1.xml").unwrap(), &mut xml).unwrap();
    assert!(xml.contains("<a:glow rad=\"95250\"><a:srgbClr val=\"00C2F3\"><a:alpha val=\"25000\"/>"), "{xml}");
}

#[test]
fn a_shape_without_a_glow_writes_none() {
    let mut d = deck();
    if let SlideObject::Shape { style, .. } = &mut d.slides[0].objects[0] {
        style.glow = None;
    }
    let dir = tempfile::tempdir().unwrap();
    for ext in ["pptx", "odp"] {
        let path = dir.path().join(format!("n.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &d).unwrap();
        assert_eq!(glow_of(&decks_core::read_deck(path.to_str().unwrap()).unwrap()), None, "{ext}");
    }
}
