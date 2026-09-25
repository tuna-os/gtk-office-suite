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

// ── diff and history ──────────────────────────────────────────────────────

fn with_para_styles(mut d: Document, seed: u64) -> Document {
    for (i, p) in d.paragraphs.iter_mut().enumerate() {
        match (seed + i as u64) % 4 {
            0 => p.style.heading = Some(2),
            1 => p.style.list = ListKind::Bullet,
            2 => p.style.space_after_pt = 6.0,
            _ => {}
        }
    }
    d
}

proptest! {
    /// Applying `diff(a, b)` to `a` gives `b`, for unrelated documents too,
    /// and its undo gives `a` back.
    #[test]
    fn diff_turns_one_document_into_another(a in arb_doc(), b in arb_doc(), sa in 0u64..4, sb in 0u64..4) {
        let (a, b) = (with_para_styles(a, sa), with_para_styles(b, sb));
        let ops = diff(&a, &b);
        let mut d = a.clone();
        let undo = apply_all(&mut d, &ops).unwrap();
        prop_assert_eq!(&d, &b);
        apply_all(&mut d, &undo).unwrap();
        prop_assert_eq!(d, a);
    }

    /// A small edit diffs to small ops: typing in one paragraph is one
    /// insert, never a rewrite.
    #[test]
    fn a_small_edit_diffs_to_a_small_op(a in arb_doc(), at in any::<usize>()) {
        let mut b = a.clone();
        let at = at % (doc_len(&a) + 1);
        apply(&mut b, &Op::Insert { at, content: vec![Paragraph { style: ParaStyle::default(), runs: vec![Run::plain("zz")] }] }).unwrap();
        let ops = diff(&a, &b);
        prop_assert!(ops.len() == 1, "{:?}", ops);
        let is_small_insert = matches!(&ops[0], Op::Insert { content, .. } if seq_len(&content[0].runs) == 2);
        prop_assert!(is_small_insert, "{:?}", ops);
    }
}

#[test]
fn diff_handles_table_structure_with_a_block_op() {
    let a = Document::from_plain_text("before\nafter");
    let mut b = a.clone();
    b.insert_table_at(1, 2, 2);
    let ops = diff(&a, &b);
    assert!(ops.iter().any(|o| matches!(o, Op::SetParagraphs { .. })), "{ops:?}");
    let mut d = a.clone();
    let undo = apply_all(&mut d, &ops).unwrap();
    assert_eq!(d, b);
    // Typing in a cell is a text op, not a block op.
    let mut c = b.clone();
    let cell = c.paragraphs.iter().position(|p| p.style.table_cell.is_some()).unwrap();
    c.paragraphs[cell].runs = vec![Run::plain("x")];
    let ops = diff(&b, &c);
    assert!(matches!(ops.as_slice(), [Op::Insert { .. }]), "{ops:?}");
    apply_all(&mut d, &undo).unwrap();
    assert_eq!(d, a);
}

#[test]
fn history_undoes_and_redoes_whole_user_actions() {
    let mut d = Document::from_plain_text("hello");
    let mut h = History::default();
    // One action: two ops.
    h.begin();
    let op = typing(&d, 5, " there").unwrap();
    h.record(apply(&mut d, &op).unwrap());
    h.record(apply(&mut d, &Op::Mark { start: 0, end: 5, key: MarkKey::Bold, value: bold() }).unwrap());
    h.end();
    // A second action.
    let op = typing(&d, 11, "!").unwrap();
    h.record(apply(&mut d, &op).unwrap());
    let after = d.clone();
    h.undo(&mut d).unwrap();
    assert_eq!(sequence_text(&d), "hello there");
    h.undo(&mut d).unwrap();
    assert_eq!(d, Document::from_plain_text("hello"), "both ops of the first action undone together");
    assert!(!h.can_undo());
    h.redo(&mut d).unwrap();
    h.redo(&mut d).unwrap();
    assert_eq!(d, after);
    // Typing word characters merges into one step; a space ends the word.
    let mut e = Document::from_plain_text("");
    let mut g = History::default();
    for (i, c) in "ab c".chars().enumerate() {
        let op = typing(&e, i, &c.to_string()).unwrap();
        g.set_merge(!c.is_whitespace());
        g.record(apply(&mut e, &op).unwrap());
    }
    g.undo(&mut e).unwrap();
    assert_eq!(sequence_text(&e), "ab ");
    g.undo(&mut e).unwrap();
    assert_eq!(sequence_text(&e), "ab");
    g.undo(&mut e).unwrap();
    assert_eq!(sequence_text(&e), "", "'ab' was one step");
    // A new change clears redo.
    h.undo(&mut d).unwrap();
    let op = typing(&d, 0, ">").unwrap();
    h.record(apply(&mut d, &op).unwrap());
    assert!(!h.can_redo());
}
