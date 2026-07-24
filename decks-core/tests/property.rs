// property.rs — generative round-trip testing (#106), mirrors
// tables-core/tests/property.rs and letters-core/tests/property.rs.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// proptest generates decks with a few slides, each holding a few plain
// text boxes, and asserts the pptx write/read round trip preserves
// object count, order, and text exactly. Deliberately conservative like
// the other two apps' first slices: TextBox text only (no runs/styling),
// no Rect/Circle/Image geometry, no masters — those have their own
// hand-authored coverage (engine.rs's test_pptx_roundtrip already
// exercises Rect/Circle) and aren't blurred into this property.
//
// Slide *titles* are deliberately not asserted: pptx read synthesizes
// "Slide N" from position rather than reading back a written title
// (discovered and documented earlier this session, in the
// DecksController round-trip test) — that's a pre-existing engine.rs
// characteristic, not something this property should re-litigate.

use proptest::prelude::*;
use decks_core::engine::{read_pptx, write_pptx, Deck, Slide, SlideObject};

fn text_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec("[a-z]{1,8}", 1..5).prop_map(|words| words.join(" "))
}

fn slide_strategy() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(text_strategy(), 0..3)
}

fn deck_strategy() -> impl Strategy<Value = Vec<Vec<String>>> {
    prop::collection::vec(slide_strategy(), 1..4)
}

fn build_deck(slides: &[Vec<String>]) -> Deck {
    let slides = slides
        .iter()
        .map(|texts| Slide {
            title: "Slide".into(),
            background: "#ffffff".into(),
            objects: texts
                .iter()
                .map(|t| SlideObject::TextBox {
                    text: t.clone(),
                    x: 10.0, y: 10.0, w: 100.0, h: 20.0,
                    runs: vec![],
                })
                .collect(),
            notes: String::new(),
            master_idx: Some(0),
        })
        .collect();
    Deck { slides, masters: vec![] }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn pptx_round_trip_preserves_text_box_content(slides in deck_strategy()) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prop.pptx");

        let deck = build_deck(&slides);
        write_pptx(path.to_str().unwrap(), &deck).unwrap();
        let read_back = read_pptx(path.to_str().unwrap()).unwrap();

        prop_assert_eq!(read_back.slides.len(), deck.slides.len());
        for (expected, actual) in deck.slides.iter().zip(read_back.slides.iter()) {
            let expected_texts: Vec<&str> = expected.objects.iter().map(|o| match o {
                SlideObject::TextBox { text, .. } => text.as_str(),
                _ => unreachable!(),
            }).collect();
            let actual_texts: Vec<&str> = actual.objects.iter().filter_map(|o| match o {
                SlideObject::TextBox { text, .. } => Some(text.as_str()),
                _ => None,
            }).collect();
            prop_assert_eq!(actual_texts, expected_texts);
        }
    }
}
