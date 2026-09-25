// A plain-text document and its two ops: the smallest thing the shared
// history can be tested on without any app's model.

use super::*;
use proptest::prelude::*;

#[derive(Clone, Debug, PartialEq)]
enum TextOp {
    Insert { at: usize, text: String },
    Delete { at: usize, len: usize },
}

#[derive(Debug, PartialEq)]
struct OutOfRange;

impl Op for TextOp {
    type Doc = Vec<char>;
    type Error = OutOfRange;

    fn apply(&self, doc: &mut Vec<char>) -> Result<Vec<Self>, OutOfRange> {
        match self {
            TextOp::Insert { at, text } => {
                if *at > doc.len() {
                    return Err(OutOfRange);
                }
                doc.splice(*at..*at, text.chars());
                Ok(vec![TextOp::Delete { at: *at, len: text.chars().count() }])
            }
            TextOp::Delete { at, len } => {
                if at + len > doc.len() {
                    return Err(OutOfRange);
                }
                let text: String = doc.drain(*at..at + len).collect();
                Ok(vec![TextOp::Insert { at: *at, text }])
            }
        }
    }

    fn coalesce(&mut self, next: &Self) -> bool {
        match (self, next) {
            (TextOp::Delete { at, len }, TextOp::Delete { at: next_at, len: next_len }) if *at + *len == *next_at => {
                *len += next_len;
                true
            }
            _ => false,
        }
    }
}

fn text(doc: &[char]) -> String {
    doc.iter().collect()
}

/// Type `s` at the end of `doc` a character at a time, as a keyboard does:
/// word characters marked for merging, anything else not.
fn type_chars(doc: &mut Vec<char>, h: &mut History<TextOp>, s: &str) {
    for c in s.chars() {
        let op = TextOp::Insert { at: doc.len(), text: c.to_string() };
        h.set_merge(!c.is_whitespace());
        h.record(op.apply(doc).unwrap());
    }
}

#[test]
fn typed_words_are_one_step_each_and_a_space_is_its_own() {
    let mut doc = Vec::new();
    let mut h = History::default();
    type_chars(&mut doc, &mut h, "ab cd");
    assert_eq!(text(&doc), "ab cd");
    h.undo(&mut doc).unwrap();
    assert_eq!(text(&doc), "ab ", "'cd' was one step");
    h.undo(&mut doc).unwrap();
    assert_eq!(text(&doc), "ab", "the space was a step of its own");
    h.undo(&mut doc).unwrap();
    assert_eq!(text(&doc), "", "'ab' was one step");
    assert!(!h.can_undo());
    h.redo(&mut doc).unwrap();
    h.redo(&mut doc).unwrap();
    h.redo(&mut doc).unwrap();
    assert_eq!(text(&doc), "ab cd");
    assert!(!h.can_redo());
}

#[test]
fn typing_after_an_undo_or_elsewhere_starts_a_new_step() {
    let mut doc = Vec::new();
    let mut h = History::default();
    type_chars(&mut doc, &mut h, "ab");
    h.undo(&mut doc).unwrap();
    type_chars(&mut doc, &mut h, "x");
    assert!(!h.can_redo(), "a new change clears redo");
    // Typing at the start is not contiguous with the step before: its
    // inverse does not coalesce, so it is a step of its own.
    let op = TextOp::Insert { at: 0, text: "y".into() };
    h.set_merge(true);
    h.record(op.apply(&mut doc).unwrap());
    h.undo(&mut doc).unwrap();
    assert_eq!(text(&doc), "x");
}

#[test]
fn a_group_is_one_step_undone_in_reverse() {
    let mut doc: Vec<char> = "hello".chars().collect();
    let mut h = History::default();
    h.begin();
    h.record(TextOp::Insert { at: 5, text: " world".into() }.apply(&mut doc).unwrap());
    h.record(TextOp::Delete { at: 0, len: 1 }.apply(&mut doc).unwrap());
    h.end();
    assert_eq!(text(&doc), "ello world");
    let applied = h.undo(&mut doc).unwrap();
    assert_eq!(applied.len(), 2);
    assert_eq!(text(&doc), "hello");
}

#[test]
fn apply_all_leaves_the_document_alone_when_an_op_fails() {
    let mut doc: Vec<char> = "abc".chars().collect();
    let ops = [TextOp::Insert { at: 3, text: "d".into() }, TextOp::Delete { at: 9, len: 1 }];
    assert_eq!(apply_all(&mut doc, &ops), Err(OutOfRange));
    assert_eq!(text(&doc), "abc");
}

#[test]
fn a_history_that_no_longer_fits_is_dropped_not_half_applied() {
    let mut doc = Vec::new();
    let mut h = History::default();
    type_chars(&mut doc, &mut h, "abc");
    // The document is replaced under the history.
    doc = vec!['z'];
    assert!(h.undo(&mut doc).is_none());
    assert_eq!(text(&doc), "z");
    assert!(!h.can_undo() && !h.can_redo());
}

#[derive(Clone, Debug)]
enum Action {
    Type(String),
    Delete(usize, usize),
    Undo,
    Redo,
}

fn arb_action() -> impl Strategy<Value = Action> {
    prop_oneof![
        "[ab ]{1,4}".prop_map(Action::Type),
        (any::<usize>(), 0usize..4).prop_map(|(a, n)| Action::Delete(a, n)),
        Just(Action::Undo),
        Just(Action::Redo),
    ]
}

proptest! {
    /// Whatever mix of typing, deleting, undo and redo: undoing everything
    /// gives the original document, and redoing everything gives back the
    /// document as it was before those undos.
    #[test]
    fn undo_all_restores_and_redo_all_replays(start in "[a-c]{0,6}", actions in prop::collection::vec(arb_action(), 1..24)) {
        let original: Vec<char> = start.chars().collect();
        let mut doc = original.clone();
        let mut h = History::default();
        for action in actions {
            match action {
                Action::Type(s) => {
                    let at = doc.len();
                    for (i, c) in s.chars().enumerate() {
                        h.set_merge(!c.is_whitespace());
                        h.record(TextOp::Insert { at: at + i, text: c.to_string() }.apply(&mut doc).unwrap());
                    }
                }
                Action::Delete(a, n) => {
                    let at = a % (doc.len() + 1);
                    let op = TextOp::Delete { at, len: n.min(doc.len() - at) };
                    h.record(op.apply(&mut doc).unwrap());
                }
                Action::Undo => { h.undo(&mut doc); }
                Action::Redo => { h.redo(&mut doc); }
            }
        }
        let end = doc.clone();
        let mut undone = 0;
        while h.undo(&mut doc).is_some() {
            undone += 1;
        }
        prop_assert_eq!(&doc, &original);
        for _ in 0..undone {
            prop_assert!(h.redo(&mut doc).is_some());
        }
        prop_assert_eq!(doc, end);
    }
}
