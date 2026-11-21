// run_caps.rs — a run's capitals survive a pptx save.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// DrawingML's `cap="all"` and `cap="small"` draw a run in capitals or
// small capitals whatever case it is typed in. Decks read neither: the
// staffing questionnaire's titles, typed in lower case under
// `cap="small"`, came out in lower case.

use decks_core::engine::{read_pptx, write_pptx, Deck, SlideObject};
use suite_common_core::document::{Run, RunStyle};

#[test]
fn capitals_and_small_capitals_survive_a_pptx_save() {
    let mut d = Deck::new();
    let caps = RunStyle { caps: true, ..Default::default() };
    let small = RunStyle { small_caps: true, ..Default::default() };
    d.slides[0].objects = vec![SlideObject::TextBox {
        text: "all small plain".into(),
        x: 100.0,
        y: 100.0,
        w: 400.0,
        h: 100.0,
        rotation: 0.0,
        runs: vec![
            Run { text: "all ".into(), style: caps },
            Run { text: "small ".into(), style: small },
            Run { text: "plain".into(), style: RunStyle::default() },
        ],
        body: Default::default(),
    }];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("caps.pptx");
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
    let looks: Vec<(String, bool, bool)> = runs.iter().map(|r| (r.text.clone(), r.style.caps, r.style.small_caps)).collect();
    assert_eq!(
        looks,
        [("all ".into(), true, false), ("small ".into(), false, true), ("plain".into(), false, false)],
        "{looks:?}"
    );
}
