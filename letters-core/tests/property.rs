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

// ── Unicode/grapheme-cluster coverage (#106) ────────────────────────────
//
// Document::insert_text/delete_range/style_at split runs by *char* index
// (Rust `char`, i.e. Unicode scalar value — not grapheme cluster; that's
// a pre-existing, documented characteristic, not something this test
// asserts is wrong). What they must never do, on any input, is panic —
// Rust string slicing on a non-char-boundary byte offset panics, and
// that's exactly the failure mode multi-byte UTF-8 (combining marks,
// emoji, CJK, RTL) would trigger if the offset math were byte-based
// instead of char-based anywhere in the split path. These properties
// stress that with generated Unicode text no hand-written test would
// think to compose, and check two structural invariants alongside the
// no-panic guarantee.

fn unicode_char_strategy() -> impl Strategy<Value = char> {
    prop_oneof![
        3 => prop::char::range('a', 'z'),
        // Combining acute accent — combines with the preceding char into
        // one grapheme cluster but is a second `char`/scalar value.
        1 => Just('\u{0301}'),
        // Multi-byte, single scalar value, single grapheme: CJK.
        1 => prop::char::range('\u{4e00}', '\u{4eff}'),
        // Emoji: multi-byte, well outside the BMP.
        1 => prop::char::range('\u{1f600}', '\u{1f64f}'),
        // RTL (Hebrew block): exercises bidi-adjacent text without
        // requiring actual bidi layout logic in this GTK-free model.
        1 => prop::char::range('\u{05d0}', '\u{05ea}'),
    ]
}

fn unicode_text_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(unicode_char_strategy(), 0..12)
        .prop_map(|chars| chars.into_iter().collect())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Inserting Unicode text at any valid offset, then deleting exactly
    /// that range back out, must be a no-op on the document's plain text
    /// — and must not panic on any char-boundary math along the way.
    #[test]
    fn insert_then_delete_same_range_is_identity(
        base in unicode_text_strategy(),
        insert in unicode_text_strategy(),
    ) {
        let mut doc = Document::from_plain_text(&base);
        let before = doc.to_plain_text();
        let doc_len = doc.char_len();
        let offset = if doc_len == 0 { 0 } else { doc_len / 2 };

        doc.insert_text(offset, &insert);
        doc.delete_range(offset, offset + insert.chars().count());

        prop_assert_eq!(doc.to_plain_text(), before);
    }

    /// char_len() must always agree with actually counting the
    /// paragraph-joined plain text's chars, for any Unicode input —
    /// catches drift between the two independent counting paths (one
    /// sums run lengths + paragraph breaks, the other walks the
    /// assembled string) rather than assuming they can't diverge.
    #[test]
    fn char_len_matches_plain_text_char_count(text in unicode_text_strategy()) {
        let doc = Document::from_plain_text(&text);
        prop_assert_eq!(doc.char_len(), doc.to_plain_text().chars().count());
    }

    /// style_at must not panic at any offset in [0, char_len()], the
    /// documented valid range (including the end-of-document position),
    /// regardless of what Unicode content precedes it.
    #[test]
    fn style_at_does_not_panic_across_the_full_offset_range(text in unicode_text_strategy()) {
        let doc = Document::from_plain_text(&text);
        let len = doc.char_len();
        for offset in 0..=len {
            let _ = doc.style_at(offset);
        }
    }
}
