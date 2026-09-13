// snapshot_fidelity.rs — what a Decks crash-recovery snapshot keeps (#322).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// A Decks snapshot is whatever `write_deck_bytes` produces for the document's
// format hint — pptx for an unsaved or .pptx deck, odp for a .odp one — and
// recovery reads it back with `read_deck`. So #322's "no recovery format
// silently strips supported content" is a claim about that round trip, in
// both formats, and it needs to be *our* writer paired with *our* reader.
//
// That pairing is the point. `soffice_oracle.rs` covers a dozen features by
// having LibreOffice rewrite the file and asserting the feature is still
// there, which is a true claim about LibreOffice and says nothing about
// whether Decks can read its own output. Tables had three such oracle tests
// passing while its reader silently dropped every column width, row height,
// frozen pane and merge (#716). These tests close the same gap for Decks
// before it can open.

use decks_core::engine::{Deck, MasterSlide, Slide, SlideObject};
use letters_core::model::{Run, RunStyle};

/// Both snapshot formats, so a feature preserved by one and dropped by the
/// other cannot hide behind the other's pass. A deck that was never saved
/// gets the pptx hint, which makes pptx the case that matters most for
/// recovery — it is the format unsaved work is held in.
const FORMATS: [&str; 2] = ["pptx", "odp"];

fn through_a_snapshot(deck: &Deck, kind: &str, tag: &str) -> Deck {
    let bytes = decks_core::write_deck_bytes(kind, deck)
        .unwrap_or_else(|e| panic!("{kind}: writing the snapshot failed: {e}"));
    let dir = tempfile::tempdir().expect("tempdir");
    // Named with the real extension because `read_deck` dispatches on it,
    // exactly as the recovery path's temp file does.
    let path = dir.path().join(format!("{tag}.{kind}"));
    std::fs::write(&path, &bytes).expect("write snapshot");
    decks_core::read_deck(path.to_str().unwrap())
        .unwrap_or_else(|e| panic!("{kind}: recovering the snapshot failed: {e}"))
}

/// A deck of exactly these slides, with no masters.
fn deck_of(slides: Vec<Slide>) -> Deck {
    Deck { slides, masters: vec![] }
}

/// A slide carrying only what the caller cares about.
fn slide_of(objects: Vec<SlideObject>, notes: &str, background: &str) -> Slide {
    Slide {
        title: String::new(),
        background: background.to_string(),
        objects,
        notes: notes.to_string(),
        master_idx: None,
    }
}

fn styled_run(text: &str) -> Run {
    Run {
        text: text.to_string(),
        style: RunStyle { bold: true, italic: true, ..RunStyle::default() },
    }
}

fn text_box(text: &str, x: f64, y: f64) -> SlideObject {
    SlideObject::TextBox {
        text: text.to_string(),
        x, y, w: 300.0, h: 80.0,
        rotation: 0.0,
        runs: vec![],
    }
}

#[test]
fn speaker_notes_survive_a_snapshot() {
    for kind in FORMATS {
        let deck = deck_of(vec![slide_of(
            vec![text_box("body", 10.0, 10.0)],
            "what to say out loud",
            "",
        )]);
        let back = through_a_snapshot(&deck, kind, "notes");
        assert_eq!(
            back.slides[0].notes, "what to say out loud",
            "{kind}: notes came back as {:?}",
            back.slides[0].notes,
        );
    }
}

#[test]
fn slide_backgrounds_survive_a_snapshot() {
    for kind in FORMATS {
        let deck = deck_of(vec![slide_of(
            vec![text_box("body", 10.0, 10.0)],
            "",
            "#112233",
        )]);
        let back = through_a_snapshot(&deck, kind, "bg");
        assert_eq!(
            back.slides[0].background, "#112233",
            "{kind}: background came back as {:?}",
            back.slides[0].background,
        );
    }
}

#[test]
fn object_geometry_survives_a_snapshot() {
    for kind in FORMATS {
        let deck = deck_of(vec![slide_of(
            vec![
                SlideObject::TextBox {
                    text: "placed".into(),
                    x: 137.0, y: 211.0, w: 320.0, h: 96.0,
                    rotation: 0.0,
                    runs: vec![],
                },
                SlideObject::Rect { x: 40.0, y: 50.0, w: 60.0, h: 70.0, rotation: 0.0 },
                SlideObject::Circle { x: 200.0, y: 240.0, r: 55.0, rotation: 0.0 },
            ],
            "",
            "",
        )]);
        let back = through_a_snapshot(&deck, kind, "geometry");
        let objects = &back.slides[0].objects;
        assert_eq!(objects.len(), 3, "{kind}: came back with {} objects", objects.len());

        // Position is asserted per object rather than as a count: a reader
        // that produced three objects at the origin would satisfy any
        // "nothing was dropped" check while losing every placement.
        // EMU/points conversions are lossy at the sub-point level, so this
        // allows a point of drift and no more.
        let near = |a: f64, b: f64| (a - b).abs() < 1.0;
        assert!(
            near(objects[0].x(), 137.0) && near(objects[0].y(), 211.0),
            "{kind}: text box came back at ({}, {}), not (137, 211)",
            objects[0].x(), objects[0].y(),
        );
        assert!(
            near(objects[1].x(), 40.0) && near(objects[1].y(), 50.0),
            "{kind}: rect came back at ({}, {})",
            objects[1].x(), objects[1].y(),
        );
        // Circle x()/y() report the bounding box, so a 55pt radius at
        // (200, 240) has its corner at (145, 185).
        assert!(
            near(objects[2].x(), 145.0) && near(objects[2].y(), 185.0),
            "{kind}: circle came back at ({}, {})",
            objects[2].x(), objects[2].y(),
        );
    }
}

/// Rotation, pptx only — deliberately, and the reason is recorded in
/// docs/readiness-2026-09/recovery.md rather than left as a silent hole.
///
/// pptx is the format an unsaved deck defaults to, so it is the one a crash
/// snapshot uses and the one this row is really about. Both writers used to
/// emit no rotation at all and both readers hardcoded zero, so a rotated
/// shape came back square either way. pptx's `a:xfrm/@rot` is one attribute
/// and is fixed here; ODF spells the same thing as
/// `draw:transform="rotate(θ) translate(x y)"`, where the rotation and the
/// position are entangled in one attribute and LibreOffice emits several
/// shapes of it. That is a larger change than this one and is not pretended
/// to be done.
///
/// Testing one format in a file whose other tests loop over both is worth a
/// word: the loop is what hid this. Asserting inside `for kind in FORMATS`
/// aborts on the first format that fails, so while pptx was broken the odp
/// failure was invisible — I only found it by fixing pptx and watching the
/// same assertion fail again with a different prefix.
#[test]
fn shape_rotation_survives_a_pptx_snapshot() {
    let deck = deck_of(vec![slide_of(
        vec![
            SlideObject::Rect { x: 40.0, y: 50.0, w: 60.0, h: 70.0, rotation: 30.0 },
            SlideObject::TextBox {
                text: "tilted".into(),
                x: 10.0, y: 10.0, w: 200.0, h: 50.0,
                rotation: 315.0,
                runs: vec![],
            },
        ],
        "",
        "",
    )]);
    let back = through_a_snapshot(&deck, "pptx", "rotation");
    let objects = &back.slides[0].objects;

    let rotation_of = |o: &SlideObject| match o {
        SlideObject::Rect { rotation, .. }
        | SlideObject::TextBox { rotation, .. }
        | SlideObject::Circle { rotation, .. }
        | SlideObject::Image { rotation, .. } => *rotation,
    };
    // OOXML stores sixtieth-thousandths of a degree, so a whole number of
    // degrees round-trips exactly; the tolerance is for the f64 division.
    assert!(
        (rotation_of(&objects[0]) - 30.0).abs() < 0.01,
        "the rect's 30 degree rotation came back as {}",
        rotation_of(&objects[0]),
    );
    // A rotation past a half turn, because normalising with the wrong
    // modulo (or dropping the sign) turns 315 into -45 or 45 and a test
    // using only 30 degrees would never notice.
    assert!(
        (rotation_of(&objects[1]) - 315.0).abs() < 0.01,
        "the text box's 315 degree rotation came back as {}",
        rotation_of(&objects[1]),
    );
}

#[test]
fn run_styles_survive_a_snapshot() {
    for kind in FORMATS {
        let deck = deck_of(vec![slide_of(
            vec![SlideObject::TextBox {
                text: "loud".into(),
                x: 10.0, y: 10.0, w: 200.0, h: 50.0,
                rotation: 0.0,
                runs: vec![styled_run("loud")],
            }],
            "",
            "",
        )]);
        let back = through_a_snapshot(&deck, kind, "runs");
        let SlideObject::TextBox { runs, text, .. } = &back.slides[0].objects[0] else {
            panic!("{kind}: the text box came back as a different object kind");
        };
        assert_eq!(text, "loud", "{kind}: text came back as {text:?}");
        let style = &runs
            .first()
            .unwrap_or_else(|| panic!("{kind}: the styled run came back with no runs"))
            .style;
        assert!(style.bold, "{kind}: bold was dropped");
        assert!(style.italic, "{kind}: italic was dropped");
    }
}

#[test]
fn slide_order_survives_a_snapshot() {
    for kind in FORMATS {
        let deck = deck_of(
            (0..4)
                .map(|i| {
                    slide_of(
                        vec![text_box(&format!("slide {i}"), 10.0, 10.0)],
                        &format!("note {i}"),
                        "",
                    )
                })
                .collect(),
        );
        let back = through_a_snapshot(&deck, kind, "order");
        assert_eq!(back.slides.len(), 4, "{kind}: {} slides came back", back.slides.len());
        // Notes rather than the generated titles: pptx regenerates
        // `Slide N` from the index on read, so titles would agree even if
        // the slides came back shuffled.
        let notes: Vec<&str> = back.slides.iter().map(|s| s.notes.as_str()).collect();
        assert_eq!(
            notes,
            vec!["note 0", "note 1", "note 2", "note 3"],
            "{kind}: slides came back in a different order",
        );
    }
}

/// Masters, in both formats — and currently failing in both, which is why
/// it is ignored with the reason rather than deleted or quietly narrowed.
///
/// Decks *reads* masters from an imported deck and renders them (the canvas
/// and the sidebar thumbnails both consult them), but neither writer emits
/// one: pptx writes no `slideMaster` part at all, odp no master page, and
/// both readers then synthesise a white default in its place. That is a
/// writer/reader asymmetry like #716's pointing the other way, and it costs
/// the design of any imported deck on *every* save, not only on recovery.
///
/// Implementing it means new slideMaster/slideLayout parts and their rels
/// for pptx and a styles.xml master page for odp — a feature, not the fix
/// this change is, and not something to half-land inside it. The test is
/// the specification: unignore it when a writer starts emitting masters and
/// it should pass as written. The gap is recorded in
/// docs/readiness-2026-09/recovery.md, which is why #322's row stays
/// partial.
#[test]
#[ignore = "neither writer emits a master yet; #322 row records the gap and this test specifies the fix"]
fn masters_survive_a_snapshot() {
    for kind in FORMATS {
        let deck = Deck {
            masters: vec![MasterSlide {
                name: "House".into(),
                background: "#204060".into(),
                default_font: "Cantarell".into(),
                shapes: vec![],
            }],
            slides: vec![Slide {
                title: String::new(),
                background: String::new(),
                objects: vec![text_box("on the house master", 10.0, 10.0)],
                notes: String::new(),
                master_idx: Some(0),
            }],
        };
        let back = through_a_snapshot(&deck, kind, "masters");
        assert!(
            !back.masters.is_empty(),
            "{kind}: the deck came back with no masters at all",
        );
        assert_eq!(
            back.masters[0].background, "#204060",
            "{kind}: the master's background came back as {:?}",
            back.masters[0].background,
        );
    }
}

#[test]
fn an_empty_deck_gains_nothing_it_never_had() {
    // The other direction, and the one a careless reader breaks: parsing a
    // master's placeholder prompts ("Click to edit Master title style") as
    // slide content, or reading the notes placeholder as notes, would give
    // every recovered deck text nobody typed.
    for kind in FORMATS {
        let deck = deck_of(vec![slide_of(vec![], "", "")]);
        let back = through_a_snapshot(&deck, kind, "empty");
        assert_eq!(back.slides.len(), 1, "{kind}: one empty slide became {}", back.slides.len());
        assert_eq!(
            back.slides[0].notes, "",
            "{kind}: an empty slide gained notes: {:?}",
            back.slides[0].notes,
        );
        assert!(
            back.slides[0].objects.is_empty(),
            "{kind}: an empty slide gained {} objects: {:?}",
            back.slides[0].objects.len(),
            back.slides[0].objects,
        );
    }
}
