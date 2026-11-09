// theme_inheritance.rs — DCK-006: themes, references and absence survive
// both formats with their relationships intact, not just their resolved
// appearance.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// A two-master deck whose masters name different themes: each master's
// inheritors resolve the same slot to their own theme's colour, an
// explicit local fill stays put, and an absent background stays absent
// (never white smuggled in at import). Both formats, plus placeholder
// source identity through pptx.

use std::collections::BTreeMap;
use std::io::Read;

use decks_core::engine::shape::{Color, ShapeKind, ShapeStyle};
use decks_core::engine::theme::{ColorRef, FillSrc, Theme, ThemeColor};
use decks_core::engine::{MasterSlide, Slide, SlideObject, TextBody};
use decks_core::layouts::{Layout, LayoutKind, LayoutPlaceholder, Placeholder};

fn theme(id: u64, accent1: &str, source: &str) -> Theme {
    let colors: BTreeMap<String, String> = Theme::default_colors()
        .into_iter()
        .map(|(k, v)| if k == "accent1" { (k, accent1.to_string()) } else { (k, v) })
        .collect();
    Theme {
        id,
        name: format!("Theme{id}"),
        colors,
        major_font: Some("Heading Face".into()),
        minor_font: Some("Body Face".into()),
        source: source.into(),
    }
}
/// A shape with a live theme reference (an inheritor) or an explicit
/// local colour (an override). The resolved fill always agrees with the
/// reference — as an import leaves them — because the ODP writer draws
/// the cache while the pptx writer draws the reference.
fn inheritor(kind: ShapeKind, resolved: Color) -> SlideObject {
    SlideObject::Shape {
        kind,
        x: 10.0, y: 10.0, w: 100.0, h: 100.0,
        rotation: 0.0,
        style: ShapeStyle {
            fill: Some(resolved),
            gradient: None,
            stroke: None,
            fill_src: Some(FillSrc::Color(ColorRef::Theme(ThemeColor::unmodulated("accent1")))),
            stroke_src: None,
        },
    }
}

fn local(kind: ShapeKind) -> SlideObject {
    SlideObject::Shape {
        kind,
        x: 10.0, y: 10.0, w: 100.0, h: 100.0,
        rotation: 0.0,
        style: ShapeStyle {
            fill: Some(Color(0x00, 0x80, 0x00)),
            gradient: None,
            stroke: None,
            fill_src: Some(FillSrc::Color(ColorRef::Rgb("008000".into()))),
            stroke_src: None,
        },
    }
}

fn slide_on(master_id: u64, background: &str, objects: Vec<SlideObject>) -> Slide {
    Slide { hidden: false,
        title: "S".into(),
        background: background.into(),
        background_picture: None,
        objects,
        notes: String::new(),
        master_id: Some(master_id),
        transition: Default::default(),
        builds: Vec::new(),
        ids: Default::default(),
        layout_id: None,
    }
}

fn master(id: u64, name: &str, theme_id: u64) -> MasterSlide {
    MasterSlide {
        id,
        name: name.into(),
        background: String::new(),
        default_font: String::new(),
        shapes: vec![],
        theme_id: Some(theme_id),
        color_map: Default::default(),
        layouts: Vec::new(),
    }
}

fn two_theme_deck() -> decks_core::engine::Deck {
    let themes = vec![theme(7, "FF0000", "ppt/theme/theme1.xml"), theme(8, "0000FF", "ppt/theme/theme2.xml")];
    let masters = vec![master(1, "Red", 7), master(2, "Blue", 8)];
    let slides = vec![
        slide_on(1, "", vec![inheritor(ShapeKind::Rect, Color(0xFF, 0, 0)), local(ShapeKind::Ellipse)]),
        slide_on(2, "", vec![inheritor(ShapeKind::Rect, Color(0, 0, 0xFF))]),
    ];
    decks_core::engine::Deck { slides, masters, page_size: None, themes, skipped_slides: Vec::new() }
}

fn part(path: &std::path::Path, name: &str) -> String {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut s = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

fn accent_of(deck: &decks_core::engine::Deck, slide: usize) -> Color {
    let m = deck.master_of(&deck.slides[slide]).expect("the slide's master");
    let t = deck.theme_of(m).expect("the master's theme");
    t.slot("accent1").expect("accent1")
}

#[test]
fn multiple_themes_keep_local_overrides() {
    // Each master's inheritors resolve accent1 to their own theme's
    // colour; the explicit green fill is untouched by either theme.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("two-themes.pptx");
    decks_core::engine::write_pptx(path.to_str().unwrap(), &two_theme_deck()).unwrap();
    // One theme part per master, each carrying its own accent.
    assert!(part(&path, "ppt/theme/theme1.xml").contains("val=\"FF0000\""), "theme 1 keeps its red");
    assert!(part(&path, "ppt/theme/theme2.xml").contains("val=\"0000FF\""), "theme 2 keeps its blue");
    assert!(part(&path, "ppt/slides/slide1.xml").contains("val=\"accent1\""), "the reference writes back as a token");

    let back = decks_core::engine::read_pptx(path.to_str().unwrap()).unwrap();
    assert_eq!(back.themes.len(), 2, "both themes are retained");
    assert_eq!(accent_of(&back, 0), Color(0xFF, 0, 0), "master 1 inherits red");
    assert_eq!(accent_of(&back, 1), Color(0, 0, 0xFF), "master 2 inherits blue");
    let fills: Vec<Option<Color>> = back.slides[0]
        .objects
        .iter()
        .filter_map(|o| match o {
            SlideObject::Shape { style, .. } => Some(style.fill),
            _ => None,
        })
        .collect();
    assert_eq!(fills, vec![Some(Color(0xFF, 0, 0)), Some(Color(0, 0x80, 0))], "inheritor moved, override stayed: {fills:?}");

    // A second save through the retained model changes nothing.
    let again = dir.path().join("again.pptx");
    decks_core::engine::write_pptx(again.to_str().unwrap(), &back).unwrap();
    for name in ["ppt/theme/theme1.xml", "ppt/theme/theme2.xml", "ppt/slides/slide1.xml"] {
        assert_eq!(part(&again, name), part(&path, name), "{name} churned on a second save");
    }
    let reopened = decks_core::engine::read_pptx(again.to_str().unwrap()).unwrap();
    assert_eq!(accent_of(&reopened, 0), Color(0xFF, 0, 0));
    assert_eq!(accent_of(&reopened, 1), Color(0, 0, 0xFF));
}

#[test]
fn theme_colours_resolve_to_the_same_hex_in_odp() {
    // ODP has no theme parts: the references resolve to hex at the border
    // (documented), and the colours on the page are unchanged.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("two-themes.odp");
    decks_core::write_deck(path.to_str().unwrap(), &two_theme_deck()).unwrap();
    let back = decks_core::read_deck(path.to_str().unwrap()).unwrap();
    let fills: Vec<Option<Color>> = back.slides[0]
        .objects
        .iter()
        .filter_map(|o| match o {
            SlideObject::Shape { style, .. } => Some(style.fill),
            _ => None,
        })
        .collect();
    assert_eq!(fills, vec![Some(Color(0xFF, 0, 0)), Some(Color(0, 0x80, 0))], "{fills:?}");
    assert_eq!(back.slides[1].objects.len(), 1);
    let blue = match &back.slides[1].objects[0] {
        SlideObject::Shape { style, .. } => style.fill,
        other => panic!("{other:?}"),
    };
    assert_eq!(blue, Some(Color(0, 0, 0xFF)), "master 2 still inherits blue, as hex");
}

#[test]
fn absent_backgrounds_stay_absent_through_both_formats() {
    // Absence is inherited, never white at import; an explicit white slide
    // on a coloured master stays white.
    let mut deck = two_theme_deck();
    deck.masters[0].background = "#0b3d6b".into();
    deck.slides[0].background = String::new();
    deck.slides[1].background = "#ffffff".into();
    for ext in ["pptx", "odp"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("bg.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &deck).unwrap();
        let back = decks_core::read_deck(path.to_str().unwrap()).unwrap();
        assert_eq!(back.slides[0].background, "", "{ext}: the absent background stayed absent");
        assert_eq!(back.slides[1].background, "#ffffff", "{ext}: the explicit white stayed explicit");
        assert_eq!(back.masters[0].background, "#0b3d6b", "{ext}: the master colour survived");
        let resolved = decks_core::layouts::resolve_background(&back.slides[0].background, None, &back.masters[0].background);
        assert_eq!(resolved, "#0b3d6b", "{ext}: absence inherits at draw time");
    }
}

#[test]
fn placeholder_indices_survive_a_pptx_round_trip() {
    // Duplicate body places keep their source identity through a save, so
    // a later layout application matches by index, not by nth body.
    let layout = Layout {
        id: 5,
        name: "Two".into(),
        kind: LayoutKind::TwoContent,
        background: None,
        shapes: Vec::new(),
        placeholders: vec![
            LayoutPlaceholder { id: 6, role: Placeholder::Body, x: 10.0, y: 10.0, w: 100.0, h: 100.0, source_ty: "obj".into(), source_idx: Some(1) },
            LayoutPlaceholder { id: 7, role: Placeholder::Body, x: 200.0, y: 10.0, w: 100.0, h: 100.0, source_ty: "obj".into(), source_idx: Some(2) },
        ],
    };
    let keyed = |idx: u32, text: &str| SlideObject::TextBox {
        text: text.into(),
        x: 1.0, y: 2.0, w: 3.0, h: 4.0,
        rotation: 0.0,
        runs: Vec::new(),
        body: TextBody {
            placeholder: Some(Placeholder::Body),
            placeholder_key: Some(decks_core::layouts::PlaceholderKey { ty: "obj".into(), idx: Some(idx) }),
            ..Default::default()
        },
    };
    let mut masters = vec![master(1, "M", 7)];
    masters[0].layouts = vec![layout];
    let mut deck = decks_core::engine::Deck {
        slides: vec![slide_on(1, "", vec![keyed(2, "second"), keyed(1, "first")])],
        masters,
        page_size: None,
        themes: vec![theme(7, "FF0000", "ppt/theme/theme1.xml")],
        skipped_slides: Vec::new(),
    };
    decks_core::ops::ensure_all_ids(&mut deck.slides, &mut deck.masters);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("idx.pptx");
    decks_core::engine::write_pptx(path.to_str().unwrap(), &deck).unwrap();
    let back = decks_core::engine::read_pptx(path.to_str().unwrap()).unwrap();
    let places: Vec<(String, Option<u32>)> = back.masters[0].layouts[0]
        .placeholders
        .iter()
        .map(|p| (p.source_ty.clone(), p.source_idx))
        .collect();
    assert_eq!(places, vec![("obj".to_string(), Some(1)), ("obj".to_string(), Some(2))], "{places:?}");
    let keys: Vec<Option<u32>> = back.slides[0]
        .objects
        .iter()
        .filter_map(|o| match o {
            SlideObject::TextBox { body, .. } => Some(body.placeholder_key.as_ref().and_then(|k| k.idx)),
            _ => None,
        })
        .collect();
    assert_eq!(keys, vec![Some(2), Some(1)], "{keys:?}");
}
