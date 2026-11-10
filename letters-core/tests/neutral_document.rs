// SPDX-License-Identifier: GPL-3.0-or-later
//! Letters' text against the suite's neutral text (ADR 0012): the shared
//! enums are the suite's own, the JSON one writes reads in the other, and
//! the conversions keep every neutral field and drop only Letters' own.

use letters_core::model::{Alignment, ListKind, ParaStyle, Paragraph, Run, RunStyle, VertAlign};
use suite_common_core::document as neutral;

fn letters_paragraph() -> Paragraph {
    Paragraph {
        style: ParaStyle {
            heading: Some(2),
            alignment: Alignment::Justify,
            list: ListKind::Numbered,
            list_level: 1,
            line_spacing: 1.5,
            space_before_pt: 6.0,
            space_after_pt: 12.0,
            named_style: Some("Subtitle".into()),
            // Letters' own.
            left_indent_pt: 36.0,
            block_quote: true,
            keep_with_next: true,
            ..ParaStyle::default()
        },
        runs: vec![
            Run {
                text: "Shared".into(),
                style: RunStyle {
                    bold: true,
                    regular: true,
                    italic: true,
                    upright: true,
                    caps: true,
                    underline: true,
                    strikethrough: true,
                    highlight: true,
                    code: true,
                    link: Some("https://example.org".into()),
                    font_family: Some("Inter".into()),
                    font_size_hp: Some(28),
                    color: Some("336699".into()),
                    vert_align: Some(VertAlign::Subscript),
                    // Letters' own.
                    kerned: true,
                    footnote: Some(0),
                    comments: vec![4],
                    letter_spacing_tw: Some(-10),
                    ..RunStyle::default()
                },
            },
            Run::plain(" text"),
        ],
    }
}

/// What converting to the neutral form should leave: the Letters paragraph
/// with its own fields at their defaults.
fn without_letters_fields(mut p: Paragraph) -> Paragraph {
    p.style.left_indent_pt = 0.0;
    p.style.block_quote = false;
    p.style.keep_with_next = false;
    let s = &mut p.runs[0].style;
    s.kerned = false;
    s.footnote = None;
    s.comments.clear();
    s.letter_spacing_tw = None;
    p
}

#[test]
fn the_shared_enums_are_the_suites_own() {
    let a: neutral::Alignment = Alignment::Center;
    let l: neutral::ListKind = ListKind::Bullet;
    let v: neutral::VertAlign = VertAlign::Superscript;
    assert_eq!((a, l, v), (neutral::Alignment::Center, neutral::ListKind::Bullet, neutral::VertAlign::Superscript));
}

#[test]
fn converting_keeps_every_neutral_field_and_drops_only_letters_own() {
    let p = letters_paragraph();
    let n: neutral::Paragraph = (&p).into();
    assert_eq!(n.style.alignment, neutral::Alignment::Justify);
    assert_eq!(n.style.named_style.as_deref(), Some("Subtitle"));
    assert_eq!(n.runs[0].style.color.as_deref(), Some("336699"));
    assert_eq!(n.text(), "Shared text");

    let back: Paragraph = n.clone().into();
    assert_eq!(back, without_letters_fields(p));
    assert_eq!(neutral::Paragraph::from(&back), n, "neutral to Letters and back is lossless");
}

#[test]
fn letters_json_reads_as_neutral_text_with_letters_fields_ignored() {
    let p = letters_paragraph();
    let json = serde_json::to_string(&p).unwrap();
    let n: neutral::Paragraph = serde_json::from_str(&json).unwrap();
    assert_eq!(n, neutral::Paragraph::from(&p));
}

#[test]
fn neutral_json_reads_in_letters_with_letters_fields_at_their_defaults() {
    let n = neutral::Paragraph::from(&letters_paragraph());
    let json = serde_json::to_string(&n).unwrap();
    let p: Paragraph = serde_json::from_str(&json).unwrap();
    assert_eq!(p, Paragraph::from(n));

    // An empty style reads as Letters' default, line spacing and all.
    let bare: Paragraph = serde_json::from_str(r#"{"style":{},"runs":[{"text":"x","style":{}}]}"#).unwrap();
    assert_eq!(bare.style, ParaStyle::default());
    assert_eq!(bare.runs[0].style, RunStyle::default());
}
