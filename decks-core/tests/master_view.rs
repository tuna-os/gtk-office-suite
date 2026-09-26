// master_view.rs — a master edited in the master view is saved and read
// back, in both formats, with the decorations it was given there.
// soffice_oracle.rs's impress_keeps_a_master_view_edit checks LibreOffice.
// SPDX-License-Identifier: GPL-3.0-or-later

use decks_core::engine::shape::{Color, ShapeKind, ShapeStyle};
use decks_core::engine::SlideObject;
use decks_core::DecksController;

/// A deck from the Paper theme whose master gets a red triangle in the
/// master view.
pub fn edited() -> (DecksController, SlideObject) {
    let (slides, masters) = decks_core::templates::deck(3).unwrap();
    let c = DecksController::new(slides, masters);
    let triangle = SlideObject::Shape {
        kind: ShapeKind::Triangle,
        x: 860.0,
        y: 20.0,
        w: 80.0,
        h: 80.0,
        rotation: 0.0,
        style: ShapeStyle { fill: Some(Color(0xE0, 0x1B, 0x24)), gradient: None, stroke: None },
    };
    c.edit_master(0).unwrap();
    c.add_object(0, triangle.clone());
    c.finish_master().unwrap();
    (c, triangle)
}

#[test]
fn a_master_view_edit_survives_pptx_and_odp() {
    let (c, triangle) = edited();
    let deck = c.deck();
    assert_eq!(deck.masters[0].shapes.last().map(|o| format!("{o:?}")), Some(format!("{triangle:?}")));
    for ext in ["pptx", "odp"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("edited.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &deck).unwrap();
        let back = decks_core::read_deck(path.to_str().unwrap()).unwrap();
        let shapes: Vec<String> = back.masters[0].shapes.iter().map(|o| format!("{o:?}")).collect();
        assert!(shapes.contains(&format!("{triangle:?}")), "{ext}: {shapes:#?}");
        assert_eq!(back.masters[0].shapes.len(), deck.masters[0].shapes.len(), "{ext}: nothing lost or added");
        assert_eq!(back.slides.len(), deck.slides.len(), "{ext}: the slides are the deck's, not the master view's");
    }
}
