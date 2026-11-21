// empty_paragraphs.rs — an empty paragraph is as tall as its end mark.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// A blank paragraph between two others has no run, so its height is its
// `a:endParaRPr` size. Decks used the run before it instead, so an 8 pt
// spacer line between 20 pt paragraphs pushed everything after it down a
// whole 20 pt line, and most real decks use such spacers.

use decks_core::engine::{read_pptx, write_pptx, Deck, ParaStyle, SlideObject, TextBody};
use suite_common_core::document::{Run, RunStyle};

fn ends(d: &Deck) -> Vec<Option<u16>> {
    d.slides[0]
        .objects
        .iter()
        .find_map(|o| match o {
            SlideObject::TextBox { body, .. } => {
                Some(body.paras.iter().map(|p| p.end_run.as_ref().and_then(|r| r.font_size_hp)).collect())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no text: {:?}", d.slides[0].objects))
}

#[test]
fn an_empty_paragraphs_size_survives_a_pptx_save() {
    let big = RunStyle { font_size_hp: Some(40), ..Default::default() };
    let mut d = Deck::new();
    let spacer = ParaStyle { end_run: Some(RunStyle { font_size_hp: Some(16), ..Default::default() }), ..Default::default() };
    d.slides[0].objects = vec![SlideObject::TextBox {
        text: "a\n\nb".into(),
        x: 100.0,
        y: 100.0,
        w: 400.0,
        h: 200.0,
        rotation: 0.0,
        runs: vec![Run { text: "a\n\n".into(), style: big.clone() }, Run { text: "b".into(), style: big }],
        body: TextBody { paras: vec![ParaStyle::default(), spacer, ParaStyle::default()], ..Default::default() },
    }];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("e.pptx");
    write_pptx(path.to_str().unwrap(), &d).unwrap();
    let back = ends(&read_pptx(path.to_str().unwrap()).unwrap());
    assert_eq!(back.get(1).copied().flatten(), Some(16), "{back:?}");
    assert_eq!(back[0], None, "a paragraph with text has no end look of its own");
}
