// precedence_resolver.rs — DCK-006: one resolver answers what a slide's
// background, a shape's fill and outline and a run's font are, walking
// object → layout → master → theme → documented fallback.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Consumer-level: every case builds a `Deck` as the readers would leave
// it (absence kept absent, tokens beside their resolved caches) and asks
// the resolver, the way a renderer or writer does.

use std::collections::BTreeMap;

use decks_core::engine::shape::{Color, ColorModulation, ShapeKind, ShapeStyle, Stroke};
use decks_core::engine::text_body::Frame;
use decks_core::engine::theme::{ColorMod, ColorRef, FillSrc, StrokeSrc, Theme, ThemeColor};
use decks_core::engine::{Deck, MasterSlide, PrecedenceResolver, Slide, SlideObject, TextBody};
use decks_core::layouts::{self, Layout, LayoutKind, Placeholder, FALLBACK_BACKGROUND};

fn theme(id: u64, dk1: &str, lt1: &str, accent1: &str, major: Option<&str>, minor: Option<&str>) -> Theme {
    let mut colors: BTreeMap<String, String> = Theme::default_colors();
    colors.insert("dk1".into(), dk1.into());
    colors.insert("lt1".into(), lt1.into());
    colors.insert("accent1".into(), accent1.into());
    Theme {
        id,
        name: "Same Name".into(),
        colors,
        major_font: major.map(Into::into),
        minor_font: minor.map(Into::into),
        source: format!("ppt/theme/theme{id}.xml"),
    }
}

fn master(id: u64, theme_id: Option<u64>) -> MasterSlide {
    MasterSlide {
        id,
        name: format!("Master {id}"),
        background: String::new(),
        default_font: String::new(),
        shapes: vec![],
        theme_id,
        color_map: Default::default(),
        layouts: vec![],
    }
}

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn layout(id: u64, background: Option<&str>) -> Layout {
    Layout { id, name: format!("Layout {id}"), kind: LayoutKind::Custom, background: background.map(Into::into), shapes: vec![], placeholders: vec![] }
}

fn slide(master_id: u64, layout_id: Option<u64>, background: &str, objects: Vec<SlideObject>) -> Slide {
    Slide {
        hidden: false,
        title: "S".into(),
        background: background.into(),
        background_picture: None,
        objects,
        notes: String::new(),
        master_id: Some(master_id),
        transition: Default::default(),
        builds: Vec::new(),
        ids: Default::default(),
        layout_id,
    }
}

fn deck(themes: Vec<Theme>, masters: Vec<MasterSlide>, slides: Vec<Slide>) -> Deck {
    Deck { slides, masters, page_size: None, themes }
}

fn token(slot: &str) -> ColorRef {
    ColorRef::Theme(ThemeColor::unmodulated(slot))
}

fn shape(fill_src: Option<FillSrc>, fill: Option<Color>, stroke_src: Option<StrokeSrc>, stroke: Option<Stroke>) -> SlideObject {
    SlideObject::Shape {
        kind: ShapeKind::Rect,
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
        rotation: 0.0,
        style: ShapeStyle { fill, gradient: None, stroke, fill_src, stroke_src },
    }
}

fn hex(c: Option<Color>) -> String {
    c.map(|c| c.to_hex()).unwrap_or_else(|| "none".into())
}

#[test]
fn resolver_local_override_preempts_every_ancestor() {
    // Every level states something different; the object/slide wins each.
    let mut m = master(1, Some(7));
    m.background = "#333333".into();
    m.default_font = "Master Face".into();
    let t = theme(7, "010101", "FEFEFE", "FF0000", Some("Theme Heading"), Some("Theme Body"));
    let mut m2 = m.clone();
    m2.layouts.push(layout(5, Some("#222222")));
    let own = shape(
        Some(FillSrc::Color(ColorRef::Rgb("00AA00".into()))),
        Some(Color(0x00, 0xAA, 0x00)),
        Some(StrokeSrc::Color(ColorRef::Rgb("0000AA".into()))),
        Some(Stroke { color: Color(0, 0, 0xAA), width: 3.0 }),
    );
    let d = deck(vec![t], vec![m2], vec![slide(1, Some(5), "#111111", vec![own])]);
    let r = PrecedenceResolver::new(&d);
    let s = &d.slides[0];

    assert_eq!(r.background(s), "#111111", "the slide's own background beats layout #222222 and master #333333");
    assert_eq!(hex(r.fill(s, &s.objects[0])), "00AA00", "a local fill beats the theme's accent1");
    let stroke = r.stroke(s, &s.objects[0]).expect("a local outline");
    assert_eq!((stroke.color.to_hex().as_str(), stroke.width), ("0000AA", 3.0));
    assert_eq!(r.font(s, None, Some("Run Face")), "Run Face", "a run's own family beats master and theme");
    assert_eq!(r.font(s, Some(Placeholder::Title), Some("Run Face")), "Run Face", "even in a title");
    assert_eq!(r.font(s, None, Some("   ")), "Master Face", "a blank own family is absence, not a font");
}

#[test]
fn resolver_an_explicit_no_fill_stops_inheritance() {
    // `a:noFill` is a statement; the cache beside it and the theme do not
    // get to fill the shape back in.
    let nofill = shape(Some(FillSrc::NoFill), Some(Color(1, 2, 3)), Some(StrokeSrc::NoLine), Some(Stroke { color: Color(9, 9, 9), width: 2.0 }));
    let d = deck(vec![], vec![master(1, None)], vec![slide(1, None, "", vec![nofill])]);
    let r = PrecedenceResolver::new(&d);
    assert_eq!(r.fill(&d.slides[0], &d.slides[0].objects[0]), None);
    assert_eq!(r.stroke(&d.slides[0], &d.slides[0].objects[0]), None);
}

#[test]
fn resolver_layout_overrides_master_and_unknown_layout_does_not_guess() {
    let mut m = master(1, None);
    m.background = "#334455".into();
    m.layouts = vec![layout(5, Some("#aabbcc")), layout(6, None), layout(7, Some(""))];
    let d = deck(
        vec![],
        vec![m],
        vec![
            slide(1, Some(5), "", vec![]),
            slide(1, Some(6), "", vec![]),
            slide(1, Some(7), "", vec![]),
            slide(1, Some(99), "", vec![]),
            slide(1, None, "", vec![]),
            slide(1, Some(5), "#ffffff", vec![]),
        ],
    );
    let r = PrecedenceResolver::new(&d);
    let bg: Vec<_> = d.slides.iter().map(|s| r.background(s).into_owned()).collect();
    assert_eq!(bg[0], "#aabbcc", "the layout's background over the master's");
    assert_eq!(bg[1], "#334455", "a layout stating none inherits the master's");
    assert_eq!(bg[2], "#334455", "an empty layout background is none, not white");
    assert_eq!(bg[3], "#334455", "an unknown layout id draws the master's, not another layout's");
    assert_eq!(bg[4], "#334455", "no layout at all");
    assert_eq!(bg[5], "#ffffff", "an explicit white slide stays white over a coloured layout");
}

#[test]
fn resolver_theme_inheritance_master_absence_takes_theme_slots_and_fonts() {
    // A master that states nothing inherits its theme: the background
    // from bg1, the font from the theme, a token fill live — not from the
    // stale cache an earlier theme left beside the reference.
    let mut m = master(1, Some(7));
    let stale_cache = Color(0xFF, 0x00, 0x00);
    let inheritor = shape(Some(FillSrc::Color(token("accent1"))), Some(stale_cache), None, None);
    let modulated = shape(
        Some(FillSrc::Color(ColorRef::Theme(ThemeColor { slot: "accent1".into(), mods: vec![ColorMod::LumMod(50_000)] }))),
        None,
        None,
        None,
    );
    let t = theme(7, "101010", "F4F0E8", "1166CC", Some("Heading Face"), Some("Body Face"));
    let d = deck(vec![t], vec![m.clone()], vec![slide(1, None, "", vec![inheritor, modulated])]);
    let r = PrecedenceResolver::new(&d);
    let s = &d.slides[0];

    assert_eq!(r.background(s), "#f4f0e8", "absent at every level: the theme's bg1 (lt1), not white");
    assert_eq!(hex(r.fill(s, &s.objects[0])), "1166CC", "the token resolves live; the FF0000 cache is stale");
    let expected = Color(0x11, 0x66, 0xCC).lum(Some(50_000), None);
    assert_eq!(r.fill(s, &s.objects[1]), Some(expected), "the source modulation applies to the theme slot");
    assert_ne!(expected, Color(0x11, 0x66, 0xCC));

    assert_eq!(r.font(s, None, None), "Body Face", "body text: the theme's minor face");
    assert_eq!(r.font(s, Some(Placeholder::Body), None), "Body Face");
    assert_eq!(r.font(s, Some(Placeholder::Title), None), "Heading Face", "a title: the theme's major face");

    // The master's own default font is stated closer than the theme's,
    // for body text; a title still takes the heading face.
    m.default_font = "Master Face".into();
    let d2 = deck(d.themes.clone(), vec![m], d.slides.clone());
    let r2 = PrecedenceResolver::new(&d2);
    assert_eq!(r2.font(&d2.slides[0], None, None), "Master Face");
    assert_eq!(r2.font(&d2.slides[0], Some(Placeholder::Title), None), "Heading Face");
}

#[test]
fn resolver_color_map_remaps_the_same_token_per_master_theme() {
    // Three masters, two themes that share a name; the same `tx1`/`bg1`
    // tokens mean different colours on each.
    let light = theme(7, "101010", "F0F0F0", "FF0000", None, None);
    let dark = theme(8, "202020", "E0E0E0", "0000FF", None, None);
    let identity = master(1, Some(7));
    let mut inverted = master(2, Some(8));
    inverted.color_map = map(&[("tx1", "lt1"), ("bg1", "dk1")]);
    let mut accented = master(3, Some(7));
    accented.color_map = map(&[("tx1", "accent1"), ("bg1", "dk1")]);
    let mut explicit = master(4, Some(8));
    explicit.color_map = map(&[("tx1", "dk1"), ("bg1", "lt1")]);

    let objects = |slot: &str| vec![shape(Some(FillSrc::Color(token(slot))), None, None, None)];
    let slides = (1..=4u64).map(|m| slide(m, None, "", objects("tx1"))).collect::<Vec<_>>();
    let d = deck(vec![light, dark], vec![identity, inverted, accented, explicit], slides);
    let r = PrecedenceResolver::new(&d);
    let tx1 = |i: usize| hex(r.fill(&d.slides[i], &d.slides[i].objects[0]));
    assert_eq!(tx1(0), "101010", "no map: tx1 is dk1 of its own theme");
    assert_eq!(tx1(1), "E0E0E0", "a dark master maps tx1 to its own theme's lt1");
    assert_eq!(tx1(2), "FF0000", "tx1 may map onto any slot, here accent1");
    assert_eq!(tx1(3), "202020", "an explicit tx1 -> dk1 map is the identity under its own theme");

    // Backgrounds follow bg1 through the same map when nothing states one.
    let bg: Vec<_> = d.slides.iter().map(|s| r.background(s).into_owned()).collect();
    assert_eq!(bg, ["#f0f0f0", "#202020", "#101010", "#e0e0e0"]);

    // A slot the map does not name is looked up directly.
    let direct = shape(Some(FillSrc::Color(token("dk1"))), None, None, None);
    let m = &d.masters[1];
    assert_eq!(hex(r.color(Some(m), &token("dk1"))), "202020", "dk1 is not a presentation name: not remapped");
    assert_eq!(hex(r.fill(&d.slides[1], &direct)), "202020");
}

#[test]
fn resolver_unspecified_properties_yield_documented_defaults() {
    let bare = shape(None, None, None, None);
    let text_frame = SlideObject::TextBox {
        text: "t".into(),
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
        rotation: 0.0,
        runs: vec![],
        body: TextBody { frame: Some(Frame { kind: ShapeKind::Rect, style: ShapeStyle { fill: None, gradient: None, stroke: None, fill_src: None, stroke_src: None } }), ..Default::default() },
    };
    let plain_text = SlideObject::TextBox { text: "t".into(), x: 0.0, y: 0.0, w: 1.0, h: 1.0, rotation: 0.0, runs: vec![], body: TextBody::default() };
    let d = deck(vec![], vec![master(1, None)], vec![slide(1, None, "", vec![bare, text_frame, plain_text])]);
    let r = PrecedenceResolver::new(&d);
    let s = &d.slides[0];

    assert_eq!(r.background(s), FALLBACK_BACKGROUND, "nothing states one: the documented white");
    assert_eq!(r.font(s, None, None), MasterSlide::DEFAULT_FONT);
    assert_eq!(r.font(s, Some(Placeholder::Title), None), MasterSlide::DEFAULT_FONT);
    for o in &s.objects {
        assert_eq!(r.fill(s, o), None, "an unspecified fill is no fill, not a default one");
        assert_eq!(r.stroke(s, o), None, "an unspecified outline is no outline");
    }

    // A slide whose master id names no master resolves against the
    // defaults rather than another master's look.
    let orphan = deck(vec![], vec![{
        let mut m = master(1, None);
        m.background = "#123456".into();
        m
    }], vec![slide(42, None, "", vec![])]);
    assert_eq!(PrecedenceResolver::new(&orphan).background(&orphan.slides[0]), FALLBACK_BACKGROUND);
}

#[test]
fn resolver_unknown_token_keeps_the_cached_colour_and_default_theme_answers_without_one() {
    // No retained theme (theme_id None, or pointing at nothing): the
    // documented default theme answers, so accent1 is Office blue.
    let ok = shape(Some(FillSrc::Color(token("accent1"))), None, None, None);
    // A token no theme knows draws as the colour the file last drew.
    let unknown = shape(Some(FillSrc::Color(token("phClr"))), Some(Color(0x12, 0x34, 0x56)), None, None);
    let dangling = master(2, Some(404));
    let d = deck(vec![], vec![master(1, None), dangling], vec![
        slide(1, None, "", vec![ok.clone(), unknown]),
        slide(2, None, "", vec![ok]),
    ]);
    let r = PrecedenceResolver::new(&d);
    assert_eq!(hex(r.fill(&d.slides[0], &d.slides[0].objects[0])), "4472C4");
    assert_eq!(hex(r.fill(&d.slides[0], &d.slides[0].objects[1])), "123456");
    assert_eq!(hex(r.fill(&d.slides[1], &d.slides[1].objects[0])), "4472C4", "a missing theme id resolves to the default theme");
}

#[test]
fn resolver_layouts_resolve_background_is_the_same_precedence() {
    // `layouts::resolve_background` takes pre-looked-up levels; the
    // deck-level resolver must agree with it whenever the theme adds
    // nothing (absent everywhere, default theme: white).
    let combos: [(&str, Option<&str>, &str); 7] = [
        ("#112233", Some("#445566"), "#778899"),
        ("", Some("#445566"), "#778899"),
        ("", None, "#778899"),
        ("", Some(""), "#778899"),
        ("", None, ""),
        ("#ffffff", None, "#0b3d6b"),
        ("#ffffff", Some("#445566"), ""),
    ];
    for (slide_bg, layout_bg, master_bg) in combos {
        let mut m = master(1, None);
        m.background = master_bg.into();
        m.layouts = vec![layout(5, layout_bg)];
        let d = deck(vec![], vec![m], vec![slide(1, Some(5), slide_bg, vec![])]);
        let via_deck = PrecedenceResolver::new(&d).background(&d.slides[0]).into_owned();
        let via_levels = layouts::resolve_background(slide_bg, layout_bg, master_bg);
        assert_eq!(via_deck, via_levels, "slide {slide_bg:?} layout {layout_bg:?} master {master_bg:?}");
    }
}

#[test]
fn resolver_never_mutates_the_deck() {
    let mut m = master(1, Some(7));
    m.color_map = map(&[("tx1", "lt1")]);
    m.layouts = vec![layout(5, Some("#abcdef"))];
    let t = theme(7, "101010", "F0F0F0", "FF0000", Some("H"), Some("B"));
    let d = deck(vec![t], vec![m], vec![slide(1, Some(5), "", vec![shape(Some(FillSrc::Color(token("tx1"))), None, None, None)])]);
    let before = serde_json::to_string(&d).unwrap();
    let r = PrecedenceResolver::new(&d);
    let s = &d.slides[0];
    let _ = (r.background(s), r.fill(s, &s.objects[0]), r.stroke(s, &s.objects[0]), r.font(s, Some(Placeholder::Title), None));
    assert_eq!(serde_json::to_string(&d).unwrap(), before);
}
