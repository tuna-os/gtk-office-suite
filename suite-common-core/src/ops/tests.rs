// A plain-text document and its two ops: the smallest thing the shared
// history can be tested on without any app's model.

use super::*;
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use std::collections::{BTreeMap, BTreeSet};

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
fn a_step_that_no_longer_fits_is_dropped_not_half_applied() {
    let mut doc = Vec::new();
    let mut h = History::default();
    type_chars(&mut doc, &mut h, "abc");
    // The document is replaced under the history.
    doc = vec!['z'];
    assert!(h.undo(&mut doc).is_none());
    assert_eq!(text(&doc), "z");
    assert!(!h.can_undo() && !h.can_redo(), "the one step there was is gone");
}

#[test]
fn an_undo_that_no_longer_fits_drops_that_step_and_keeps_the_ones_before_it() {
    let mut doc = Vec::new();
    let mut h = History::default();
    type_chars(&mut doc, &mut h, "ab cd");
    // A peer deletes "cd" under the history, which was not recorded in it.
    doc.truncate(3);
    assert!(h.undo(&mut doc).is_none(), "the 'cd' step addresses text that is gone");
    assert_eq!(text(&doc), "ab ", "and it was not half applied");
    assert!(!h.can_redo(), "a dropped step cannot be redone either");
    assert!(h.can_undo(), "the steps before it are still there");
    h.undo(&mut doc).unwrap();
    assert_eq!(text(&doc), "ab", "the space was a step of its own");
    h.undo(&mut doc).unwrap();
    assert_eq!(text(&doc), "", "so was 'ab'");
    assert!(!h.can_undo());
    h.redo(&mut doc).unwrap();
    h.redo(&mut doc).unwrap();
    assert_eq!(text(&doc), "ab ", "and what was kept redoes; 'cd' never comes back");
    assert!(!h.can_redo());
}

#[test]
fn a_redo_that_no_longer_fits_drops_that_step_and_keeps_the_ones_after_it() {
    let mut doc = Vec::new();
    let mut h = History::default();
    type_chars(&mut doc, &mut h, "ab cd");
    while h.undo(&mut doc).is_some() {}
    assert_eq!(text(&doc), "");
    h.redo(&mut doc).unwrap();
    assert_eq!(text(&doc), "ab");
    // A peer clears the document: the space (at 2) and 'cd' (at 3) address
    // text that is not there.
    doc.clear();
    assert!(h.redo(&mut doc).is_none());
    assert!(doc.is_empty(), "the space step was dropped, not half applied");
    assert!(h.can_redo(), "the step after it stays");
    assert!(h.can_undo(), "and the undo stack is not the redo's to touch");
    // A peer types three characters, so 'cd' fits again.
    doc = "xyz".chars().collect();
    h.redo(&mut doc).unwrap();
    assert_eq!(text(&doc), "xyzcd");
}

#[test]
fn rebase_visits_each_chain_top_first_and_drops_one_step_alone() {
    let mut doc = Vec::new();
    let mut h = History::default();
    type_chars(&mut doc, &mut h, "ab cd");
    h.undo(&mut doc).unwrap();
    assert_eq!(text(&doc), "ab ");
    let starts = std::cell::Cell::new(0);
    let mut visited = Vec::new();
    let mut indexes = Vec::new();
    h.rebase(
        || {
            starts.set(starts.get() + 1);
            0usize
        },
        |index, ops| {
            visited.push(ops.to_vec());
            indexes.push(*index);
            *index += 1;
            match ops {
                // The space step: dropped.
                [TextOp::Delete { at: 2, len: 1 }] => None,
                // A peer typed one character at the start: shift by one.
                [TextOp::Delete { at, len }] => Some(vec![TextOp::Delete { at: at + 1, len: *len }]),
                [TextOp::Insert { at, text }] => Some(vec![TextOp::Insert { at: at + 1, text: text.clone() }]),
                other => panic!("unexpected step {other:?}"),
            }
        },
    );
    assert_eq!(starts.get(), 2, "one state for the undo chain, one for the redo chain");
    assert_eq!(
        visited,
        [
            vec![TextOp::Delete { at: 2, len: 1 }],
            vec![TextOp::Delete { at: 0, len: 2 }],
            vec![TextOp::Insert { at: 3, text: "cd".into() }],
        ],
        "the top step of each chain first"
    );
    assert_eq!(indexes, [0, 1, 0]);

    doc.insert(0, 'X');
    h.undo(&mut doc).unwrap();
    assert_eq!(text(&doc), "X ", "the 'ab' step, rebased, takes back 'ab' and not the peer's X");
    assert!(!h.can_undo(), "the space step was dropped alone; 'ab' went with its own undo");
    h.redo(&mut doc).unwrap();
    assert_eq!(text(&doc), "Xab ");
    h.redo(&mut doc).unwrap();
    assert_eq!(text(&doc), "Xab cd", "and the rebased redo step is still there");
}

#[test]
fn a_group_that_fails_part_way_is_dropped_whole() {
    let mut doc: Vec<char> = "hello".chars().collect();
    let mut h = History::default();
    h.begin();
    h.record(TextOp::Insert { at: 5, text: " world".into() }.apply(&mut doc).unwrap());
    h.record(TextOp::Delete { at: 0, len: 1 }.apply(&mut doc).unwrap());
    h.end();
    h.record(TextOp::Insert { at: 0, text: ">".into() }.apply(&mut doc).unwrap());
    assert_eq!(text(&doc), ">ello world");
    // A peer cuts the text to four characters (">ell"): the undo of the
    // group puts the "h" back (fits) and then removes " world" (does not).
    doc.truncate(4);
    h.undo(&mut doc).unwrap();
    assert_eq!(text(&doc), "ell", "the '>' step fitted and was undone");
    assert!(h.undo(&mut doc).is_none(), "the group does not fit");
    assert_eq!(text(&doc), "ell", "its first half (the 'h') was rolled back, not kept");
    assert!(!h.can_undo());
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

// ---- a peer's edits under a history (ADR 0011, RFC-0005 2.4) ---------------
//
// A peer's edits do not enter the local history, so a stored inverse can
// meet a document that has moved on. A book of numbered sheets of numbered
// cells is the smallest document where that happens the way it does in
// Tables: a peer takes a sheet away, and a step that addresses it no longer
// fits.

type Book = BTreeMap<u8, BTreeMap<u8, i32>>;

#[derive(Clone, Debug, PartialEq)]
enum BookOp {
    /// Set a cell, or clear it with `None`; fails when the sheet is gone.
    Cell { sheet: u8, key: u8, value: Option<i32> },
    /// Put a sheet back wholesale, or take it away with `None`.
    Sheet { sheet: u8, cells: Option<BTreeMap<u8, i32>> },
}

#[derive(Debug, PartialEq)]
struct NoSheet(u8);

impl Op for BookOp {
    type Doc = Book;
    type Error = NoSheet;

    fn apply(&self, doc: &mut Book) -> Result<Vec<Self>, NoSheet> {
        match self {
            BookOp::Cell { sheet, key, value } => {
                let cells = doc.get_mut(sheet).ok_or(NoSheet(*sheet))?;
                let was = match value {
                    Some(v) => cells.insert(*key, *v),
                    None => cells.remove(key),
                };
                Ok(vec![BookOp::Cell { sheet: *sheet, key: *key, value: was }])
            }
            BookOp::Sheet { sheet, cells } => {
                let was = match cells {
                    Some(c) => doc.insert(*sheet, c.clone()),
                    None => doc.remove(sheet),
                };
                Ok(vec![BookOp::Sheet { sheet: *sheet, cells: was }])
            }
        }
    }
}

#[derive(Clone, Debug)]
enum Act {
    /// One user step on one sheet: one to three cell changes, grouped.
    Local { sheet: usize, cells: Vec<(u8, Option<i32>)> },
    /// A peer's edit: applied to the document, never recorded.
    Peer(PeerEdit),
    Undo,
    Redo,
}

#[derive(Clone, Debug)]
enum PeerEdit {
    DropSheet(usize),
    AddSheet(u8),
    SetCell { sheet: usize, key: u8, value: Option<i32> },
}

fn arb_cell() -> impl Strategy<Value = (u8, Option<i32>)> {
    (0u8..4, prop::option::of(-5i32..5))
}

fn arb_act() -> impl Strategy<Value = Act> {
    prop_oneof![
        4 => (any::<usize>(), prop::collection::vec(arb_cell(), 1..4)).prop_map(|(sheet, cells)| Act::Local { sheet, cells }),
        2 => prop_oneof![
            any::<usize>().prop_map(PeerEdit::DropSheet),
            (0u8..6).prop_map(PeerEdit::AddSheet),
            (any::<usize>(), arb_cell()).prop_map(|(sheet, (key, value))| PeerEdit::SetCell { sheet, key, value }),
        ]
        .prop_map(Act::Peer),
        3 => Just(Act::Undo),
        2 => Just(Act::Redo),
    ]
}

fn start_book() -> Book {
    (0u8..3).map(|s| (s, (0u8..3).map(|k| (k, i32::from(s) * 10 + i32::from(k))).collect())).collect()
}

/// The `i`th sheet the book has (wrapping), or `None` for an empty book.
fn nth_sheet(doc: &Book, i: usize) -> Option<u8> {
    doc.keys().copied().nth(i % doc.len().max(1))
}

/// Undo (or redo) once, and check it against what applying that step to a
/// copy of the document says must happen: a step that fits is applied whole
/// and moves to the other stack; a step that does not is dropped whole, the
/// document is as it was and the other stack is as it was; either way
/// exactly that one step left the stack it came from.
fn step_checked(h: &mut History<BookOp>, doc: &mut Book, redo: bool) -> Result<(), TestCaseError> {
    let (undo_was, redo_was) = (h.undo.clone(), h.redo.clone());
    let (from_was, to_was) = if redo { (&redo_was, &undo_was) } else { (&undo_was, &redo_was) };
    let before = doc.clone();
    let mut expected_doc = doc.clone();
    let expected = from_was.last().map(|step| apply_all(&mut expected_doc, step));
    let done = if redo { h.redo(doc) } else { h.undo(doc) };
    let (from_now, to_now) = if redo { (&h.redo, &h.undo) } else { (&h.undo, &h.redo) };
    match expected {
        None => {
            prop_assert!(done.is_none(), "nothing to step");
            prop_assert_eq!(&*doc, &before);
        }
        Some(Ok(inverse)) => {
            prop_assert_eq!(done.as_ref(), from_was.last(), "a step that fits reports the ops it applied");
            prop_assert_eq!(&*doc, &expected_doc, "and applied all of them");
            prop_assert_eq!(to_now.len(), to_was.len() + 1);
            prop_assert_eq!(&to_now[..to_was.len()], &to_was[..]);
            prop_assert_eq!(to_now.last(), Some(&inverse), "its inverse is on the other stack");
        }
        Some(Err(_)) => {
            prop_assert!(done.is_none(), "a step that does not fit reports nothing done");
            prop_assert_eq!(&*doc, &before, "and leaves the document as it was");
            prop_assert_eq!(to_now, to_was, "and puts nothing on the other stack");
        }
    }
    prop_assert_eq!(&from_now[..], &from_was[..from_was.len().saturating_sub(1)], "exactly one step left the stack");
    Ok(())
}

proptest! {
    /// A peer's edits interleaved with local steps, undo and redo: whatever
    /// fits is applied whole, whatever does not is dropped alone and whole
    /// (`step_checked`), and a sheet no peer touched is exactly as it
    /// started once every remaining step is undone: no step that fitted
    /// was lost with one that did not.
    #[test]
    fn remote_ops_drop_only_the_steps_they_break(acts in prop::collection::vec(arb_act(), 1..48)) {
        let original = start_book();
        let mut doc = original.clone();
        let mut h: History<BookOp> = History::default();
        let mut touched: BTreeSet<u8> = BTreeSet::new();
        for act in acts {
            match act {
                Act::Local { sheet, cells } => {
                    let Some(sheet) = nth_sheet(&doc, sheet) else { continue };
                    h.begin();
                    for (key, value) in cells {
                        h.record(BookOp::Cell { sheet, key, value }.apply(&mut doc).unwrap());
                    }
                    h.end();
                }
                Act::Peer(edit) => {
                    let (sheet, op) = match edit {
                        PeerEdit::DropSheet(i) => match nth_sheet(&doc, i) {
                            Some(sheet) => (sheet, BookOp::Sheet { sheet, cells: None }),
                            None => continue,
                        },
                        PeerEdit::AddSheet(sheet) if !doc.contains_key(&sheet) => {
                            (sheet, BookOp::Sheet { sheet, cells: Some(BTreeMap::new()) })
                        }
                        PeerEdit::AddSheet(_) => continue,
                        PeerEdit::SetCell { sheet, key, value } => match nth_sheet(&doc, sheet) {
                            Some(sheet) => (sheet, BookOp::Cell { sheet, key, value }),
                            None => continue,
                        },
                    };
                    touched.insert(sheet);
                    op.apply(&mut doc).unwrap();
                }
                Act::Undo => step_checked(&mut h, &mut doc, false)?,
                Act::Redo => step_checked(&mut h, &mut doc, true)?,
            }
        }
        while h.can_undo() {
            step_checked(&mut h, &mut doc, false)?;
        }
        for (sheet, cells) in &original {
            if !touched.contains(sheet) {
                prop_assert_eq!(doc.get(sheet), Some(cells), "sheet {} no peer touched", sheet);
            }
        }
    }
}
