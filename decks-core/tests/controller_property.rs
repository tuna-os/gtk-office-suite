// controller_property.rs — decks-readiness.md, "Undo/redo mixed object and
// slide edits, including deletion and selection repair; no detached state
// or reentrant RefCell panic", through DecksController, the API the window
// calls (ops_property.rs proves the ops underneath).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Random sequences of object edits, slide edits, undos and redos. Each
// edit that changes the deck is one undo step: a model of the history (a
// list of states and a cursor) predicts what every undo and redo shows.
// After every step the repaired selection points at something that exists.

use decks_core::engine::{Slide, SlideObject, Transition};
use decks_core::undo::ZOrderOp;
use decks_core::DecksController;
use proptest::prelude::*;

#[derive(Clone, Debug)]
enum Step {
    AddObject { slide: usize, x: f64 },
    DeleteObject { slide: usize, index: usize },
    MoveObject { slide: usize, index: usize, dx: f64 },
    Resize { slide: usize, index: usize, w: f64 },
    Rotate { slide: usize, index: usize, angle: f64 },
    ZOrder { slide: usize, index: usize, front: bool },
    AddSlide { at: usize },
    DeleteSlide { slide: usize },
    DuplicateSlide { slide: usize },
    SlideUp { slide: usize },
    SlideDown { slide: usize },
    Undo,
    Redo,
}

fn step() -> impl Strategy<Value = Step> {
    let n = 0usize..5;
    prop_oneof![
        (n.clone(), 0.0..900.0).prop_map(|(slide, x)| Step::AddObject { slide, x }),
        (n.clone(), n.clone()).prop_map(|(slide, index)| Step::DeleteObject { slide, index }),
        (n.clone(), n.clone(), 1.0..50.0).prop_map(|(slide, index, dx)| Step::MoveObject { slide, index, dx }),
        (n.clone(), n.clone(), 30.0..300.0).prop_map(|(slide, index, w)| Step::Resize { slide, index, w }),
        (n.clone(), n.clone(), 1.0..359.0).prop_map(|(slide, index, angle)| Step::Rotate { slide, index, angle }),
        (n.clone(), n.clone(), any::<bool>()).prop_map(|(slide, index, front)| Step::ZOrder { slide, index, front }),
        n.clone().prop_map(|at| Step::AddSlide { at }),
        n.clone().prop_map(|slide| Step::DeleteSlide { slide }),
        n.clone().prop_map(|slide| Step::DuplicateSlide { slide }),
        n.clone().prop_map(|slide| Step::SlideUp { slide }),
        n.prop_map(|slide| Step::SlideDown { slide }),
        Just(Step::Undo),
        Just(Step::Redo),
    ]
}

fn rect(x: f64) -> SlideObject {
    SlideObject::Rect { x, y: 10.0, w: 20.0, h: 20.0, rotation: 0.0 }
}

fn slide(title: &str, xs: &[f64]) -> Slide {
    Slide {
        title: title.into(),
        background: "#ffffff".into(), background_image: None,
        objects: xs.iter().map(|x| rect(*x)).collect(),
        notes: String::new(),
        master_idx: None,
        transition: Transition::None,
        builds: vec![],
        ids: Default::default(),
        layout: None,
    }
}

/// The deck, ids and all but the tombstones (undoing an insert tombstones
/// the id it gave). With the ids, moving one of two identical slides past
/// the other is a change, as it is to the history.
fn content(c: &DecksController) -> String {
    let plain: Vec<Slide> = c
        .slides
        .borrow()
        .iter()
        .map(|s| {
            let mut s = s.clone();
            s.ids.deleted.clear();
            s
        })
        .collect();
    format!("{plain:?}")
}

fn object(c: &DecksController, slide: usize, index: usize) -> Option<SlideObject> {
    c.slides.borrow().get(slide)?.objects.get(index).cloned()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn mixed_edits_undo_and_redo_one_step_each_and_the_selection_stays_valid(steps in prop::collection::vec(step(), 1..60)) {
        let c = DecksController::new(vec![slide("a", &[0.0, 100.0]), slide("b", &[50.0]), slide("c", &[])], vec![]);
        decks_core::ops::ensure_ids(&mut c.slides.borrow_mut());
        // The history model: every state the deck has been in that undo or
        // redo can reach, and where we are among them.
        let mut states = vec![content(&c)];
        let mut at = 0usize;
        let (mut current, mut selected) = (0usize, None::<usize>);
        for s in &steps {
            let before = content(&c);
            match s {
                Step::Undo => {
                    let undone = c.undo();
                    prop_assert_eq!(undone, at > 0, "undo available exactly when the model has a step");
                    if undone {
                        at -= 1;
                        prop_assert_eq!(content(&c), states[at].clone(), "undo shows the state before the step");
                    }
                }
                Step::Redo => {
                    let redone = c.redo();
                    prop_assert_eq!(redone, at + 1 < states.len(), "redo available exactly when the model has one");
                    if redone {
                        at += 1;
                        prop_assert_eq!(content(&c), states[at].clone(), "redo shows the state after the step");
                    }
                }
                edit => {
                    match edit {
                        Step::AddObject { slide, x } => {
                            c.add_object(*slide, rect(*x));
                            selected = c.slides.borrow().get(*slide).map(|s| s.objects.len() - 1);
                            current = *slide;
                        }
                        Step::DeleteObject { slide, index } => {
                            if let Some(o) = object(&c, *slide, *index) {
                                c.delete_object(*slide, *index, o);
                            }
                        }
                        Step::MoveObject { slide, index, dx } => c.move_object(*slide, *index, *dx, 0.0),
                        Step::Resize { slide, index, w } => {
                            if let Some(o) = object(&c, *slide, *index) {
                                let old = decks_core::undo::obj_bounds(&o);
                                c.resize_object(*slide, *index, old, (old.0, old.1, *w, old.3));
                            }
                        }
                        Step::Rotate { slide, index, angle } => {
                            if let Some(o) = object(&c, *slide, *index) {
                                c.rotate_object(*slide, *index, o.rotation(), *angle);
                            }
                        }
                        Step::ZOrder { slide, index, front } => {
                            c.z_order_object(*slide, *index, if *front { ZOrderOp::BringToFront } else { ZOrderOp::SendToBack })
                        }
                        Step::AddSlide { at } => current = c.add_slide(*at, slide("new", &[300.0])),
                        Step::DeleteSlide { slide } => {
                            if let Some(i) = c.delete_slide(*slide) {
                                current = i;
                            }
                        }
                        Step::DuplicateSlide { slide } => {
                            if let Some(i) = c.duplicate_slide(*slide) {
                                current = i;
                            }
                        }
                        Step::SlideUp { slide } => {
                            if let Some(i) = c.move_slide_up(*slide) {
                                current = i;
                            }
                        }
                        Step::SlideDown { slide } => {
                            if let Some(i) = c.move_slide_down(*slide) {
                                current = i;
                            }
                        }
                        Step::Undo | Step::Redo => unreachable!(),
                    }
                    let after = content(&c);
                    if after != before {
                        // One edit, one step; it drops anything to redo.
                        states.truncate(at + 1);
                        states.push(after);
                        at += 1;
                        prop_assert!(!c.can_redo(), "{:?} left something to redo", edit);
                    } else {
                        prop_assert_eq!(c.can_undo(), at > 0, "{:?} changed nothing but touched the history", edit);
                    }
                }
            }
            prop_assert!(c.slide_count() >= 1, "Decks always keeps a slide");
            (current, selected) = c.repair_selection(current, selected);
            let slides = c.slides.borrow();
            prop_assert!(current < slides.len(), "current slide {} of {}", current, slides.len());
            if let Some(i) = selected {
                prop_assert!(i < slides[current].objects.len(), "selected object {} of {}", i, slides[current].objects.len());
            }
            // Ids stay attached to what they name: one per object.
            for s in slides.iter().filter(|s| !s.ids.objects.is_empty()) {
                prop_assert_eq!(s.ids.objects.len(), s.objects.len());
            }
        }
        // And all the way back.
        while c.undo() {}
        prop_assert_eq!(content(&c), states[0].clone());
    }
}

#[test]
fn undoing_an_added_slide_and_object_repairs_a_selection_past_the_end() {
    let c = DecksController::new(vec![slide("a", &[0.0])], vec![]);
    let current = c.add_slide(1, slide("b", &[]));
    c.add_object(current, rect(5.0));
    assert_eq!(c.repair_selection(current, Some(0)), (1, Some(0)));
    assert!(c.undo());
    assert_eq!(c.repair_selection(current, Some(0)), (1, None), "the object went with the undo");
    assert!(c.undo());
    assert_eq!(c.repair_selection(current, Some(0)), (0, None), "so did the slide, and object 0 of another slide is not what was selected");
    assert!(c.redo() && c.redo());
    assert_eq!(c.repair_selection(0, Some(0)), (0, Some(0)));
}
