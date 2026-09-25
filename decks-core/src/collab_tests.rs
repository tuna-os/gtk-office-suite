// SPDX-License-Identifier: GPL-3.0-or-later
//! Two peers in one process (RFC-0001 Phase 3): seeded random op groups on
//! each side, applied concurrently and merged, as Tables' collab tests do.

use super::Replica;
use crate::builds::{Build, BuildEffect};
use crate::engine::{Slide, SlideObject, Transition};
use crate::insert::{shape, shape_library, table, text_box};
use crate::ops::{apply_all, next_id, Op, SlideProps};
use suite_common_core::ops::History;

const SEEDS: &[u64] = &[1, 7, 42, 1009, 31337, 271828, 0xDECC5, 0xC011AB];

/// xorshift64*: enough randomness for op sequences, and seedable.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn rect(x: f64) -> SlideObject {
    SlideObject::Rect { x, y: 10.0, w: 20.0, h: 20.0, rotation: 0.0 }
}

fn slide(title: &str, objects: Vec<SlideObject>) -> Slide {
    Slide {
        title: title.into(),
        background: "#ffffff".into(),
        objects,
        notes: String::new(),
        master_idx: None,
        transition: Transition::None,
        builds: vec![],
        ids: Default::default(),
    }
}

fn base() -> Vec<Slide> {
    vec![
        slide("one", vec![rect(0.0), text_box(), rect(200.0)]),
        slide("two", vec![shape(shape_library()[2].kind.clone()), table(2, 3)]),
        slide("three", vec![]),
    ]
}

/// The deck as the document defines it: what a person sees, without the
/// local ids (each peer numbers its own).
fn content(deck: &[Slide]) -> String {
    let plain: Vec<Slide> = deck.iter().map(|s| Slide { ids: Default::default(), ..s.clone() }).collect();
    format!("{plain:#?}")
}

/// Any kind of object, so every kind goes through the document.
fn any_object(rng: &mut Rng) -> SlideObject {
    let lib = shape_library();
    match rng.below(4) {
        0 => rect(rng.below(900) as f64 + 0.5),
        1 => text_box(),
        2 => shape(lib[rng.below(lib.len())].kind.clone()),
        _ => table(1 + rng.below(3), 1 + rng.below(3)),
    }
}

fn nudged(object: &SlideObject, dx: f64) -> SlideObject {
    let mut o = object.clone();
    match &mut o {
        SlideObject::TextBox { x, text, .. } => {
            *x += dx;
            text.push('!');
        }
        SlideObject::Rect { x, .. } | SlideObject::Shape { x, .. } | SlideObject::Image { x, .. } | SlideObject::Table { x, .. } => *x += dx,
        SlideObject::Circle { x, .. } => *x += dx,
    }
    o
}

/// A random op against `deck` (ids assigned), or none when the pick
/// doesn't apply there.
fn random_op(rng: &mut Rng, deck: &[Slide]) -> Option<Op> {
    let si = rng.below(deck.len());
    let s = &deck[si];
    let n = s.objects.len();
    let pick = (n > 0).then(|| rng.below(n));
    Some(match rng.below(11) {
        0 | 1 => Op::InsertObject { slide: s.ids.slide, at: rng.below(n + 1), id: next_id(deck), object: Box::new(any_object(rng)) },
        2 => Op::DeleteObject { slide: s.ids.slide, id: s.ids.objects[pick?] },
        3 => Op::MoveObject { slide: s.ids.slide, id: s.ids.objects[pick?], to: rng.below(n) },
        4 | 5 => {
            let i = pick?;
            Op::SetObject { slide: s.ids.slide, id: s.ids.objects[i], object: Box::new(nudged(&s.objects[i], rng.below(40) as f64 - 20.0)) }
        }
        6 => Op::InsertSlide { at: rng.below(deck.len() + 1), slide: Box::new(slide(&format!("s{}", rng.below(99)), vec![rect(1.0), text_box()])) },
        7 if deck.len() > 1 => Op::DeleteSlide { slide: s.ids.slide },
        7 => return None,
        8 => Op::MoveSlide { slide: s.ids.slide, to: rng.below(deck.len()) },
        9 => {
            let mut p = SlideProps::of(s);
            p.notes = format!("notes {}", rng.below(999));
            p.title = format!("title {}", rng.below(999));
            p.transition = Transition::ALL[rng.below(Transition::ALL.len())];
            Op::SetSlide { slide: s.ids.slide, props: Box::new(p) }
        }
        _ => {
            let mut p = SlideProps::of(s);
            p.builds.push(Build { object: pick?, effect: BuildEffect::ALL[rng.below(BuildEffect::ALL.len())], out: rng.below(2) == 1 });
            Op::SetSlide { slide: s.ids.slide, props: Box::new(p) }
        }
    })
}

#[test]
fn the_document_holds_exactly_the_deck_after_every_edit() {
    for &seed in SEEDS {
        let mut rng = Rng::new(seed ^ 0xD0C);
        let mut deck = base();
        let mut replica = Replica::new(1, &mut deck).unwrap();
        let mut history: History<Op> = History::default();
        for n in 0..80 {
            undo_or_edit(&mut rng, &mut deck, &mut replica, &mut history);
            let (_, rebuilt) = Replica::from_updates(9, &replica.export_all()).unwrap();
            assert_eq!(content(&rebuilt), content(&deck), "seed {seed}, step {n}");
        }
    }
}

/// A random local step: mostly a new op, sometimes an undo or redo, which
/// are op groups too (the inverses), recorded like any other.
fn undo_or_edit(rng: &mut Rng, deck: &mut Vec<Slide>, replica: &mut Replica, history: &mut History<Op>) {
    match rng.below(8) {
        0 => undo_step(deck, replica, history, true),
        1 => undo_step(deck, replica, history, false),
        _ => {
            let Some(op) = random_op(rng, deck) else { return };
            let group = vec![op];
            let Ok(inverse) = apply_all(deck, &group) else { return };
            if inverse.is_empty() {
                return;
            }
            history.record(inverse);
            replica.record(&group).expect("a group that applied locally applies to the replica");
        }
    }
}

/// Undo (or redo) the last step. The history hands back the ops it
/// applied (the stored inverses), and the replica records those, like any
/// other group.
fn undo_step(deck: &mut Vec<Slide>, replica: &mut Replica, history: &mut History<Op>, undo: bool) {
    let applied = if undo { history.undo(deck) } else { history.redo(deck) };
    if let Some(ops) = applied {
        replica.record(&ops).expect("an undo step applies to the replica");
    }
}

#[test]
fn two_peers_editing_at_once_converge_and_equal_a_replay() {
    for &seed in SEEDS {
        let mut rng = Rng::new(seed ^ 0x2BEE5);
        let mut deck = base();
        let mut a = Replica::new(1, &mut deck).unwrap();
        let (mut b, mut deck_b) = a.fork(2).unwrap();
        let mut deck_a = a.rebuild().unwrap();
        for round in 0..8 {
            // Each side edits (and undoes) without seeing the other's edits.
            let (mut ha, mut hb): (History<Op>, History<Op>) = Default::default();
            for _ in 0..1 + rng.below(8) {
                undo_or_edit(&mut rng, &mut deck_a, &mut a, &mut ha);
            }
            for _ in 0..1 + rng.below(8) {
                undo_or_edit(&mut rng, &mut deck_b, &mut b, &mut hb);
            }
            a.merge_from(&b).unwrap();
            b.merge_from(&a).unwrap();
            deck_a = a.rebuild().unwrap();
            deck_b = b.rebuild().unwrap();
            assert_eq!(content(&deck_a), content(&deck_b), "seed {seed}, round {round}: the peers differ");
            // The merged history, replayed into a peer that saw none of it.
            let (_, replayed) = Replica::from_updates(3, &a.export_all()).unwrap();
            assert_eq!(content(&replayed), content(&deck_a), "seed {seed}, round {round}: a replay differs");
        }
    }
}

/// Two peers from `base()`, and their decks.
fn pair() -> (Replica, Vec<Slide>, Replica, Vec<Slide>) {
    let mut deck = base();
    let mut a = Replica::new(1, &mut deck).unwrap();
    let (b, deck_b) = a.fork(2).unwrap();
    let deck_a = a.rebuild().unwrap();
    (a, deck_a, b, deck_b)
}

fn edit(deck: &mut Vec<Slide>, replica: &mut Replica, ops: Vec<Op>) {
    apply_all(deck, &ops).unwrap();
    replica.record(&ops).unwrap();
}

fn merged(mut a: Replica, mut b: Replica) -> (Vec<Slide>, Vec<Slide>) {
    a.merge_from(&b).unwrap();
    b.merge_from(&a).unwrap();
    (a.rebuild().unwrap(), b.rebuild().unwrap())
}

#[test]
fn a_delete_wins_over_a_concurrent_move_and_edit_of_the_same_object() {
    let (mut a, mut deck_a, mut b, mut deck_b) = pair();
    let (sa, oa) = (deck_a[0].ids.slide, deck_a[0].ids.objects[0]);
    edit(&mut deck_a, &mut a, vec![Op::DeleteObject { slide: sa, id: oa }]);
    let (sb, ob) = (deck_b[0].ids.slide, deck_b[0].ids.objects[0]);
    let moved = nudged(&deck_b[0].objects[0], 300.0);
    edit(&mut deck_b, &mut b, vec![Op::MoveObject { slide: sb, id: ob, to: 2 }, Op::SetObject { slide: sb, id: ob, object: Box::new(moved) }]);
    let (deck_a, deck_b) = merged(a, b);
    for deck in [&deck_a, &deck_b] {
        assert_eq!(deck[0].objects.len(), 2, "the deleted object stays deleted: {:#?}", deck[0].objects);
        assert!(deck[0].objects.iter().all(|o| !matches!(o, SlideObject::Rect { x, .. } if *x == 0.0 || *x == 300.0)));
    }
    assert_eq!(content(&deck_a), content(&deck_b));
}

#[test]
fn a_slide_delete_wins_over_a_concurrent_move_and_edit_of_that_slide() {
    let (mut a, mut deck_a, mut b, mut deck_b) = pair();
    let doomed = deck_a[1].ids.slide;
    edit(&mut deck_a, &mut a, vec![Op::DeleteSlide { slide: doomed }]);
    let mut props = SlideProps::of(&deck_b[1]);
    props.title = "renamed".into();
    let id = deck_b[1].ids.slide;
    edit(&mut deck_b, &mut b, vec![Op::MoveSlide { slide: id, to: 0 }, Op::SetSlide { slide: id, props: Box::new(props) }]);
    let (deck_a, deck_b) = merged(a, b);
    for deck in [&deck_a, &deck_b] {
        let titles: Vec<&str> = deck.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["one", "three"]);
    }
}

#[test]
fn undoing_a_delete_brings_the_same_object_back_for_everyone() {
    let (mut a, mut deck_a, b, _) = pair();
    let (s, o) = (deck_a[0].ids.slide, deck_a[0].ids.objects[1]);
    let mut history: History<Op> = History::default();
    let ops = vec![Op::DeleteObject { slide: s, id: o }];
    history.record(apply_all(&mut deck_a, &ops).unwrap());
    a.record(&ops).unwrap();
    undo_step(&mut deck_a, &mut a, &mut history, true);
    let (deck_a, deck_b) = merged(a, b);
    assert_eq!(content(&deck_a), content(&base()));
    assert_eq!(content(&deck_b), content(&base()));
}

#[test]
fn concurrent_inserts_at_one_place_keep_both_in_one_order() {
    let (mut a, mut deck_a, mut b, mut deck_b) = pair();
    let ins = |deck: &[Slide], x: f64| Op::InsertObject { slide: deck[0].ids.slide, at: 1, id: next_id(deck), object: Box::new(rect(x)) };
    let op = ins(&deck_a, 111.0);
    edit(&mut deck_a, &mut a, vec![op]);
    let op = ins(&deck_b, 222.0);
    edit(&mut deck_b, &mut b, vec![op]);
    let (deck_a, deck_b) = merged(a, b);
    assert_eq!(deck_a[0].objects.len(), 5);
    assert_eq!(content(&deck_a), content(&deck_b));
}

#[test]
fn builds_follow_their_object_through_concurrent_reorders() {
    let (mut a, mut deck_a, mut b, mut deck_b) = pair();
    // A builds the third object in; B moves the first object to the top.
    let mut p = SlideProps::of(&deck_a[0]);
    p.builds.push(Build { object: 2, effect: BuildEffect::Dissolve, out: false });
    let sa = deck_a[0].ids.slide;
    edit(&mut deck_a, &mut a, vec![Op::SetSlide { slide: sa, props: Box::new(p) }]);
    let (sb, ob) = (deck_b[0].ids.slide, deck_b[0].ids.objects[0]);
    edit(&mut deck_b, &mut b, vec![Op::MoveObject { slide: sb, id: ob, to: 2 }]);
    let (deck_a, deck_b) = merged(a, b);
    for deck in [&deck_a, &deck_b] {
        let built = &deck[0].objects[deck[0].builds[0].object];
        assert!(matches!(built, SlideObject::Rect { x, .. } if *x == 200.0), "the build still names the same object: {built:?}");
    }
}

#[test]
fn every_kind_of_object_survives_the_document() {
    let mut deck = base();
    let lib = shape_library();
    deck[2].objects = lib.iter().map(|s| shape(s.kind.clone())).chain([text_box(), table(3, 3), rect(3.25)]).collect();
    let replica = Replica::new(1, &mut deck).unwrap();
    let (_, rebuilt) = Replica::from_updates(2, &replica.export_all()).unwrap();
    assert_eq!(content(&rebuilt), content(&deck));
}
