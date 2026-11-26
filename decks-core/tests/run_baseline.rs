// run_baseline.rs — a run's superscript or subscript survives a pptx save.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// DrawingML's `baseline` raises (above 0) or lowers (below 0) a run, which
// PowerPoint and Impress draw smaller. Decks read it as plain text, so a
// PDF-converted dashboard's superscript labels ("CIRNAC" at
// baseline="33333") were drawn full size and wrapped in their boxes.

use decks_core::engine::{read_pptx, write_pptx, Deck, SlideObject};
use suite_common_core::document::{Run, RunStyle, VertAlign};

#[test]
fn superscript_and_subscript_survive_a_pptx_save() {
    let mut d = Deck::new();
    let up = RunStyle { vert_align: Some(VertAlign::Superscript), ..Default::default() };
    let down = RunStyle { vert_align: Some(VertAlign::Subscript), ..Default::default() };
    d.slides[0].objects = vec![SlideObject::TextBox {
        text: "x2 H2O".into(),
        x: 100.0,
        y: 100.0,
        w: 400.0,
        h: 100.0,
        rotation: 0.0,
        runs: vec![
            Run { text: "x".into(), style: RunStyle::default() },
            Run { text: "2".into(), style: up },
            Run { text: " H".into(), style: RunStyle::default() },
            Run { text: "2".into(), style: down },
            Run { text: "O".into(), style: RunStyle::default() },
        ],
        body: Default::default(),
    }];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("baseline.pptx");
    write_pptx(path.to_str().unwrap(), &d).unwrap();
    let back = read_pptx(path.to_str().unwrap()).unwrap();
    let runs = back.slides[0]
        .objects
        .iter()
        .find_map(|o| match o {
            SlideObject::TextBox { runs, .. } => Some(runs.clone()),
            _ => None,
        })
        .expect("the text box");
    let looks: Vec<(String, Option<VertAlign>)> = runs.iter().map(|r| (r.text.clone(), r.style.vert_align)).collect();
    assert_eq!(
        looks,
        [
            ("x".into(), None),
            ("2".into(), Some(VertAlign::Superscript)),
            (" H".into(), None),
            ("2".into(), Some(VertAlign::Subscript)),
            ("O".into(), None),
        ],
        "{looks:?}"
    );
}
