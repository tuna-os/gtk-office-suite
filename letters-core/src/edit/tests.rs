use super::*;
use crate::model::{ListKind, TableCell};
use proptest::prelude::*;

fn bold() -> RunStyle {
    RunStyle { bold: true, ..Default::default() }
}

fn link() -> RunStyle {
    RunStyle { link: Some("https://gnome.org".into()), ..Default::default() }
}

/// "bold link plain", as the CRDT spike's Letters probe marks it.
fn probe_doc() -> Document {
    let mut d = Document::new();
    d.paragraphs[0].runs = vec![
        Run { text: "bold".into(), style: bold() },
        Run::plain(" "),
        Run { text: "link".into(), style: link() },
        Run::plain(" plain"),
    ];
    d
}

#[test]
fn typing_after_bold_is_bold_after_a_link_is_not() {
    let d = probe_doc();
    assert!(typing_style(&d, 4).bold, "at the end of 'bold'");
    assert_eq!(typing_style(&d, 9).link, None, "at the end of 'link'");
    assert_eq!(typing_style(&d, 7), RunStyle::default(), "inside a link: links never expand");
    assert!(!typing_style(&d, 5).bold, "after the plain space");
    // Typing at a paragraph's start takes its first text's marks.
    assert!(typing_style(&d, 0).bold);
}

#[test]
fn typed_text_carries_its_style_explicitly() {
    let mut d = probe_doc();
    let op = typing(&d, 4, "er").unwrap();
    apply(&mut d, &op).unwrap();
    assert_eq!(d.paragraphs[0].runs[0], Run { text: "bolder".into(), style: bold() });
    // Replaying the same op on another copy gives the same result without
    // consulting any expand rule: the op is self-describing.
    let mut other = probe_doc();
    apply(&mut other, &op).unwrap();
    assert_eq!(other, d);
}

#[test]
fn enter_splits_a_paragraph_and_undo_joins_it() {
    let mut d = Document::from_plain_text("hello world");
    d.paragraphs[0].style.list = ListKind::Bullet;
    let before = d.clone();
    let op = typing(&d, 5, "\n").unwrap();
    let undo = apply(&mut d, &op).unwrap();
    assert_eq!(sequence_text(&d), "hello\n world");
    assert_eq!(d.paragraphs[1].style.list, ListKind::Bullet, "the new item continues the list");
    apply_all(&mut d, &undo).unwrap();
    assert_eq!(d, before);
}

#[test]
fn deleting_a_break_joins_paragraphs_and_undo_restores_both_styles() {
    let mut d = Document::from_plain_text("Title\nbody");
    d.paragraphs[0].style.heading = Some(1);
    d.paragraphs[1].style.space_before_pt = 12.0;
    let before = d.clone();
    let undo = apply(&mut d, &Op::Delete { at: 3, len: 4 }).unwrap();
    assert_eq!(sequence_text(&d), "Titody", "\"le\\nb\" removed");
    assert_eq!(d.paragraphs.len(), 1);
    assert_eq!(d.paragraphs[0].style.heading, Some(1), "the first paragraph's style survives");
    apply_all(&mut d, &undo).unwrap();
    assert_eq!(d, before);
}

#[test]
fn a_mark_undo_restores_mixed_values() {
    let mut d = probe_doc();
    let before = d.clone();
    let undo = apply(&mut d, &Op::Mark { start: 0, end: 15, key: MarkKey::Bold, value: bold() }).unwrap();
    assert!(d.paragraphs[0].runs.iter().all(|r| r.style.bold));
    apply_all(&mut d, &undo).unwrap();
    assert_eq!(d, before);
}

#[test]
fn objects_are_one_sequence_char_and_keep_their_style() {
    let mut d = Document::from_plain_text("see  here");
    d.paragraphs[0].runs = vec![
        Run::plain("see "),
        Run { text: "alt".into(), style: RunStyle { image: Some("a.png".into()), ..Default::default() } },
        Run { text: String::new(), style: RunStyle { footnote: Some(0), ..Default::default() } },
        Run::plain(" here"),
    ];
    assert_eq!(doc_len(&d), 11);
    assert_eq!(sequence_text(&d), "see \u{FFFC}\u{FFFC} here");
    let before = d.clone();
    let undo = apply(&mut d, &Op::Mark { start: 0, end: 11, key: MarkKey::Italic, value: RunStyle { italic: true, ..Default::default() } }).unwrap();
    assert!(!d.paragraphs[0].runs[1].style.italic, "an image is not italicised");
    apply_all(&mut d, &undo).unwrap();
    assert_eq!(d, before);
    // Deleting the image removes it whole, and undo puts it back.
    let undo = apply(&mut d, &Op::Delete { at: 4, len: 1 }).unwrap();
    assert_eq!(sequence_text(&d), "see \u{FFFC} here");
    apply_all(&mut d, &undo).unwrap();
    assert_eq!(d, before);
}

#[test]
fn a_table_cells_boundaries_cannot_be_joined_or_split() {
    let mut d = Document::from_plain_text("after");
    d.insert_table_at(0, 1, 2);
    let before = d.clone();
    // The break between the two cells is at offset 0 (both empty).
    assert_eq!(apply(&mut d, &Op::Delete { at: 0, len: 1 }), Err(EditError::TableStructure));
    assert_eq!(apply(&mut d, &typing(&before, 0, "\n").unwrap()), Err(EditError::TableStructure));
    assert_eq!(d, before, "a refused op changes nothing");
    // Typing inside a cell is fine.
    apply(&mut d, &typing(&before, 0, "x").unwrap()).unwrap();
    assert_eq!(d.paragraphs[0].style.table_cell, Some(TableCell { table: 1, row: 0, col: 0 }));
    assert_eq!(d.paragraphs[0].text(), "x");
}

#[test]
fn ops_serialise() {
    let op = Op::Mark { start: 1, end: 3, key: MarkKey::Link, value: link() };
    let json = serde_json::to_string(&op).unwrap();
    assert_eq!(serde_json::from_str::<Op>(&json).unwrap(), op);
}

// ── Property: ops behave as sequence edits, and undo is exact ─────────────

fn arb_style() -> impl Strategy<Value = RunStyle> {
    (any::<bool>(), any::<bool>(), prop::option::of(Just("https://x".to_string())))
        .prop_map(|(bold, italic, link)| RunStyle { bold, italic, link, ..Default::default() })
}

fn arb_doc() -> impl Strategy<Value = Document> {
    prop::collection::vec(prop::collection::vec(("[a-c ]{1,4}", arb_style()), 0..4), 1..5).prop_map(|paras| {
        let mut d = Document::new();
        d.paragraphs = paras
            .into_iter()
            .map(|runs| {
                let mut runs: Vec<Run> = runs.into_iter().map(|(text, style)| Run { text, style }).collect();
                normalize(&mut runs);
                Paragraph { style: ParaStyle::default(), runs }
            })
            .collect();
        d
    })
}

#[derive(Clone, Debug)]
enum Action {
    Type(usize, String),
    Delete(usize, usize),
    Bold(usize, usize, bool),
}

fn arb_action() -> impl Strategy<Value = Action> {
    prop_oneof![
        (any::<usize>(), "[x\n]{1,3}").prop_map(|(a, t)| Action::Type(a, t)),
        (any::<usize>(), 0usize..6).prop_map(|(a, n)| Action::Delete(a, n)),
        (any::<usize>(), 0usize..8, any::<bool>()).prop_map(|(a, n, b)| Action::Bold(a, n, b)),
    ]
}

proptest! {
    #[test]
    fn ops_are_sequence_edits_and_undo_restores_the_document(doc in arb_doc(), actions in prop::collection::vec(arb_action(), 1..8)) {
        let mut d = doc.clone();
        let mut text: Vec<char> = sequence_text(&d).chars().collect();
        let mut undo: Vec<Vec<Op>> = Vec::new();
        for action in actions {
            let len = doc_len(&d);
            let op = match action {
                Action::Type(a, t) => typing(&d, a % (len + 1), &t).unwrap(),
                Action::Delete(a, n) => { let at = a % (len + 1); Op::Delete { at, len: n.min(len - at) } }
                Action::Bold(a, n, b) => {
                    let at = a % (len + 1);
                    Op::Mark { start: at, end: (at + n).min(len), key: MarkKey::Bold, value: RunStyle { bold: b, ..Default::default() } }
                }
            };
            let u = apply(&mut d, &op).unwrap();
            // The sequence text changes exactly as a string edit would.
            match &op {
                Op::Insert { at, content } => {
                    let s: String = content.iter().map(|p| crate::layout::layout_text(&p.runs)).collect::<Vec<_>>().join("\n");
                    text.splice(*at..*at, s.chars());
                }
                Op::Delete { at, len } => { text.drain(*at..*at + *len); }
                _ => {}
            }
            prop_assert_eq!(sequence_text(&d), text.iter().collect::<String>());
            prop_assert_eq!(doc_len(&d), text.len());
            undo.push(u);
        }
        for u in undo.into_iter().rev() {
            apply_all(&mut d, &u).unwrap();
        }
        prop_assert_eq!(d, doc);
    }
}
