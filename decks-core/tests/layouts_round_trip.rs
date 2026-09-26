// layouts_round_trip.rs — a master's layouts, each slide's layout and the
// placeholders its text boxes fill come back from pptx and odp as they were
// saved, including a layout applied in the editor and the empty
// placeholder it added. soffice_oracle.rs checks what LibreOffice keeps.
// SPDX-License-Identifier: GPL-3.0-or-later

use decks_core::engine::{Deck, SlideObject};
use decks_core::layouts::role_of;
use decks_core::DecksController;

/// A Paper-theme deck whose second slide has been put on Two Content.
pub fn deck() -> Deck {
    let (slides, masters) = decks_core::templates::deck(3).unwrap();
    let c = DecksController::new(slides, masters);
    assert!(c.apply_layout(1, 3), "Two Content applies");
    c.deck()
}

fn r(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// The master's layouts: name, kind and placeholder places.
fn layouts(d: &Deck) -> Vec<String> {
    d.masters[0]
        .layouts
        .iter()
        .map(|l| {
            let places: Vec<String> =
                l.placeholders.iter().map(|p| format!("{:?} {} {} {} {}", p.role, r(p.x), r(p.y), r(p.w), r(p.h))).collect();
            format!("{} {:?} {places:?}", l.name, l.kind)
        })
        .collect()
}

/// Each slide's layout and its boxes' roles, places and text.
fn slides(d: &Deck) -> Vec<String> {
    d.slides
        .iter()
        .map(|s| {
            let boxes: Vec<String> = s
                .objects
                .iter()
                .map(|o| match o {
                    SlideObject::TextBox { text, x, y, w, h, .. } => format!("{:?} {} {} {} {} {text:?}", role_of(o), r(*x), r(*y), r(*w), r(*h)),
                    other => format!("{other:?}"),
                })
                .collect();
            format!("{:?} {boxes:?}", s.layout)
        })
        .collect()
}

#[test]
fn layouts_and_placeholders_survive_pptx_and_odp() {
    let d = deck();
    assert_eq!(d.masters[0].layouts.len(), 6);
    assert_eq!(d.slides[1].layout, Some(3));
    // The applied layout added an empty second body.
    assert_eq!(d.slides[1].objects.len(), 3);
    for ext in ["pptx", "odp"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("layouts.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &d).unwrap();
        let back = decks_core::read_deck(path.to_str().unwrap()).unwrap();
        assert_eq!(layouts(&back), layouts(&d), "{ext}: the master's layouts");
        assert_eq!(slides(&back), slides(&d), "{ext}: the slides' layouts and placeholders");
        // A second save changes nothing.
        let again = dir.path().join(format!("again.{ext}"));
        decks_core::write_deck(again.to_str().unwrap(), &back).unwrap();
        let twice = decks_core::read_deck(again.to_str().unwrap()).unwrap();
        assert_eq!((layouts(&twice), slides(&twice)), (layouts(&d), slides(&d)), "{ext}: a second save");
    }
}

#[test]
fn a_deck_without_layouts_is_read_back_without_any() {
    let d = Deck::new();
    for ext in ["pptx", "odp"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("plain.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &d).unwrap();
        let back = decks_core::read_deck(path.to_str().unwrap()).unwrap();
        assert!(back.masters[0].layouts.is_empty(), "{ext}: {:?}", back.masters[0].layouts);
        assert_eq!(back.slides[0].layout, None, "{ext}");
    }
}

#[test]
fn a_layout_with_its_own_look_keeps_it_in_both_formats() {
    // As an imported deck has them: a layout with its own background and a
    // decoration over the master's.
    let mut d = deck();
    let bar = SlideObject::Rect { x: 0.0, y: 500.0, w: 960.0, h: 40.0, rotation: 0.0 };
    d.masters[0].layouts[2].background = Some("#203040".into());
    d.masters[0].layouts[2].shapes.push(bar.clone());
    d.slides[0].layout = Some(2);
    for ext in ["pptx", "odp"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("look.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &d).unwrap();
        let back = decks_core::read_deck(path.to_str().unwrap()).unwrap();
        let l = &back.masters[0].layouts[2];
        assert_eq!(l.background.as_deref(), Some("#203040"), "{ext}");
        assert_eq!(l.shapes.len(), 1, "{ext}: {:?}", l.shapes);
        assert_eq!(back.masters[0].shapes.len(), d.masters[0].shapes.len(), "{ext}: the master's own decorations");
        assert_eq!(back.slides[0].layout, Some(2), "{ext}");
        assert_eq!(layouts(&back), layouts(&d), "{ext}");
    }
}
