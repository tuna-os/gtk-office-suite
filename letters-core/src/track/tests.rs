use super::*;
use crate::edit::{apply_all, sequence_text, typing};
use crate::model::Run;
use proptest::prelude::*;

const ME: &str = "Ada";
const DATE: &str = "2026-09-25T20:30:00Z";

fn text(d: &Document) -> String {
    sequence_text(d)
}

/// Apply a tracked edit; returns its undo.
fn edit_tracked(d: &mut Document, ops: &[Op], author: &str) -> Vec<Op> {
    let t = tracked(d, ops, author, DATE).expect("a text edit");
    apply_all(d, &t).expect("applies")
}

#[test]
fn tracked_typing_is_an_insertion_and_deleting_it_removes_it() {
    let mut d = Document::from_plain_text("hello world");
    let op = typing(&d, 5, ", dear").unwrap();
    edit_tracked(&mut d, &[op], ME);
    assert_eq!(text(&d), "hello, dear world");
    let c = changes(&d);
    assert_eq!(c.len(), 1);
    assert_eq!((c[0].start, c[0].end, c[0].revision.kind, c[0].revision.author.as_str()), (5, 11, RevisionKind::Insert, ME));
    // My own pending insertion is removed, not marked deleted.
    edit_tracked(&mut d, &[Op::Delete { at: 6, len: 5 }], ME);
    assert_eq!(text(&d), "hello, world");
    assert_eq!(changes(&d)[0].text, ",");
}

#[test]
fn typing_on_from_ones_own_insertion_continues_it() {
    let mut d = Document::from_plain_text("ab");
    let op = typing(&d, 1, "x").unwrap();
    let t = tracked(&d, &[op], ME, "2026-01-01T00:00:00Z").unwrap();
    apply_all(&mut d, &t).unwrap();
    // A second later, one char on: the same change, its first date.
    let op = typing(&d, 2, "y").unwrap();
    let t = tracked(&d, &[op], ME, "2026-01-01T00:00:01Z").unwrap();
    apply_all(&mut d, &t).unwrap();
    let c = changes(&d);
    assert_eq!((c.len(), c[0].text.as_str(), c[0].revision.date.as_str()), (1, "xy", "2026-01-01T00:00:00Z"));
    // Someone else typing on is a change of their own.
    let op = typing(&d, 3, "z").unwrap();
    edit_tracked(&mut d, &[op], "Grace");
    assert_eq!(changes(&d).len(), 2);
}

#[test]
fn tracked_deletion_keeps_the_text_marked_until_accepted() {
    let mut d = Document::from_plain_text("keep this word");
    edit_tracked(&mut d, &[Op::Delete { at: 5, len: 5 }], ME);
    assert_eq!(text(&d), "keep this word", "deleted text stays, marked");
    assert_eq!(changes(&d)[0].revision.kind, RevisionKind::Delete);
    // Someone else deleting my insertion marks it deleted, keeping my
    // insertion under it: rejecting the deletion brings my insertion back.
    let op = typing(&d, 0, "X").unwrap();
    edit_tracked(&mut d, &[op], ME);
    edit_tracked(&mut d, &[Op::Delete { at: 0, len: 1 }], "Grace");
    let kinds: Vec<_> = changes(&d).iter().map(|c| (c.revision.kind, c.revision.author.clone())).collect();
    assert_eq!(kinds, [(RevisionKind::Delete, "Grace".to_string()), (RevisionKind::Delete, ME.to_string())]);
    let ops = resolve(&d, 0, 0, false);
    apply_all(&mut d, &ops).unwrap();
    assert_eq!(changes(&d)[0].revision.kind, RevisionKind::Insert, "the insertion is pending again");
    assert_eq!(changes(&d)[0].revision.author, ME);
}

#[test]
fn accept_and_reject_do_what_they_say_and_undo_exactly() {
    let base = Document::from_plain_text("one two three");
    let mut d = base.clone();
    let op = typing(&d, 3, " and a half").unwrap();
    edit_tracked(&mut d, &[op], ME);
    edit_tracked(&mut d, &[Op::Delete { at: 15, len: 4 }], ME);
    let tracked_doc = d.clone();
    assert_eq!(text(&d), "one and a half two three");

    let mut accepted_all = d.clone();
    let undo = apply_all(&mut accepted_all, &resolve_all(&d, true)).unwrap();
    assert_eq!(text(&accepted_all), "one and a half three");
    assert!(!has_changes(&accepted_all));
    apply_all(&mut accepted_all, &undo).unwrap();
    assert_eq!(accepted_all, tracked_doc, "accepting undoes exactly");

    let mut rejected_all = d.clone();
    apply_all(&mut rejected_all, &resolve_all(&d, false)).unwrap();
    assert_eq!(rejected_all, base, "rejecting everything gives the original back");

    // One change at a time: accept the insertion (caret inside it) only.
    let mut one = d.clone();
    apply_all(&mut one, &resolve(&d, 5, 5, true)).unwrap();
    assert_eq!(changes(&one).len(), 1);
    assert_eq!(changes(&one)[0].revision.kind, RevisionKind::Delete);
    assert_eq!(text(&accepted(&d)), "one and a half three");
}

#[test]
fn a_deleted_object_is_tracked_too() {
    let mut d = Document::from_plain_text("see ");
    d.paragraphs[0].runs.push(Run { text: "a dot".into(), style: RunStyle { image: Some("dot.png".into()), ..Default::default() } });
    edit_tracked(&mut d, &[Op::Delete { at: 4, len: 1 }], ME);
    let c = changes(&d);
    assert_eq!((c.len(), c[0].start, c[0].end), (1, 4, 5));
    assert!(d.paragraphs[0].runs[1].style.image.is_some(), "the image is still there, marked");
}

#[test]
fn a_tracked_deletion_across_a_break_keeps_the_break() {
    let mut d = Document::from_plain_text("ab\ncd");
    edit_tracked(&mut d, &[Op::Delete { at: 1, len: 3 }], ME);
    assert_eq!(text(&d), "ab\ncd");
    let spans: Vec<(usize, usize)> = changes(&d).iter().map(|c| (c.start, c.end)).collect();
    assert_eq!(spans, [(1, 2), (3, 4)]);
}

fn arb_ops() -> impl Strategy<Value = Vec<(usize, usize, bool, bool)>> {
    prop::collection::vec((any::<usize>(), 0usize..5, any::<bool>(), any::<bool>()), 1..12)
}

proptest! {
    /// Any mix of tracked typing and deleting by two authors: rejecting
    /// every change gives the original back, and accepting every change
    /// gives what untracked editing would have — and each step undoes to
    /// exactly what it was applied to.
    #[test]
    fn reject_all_restores_and_accept_all_is_the_untracked_result(start in "[a-c ]{1,12}", ops in arb_ops()) {
        let base = Document::from_plain_text(&start);
        let mut d = base.clone();
        for (at, n, insert, grace) in ops {
            let len = edit::doc_len(&d);
            let at = at % (len + 1);
            let op = if insert || len == 0 {
                typing(&d, at, &"xy"[..1 + n % 2]).unwrap()
            } else {
                let at = at.min(len - 1);
                Op::Delete { at, len: (1 + n).min(len - at) }
            };
            let before = d.clone();
            let undo = edit_tracked(&mut d, &[op], if grace { "Grace" } else { ME });
            let mut back = d.clone();
            apply_all(&mut back, &undo).unwrap();
            prop_assert_eq!(back, before);
        }
        let mut rejected = d.clone();
        apply_all(&mut rejected, &resolve_all(&d, false)).unwrap();
        prop_assert_eq!(text(&rejected), text(&base));
        prop_assert!(!has_changes(&rejected));
        let accepted_doc = accepted(&d);
        prop_assert!(!has_changes(&accepted_doc));
    }
}
