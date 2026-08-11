// fragment.rs — Decks side of the cross-app clipboard.
// SPDX-License-Identifier: GPL-3.0-or-later

pub use letters_core::fragment::{Fragment, MIME};
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
    SlideObject::TextBox { text, x, y, w: 400.0, h: 100.0, rotation: 0.0, runs }
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
            rotation: 0.0,
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

    // ── copy_object edge cases ───────────────────────────────────────────────

    fn textbox(text: &str, runs: Vec<Run>) -> SlideObject {
        SlideObject::TextBox { text: text.into(), x: 0.0, y: 0.0, w: 100.0, h: 50.0, runs, rotation: 0.0 }
    }

    #[test]
    fn copy_non_textbox_returns_none() {
        assert!(copy_object(&SlideObject::Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0, rotation: 0.0 }).is_none());
        assert!(copy_object(&SlideObject::Circle { x: 0.0, y: 0.0, r: 1.0, rotation: 0.0 }).is_none());
        assert!(copy_object(&SlideObject::Image { path: "p.png".into(), x: 0.0, y: 0.0, w: 1.0, h: 1.0, rotation: 0.0 }).is_none());
    }

    #[test]
    fn copy_unstyled_textbox_falls_back_to_plain_text() {
        let frag = copy_object(&textbox("alpha\nbeta", vec![])).expect("fragment");
        match frag {
            Fragment::Text(paras) => {
                assert_eq!(paras.len(), 2);
                assert_eq!(paras[0].text(), "alpha");
                assert_eq!(paras[1].text(), "beta");
            }
            _ => panic!("expected text fragment"),
        }
    }

    #[test]
    fn copy_styled_textbox_keeps_runs_verbatim() {
        let runs = vec![Run { text: "b".into(), style: RunStyle { bold: true, ..Default::default() } }];
        let frag = copy_object(&textbox("b", runs.clone())).expect("fragment");
        match frag {
            Fragment::Text(paras) => {
                assert_eq!(paras.len(), 1);
                assert_eq!(paras[0].runs, runs);
            }
            _ => panic!("expected text fragment"),
        }
    }

    // ── paste_as_text_box edge cases ────────────────────────────────────────

    #[test]
    fn paste_multiparagraph_text_drops_runs() {
        let d = letters_core::model::Document::from_plain_text("l1\nl2");
        let frag = Fragment::Text(d.paragraphs);
        let back = paste_as_text_box(&frag, 1.0, 2.0);
        match back {
            SlideObject::TextBox { text, runs, x, y, .. } => {
                assert_eq!(text, "l1\nl2");
                assert!(runs.is_empty(), "multi-paragraph paste must drop runs");
                assert_eq!((x, y), (1.0, 2.0));
            }
            _ => panic!("wrong object"),
        }
    }

    #[test]
    fn paste_single_paragraph_keeps_runs() {
        let mut d = letters_core::model::Document::from_plain_text("styled");
        d.paragraphs[0].runs = vec![Run { text: "styled".into(), style: RunStyle { italic: true, ..Default::default() } }];
        let frag = Fragment::Text(d.paragraphs);
        let back = paste_as_text_box(&frag, 0.0, 0.0);
        match back {
            SlideObject::TextBox { runs, .. } => {
                assert_eq!(runs.len(), 1);
                assert!(runs[0].style.italic);
            }
            _ => panic!("wrong object"),
        }
    }

    #[test]
    fn grid_pastes_as_tsv_with_default_geometry() {
        use letters_core::fragment::GridCell;
        let frag = Fragment::Grid(vec![
            vec![GridCell { value: "a".into(), ..Default::default() }],
            vec![GridCell { value: "b".into(), ..Default::default() }],
        ]);
        match paste_as_text_box(&frag, 5.0, 6.0) {
            SlideObject::TextBox { text, x, y, w, h, .. } => {
                assert_eq!(text, "a\nb");
                assert_eq!((x, y, w, h), (5.0, 6.0, 400.0, 100.0));
            }
            _ => panic!("wrong object"),
        }
    }
}
