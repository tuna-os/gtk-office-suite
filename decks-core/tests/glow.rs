// glow.rs — a shape's glow and outer shadow survive both formats (#2332).
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
            shadow: None,
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

// ── Outer shadows: 119 across 13 of the real decks ──────────────────────

fn shadowed() -> Deck {
    let mut d = deck();
    if let SlideObject::Shape { style, .. } = &mut d.slides[0].objects[0] {
        style.glow = None;
        style.shadow = Some(decks_core::engine::shape::Shadow { dx: 3.0, dy: 4.0, blur: 6.0, color: Color(0, 0, 0), alpha: 0.4, rotate_with_shape: false });
    }
    d
}

fn shadow_of(d: &Deck) -> Option<decks_core::engine::shape::Shadow> {
    d.slides[0].objects.iter().find_map(|o| match o {
        SlideObject::Shape { style, .. } => style.shadow,
        _ => None,
    })
}

#[test]
fn a_shadow_survives_both_formats() {
    let want = shadow_of(&shadowed()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    for ext in ["pptx", "odp"] {
        let path = dir.path().join(format!("s.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &shadowed()).unwrap();
        let got = shadow_of(&decks_core::read_deck(path.to_str().unwrap()).unwrap()).unwrap_or_else(|| panic!("{ext}: no shadow"));
        assert!(
            (got.dx - want.dx).abs() < 0.05
                && (got.dy - want.dy).abs() < 0.05
                && (got.blur - want.blur).abs() < 0.05
                && got.color == want.color
                && (got.alpha - want.alpha).abs() < 0.01,
            "{ext}: {got:?}"
        );
    }
}

#[test]
fn a_glow_and_a_shadow_are_written_in_effect_list_order() {
    let mut d = shadowed();
    if let SlideObject::Shape { style, .. } = &mut d.slides[0].objects[0] {
        style.glow = Some(glow());
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("both.pptx");
    decks_core::write_deck(path.to_str().unwrap(), &d).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("ppt/slides/slide1.xml").unwrap(), &mut xml).unwrap();
    let (g, s) = (xml.find("<a:glow").unwrap(), xml.find("<a:outerShdw").unwrap());
    assert!(g < s, "CT_EffectList puts the glow before the outer shadow: {xml}");
    let back = decks_core::read_deck(path.to_str().unwrap()).unwrap();
    assert!(glow_of(&back).is_some() && shadow_of(&back).is_some());
}

#[test]
fn whether_a_shadow_turns_with_its_shape_survives_pptx() {
    // A rotated shape's shadow either turns with it (rotWithShape, the
    // default) or keeps its direction on the slide.
    let dir = tempfile::tempdir().unwrap();
    for turns in [true, false] {
        let mut d = shadowed();
        if let SlideObject::Shape { style, rotation, .. } = &mut d.slides[0].objects[0] {
            *rotation = 30.0;
            if let Some(sh) = &mut style.shadow {
                sh.rotate_with_shape = turns;
            }
        }
        let path = dir.path().join(format!("r{turns}.pptx"));
        decks_core::write_deck(path.to_str().unwrap(), &d).unwrap();
        let back = shadow_of(&decks_core::read_deck(path.to_str().unwrap()).unwrap()).unwrap();
        assert_eq!(back.rotate_with_shape, turns);
    }
}
