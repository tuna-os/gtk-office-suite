// ops_property.rs — RFC-0001 Phase 0 for Decks: random edit sequences
// applied as ops equal the same edits made directly on the model, undoing
// every step through the History restores the deck, and redoing every
// step brings the edits back.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Mirrors tables-core's ops tests and letters-core's edit property tests.
// Edits are chosen by position (what a user does: "delete the second
// object"), turned into id-addressed ops against the ops deck, and made
// directly on a plain copy by the obvious Vec operations.

use decks_core::builds::{Build, BuildEffect};
use decks_core::engine::{Slide, SlideObject, Transition};
use decks_core::ops::{apply_all, ensure_ids, next_id, with_slides, Op, SlideProps};
use suite_common_core::ops::History;
use proptest::prelude::*;

#[derive(Clone, Debug)]
enum Edit {
    AddObject { slide: usize, at: usize, x: f64 },
    DeleteObject { slide: usize, index: usize },
    MoveObject { slide: usize, index: usize, to: usize },
    NudgeObject { slide: usize, index: usize, dx: f64 },
    AddSlide { at: usize, title: String },
    DeleteSlide { slide: usize },
    MoveSlide { slide: usize, to: usize },
    SetNotes { slide: usize, notes: String },
    AddBuild { slide: usize, index: usize },
}

fn edit() -> impl Strategy<Value = Edit> {
    let n = 0usize..6;
    prop_oneof![
        (n.clone(), n.clone(), 0.0..900.0).prop_map(|(slide, at, x)| Edit::AddObject { slide, at, x }),
        (n.clone(), n.clone()).prop_map(|(slide, index)| Edit::DeleteObject { slide, index }),
        (n.clone(), n.clone(), n.clone()).prop_map(|(slide, index, to)| Edit::MoveObject { slide, index, to }),
        (n.clone(), n.clone(), -50.0..50.0).prop_map(|(slide, index, dx)| Edit::NudgeObject { slide, index, dx }),
        (n.clone(), "[a-z]{1,6}").prop_map(|(at, title)| Edit::AddSlide { at, title }),
        n.clone().prop_map(|slide| Edit::DeleteSlide { slide }),
        (n.clone(), n.clone()).prop_map(|(slide, to)| Edit::MoveSlide { slide, to }),
        (n.clone(), "[a-z ]{0,10}").prop_map(|(slide, notes)| Edit::SetNotes { slide, notes }),
        (n.clone(), n).prop_map(|(slide, index)| Edit::AddBuild { slide, index }),
    ]
}

fn rect(x: f64) -> SlideObject {
    SlideObject::Rect { x, y: 10.0, w: 20.0, h: 20.0, rotation: 0.0 }
}

fn slide(title: &str, xs: &[f64]) -> Slide {
    Slide {
        title: title.into(),
        background: "#ffffff".into(),
        objects: xs.iter().map(|x| rect(*x)).collect(),
        notes: String::new(),
        master_idx: None,
        transition: Transition::None,
        builds: vec![],
        ids: Default::default(),
        layout: None,
    }
}

/// The ops for `e` against `slides` (ids assigned), or none when the edit
/// doesn't apply there.
fn ops_for(slides: &[Slide], e: &Edit) -> Vec<Op> {
    let sid = |i: usize| slides.get(i).map(|s| s.ids.slide);
    let oid = |s: usize, i: usize| slides.get(s).and_then(|s| s.ids.objects.get(i).copied());
    match e {
        Edit::AddObject { slide, at, x } => match slides.get(*slide) {
            Some(s) if *at <= s.objects.len() => {
                vec![Op::InsertObject { slide: s.ids.slide, at: *at, id: next_id(slides), object: Box::new(rect(*x)) }]
            }
            _ => vec![],
        },
        Edit::DeleteObject { slide, index } => match oid(*slide, *index) {
            Some(id) => vec![Op::DeleteObject { slide: sid(*slide).unwrap(), id }],
            None => vec![],
        },
        Edit::MoveObject { slide, index, to } => match (oid(*slide, *index), slides.get(*slide)) {
            (Some(id), Some(s)) if *to < s.objects.len() => vec![Op::MoveObject { slide: s.ids.slide, id, to: *to }],
            _ => vec![],
        },
        Edit::NudgeObject { slide, index, dx } => match (oid(*slide, *index), slides.get(*slide)) {
            (Some(id), Some(s)) => {
                let mut o = s.objects[*index].clone();
                if let SlideObject::Rect { x, .. } = &mut o {
                    *x += dx;
                }
                vec![Op::SetObject { slide: s.ids.slide, id, object: Box::new(o) }]
            }
            _ => vec![],
        },
        Edit::AddSlide { at, title } if *at <= slides.len() => vec![Op::InsertSlide { at: *at, slide: Box::new(slide(title, &[])) }],
        Edit::AddSlide { .. } => vec![],
        Edit::DeleteSlide { slide } if slides.len() > 1 => sid(*slide).map(|s| vec![Op::DeleteSlide { slide: s }]).unwrap_or_default(),
        Edit::DeleteSlide { .. } => vec![],
        Edit::MoveSlide { slide, to } if *to < slides.len() => sid(*slide).map(|s| vec![Op::MoveSlide { slide: s, to: *to }]).unwrap_or_default(),
        Edit::MoveSlide { .. } => vec![],
        Edit::SetNotes { slide, notes } => match slides.get(*slide) {
            Some(s) => {
                let mut p = SlideProps::of(s);
                p.notes = notes.clone();
                vec![Op::SetSlide { slide: s.ids.slide, props: Box::new(p) }]
            }
            None => vec![],
        },
        Edit::AddBuild { slide, index } => match slides.get(*slide) {
            Some(s) if *index < s.objects.len() => {
                let mut p = SlideProps::of(s);
                p.builds.push(Build { object: *index, effect: BuildEffect::Dissolve, out: false });
                vec![Op::SetSlide { slide: s.ids.slide, props: Box::new(p) }]
            }
            _ => vec![],
        },
    }
}

/// `e` made directly, the way the model would be changed without ops.
fn direct(slides: &mut Vec<Slide>, e: &Edit) {
    match e {
        Edit::AddObject { slide, at, x } => {
            if let Some(s) = slides.get_mut(*slide) {
                if *at <= s.objects.len() {
                    s.objects.insert(*at, rect(*x));
                    for b in &mut s.builds {
                        if b.object >= *at {
                            b.object += 1;
                        }
                    }
                }
            }
        }
        Edit::DeleteObject { slide, index } => {
            if let Some(s) = slides.get_mut(*slide) {
                if *index < s.objects.len() {
                    s.objects.remove(*index);
                    s.builds = decks_core::builds::after_delete(&s.builds, *index);
                }
            }
        }
        Edit::MoveObject { slide, index, to } => {
            if let Some(s) = slides.get_mut(*slide) {
                if *index < s.objects.len() && *to < s.objects.len() {
                    let o = s.objects.remove(*index);
                    s.objects.insert(*to, o);
                    let mut order: Vec<usize> = (0..s.objects.len()).collect();
                    let i = order.remove(*index);
                    order.insert(*to, i);
                    s.builds = decks_core::builds::after_reorder(&s.builds, &order);
                }
            }
        }
        Edit::NudgeObject { slide, index, dx } => {
            if let Some(SlideObject::Rect { x, .. }) = slides.get_mut(*slide).and_then(|s| s.objects.get_mut(*index)) {
                *x += dx;
            }
        }
        Edit::AddSlide { at, title } => {
            if *at <= slides.len() {
                slides.insert(*at, slide(title, &[]));
            }
        }
        Edit::DeleteSlide { slide } => {
            if slides.len() > 1 && *slide < slides.len() {
                slides.remove(*slide);
            }
        }
        Edit::MoveSlide { slide, to } => {
            if *slide < slides.len() && *to < slides.len() {
                let s = slides.remove(*slide);
                slides.insert(*to, s);
            }
        }
        Edit::SetNotes { slide, notes } => {
            if let Some(s) = slides.get_mut(*slide) {
                s.notes = notes.clone();
            }
        }
        Edit::AddBuild { slide, index } => {
            if let Some(s) = slides.get_mut(*slide) {
                if *index < s.objects.len() {
                    s.builds.push(Build { object: *index, effect: BuildEffect::Dissolve, out: false });
                }
            }
        }
    }
}

/// A deck without its ids: what a user sees.
fn content(slides: &[Slide]) -> String {
    let plain: Vec<Slide> = slides.iter().map(|s| Slide { ids: Default::default(), ..s.clone() }).collect();
    format!("{plain:?}")
}

/// A deck without its tombstones: undoing an insert tombstones the id it
/// gave, so undo-all restores everything but that list.
fn live(slides: &[Slide]) -> String {
    let plain: Vec<Slide> = slides
        .iter()
        .map(|s| {
            let mut s = s.clone();
            s.ids.deleted.clear();
            s
        })
        .collect();
    format!("{plain:?}")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn ops_equal_direct_edits_and_undo_all_restores_the_deck(edits in prop::collection::vec(edit(), 1..40)) {
        let start = vec![slide("a", &[0.0, 100.0, 200.0]), slide("b", &[50.0]), slide("c", &[])];
        let mut via_ops = start.clone();
        ensure_ids(&mut via_ops);
        let initial = live(&via_ops);
        let mut plain = start;
        let mut history: History<Op> = History::default();
        for e in &edits {
            let ops = ops_for(&via_ops, e);
            let inverse = apply_all(&mut via_ops, &ops).expect("a valid edit applies");
            history.record(inverse);
            direct(&mut plain, e);
            prop_assert_eq!(content(&via_ops), content(&plain), "after {:?}", e);
        }
        let edited = content(&via_ops);
        while history.can_undo() {
            prop_assert!(with_slides(&mut via_ops, |d| history.undo(d)).is_some(), "an undo step applies");
        }
        prop_assert_eq!(live(&via_ops), initial);
        // And redo-all brings the edits back.
        while history.can_redo() {
            prop_assert!(with_slides(&mut via_ops, |d| history.redo(d)).is_some(), "a redo step applies");
        }
        prop_assert_eq!(content(&via_ops), edited);
    }
}
