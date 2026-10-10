// line_breaks.rs — a line break inside a paragraph stays one.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The pay centre deck's title is one paragraph broken over three lines
// with `a:br` ("Unified Actions for Pay (UAP) and" / "Departmental
// Surcharge"): the reader dropped the breaks, and the words ran together
// ("andDepartmental").

use decks_core::engine::{read_pptx, write_pptx, Deck, Run, RunStyle, SlideObject};

const BREAK: char = '\u{2028}';

fn deck() -> Deck {
    let mut d = Deck::new();
    let text = format!("Unified Actions for Pay (UAP) and{BREAK}Departmental Surcharge\nNovember 2024");
    let bold = RunStyle { bold: true, ..Default::default() };
    d.slides[0].objects = vec![SlideObject::TextBox {
        text: text.clone(),
        x: 50.0,
        y: 50.0,
        w: 600.0,
        h: 200.0,
        rotation: 0.0,
        runs: vec![
            Run { text: format!("Unified Actions for Pay (UAP) and{BREAK}Departmental Surcharge"), style: bold },
            Run { text: "\nNovember 2024".into(), style: RunStyle::default() },
        ],
        body: Default::default(),
    }];
    d
}

fn text_of(d: &Deck) -> String {
    match &d.slides[0].objects[0] {
        SlideObject::TextBox { text, .. } => text.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_line_break_survives_pptx_as_a_break() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("br.pptx");
    write_pptx(path.to_str().unwrap(), &deck()).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("ppt/slides/slide1.xml").unwrap(), &mut xml).unwrap();
    assert!(xml.contains("and</a:t></a:r><a:br/><a:r>"), "an a:br between the runs: {xml}");
    assert!(!xml.contains(BREAK), "no raw separator in the text");
    let back = read_pptx(path.to_str().unwrap()).unwrap();
    assert_eq!(text_of(&back), text_of(&deck()));
    let SlideObject::TextBox { runs, .. } = &back.slides[0].objects[0] else { unreachable!() };
    assert!(runs[0].style.bold, "the run after the break keeps its look: {runs:?}");
}

#[test]
fn a_line_break_survives_odp_as_a_break() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("br.odp");
    decks_core::odp::write(&deck(), path.to_str().unwrap()).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("content.xml").unwrap(), &mut xml).unwrap();
    assert!(xml.contains("<text:line-break/>"), "{xml}");
    let back = decks_core::odp::read(path.to_str().unwrap()).unwrap();
    assert_eq!(text_of(&back), text_of(&deck()));
}
