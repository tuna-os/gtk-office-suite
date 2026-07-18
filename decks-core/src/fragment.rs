// fragment.rs — Decks side of the cross-app clipboard.
// SPDX-License-Identifier: GPL-3.0-or-later

use letters_core::fragment::Fragment;
use letters_core::model::Paragraph;

use crate::engine::SlideObject;

/// Copy a text box as a styled-text fragment (runs carried verbatim).
pub fn copy_object(obj: &SlideObject) -> Option<Fragment> {
    match obj {
        SlideObject::TextBox { text, runs, .. } => {
            let paras = if runs.is_empty() {
                letters_core::model::Document::from_plain_text(text).paragraphs
            } else {
                vec![Paragraph { style: Default::default(), runs: runs.clone() }]
            };
            Some(Fragment::Text(paras))
        }
        _ => None,
    }
}

/// Paste a fragment as a new text box at the given position.
pub fn paste_as_text_box(frag: &Fragment, x: f64, y: f64) -> SlideObject {
    let (text, runs) = match frag {
        Fragment::Text(paras) => {
            let text = paras.iter().map(|p| p.text()).collect::<Vec<_>>().join("\n");
            let runs = if paras.len() == 1 { paras[0].runs.clone() } else { vec![] };
            (text, runs)
        }
        Fragment::Grid(_) => (frag.to_plain(), vec![]),
    };
    SlideObject::TextBox { text, x, y, w: 400.0, h: 100.0, runs }
}

#[cfg(test)]
mod tests {
    use super::*;
    use letters_core::model::{Run, RunStyle};

    #[test]
    fn styled_text_survives_decks_round_trip() {
        let obj = SlideObject::TextBox {
            text: "plain bold".into(),
            x: 0.0, y: 0.0, w: 100.0, h: 50.0,
            runs: vec![
                Run { text: "plain ".into(), style: RunStyle::default() },
                Run { text: "bold".into(), style: RunStyle { bold: true, ..Default::default() } },
            ],
        };
        let frag = copy_object(&obj).expect("fragment");
        let back = paste_as_text_box(&frag, 10.0, 10.0);
        match back {
            SlideObject::TextBox { runs, text, .. } => {
                assert_eq!(text, "plain bold");
                assert!(runs.iter().any(|r| r.style.bold && r.text == "bold"),
                        "bold run lost through clipboard");
            }
            _ => panic!("wrong object"),
        }
    }

    #[test]
    fn grid_pastes_as_tsv_text_box() {
        use letters_core::fragment::GridCell;
        let frag = Fragment::Grid(vec![vec![
            GridCell { value: "a".into(), ..Default::default() },
            GridCell { value: "b".into(), ..Default::default() },
        ]]);
        match paste_as_text_box(&frag, 0.0, 0.0) {
            SlideObject::TextBox { text, .. } => assert_eq!(text, "a\tb"),
            _ => panic!("wrong object"),
        }
    }
}
