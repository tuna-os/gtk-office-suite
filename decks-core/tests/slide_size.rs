// slide_size.rs — a deck keeps its slide size through open, save and
// reopen (issue triage for #440: saving a PPTX used to write our own
// 10in x 5.625in `p:sldSz` over the source's, squashing a 4:3 or a
// custom-size deck into 16:9).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The model is always 960x540; readers scale onto it and the writer must
// scale back. Checked here on the package itself (the declared size, a
// full-bleed shape's EMU, a font size) and on what our reader makes of it;
// soffice_oracle.rs checks that LibreOffice agrees.

use std::io::Read;

use decks_core::engine::{read_pptx, write_pptx, Deck, Run, RunStyle, SlideObject};

/// 4:3 (PowerPoint's old default, 10in x 7.5in) and a square custom size.
const SIZES: [(f64, f64); 2] = [(9_144_000.0, 6_858_000.0), (7_200_000.0, 7_200_000.0)];

/// A deck on a `size` slide: a full-bleed rectangle and a 24 pt text box.
fn deck(size: Option<(f64, f64)>) -> Deck {
    let mut d = Deck::new();
    d.masters[0].page_emu = size;
    d.slides[0].objects = vec![
        SlideObject::Rect { x: 0.0, y: 0.0, w: 960.0, h: 540.0, rotation: 0.0 },
        SlideObject::TextBox {
            text: "hello".into(),
            x: 96.0,
            y: 54.0,
            w: 480.0,
            h: 108.0,
            rotation: 0.0,
            runs: vec![Run { text: "hello".into(), style: RunStyle { font_size_hp: Some(48), ..Default::default() } }],
            body: Default::default(),
        },
    ];
    d
}

fn part(path: &std::path::Path, name: &str) -> String {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut s = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

fn declared(path: &std::path::Path) -> (f64, f64) {
    decks_core::engine::parse_slide_size(&part(path, "ppt/presentation.xml")).expect("a declared slide size")
}

fn boxes(d: &Deck) -> Vec<(f64, f64, f64, f64)> {
    d.slides[0]
        .objects
        .iter()
        .map(|o| match o {
            SlideObject::Rect { x, y, w, h, .. } | SlideObject::Shape { x, y, w, h, .. } | SlideObject::TextBox { x, y, w, h, .. } => (*x, *y, *w, *h),
            other => panic!("unexpected {other:?}"),
        })
        .collect()
}

fn near(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    // EMU are integers: a unit is 7500 to 13333.3 of them here, so a
    // rounded coordinate is off by under a ten-thousandth of a unit.
    [(a.0, b.0), (a.1, b.1), (a.2, b.2), (a.3, b.3)].iter().all(|(p, q)| (p - q).abs() < 1e-3)
}

#[test]
fn a_4_3_or_custom_deck_keeps_its_size_through_open_save_and_reopen() {
    for size in SIZES {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.pptx");
        write_pptx(source.to_str().unwrap(), &deck(Some(size))).unwrap();
        assert_eq!(declared(&source), size, "the source declares its size");

        // Open, save, reopen.
        let opened = read_pptx(source.to_str().unwrap()).unwrap();
        assert_eq!(opened.masters[0].page_emu, Some(size), "the size is kept on open");
        let saved = dir.path().join("saved.pptx");
        write_pptx(saved.to_str().unwrap(), &opened).unwrap();
        assert_eq!(declared(&saved), size, "the save writes the source's size, not ours");
        let reopened = read_pptx(saved.to_str().unwrap()).unwrap();
        assert_eq!(reopened.masters[0].page_emu, Some(size));

        // The contents are where they were, relative to the slide: the
        // full-bleed rectangle still covers it, in the file and the model.
        let rect = part(&saved, "ppt/slides/slide1.xml");
        let (cx, cy) = (size.0 as i64, size.1 as i64);
        assert!(rect.contains(&format!("<a:ext cx=\"{cx}\" cy=\"{cy}\"/>")), "the full-bleed shape spans the slide: {rect}");
        for (got, want) in boxes(&reopened).into_iter().zip(boxes(&deck(Some(size)))) {
            assert!(near(got, want), "{size:?}: {got:?} is not {want:?}");
        }
        // And the text is the size it was, relative to the slide.
        let hp = |d: &Deck| match &d.slides[0].objects[1] {
            SlideObject::TextBox { runs, .. } => runs[0].style.font_size_hp,
            other => panic!("{other:?}"),
        };
        assert_eq!(hp(&reopened), hp(&opened), "{size:?}: the font size drifted");

        // A second save changes nothing.
        let again = dir.path().join("again.pptx");
        write_pptx(again.to_str().unwrap(), &reopened).unwrap();
        for name in ["ppt/presentation.xml", "ppt/slides/slide1.xml"] {
            assert_eq!(part(&again, name), part(&saved, name), "{size:?}: {name} changed on a second save");
        }
    }
}

#[test]
fn a_deck_of_the_default_size_is_written_as_before() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("default.pptx");
    write_pptx(path.to_str().unwrap(), &deck(None)).unwrap();
    assert_eq!(declared(&path), (9_144_000.0, 5_143_500.0));
    let back = read_pptx(path.to_str().unwrap()).unwrap();
    assert_eq!(back.masters[0].page_emu, None, "our own size reads back as the default");
    let slide = part(&path, "ppt/slides/slide1.xml");
    assert!(slide.contains("<a:ext cx=\"9144000\" cy=\"5143500\"/>"), "{slide}");
    assert!(slide.contains("sz=\"2400\""), "a 24 pt run is still sz=2400: {slide}");
}

#[test]
fn a_widescreen_deck_keeps_its_13_33_inch_size() {
    // What current PowerPoint and Impress make: 13.333in x 7.5in.
    let size = (12_192_000.0, 6_858_000.0);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wide.pptx");
    write_pptx(path.to_str().unwrap(), &deck(Some(size))).unwrap();
    let opened = read_pptx(path.to_str().unwrap()).unwrap();
    let saved = dir.path().join("saved.pptx");
    write_pptx(saved.to_str().unwrap(), &opened).unwrap();
    assert_eq!(declared(&saved), size);
    // 24 pt on the model's 10in slide is 32 pt on a 13.33in one.
    assert!(part(&saved, "ppt/slides/slide1.xml").contains("sz=\"3200\""));
}

#[test]
fn a_4_3_or_custom_deck_keeps_its_size_through_odp() {
    // The odp writer used to put every deck on its own 960x540pt page.
    for size in SIZES {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deck.odp");
        decks_core::write_deck(path.to_str().unwrap(), &deck(Some(size))).unwrap();
        let back = decks_core::read_deck(path.to_str().unwrap()).unwrap();
        let (cx, cy) = back.masters[0].page_emu.expect("the page size is read back");
        assert!((cx - size.0).abs() < 1.0 && (cy - size.1).abs() < 1.0, "{size:?}: read back as {cx}x{cy}");
        for (got, want) in boxes(&back).into_iter().zip(boxes(&deck(Some(size)))) {
            assert!(near(got, want), "{size:?}: {got:?} is not {want:?}");
        }
        // The size survives a second save, and pptx after odp.
        let again = dir.path().join("again.pptx");
        decks_core::write_deck(again.to_str().unwrap(), &back).unwrap();
        assert_eq!(declared(&again), size, "odp then pptx");
    }
}
