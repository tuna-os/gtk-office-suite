// hidden_slides.rs — a hidden slide stays hidden.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// "Supporting every young person" hides 21 of its 40 slides
// (`<p:sld show="0">`). The reader showed them all, so the deck exported
// 40 pages where PowerPoint and Impress print 19, and every page after the
// first hidden slide was compared against the wrong one.

use decks_core::engine::{read_pptx, write_pptx, Deck};

/// Three slides, the middle one hidden.
fn deck() -> Deck {
    let mut d = Deck::new();
    let proto = d.slides[0].clone();
    d.slides = (1..=3)
        .map(|k| {
            let mut s = proto.clone();
            s.title = format!("S{k}");
            s.hidden = k == 2;
            s
        })
        .collect();
    d
}

fn hidden(d: &Deck) -> Vec<bool> {
    d.slides.iter().map(|s| s.hidden).collect()
}

#[test]
fn a_hidden_slide_survives_a_pptx_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("h.pptx");
    write_pptx(path.to_str().unwrap(), &deck()).unwrap();
    let back = read_pptx(path.to_str().unwrap()).unwrap();
    assert_eq!(hidden(&back), [false, true, false]);
}

#[test]
fn a_hidden_slide_survives_an_odp_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("h.odp");
    decks_core::odp::write(&deck(), path.to_str().unwrap()).unwrap();
    let back = decks_core::odp::read(path.to_str().unwrap()).unwrap();
    assert_eq!(hidden(&back), [false, true, false]);
}

/// A shown slide carries no visibility, so a deck with none hidden is
/// written as before.
#[test]
fn a_shown_slide_writes_no_show_attribute() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.pptx");
    write_pptx(path.to_str().unwrap(), &Deck::new()).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("ppt/slides/slide1.xml").unwrap(), &mut xml).unwrap();
    assert!(!xml.contains("show="), "{xml}");
}
