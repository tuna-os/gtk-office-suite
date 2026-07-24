// property.rs — generative round-trip testing (#106), mirrors
// tables-core/tests/property.rs.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Hand-authored docx tests cover specific scenarios we thought to write
// (there are none for docx.rs specifically yet — odt.rs has several).
// proptest generates many short plain-text documents and asserts the
// docx write/read round trip is lossless for each. Deliberately
// conservative for this first slice: plain paragraphs of short lowercase
// words only, no empty paragraphs, no formatting/styles/headings — those
// have their own hand-authored coverage and interact with docx's XML
// structure in ways worth testing deliberately, not blurred into this
// property.

use proptest::prelude::*;
use letters_core::model::Document;

/// A short paragraph: 1-6 short lowercase words.
fn paragraph_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec("[a-z]{1,8}", 1..6).prop_map(|words| words.join(" "))
}

/// 1-5 non-empty paragraphs, joined the way Document::from_plain_text
/// expects (newline-separated).
fn document_text_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(paragraph_strategy(), 1..5).prop_map(|paras| paras.join("\n"))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn docx_round_trip_preserves_plain_text(text in document_text_strategy()) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prop.docx");

        let doc = Document::from_plain_text(&text);
        letters_core::docx::write(&doc, path.to_str().unwrap()).unwrap();
        let read_back = letters_core::docx::read(path.to_str().unwrap()).unwrap();

        prop_assert_eq!(
            read_back.to_plain_text(), doc.to_plain_text(),
            "docx round trip changed plain text for input {:?}", text
        );
    }
}
