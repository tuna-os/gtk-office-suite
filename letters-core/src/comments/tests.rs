use super::*;
use crate::edit::{apply_all, typing};
use proptest::prelude::*;

const DATE: &str = "2026-09-26T10:00:00Z";

fn apply(d: &mut Document, ops: &[Op]) -> Vec<Op> {
    apply_all(d, ops).expect("applies")
}

fn anchor_of(d: &Document, id: u32) -> Option<(usize, usize, String)> {
    thread(d, id)?.anchor.map(|a| (a.start, a.end, a.text))
}

#[test]
fn a_comment_is_its_body_and_a_mark_and_undoes_exactly() {
    let mut d = Document::from_plain_text("hello brave world");
    let before = d.clone();
    let (ops, id) = add(&d, 6, 11, "Ada", DATE, "Why brave?").unwrap();
    let undo = apply(&mut d, &ops);
    assert_eq!(id, 1);
    assert_eq!(anchor_of(&d, id), Some((6, 11, "brave".into())));
    assert_eq!(d.comments[0].text, "Why brave?");
    apply(&mut d, &undo);
    assert_eq!(d, before);
    assert!(add(&d, 3, 3, "Ada", DATE, "x").is_none(), "an empty range has nothing to comment on");
}

#[test]
fn comments_overlap_independently() {
    let d = sample_document();
    assert_eq!(anchor_of(&d, 1), Some((4, 21, "quarterly figures".into())));
    assert_eq!(anchor_of(&d, 2), Some((14, 33, "figures look strong".into())));
    // Deleting one leaves the other's marks as they were.
    let mut e = d.clone();
    apply(&mut e, &delete(&d, 1));
    assert_eq!(anchor_of(&e, 2), anchor_of(&d, 2));
    assert!(e.comments.iter().all(|c| c.id != 1 && c.parent != Some(1)), "the thread's replies go with it");
    assert!(e.paragraphs.iter().flat_map(|p| &p.runs).all(|r| !r.style.comments.contains(&1)));
}

#[test]
fn threads_are_in_document_order_with_their_replies() {
    let d = sample_document();
    let t = threads(&d);
    assert_eq!(t.iter().map(|t| t.comment.id).collect::<Vec<_>>(), [1, 2, 4]);
    assert_eq!(t[0].replies.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["Q3, see the appendix."]);
    assert!(t[2].comment.resolved);
    // Replying to a reply joins the thread.
    let (ops, id) = reply(&d, 3, "Ada", DATE, "Thanks").unwrap();
    let mut e = d.clone();
    apply(&mut e, &ops);
    assert_eq!(e.comments.iter().find(|c| c.id == id).unwrap().parent, Some(1));
    assert_eq!(at(&d, 15), [1, 2]);
    assert_eq!(at(&d, 34), Vec::<u32>::new());
}

#[test]
fn typing_inside_a_comment_is_in_it_and_at_its_edges_is_not() {
    let mut d = Document::from_plain_text("hello brave world");
    let (ops, id) = add(&d, 6, 11, "Ada", DATE, "?").unwrap();
    apply(&mut d, &ops);
    let op = typing(&d, 8, "a").unwrap();
    apply(&mut d, &[op]);
    assert_eq!(anchor_of(&d, id), Some((6, 12, "braave".into())));
    for at in [12, 6] {
        let op = typing(&d, at, "!").unwrap();
        apply(&mut d, &[op]);
    }
    assert_eq!(anchor_of(&d, id), Some((7, 13, "braave".into())));
}

#[test]
fn a_comment_spans_paragraphs_and_survives_as_an_orphan() {
    let mut d = Document::from_plain_text("one\ntwo\nthree");
    let (ops, id) = add(&d, 2, 6, "Ada", DATE, "?").unwrap();
    apply(&mut d, &ops);
    assert_eq!(anchor_of(&d, id), Some((2, 6, "e\ntw".into())));
    apply(&mut d, &[Op::Delete { at: 1, len: 6 }]);
    let t = thread(&d, id).unwrap();
    assert_eq!(t.anchor, None, "its text is gone");
    assert_eq!(t.comment.text, "?", "the comment is not");
    assert_eq!(threads(&d).len(), 1);
}

#[test]
fn resolve_and_edit_are_single_ops() {
    let d = sample_document();
    let mut e = d.clone();
    let undo = apply(&mut e, &set_resolved(&d, 3, true));
    assert!(thread(&e, 1).unwrap().comment.resolved, "resolving a reply resolves its thread");
    assert!(set_resolved(&e, 1, true).is_empty());
    apply(&mut e, &undo);
    assert_eq!(e, d);
    let mut e = d.clone();
    apply(&mut e, &edit_text(&d, 2, "Cite it."));
    assert_eq!(thread(&e, 2).unwrap().comment.text, "Cite it.");
}

#[test]
fn a_commented_image_is_marked_like_text() {
    let mut d = Document::from_plain_text("ab");
    d.paragraphs[0].runs.insert(1, crate::model::Run { text: "alt".into(), style: RunStyle { image: Some("x.png".into()), ..Default::default() } });
    let (ops, id) = add(&d, 0, 3, "Ada", DATE, "?").unwrap();
    apply(&mut d, &ops);
    assert!(d.paragraphs[0].runs.iter().all(|r| r.style.comments == [id]), "{:?}", d.paragraphs[0].runs);
}

#[derive(Clone, Debug)]
enum Step {
    Add(usize, usize),
    Reply(u32),
    Resolve(u32, bool),
    Delete(u32),
    Type(usize),
    Cut(usize, usize),
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        (0usize..80, 0usize..80).prop_map(|(a, b)| Step::Add(a, b)),
        (1u32..6).prop_map(Step::Reply),
        (1u32..6, any::<bool>()).prop_map(|(i, r)| Step::Resolve(i, r)),
        (1u32..6).prop_map(Step::Delete),
        (0usize..80).prop_map(Step::Type),
        (0usize..80, 1usize..8).prop_map(|(a, n)| Step::Cut(a, n)),
    ]
}

proptest! {
    /// Any sequence of comment ops and edits: every step's undo restores the
    /// document exactly, and replies always belong to a live thread.
    #[test]
    fn every_step_undoes_exactly(steps in proptest::collection::vec(step(), 1..14)) {
        let mut d = sample_document();
        for s in steps {
            let len = edit::doc_len(&d);
            let ops: Vec<Op> = match s {
                Step::Add(a, b) => add(&d, a.min(len), b.min(len), "Ada", DATE, "c").map(|(o, _)| o).unwrap_or_default(),
                Step::Reply(i) => reply(&d, i, "Ada", DATE, "r").map(|(o, _)| o).unwrap_or_default(),
                Step::Resolve(i, r) => set_resolved(&d, i, r),
                Step::Delete(i) => delete(&d, i),
                Step::Type(a) => typing(&d, a.min(len), "x").into_iter().collect(),
                Step::Cut(a, n) => {
                    let a = a.min(len);
                    vec![Op::Delete { at: a, len: n.min(len - a) }]
                }
            };
            let before = d.clone();
            let Ok(undo) = apply_all(&mut d, &ops) else { continue };
            let mut back = d.clone();
            apply_all(&mut back, &undo).unwrap();
            prop_assert_eq!(&back, &before);
            for c in &d.comments {
                if let Some(p) = c.parent {
                    prop_assert!(d.comments.iter().any(|t| t.id == p && t.parent.is_none()));
                }
            }
            let ids: Vec<u32> = d.comments.iter().map(|c| c.id).collect();
            let mut sorted = ids.clone();
            sorted.sort_unstable();
            prop_assert_eq!(ids, sorted);
        }
    }
}
