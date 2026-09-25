// template_round_trip.rs — a deck made from a built-in theme keeps its
// look through a save and reopen in either format: the master's
// background, font and decorations (fills, gradients, outlines, presets)
// and the slides' text. Both used to lose the decorations: the pptx master
// reader flattened them to plain rectangles, and the odp writer wrote a
// gradient as one colour while its reader dropped every shape's paint.
// soffice_oracle.rs checks what LibreOffice keeps.
// SPDX-License-Identifier: GPL-3.0-or-later

use decks_core::engine::{Deck, SlideObject};
use decks_core::templates::{deck, templates};

/// A shape's look and place, rounded so a point or EMU conversion's last
/// digit doesn't count.
fn look(o: &SlideObject) -> String {
    let r = |v: f64| (v * 1000.0).round() / 1000.0;
    match o {
        SlideObject::Shape { kind, x, y, w, h, style, .. } => {
            format!("{kind:?} {} {} {} {} {style:?}", r(*x), r(*y), r(*w), r(*h))
        }
        SlideObject::TextBox { text, .. } => format!("text {text:?}"),
        other => format!("{other:?}"),
    }
}

fn round_trip(d: &Deck, ext: &str) -> Deck {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("theme.{ext}"));
    decks_core::write_deck(path.to_str().unwrap(), d).unwrap();
    decks_core::read_deck(path.to_str().unwrap()).unwrap()
}

#[test]
fn every_theme_keeps_its_master_through_pptx_and_odp() {
    for (i, t) in templates().iter().enumerate() {
        let (slides, masters) = deck(i).unwrap();
        let d = Deck { slides, masters };
        for ext in ["pptx", "odp"] {
            let back = round_trip(&d, ext);
            let (m, want) = (&back.masters[0], &d.masters[0]);
            assert_eq!(m.background, want.background, "{} {ext}: background", t.name);
            assert_eq!(m.font_family(), want.font_family(), "{} {ext}: font", t.name);
            let got: Vec<String> = m.shapes.iter().map(look).collect();
            let wanted: Vec<String> = want.shapes.iter().map(look).collect();
            assert_eq!(got, wanted, "{} {ext}: the master's decorations", t.name);
            let texts = |d: &Deck| -> Vec<String> {
                d.slides.iter().flat_map(|s| &s.objects).filter(|o| matches!(o, SlideObject::TextBox { .. })).map(look).collect()
            };
            assert_eq!(texts(&back), texts(&d), "{} {ext}: the slides' text", t.name);
        }
    }
}
