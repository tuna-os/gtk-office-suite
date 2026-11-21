// line_spacing.rs — a paragraph's line spacing is kept and drawn.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Office's own themes set their body and title text at 90% line spacing
// (`a:lnSpc/a:spcPct val="90000"` in the master's text styles); the reader
// had no line spacing at all, so every line of every such deck sat 11%
// further apart than in PowerPoint or LibreOffice, and a full placeholder
// (the OERB terms of reference) ran out of its box.

use decks_core::engine::{read_pptx, write_pptx, Deck, ParaStyle, SlideObject, Spacing, TextBody};
use suite_common_core::document::Run;

fn deck(line: Spacing) -> Deck {
    let mut d = Deck::new();
    d.slides[0].objects = vec![SlideObject::TextBox {
        text: "one\ntwo".into(),
        x: 100.0,
        y: 100.0,
        w: 400.0,
        h: 200.0,
        rotation: 0.0,
        runs: vec![Run::plain("one\ntwo")],
        body: TextBody { paras: vec![ParaStyle { line_spacing: Some(line), ..Default::default() }; 2], ..Default::default() },
    }];
    d
}

fn line_spacing(d: &Deck) -> Vec<Option<Spacing>> {
    d.slides[0]
        .objects
        .iter()
        .find_map(|o| match o {
            SlideObject::TextBox { body, .. } => Some(body.paras.iter().map(|p| p.line_spacing).collect()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no text: {:?}", d.slides[0].objects))
}

#[test]
fn a_proportional_line_spacing_survives_a_pptx_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("l.pptx");
    write_pptx(path.to_str().unwrap(), &deck(Spacing::Lines(0.9))).unwrap();
    assert_eq!(line_spacing(&read_pptx(path.to_str().unwrap()).unwrap()), [Some(Spacing::Lines(0.9)); 2]);
}

#[test]
fn an_exact_line_pitch_survives_a_pptx_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("e.pptx");
    write_pptx(path.to_str().unwrap(), &deck(Spacing::Units(40.0))).unwrap();
    let back = line_spacing(&read_pptx(path.to_str().unwrap()).unwrap());
    assert!(matches!(back[0], Some(Spacing::Units(u)) if (u - 40.0).abs() < 0.5), "{back:?}");
}

#[test]
fn a_proportional_line_spacing_survives_an_odp_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("l.odp");
    decks_core::odp::write(&deck(Spacing::Lines(0.9)), path.to_str().unwrap()).unwrap();
    assert_eq!(line_spacing(&decks_core::odp::read(path.to_str().unwrap()).unwrap()), [Some(Spacing::Lines(0.9)); 2]);
}

/// A stated 100% is kept: it overrides a master's 90% (review on #2353).
#[test]
fn a_stated_single_spacing_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.pptx");
    write_pptx(path.to_str().unwrap(), &deck(Spacing::Lines(1.0))).unwrap();
    assert_eq!(line_spacing(&read_pptx(path.to_str().unwrap()).unwrap()), [Some(Spacing::Lines(1.0)); 2]);
}

#[test]
fn no_stated_spacing_writes_none() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("n.pptx");
    let mut d = deck(Spacing::Lines(1.0));
    if let SlideObject::TextBox { body, .. } = &mut d.slides[0].objects[0] {
        for p in &mut body.paras {
            p.line_spacing = None;
        }
    }
    write_pptx(path.to_str().unwrap(), &d).unwrap();
    assert_eq!(line_spacing(&read_pptx(path.to_str().unwrap()).unwrap()), [None; 2]);
}
