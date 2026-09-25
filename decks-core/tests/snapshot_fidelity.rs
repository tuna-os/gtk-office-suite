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
        body: Default::default(),
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
                    body: Default::default(),
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

/// Rotation, in both formats, and the position it is entangled with.
///
/// Both writers used to emit no rotation at all and both readers hardcoded
/// zero, so a rotated shape came back square either way and the rotate
/// gesture's result was discarded by every save. pptx's `a:xfrm/@rot` is
/// one attribute and was fixed first; ODF has no rotation attribute and
/// spells the same thing as a `draw:transform` list, which carries the
/// rotation *and* the position together — so this test asserts the
/// position of every rotated shape as well. A translate computed with the
/// wrong matrix leaves the angle perfect and moves the shape, which an
/// angle-only test would pass.
///
/// The loop is written to report every format rather than abort on the
/// first, because the earlier version of this test asserted inside
/// `for kind in FORMATS` and that is exactly what hid the odp gap: pptx
/// failed first, and the odp failure only became visible once pptx was
/// fixed. Collecting the complaints and asserting once at the end means a
/// regression in either format names itself.
#[test]
fn shape_rotation_survives_a_snapshot() {
    let deck = deck_of(vec![slide_of(
        vec![
            SlideObject::Rect { x: 40.0, y: 50.0, w: 60.0, h: 70.0, rotation: 30.0 },
            SlideObject::TextBox {
                text: "tilted".into(),
                x: 10.0, y: 10.0, w: 200.0, h: 50.0,
                // A rotation past a half turn, because normalising with the
                // wrong modulo (or dropping the sign) turns 315 into -45 or
                // 45 and a test using only 30 degrees would never notice.
                rotation: 315.0,
                runs: vec![],
                body: Default::default(),
            },
            SlideObject::Circle { x: 300.0, y: 200.0, r: 45.0, rotation: 120.0 },
        ],
        "",
        "",
    )]);

    let rotation_of = |o: &SlideObject| match o {
        SlideObject::Rect { rotation, .. }
        | SlideObject::TextBox { rotation, .. }
        | SlideObject::Circle { rotation, .. }
        | SlideObject::Shape { rotation, .. }
        | SlideObject::Table { rotation, .. }
        | SlideObject::Image { rotation, .. } => *rotation,
    };
    // Where the shape's box sits. A Circle stores its centre and a Shape
    // (what a file reads back as) its corner; the box is what must not move.
    let anchor_of = |o: &SlideObject| {
        let (x, y, _, _) = decks_core::undo::obj_bounds(o);
        (x, y)
    };

    let mut complaints: Vec<String> = Vec::new();
    for kind in FORMATS {
        let back = through_a_snapshot(&deck, kind, "rotation");
        let objects = &back.slides[0].objects;
        if objects.len() != deck.slides[0].objects.len() {
            complaints.push(format!("{kind}: {} shapes came back, not 3", objects.len()));
            continue;
        }
        for (i, want) in deck.slides[0].objects.iter().enumerate() {
            // OOXML stores sixtieth-thousandths of a degree and ODF stores
            // radians, so a whole number of degrees round-trips through
            // both; the tolerance is for the f64 conversion at each end.
            let got = rotation_of(&objects[i]);
            let expected = rotation_of(want);
            if (got - expected).abs() >= 0.01 {
                complaints.push(format!(
                    "{kind}: shape {i}'s {expected} degree rotation came back as {got}"
                ));
            }
            let (gx, gy) = anchor_of(&objects[i]);
            let (wx, wy) = anchor_of(want);
            if (gx - wx).abs() >= 0.5 || (gy - wy).abs() >= 0.5 {
                complaints.push(format!(
                    "{kind}: shape {i} rotated correctly but moved from \
                     ({wx}, {wy}) to ({gx}, {gy})"
                ));
            }
        }
    }
    assert!(complaints.is_empty(), "{}", complaints.join("\n"));
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
                body: Default::default(),
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

/// Masters, in both formats.
///
/// Decks *reads* a master from an imported deck and renders it (the canvas
/// and the sidebar thumbnails both consult it), and for a long time neither
/// writer emitted one: pptx wrote no `slideMaster` part at all, odp no
/// `styles.xml`, and both readers then synthesised a white default in its
/// place. That is a writer/reader asymmetry like #716's pointing the other
/// way, and it cost the design of any imported deck on *every* save, not
/// only on recovery. This test was the specification while it was
/// `#[ignore]`d; it is now the regression guard.
///
/// It asserts the name and the decorations as well as the background,
/// because a master that comes back as a nameless empty page with the right
/// colour is not the master that went in — and because the two formats fail
/// differently: pptx can lose the whole chain (a slide relates to a layout,
/// the layout to the master) while odp can lose only the mapping, coming
/// back with the master present but every slide pointing at the wrong one.
/// Hence the second master, which nothing points at, and the assertion that
/// the slide still chose the first.
#[test]
fn masters_survive_a_snapshot() {
    let mut complaints: Vec<String> = Vec::new();
    for kind in FORMATS {
        let deck = Deck {
            masters: vec![
                MasterSlide {
                    name: "House Style".into(),
                    background: "#204060".into(),
                    default_font: "Cantarell".into(),
                    shapes: vec![text_box("a footer on the master", 20.0, 500.0)],
                },
                MasterSlide {
                    name: "Second".into(),
                    background: "#a01020".into(),
                    default_font: "Cantarell".into(),
                    shapes: vec![],
                },
            ],
            slides: vec![
                Slide {
                    title: String::new(),
                    background: String::new(),
                    objects: vec![text_box("on the house master", 10.0, 10.0)],
                    notes: String::new(),
                    master_idx: Some(0),
                },
                // On the *second* master, which is what makes the mapping
                // testable: a reader that loses it falls back to master 0,
                // and a deck where every slide is already on master 0
                // cannot tell that apart from working.
                Slide {
                    title: String::new(),
                    background: String::new(),
                    objects: vec![text_box("on the second master", 10.0, 10.0)],
                    notes: String::new(),
                    master_idx: Some(1),
                },
            ],
        };
        let back = through_a_snapshot(&deck, kind, "masters");
        if back.masters.is_empty() {
            complaints.push(format!("{kind}: the deck came back with no masters at all"));
            continue;
        }
        if back.masters[0].background != "#204060" {
            complaints.push(format!(
                "{kind}: the master's background came back as {:?}",
                back.masters[0].background,
            ));
        }
        // A name escaped into an ODF style token ("House_20_Style") and not
        // unescaped on the way back is the failure this catches.
        if back.masters[0].name != "House Style" {
            complaints.push(format!(
                "{kind}: the master's name came back as {:?}",
                back.masters[0].name,
            ));
        }
        let decorations: Vec<&str> = back.masters[0]
            .shapes
            .iter()
            .filter_map(|o| match o {
                SlideObject::TextBox { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        if decorations != vec!["a footer on the master"] {
            complaints.push(format!(
                "{kind}: the master's decorations came back as {decorations:?}",
            ));
        }
        // Each slide must still name the master it was on, not merely
        // find some master in the package.
        let mapping: Vec<Option<usize>> =
            back.slides.iter().map(|s| s.master_idx).collect();
        if mapping != vec![Some(0), Some(1)] {
            complaints.push(format!(
                "{kind}: the slides were on masters [0, 1] and came back on {mapping:?}"
            ));
        }
        // And the slide's own content must not have absorbed the master's.
        let on_slide: Vec<&str> = back.slides[0]
            .objects
            .iter()
            .filter_map(|o| match o {
                SlideObject::TextBox { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        if on_slide != vec!["on the house master"] {
            complaints.push(format!("{kind}: the slide's own content came back as {on_slide:?}"));
        }
    }
    assert!(complaints.is_empty(), "{}", complaints.join("\n"));
}

/// An embedded picture, in both formats — and its *bytes*, not merely the
/// existence of an image object.
///
/// The odp writer used to drop `SlideObject::Image` outright while the pptx
/// writer carried it, so saving a deck as odp lost every picture in it —
/// the same writer/reader asymmetry as #716's Tables bug, and like that one
/// it cost the content on any save-and-reopen, not only on recovery.
///
/// Both readers unpack the picture to a temporary file and put that path in
/// the model, so the path deliberately is *not* asserted: it cannot be the
/// one that went in, and a test demanding it would be asserting the wrong
/// thing. Comparing the bytes is what says the picture survived. An
/// assertion that some `Image` came back would pass on a writer that
/// packaged an empty file.
#[test]
fn images_survive_a_snapshot() {
    // A 2x2 red PNG, inline so no fixture file is needed.
    let png: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a,
        0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0, 2, 0, 0, 0, 2, 8, 2, 0, 0, 0,
        0xfd, 0xd4, 0x9a, 0x73,
        0, 0, 0, 21, b'I', b'D', b'A', b'T', 0x78, 0x9c, 0x62, 0xfa, 0xcf, 0xc0, 0xc0,
        0xf0, 0x1f, 0x88, 0xff, 0x33, 0x30, 0x30, 0x00, 0x00, 0x00, 0xff, 0xff,
        0x03, 0x00, 0x2b, 0x11, 0x04, 0xf9,
        0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
    ];
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("dot.png");
    std::fs::write(&source, png).unwrap();

    let deck = deck_of(vec![slide_of(
        vec![SlideObject::Image {
            path: source.to_string_lossy().to_string(),
            x: 60.0,
            y: 70.0,
            w: 200.0,
            h: 150.0,
            rotation: 0.0,
        }],
        "",
        "",
    )]);

    let mut complaints: Vec<String> = Vec::new();
    for kind in FORMATS {
        let back = through_a_snapshot(&deck, kind, "image");
        let images: Vec<(&String, f64, f64)> = back.slides[0]
            .objects
            .iter()
            .filter_map(|o| match o {
                SlideObject::Image { path, x, y, .. } => Some((path, *x, *y)),
                _ => None,
            })
            .collect();
        let [(path, x, y)] = images.as_slice() else {
            complaints.push(format!(
                "{kind}: expected one picture back, got {:?}",
                back.slides[0].objects,
            ));
            continue;
        };
        match std::fs::read(path) {
            Ok(bytes) if bytes == png => {}
            Ok(bytes) => complaints.push(format!(
                "{kind}: the picture came back as {} bytes, not the {} that went in",
                bytes.len(),
                png.len(),
            )),
            Err(e) => complaints.push(format!(
                "{kind}: the picture's path {path:?} cannot be read back: {e}"
            )),
        }
        // Geometry travels with it or the picture lands somewhere else.
        if (x - 60.0).abs() >= 0.5 || (y - 70.0).abs() >= 0.5 {
            complaints.push(format!("{kind}: the picture moved to ({x}, {y})"));
        }
    }
    assert!(complaints.is_empty(), "{}", complaints.join("\n"));
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

/// A master's font has to survive the round trip in both formats, because
/// the canvas now draws slide text in it (`canvas::master_font_family`).
/// Before that it was a field written at every construction site and read
/// at none, so this assertion would have passed on a writer that carried it
/// and a renderer that ignored it alike.
#[test]
fn the_masters_font_survives_a_snapshot() {
    let mut complaints: Vec<String> = Vec::new();
    for kind in FORMATS {
        let deck = Deck {
            masters: vec![MasterSlide {
                name: "House Style".into(),
                background: "#ffffff".into(),
                // A real face rather than a generic family: "Sans" is the
                // fallback, so a writer that drops the font entirely would
                // still come back as "Sans" and look like a pass.
                default_font: "Liberation Serif".into(),
                shapes: vec![],
            }],
            slides: vec![Slide {
                title: String::new(),
                background: String::new(),
                objects: vec![text_box("body text", 10.0, 10.0)],
                notes: String::new(),
                master_idx: Some(0),
            }],
        };
        let back = through_a_snapshot(&deck, kind, "master-font");
        match back.masters.first() {
            None => complaints.push(format!("{kind}: no master came back at all")),
            Some(m) if m.default_font != "Liberation Serif" => complaints.push(format!(
                "{kind}: the master's font came back as {:?}, not \"Liberation Serif\"",
                m.default_font
            )),
            Some(_) => {}
        }
    }
    assert!(complaints.is_empty(), "{}", complaints.join("\n"));
}

/// The formats are **not** equivalent here, and this pins the difference so
/// it stays a known property instead of becoming a surprise.
///
/// pptx gives every master its own theme part, so two masters keep two
/// fonts. ODF, as LibreOffice Impress writes it, keeps one
/// `style:default-style` for the whole document — measured by converting a
/// pptx carrying a per-master theme font through Impress and reading the
/// `.odp` back. So in odp the first master's font becomes the document's
/// and the others read back as that one.
///
/// If someone later teaches the odp writer a per-master carrier that real
/// readers honour, this test is the one that should fail.
#[test]
fn masters_keep_their_own_font_in_pptx_but_share_one_in_odp() {
    let deck = Deck {
        masters: vec![
            MasterSlide {
                name: "First".into(),
                background: "#ffffff".into(),
                default_font: "Liberation Serif".into(),
                shapes: vec![],
            },
            MasterSlide {
                name: "Second".into(),
                background: "#ffffff".into(),
                default_font: "Liberation Mono".into(),
                shapes: vec![],
            },
        ],
        slides: vec![
            Slide {
                title: String::new(),
                background: String::new(),
                objects: vec![text_box("on first", 10.0, 10.0)],
                notes: String::new(),
                master_idx: Some(0),
            },
            Slide {
                title: String::new(),
                background: String::new(),
                objects: vec![text_box("on second", 10.0, 10.0)],
                notes: String::new(),
                master_idx: Some(1),
            },
        ],
    };

    let pptx = through_a_snapshot(&deck, "pptx", "two-fonts");
    let pptx_fonts: Vec<&str> = pptx.masters.iter().map(|m| m.default_font.as_str()).collect();
    assert!(
        pptx_fonts.contains(&"Liberation Serif") && pptx_fonts.contains(&"Liberation Mono"),
        "pptx should keep a font per master, got {pptx_fonts:?}"
    );

    let odp = through_a_snapshot(&deck, "odp", "two-fonts");
    let odp_fonts: Vec<&str> = odp.masters.iter().map(|m| m.default_font.as_str()).collect();
    assert!(
        !odp_fonts.is_empty() && odp_fonts.iter().all(|f| *f == "Liberation Serif"),
        "odp has one document font, so every master should read back as the \
         first master's: got {odp_fonts:?}"
    );
}

/// The theme part has to be *declared*, not merely present.
///
/// This is a package-shape assertion rather than a round-trip one, and it
/// is here because nothing available can make it a round-trip. Deleting
/// the `[Content_Types].xml` override for the theme leaves every test in
/// this file green **and all 35 oracle tests green** — LibreOffice opens
/// the package regardless. ECMA-376 still requires the override, and a
/// stricter consumer (PowerPoint) is entitled to reject a part with no
/// declared content type, so the requirement is pinned directly.
///
/// The same leniency is why masters went out with no theme relationship at
/// all until now and nothing noticed. Asserting the bytes is the weakest
/// kind of check in this file; it is used only where a behavioural one
/// cannot exist.
#[test]
fn the_pptx_declares_the_theme_part_it_ships() {
    let deck = Deck {
        masters: vec![MasterSlide {
            name: "House Style".into(),
            background: "#ffffff".into(),
            default_font: "Liberation Serif".into(),
            shapes: vec![],
        }],
        slides: vec![Slide {
            title: String::new(),
            background: String::new(),
            objects: vec![],
            notes: String::new(),
            master_idx: Some(0),
        }],
    };
    let bytes = decks_core::write_deck_bytes("pptx", &deck).expect("write pptx");
    let mut zip =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("the package should be a zip");

    let mut read = |name: &str| -> String {
        let mut f = zip.by_name(name).unwrap_or_else(|_| panic!("{name} should be in the package"));
        let mut s = String::new();
        std::io::Read::read_to_string(&mut f, &mut s).expect("read part");
        s
    };

    let types = read("[Content_Types].xml");
    assert!(
        types.contains("/ppt/theme/theme1.xml")
            && types.contains("application/vnd.openxmlformats-officedocument.theme+xml"),
        "the theme needs a content-type override: {types}"
    );

    // And the master has to relate to it, or a reader walking the package
    // graph never reaches the part at all.
    let rels = read("ppt/slideMasters/_rels/slideMaster1.xml.rels");
    assert!(
        rels.contains("/theme\"") && rels.contains("../theme/theme1.xml"),
        "the master needs a theme relationship: {rels}"
    );
    // The layout must stay rId1, because `master_part_xml` names rId1 as
    // its layout in fixed text — swap the order and the master points at
    // its theme as though that were the layout.
    //
    // Asserting `rels.contains("rId1")` and `rels.contains("slideLayout1")`
    // separately is not this check: both hold whichever order they are
    // written in, and the first version of this test passed against the
    // swap. The relationship carrying rId1 is what has to be the layout,
    // so that is what is read out.
    let rid1_target = rels
        .split("<Relationship ")
        .find(|r| r.contains("Id=\"rId1\""))
        .and_then(|r| r.split("Target=\"").nth(1))
        .and_then(|t| t.split('"').next())
        .unwrap_or("<no rId1 at all>");
    assert!(
        rid1_target.contains("slideLayout1.xml"),
        "rId1 must be the layout, not {rid1_target:?}: {rels}"
    );
}

/// Two runs in one paragraph are one line, and stay one line.
///
/// `run_styles_survive_a_snapshot` above has covered run styling since the
/// beginning and never saw this, because it uses a *single* run — and one
/// run cannot reveal how two are joined. The pptx reader recorded one entry
/// per `a:t` and joined the lot with `\n`, so "Plain Bold" came back as two
/// lines; the same reading then dropped the space the first run ends on, so
/// removing only the newline yields "PlainBold". Both are asserted here.
///
/// This is the same shape as #725's master-mapping test needing a slide on a
/// master that is not the first: a fixture too simple to distinguish the
/// right answer from the wrong one passes either way.
#[test]
fn styled_runs_do_not_gain_line_breaks_between_them() {
    for kind in FORMATS {
        let runs = vec![
            Run { text: "Plain ".into(), style: RunStyle::default() },
            Run {
                text: "Bold".into(),
                style: RunStyle { bold: true, ..RunStyle::default() },
            },
        ];
        let deck = deck_of(vec![slide_of(
            vec![SlideObject::TextBox {
                text: "Plain Bold".into(),
                x: 10.0, y: 10.0, w: 200.0, h: 50.0,
                rotation: 0.0,
                runs,
                body: Default::default(),
            }],
            "",
            "",
        )]);
        let back = through_a_snapshot(&deck, kind, "multirun");
        let SlideObject::TextBox { runs, text, .. } = &back.slides[0].objects[0] else {
            panic!("{kind}: the text box came back as a different object kind");
        };
        assert_eq!(
            text, "Plain Bold",
            "{kind}: runs of one paragraph came back split or welded"
        );
        assert_eq!(
            runs.iter().map(|r| r.text.as_str()).collect::<String>(),
            *text,
            "{kind}: SlideObject::TextBox requires concatenated run text to equal `text`"
        );
        assert!(
            runs.iter().any(|r| r.style.bold),
            "{kind}: the emphasised run lost its styling"
        );
        assert!(
            runs.iter().any(|r| !r.style.bold),
            "{kind}: the unemphasised run gained styling"
        );
    }
}

/// A master decoration's own emphasis survives, in both formats.
///
/// This is what kept #322's imported-metadata row open after the master's
/// `default_font` landed. Neither writer emitted it, because neither reader
/// filled it — a symmetric omission rather than a silent strip, but the
/// row's claim is about supported content either way. The canvas draws a
/// decoration's runs now, so this carries something that is honoured.
#[test]
fn a_master_decorations_run_styling_survives_a_snapshot() {
    for kind in FORMATS {
        let deck = Deck {
            slides: vec![slide_of(vec![text_box("Slide body", 10.0, 10.0)], "", "")],
            masters: vec![MasterSlide {
                name: "House Style".into(),
                background: "#ffffff".into(),
                default_font: "Cantarell".into(),
                shapes: vec![SlideObject::TextBox {
                    text: "ACME Confidential".into(),
                    x: 20.0, y: 30.0, w: 300.0, h: 40.0,
                    rotation: 0.0,
                    runs: vec![
                        Run { text: "ACME ".into(), style: RunStyle::default() },
                        Run {
                            text: "Confidential".into(),
                            style: RunStyle {
                                bold: true,
                                italic: true,
                                ..RunStyle::default()
                            },
                        },
                    ],
                    body: Default::default(),
                }],
            }],
        };
        let back = through_a_snapshot(&deck, kind, "masterruns");
        let master = back
            .masters
            .first()
            .unwrap_or_else(|| panic!("{kind}: the master did not survive at all"));
        let SlideObject::TextBox { text, runs, .. } = &master.shapes[0] else {
            panic!("{kind}: the decoration came back as a different object kind");
        };
        assert_eq!(text, "ACME Confidential", "{kind}: decoration text changed");
        assert_eq!(
            runs.iter().map(|r| r.text.as_str()).collect::<String>(),
            *text,
            "{kind}: concatenated run text must equal `text`"
        );
        let emphasised = runs
            .iter()
            .find(|r| r.text.contains("Confidential"))
            .unwrap_or_else(|| panic!("{kind}: the emphasised run is missing"));
        assert!(emphasised.style.bold, "{kind}: the decoration lost bold");
        assert!(emphasised.style.italic, "{kind}: the decoration lost italic");
        let plain = runs
            .iter()
            .find(|r| r.text.starts_with("ACME"))
            .unwrap_or_else(|| panic!("{kind}: the plain run is missing"));
        assert!(
            !plain.style.bold,
            "{kind}: styling leaked onto the unemphasised run"
        );
    }
}

/// A master's run styles must not redefine a slide's.
///
/// ODF-specific, because it is a hazard of how ODF splits a document:
/// `content.xml` and `styles.xml` each carry their own
/// `office:automatic-styles`, and our reader merges both into one map keyed
/// by style name. Numbering each file's styles from 1 independently means
/// the master's first style and the first slide's are both `T1`, and
/// whichever file is parsed second wins — so adding a styled master
/// decoration would silently restyle text on an unrelated slide.
///
/// The two styles here are deliberately *different* (bold on the slide,
/// italic on the master). With one style shared between them a collision
/// would overwrite one with an identical value and the test would pass
/// while the bug was live, which is the same fixture trap as
/// `styled_runs_do_not_gain_line_breaks_between_them`.
#[test]
fn a_masters_run_styles_do_not_collide_with_a_slides_in_odp() {
    let deck = Deck {
        slides: vec![slide_of(
            vec![SlideObject::TextBox {
                text: "slide word".into(),
                x: 10.0, y: 10.0, w: 200.0, h: 50.0,
                rotation: 0.0,
                runs: vec![Run {
                    text: "slide word".into(),
                    style: RunStyle { bold: true, ..RunStyle::default() },
                }],
                body: Default::default(),
            }],
            "",
            "",
        )],
        masters: vec![MasterSlide {
            name: "House".into(),
            background: "#ffffff".into(),
            default_font: "Cantarell".into(),
            shapes: vec![SlideObject::TextBox {
                text: "master word".into(),
                x: 20.0, y: 20.0, w: 200.0, h: 50.0,
                rotation: 0.0,
                runs: vec![Run {
                    text: "master word".into(),
                    style: RunStyle { italic: true, ..RunStyle::default() },
                }],
                body: Default::default(),
            }],
        }],
    };
    let back = through_a_snapshot(&deck, "odp", "stylecollide");

    let SlideObject::TextBox { runs, .. } = &back.slides[0].objects[0] else {
        panic!("the slide's text box came back as a different object kind");
    };
    let s = &runs[0].style;
    assert!(s.bold, "the slide's run lost its own bold");
    assert!(
        !s.italic,
        "the master's style leaked onto the slide: both were named T1"
    );

    let SlideObject::TextBox { runs, .. } = &back.masters[0].shapes[0] else {
        panic!("the decoration came back as a different object kind");
    };
    let m = &runs[0].style;
    assert!(m.italic, "the master's run lost its own italic");
    assert!(!m.bold, "the slide's style leaked onto the master");
}

/// A styled box spanning two lines keeps both the break and the styling.
///
/// Two separate defects met here, and either one alone hides the other. The
/// odp writer's styled path emitted a single `text:p` with a literal
/// newline in it, which ODF collapses to a space; the odp reader then kept
/// runs only for single-paragraph boxes, so writing the break properly
/// would have swapped a lost line break for lost styling. The pptx reader
/// reached the same place from the other side, joining every run with `\n`
/// whether or not a paragraph had ended.
#[test]
fn a_styled_box_spanning_two_lines_keeps_both() {
    for kind in FORMATS {
        let deck = deck_of(vec![slide_of(
            vec![SlideObject::TextBox {
                text: "Bold one\nplain two".into(),
                x: 10.0, y: 10.0, w: 300.0, h: 80.0,
                rotation: 0.0,
                runs: vec![
                    Run {
                        text: "Bold one".into(),
                        style: RunStyle { bold: true, ..RunStyle::default() },
                    },
                    Run { text: "\nplain two".into(), style: RunStyle::default() },
                ],
                body: Default::default(),
            }],
            "",
            "",
        )]);
        let back = through_a_snapshot(&deck, kind, "styledlines");
        let SlideObject::TextBox { text, runs, .. } = &back.slides[0].objects[0] else {
            panic!("{kind}: the text box came back as a different object kind");
        };
        assert_eq!(text, "Bold one\nplain two", "{kind}: the line break moved");
        assert_eq!(
            runs.iter().map(|r| r.text.as_str()).collect::<String>(),
            *text,
            "{kind}: concatenated run text must equal `text`"
        );
        assert!(
            runs.iter().any(|r| r.style.bold),
            "{kind}: a multi-line box lost its styling"
        );
        assert!(
            runs.iter().any(|r| !r.style.bold && r.text.contains("plain two")),
            "{kind}: styling leaked onto the second line"
        );
    }
}

/// The same guarantee for a `draw:custom-shape`, which only a foreign file
/// produces.
///
/// The odp reader builds a text box at two places — the `draw:text-box`
/// inside a `draw:frame` that our own writer emits, and the
/// `draw:custom-shape` that Impress and PowerPoint's exporters use for a
/// shape carrying text. Our round trips only ever exercise the first, so
/// the second is reachable by imported documents alone; it kept its own
/// copy of the drop-the-runs rule and nothing noticed. This crafts the
/// package directly, since our writer cannot produce one.
#[test]
fn a_custom_shapes_styled_lines_survive_being_read() {
    use std::io::{Cursor, Write};
    let content = "<office:document-content \
         xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
         xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" \
         xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
         xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" \
         xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\">\
         <office:automatic-styles>\
         <style:style style:name=\"T1\" style:family=\"text\">\
         <style:text-properties fo:font-weight=\"bold\"/></style:style>\
         </office:automatic-styles>\
         <office:body><office:presentation><draw:page draw:name=\"p1\">\
         <draw:custom-shape svg:x=\"1cm\" svg:y=\"1cm\" svg:width=\"8cm\" svg:height=\"2cm\">\
         <text:p><text:span text:style-name=\"T1\">Bold one</text:span></text:p>\
         <text:p>plain two</text:p>\
         </draw:custom-shape>\
         </draw:page></office:presentation></office:body></office:document-content>";
    let mut buffer = Vec::new();
    {
        let mut w = zip::ZipWriter::new(Cursor::new(&mut buffer));
        w.start_file("content.xml", zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(content.as_bytes()).unwrap();
        w.finish().unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("foreign.odp");
    std::fs::write(&path, &buffer).unwrap();

    let deck = decks_core::read_deck(path.to_str().unwrap()).expect("read crafted odp");
    let SlideObject::TextBox { text, runs, .. } = &deck.slides[0].objects[0] else {
        panic!("the custom shape's text did not come back as a text box");
    };
    assert_eq!(text, "Bold one\nplain two");
    assert_eq!(
        runs.iter().map(|r| r.text.as_str()).collect::<String>(),
        *text,
        "concatenated run text must equal `text`"
    );
    assert!(
        runs.iter().any(|r| r.style.bold),
        "a custom shape's styling was dropped: {runs:?}"
    );
}

/// A deck whose slide is the size current PowerPoint actually uses.
///
/// `960x540` are *model units* (ADR 0004), so a coordinate means something
/// only relative to its slide. Both readers divided by a fixed constant
/// instead — which is the same thing only for a slide of exactly the size
/// our own writer emits. PowerPoint and Impress both default 16:9 decks to
/// 13.333in x 7.5in (`sldSz cx="12192000" cy="6858000"`), and on one of
/// those a full-bleed shape came back as 1280x720 in a 960x540 space: a
/// third too large, running off the canvas on every import.
///
/// No test could see it, because every test round-trips through our own
/// writer and our own writer always emits the one size the constant
/// assumed. So this crafts the package.
#[test]
fn a_deck_on_the_modern_powerpoint_slide_size_imports_at_model_scale() {
    use std::io::{Cursor, Write};
    // Full-bleed on a 12192000 x 6858000 EMU slide.
    let parts: [(&str, String); 4] = [
        (
            "[Content_Types].xml",
            "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
             <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
             <Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/>\
             <Override PartName=\"/ppt/slides/slide1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>\
             </Types>".to_string(),
        ),
        (
            "_rels/.rels",
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"ppt/presentation.xml\"/>\
             </Relationships>".to_string(),
        ),
        (
            "ppt/presentation.xml",
            "<p:presentation xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" \
             xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\
             <p:sldIdLst><p:sldId id=\"256\" r:id=\"rId1\"/></p:sldIdLst>\
             <p:sldSz cx=\"12192000\" cy=\"6858000\"/></p:presentation>".to_string(),
        ),
        (
            "ppt/slides/slide1.xml",
            "<p:sld xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" \
             xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">\
             <p:cSld><p:spTree>\
             <p:sp><p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/>\
             <a:ext cx=\"12192000\" cy=\"6858000\"/></a:xfrm>\
             <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:sp>\
             </p:spTree></p:cSld></p:sld>".to_string(),
        ),
    ];
    let mut buffer = Vec::new();
    {
        let mut w = zip::ZipWriter::new(Cursor::new(&mut buffer));
        for (name, body) in &parts {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(body.as_bytes()).unwrap();
        }
        // The slide's rels, named for the slide part.
        w.start_file("ppt/_rels/presentation.xml.rels", zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(
            b"<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
              <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide1.xml\"/>\
              </Relationships>",
        )
        .unwrap();
        w.finish().unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("modern.pptx");
    std::fs::write(&path, &buffer).unwrap();

    let deck = decks_core::read_deck(path.to_str().unwrap()).expect("read crafted pptx");
    let obj = deck.slides[0]
        .objects
        .first()
        .expect("the full-bleed shape did not survive the read");
    let (x, y, w, h) = match obj {
        SlideObject::Rect { x, y, w, h, .. }
        | SlideObject::Shape { x, y, w, h, .. }
        | SlideObject::Table { x, y, w, h, .. }
        | SlideObject::TextBox { x, y, w, h, .. } => (*x, *y, *w, *h),
        other => panic!("unexpected object kind: {other:?}"),
    };
    assert!(x.abs() < 0.01 && y.abs() < 0.01, "origin moved: ({x}, {y})");
    assert!(
        (w - 960.0).abs() < 0.05 && (h - 540.0).abs() < 0.05,
        "a full-bleed shape came back as {w}x{h}, not the model's 960x540"
    );
}

/// A slide's name survives a snapshot, in both formats.
///
/// `Slide::title` is the slide's name — `draw:page/@draw:name` in ODF,
/// `p:cSld/@name` in OOXML. The odp writer had always written it; the pptx
/// writer wrote the slide's `p:cSld` bare while setting `name` on the
/// master's and the layout's a few lines above, and the pptx reader
/// synthesised `format!("Slide {n}")` without ever looking for the real
/// one. So a deck's slide names survived a .odp save and were destroyed by
/// a .pptx one — and pptx is the format an unsaved deck is snapshotted in,
/// so crash recovery lost every one of them.
///
/// Nothing caught it, and the reason is the fixture: `slide_of` sets
/// `title: String::new()`, so every test in this file asked whether an
/// *empty* name survived — a question with the same answer whether the
/// writer carries names or not. Fourth time this session (after the
/// single-run styling test, #725's master mapping, and the 16:9-only
/// geometry fixtures) that a fixture too simple to tell right from wrong
/// passed against a live bug.
#[test]
fn a_slides_name_survives_a_snapshot() {
    for kind in FORMATS {
        let mut slide = slide_of(vec![text_box("body", 10.0, 10.0)], "", "");
        slide.title = "Quarterly Review".into();
        let deck = deck_of(vec![slide]);
        let back = through_a_snapshot(&deck, kind, "slidename");
        assert_eq!(
            back.slides[0].title, "Quarterly Review",
            "{kind}: the slide's name was replaced or dropped"
        );
    }
}

/// A name needing XML escaping survives as itself.
///
/// It goes into an attribute in both formats, so a writer that interpolates
/// it raw produces a package that either fails to parse or silently
/// truncates the name at the quote.
#[test]
fn a_slide_name_needing_escaping_survives() {
    for kind in FORMATS {
        let mut slide = slide_of(vec![text_box("body", 10.0, 10.0)], "", "");
        slide.title = r#"R&D <draft> "v2""#.into();
        let deck = deck_of(vec![slide]);
        let back = through_a_snapshot(&deck, kind, "slidenameesc");
        assert_eq!(
            back.slides[0].title, r#"R&D <draft> "v2""#,
            "{kind}: an escaped name came back changed"
        );
    }
}

/// A deck whose slides carry no name still gets a usable label.
///
/// The positional fallback is what made the loss invisible, so it has to
/// stay for foreign decks that genuinely name nothing — but only when
/// there is no name to read.
#[test]
fn an_unnamed_slide_falls_back_to_its_position() {
    for kind in FORMATS {
        // slide_of leaves the title empty, which is the unnamed case.
        let deck = deck_of(vec![
            slide_of(vec![text_box("one", 10.0, 10.0)], "", ""),
            slide_of(vec![text_box("two", 10.0, 10.0)], "", ""),
        ]);
        let back = through_a_snapshot(&deck, kind, "unnamed");
        assert_eq!(back.slides.len(), 2, "{kind}: lost a slide");
        for (i, s) in back.slides.iter().enumerate() {
            assert!(
                !s.title.trim().is_empty(),
                "{kind}: slide {i} came back with no usable label"
            );
        }
    }
}

/// Placeholders without an `a:xfrm` of their own (what PowerPoint and
/// python-pptx write for a slide built on a layout) take their geometry
/// from the layout, then the master. Reading the missing xfrm as zero put
/// the title and subtitle at the slide's corner with no width, and the
/// canvas drew them one character per line (render lab `decks/title-layout`).
/// An empty placeholder is an editing prompt, not content, and isn't drawn.
#[test]
fn slide_placeholders_inherit_geometry_from_layout_and_master() {
    use std::io::{Cursor, Write};
    const P: &str = "xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" \
                     xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
                     xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"";
    let rels = |rels: &[(&str, &str)]| {
        let body: String = rels
            .iter()
            .enumerate()
            .map(|(i, (kind, target))| {
                format!(
                    "<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}\" Target=\"{target}\"/>",
                    i + 1
                )
            })
            .collect();
        format!("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{body}</Relationships>")
    };
    let sp = |ph: &str, xfrm: &str, text: &str| {
        let body = if text.is_empty() {
            "<a:p/>".to_string()
        } else {
            format!("<a:p><a:r><a:t>{text}</a:t></a:r></a:p>")
        };
        format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"s\"/><p:cNvSpPr/><p:nvPr>{ph}</p:nvPr></p:nvSpPr>\
             <p:spPr>{xfrm}</p:spPr><p:txBody><a:bodyPr/>{body}</p:txBody></p:sp>"
        )
    };
    let xfrm = |x: i64, y: i64, w: i64, h: i64| {
        format!("<a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{w}\" cy=\"{h}\"/></a:xfrm>")
    };
    let tree = |root: &str, shapes: String| format!("<p:{root} {P}><p:cSld><p:spTree>{shapes}</p:spTree></p:cSld></p:{root}>");

    let parts: Vec<(&str, String)> = vec![
        ("_rels/.rels", rels(&[("officeDocument", "ppt/presentation.xml")])),
        (
            "ppt/presentation.xml",
            format!("<p:presentation {P}><p:sldIdLst><p:sldId id=\"256\" r:id=\"rId1\"/></p:sldIdLst>\
                     <p:sldSz cx=\"12192000\" cy=\"6858000\"/></p:presentation>"),
        ),
        ("ppt/_rels/presentation.xml.rels", rels(&[("slide", "slides/slide1.xml")])),
        (
            "ppt/slides/slide1.xml",
            tree(
                "sld",
                sp("<p:ph type=\"ctrTitle\"/>", "", "Title Slide")
                    + &sp("<p:ph type=\"subTitle\" idx=\"1\"/>", "", "Subtitle text")
                    + &sp("<p:ph type=\"dt\" idx=\"10\"/>", "", ""),
            ),
        ),
        ("ppt/slides/_rels/slide1.xml.rels", rels(&[("slideLayout", "../slideLayouts/slideLayout1.xml")])),
        (
            "ppt/slideLayouts/slideLayout1.xml",
            // The layout places the title; the subtitle it leaves to the master.
            tree(
                "sldLayout",
                sp("<p:ph type=\"ctrTitle\"/>", &xfrm(914400, 2130425, 10363200, 1470025), "")
                    + &sp("<p:ph type=\"subTitle\" idx=\"1\"/>", "", ""),
            ),
        ),
        ("ppt/slideLayouts/_rels/slideLayout1.xml.rels", rels(&[("slideMaster", "../slideMasters/slideMaster1.xml")])),
        (
            "ppt/slideMasters/slideMaster1.xml",
            tree(
                "sldMaster",
                sp("<p:ph type=\"title\"/>", &xfrm(0, 0, 100, 100), "")
                    + &sp("<p:ph type=\"body\" idx=\"1\"/>", &xfrm(1828800, 3886200, 8534400, 1752600), ""),
            ),
        ),
    ];
    let mut buffer = Vec::new();
    {
        let mut w = zip::ZipWriter::new(Cursor::new(&mut buffer));
        for (name, body) in &parts {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(body.as_bytes()).unwrap();
        }
        w.finish().unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("layout.pptx");
    std::fs::write(&path, &buffer).unwrap();

    let deck = decks_core::read_deck(path.to_str().unwrap()).expect("read crafted pptx");
    let boxes: Vec<(&str, f64, f64, f64, f64)> = deck.slides[0]
        .objects
        .iter()
        .map(|o| match o {
            SlideObject::TextBox { text, x, y, w, h, .. } => (text.as_str(), *x, *y, *w, *h),
            other => panic!("unexpected object {other:?}"),
        })
        .collect();
    assert_eq!(boxes.len(), 2, "the empty date placeholder must not be drawn: {boxes:?}");
    // 12192000 EMU across maps to 960 model units.
    let k = 960.0 / 12192000.0;
    let close = |a: f64, b: f64| (a - b).abs() < 0.01;
    let (t, x, y, w, h) = boxes[0];
    assert_eq!(t, "Title Slide");
    assert!(
        close(x, 914400.0 * k) && close(y, 2130425.0 * k) && close(w, 10363200.0 * k) && close(h, 1470025.0 * k),
        "title should sit where the layout puts it, got {:?}",
        (x, y, w, h)
    );
    let (t, x, y, w, h) = boxes[1];
    assert_eq!(t, "Subtitle text");
    assert!(
        close(x, 1828800.0 * k) && close(y, 3886200.0 * k) && close(w, 8534400.0 * k) && close(h, 1752600.0 * k),
        "subtitle should fall through to the master's body, got {:?}",
        (x, y, w, h)
    );
}

/// A one-slide pptx built from raw parts: the slide's, layout's and
/// master's `p:spTree` contents, and anything the master carries after its
/// `p:cSld` (its `p:txStyles`).
fn crafted_pptx(dir: &std::path::Path, slide: &str, layout: &str, master: &str, master_tail: &str) -> std::path::PathBuf {
    use std::io::{Cursor, Write};
    const P: &str = "xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" \
                     xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
                     xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"";
    let rels = |kind: &str, target: &str| {
        format!(
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
             <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}\" Target=\"{target}\"/></Relationships>"
        )
    };
    let tree = |root: &str, shapes: &str, tail: &str| {
        format!("<p:{root} {P}><p:cSld><p:spTree>{shapes}</p:spTree></p:cSld>{tail}</p:{root}>")
    };
    let parts: Vec<(&str, String)> = vec![
        ("_rels/.rels", rels("officeDocument", "ppt/presentation.xml")),
        (
            "ppt/presentation.xml",
            format!("<p:presentation {P}><p:sldIdLst><p:sldId id=\"256\" r:id=\"rId1\"/></p:sldIdLst>\
                     <p:sldSz cx=\"9144000\" cy=\"5143500\"/></p:presentation>"),
        ),
        ("ppt/_rels/presentation.xml.rels", rels("slide", "slides/slide1.xml")),
        ("ppt/slides/slide1.xml", tree("sld", slide, "")),
        ("ppt/slides/_rels/slide1.xml.rels", rels("slideLayout", "../slideLayouts/slideLayout1.xml")),
        ("ppt/slideLayouts/slideLayout1.xml", tree("sldLayout", layout, "")),
        ("ppt/slideLayouts/_rels/slideLayout1.xml.rels", rels("slideMaster", "../slideMasters/slideMaster1.xml")),
        ("ppt/slideMasters/slideMaster1.xml", tree("sldMaster", master, master_tail)),
    ];
    let mut buffer = Vec::new();
    {
        let mut w = zip::ZipWriter::new(Cursor::new(&mut buffer));
        for (name, body) in &parts {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(body.as_bytes()).unwrap();
        }
        w.finish().unwrap();
    }
    let path = dir.join("crafted.pptx");
    std::fs::write(&path, &buffer).unwrap();
    path
}

/// A placeholder's paragraphs take what its layout and master say about
/// them: python-pptx's default template centres the subtitle in grey and
/// gives the body bullets on hanging indents, 32 and 28 pt by level. None of
/// it is on the slide, and all of it used to be read as 18 pt left-aligned
/// plain text (render lab `decks/title-layout`, `decks/bullets`).
#[test]
fn placeholder_paragraphs_inherit_their_layout_and_master_styles() {
    use decks_core::engine::{Anchor, Bullet, ParaAlign};
    let sp = |ph: &str, xfrm: &str, lst: &str, paras: &str| {
        format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"s\"/><p:cNvSpPr/><p:nvPr>{ph}</p:nvPr></p:nvSpPr>\
             <p:spPr>{xfrm}</p:spPr><p:txBody><a:bodyPr/><a:lstStyle>{lst}</a:lstStyle>{paras}</p:txBody></p:sp>"
        )
    };
    let xfrm = "<a:xfrm><a:off x=\"457200\" y=\"457200\"/><a:ext cx=\"8229600\" cy=\"2286000\"/></a:xfrm>";
    let slide = sp("<p:ph type=\"subTitle\" idx=\"1\"/>", "", "", "<a:p><a:r><a:t>Sub</a:t></a:r></a:p>")
        + &sp(
            "<p:ph idx=\"2\"/>",
            xfrm,
            "",
            // A leading empty paragraph (dropped), two levels, and a run
            // that overrides its size.
            "<a:p/><a:p><a:r><a:t>One</a:t></a:r></a:p>\
             <a:p><a:pPr lvl=\"1\"/><a:r><a:rPr sz=\"1000\"/><a:t>Two</a:t></a:r></a:p>",
        );
    let layout = sp(
        "<p:ph type=\"subTitle\" idx=\"1\"/>",
        xfrm,
        "<a:lvl1pPr marL=\"0\" indent=\"0\" algn=\"ctr\"><a:buNone/><a:defRPr>\
         <a:solidFill><a:schemeClr val=\"tx1\"><a:tint val=\"75000\"/></a:schemeClr></a:solidFill></a:defRPr></a:lvl1pPr>",
        "<a:p/>",
    ) + &sp("<p:ph idx=\"2\"/>", xfrm, "", "<a:p/>");
    let master = format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"b\"/><p:cNvSpPr/><p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr>\
         <p:spPr>{xfrm}</p:spPr><p:txBody><a:bodyPr anchor=\"b\"/><a:lstStyle/><a:p/></p:txBody></p:sp>"
    );
    let tx_styles = "<p:txStyles><p:bodyStyle>\
        <a:lvl1pPr marL=\"342900\" indent=\"-342900\" algn=\"l\"><a:buChar char=\"•\"/><a:defRPr sz=\"3200\"/></a:lvl1pPr>\
        <a:lvl2pPr marL=\"742950\" indent=\"-285750\" algn=\"l\"><a:buChar char=\"–\"/><a:defRPr sz=\"2800\"/></a:lvl2pPr>\
        </p:bodyStyle></p:txStyles>";
    let dir = tempfile::tempdir().unwrap();
    let path = crafted_pptx(dir.path(), &slide, &layout, &master, tx_styles);
    let deck = decks_core::read_deck(path.to_str().unwrap()).expect("read crafted pptx");
    let boxes: Vec<_> = deck.slides[0]
        .objects
        .iter()
        .filter_map(|o| match o {
            SlideObject::TextBox { text, runs, body, .. } => Some((text.clone(), runs.clone(), body.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(boxes.len(), 2, "{boxes:?}");

    let (text, runs, body) = &boxes[0];
    assert_eq!(text, "Sub");
    assert_eq!(body.paras[0].align, ParaAlign::Center);
    assert_eq!(body.paras[0].bullet, Bullet::None);
    assert_eq!(runs[0].style.font_size_hp, Some(64), "32 pt from the master's body style");
    assert!(
        runs[0].style.color.as_deref().is_some_and(|c| c != "000000"),
        "tx1 at a 75% tint is grey: {:?}",
        runs[0].style.color
    );
    assert_eq!(body.anchor, Anchor::Bottom, "the master placeholder's anchor");

    let (text, runs, body) = &boxes[1];
    assert_eq!(text, "One\nTwo");
    assert_eq!(body.paras.len(), 2, "one style per line, the dropped empty paragraph excluded");
    assert_eq!((body.paras[0].level, &body.paras[0].bullet), (0, &Bullet::Char("•".into())));
    assert_eq!((body.paras[1].level, &body.paras[1].bullet), (1, &Bullet::Char("–".into())));
    assert!((body.paras[0].margin_left - 36.0).abs() < 1e-9 && (body.paras[0].indent + 36.0).abs() < 1e-9);
    assert_eq!(runs.iter().map(|r| r.style.font_size_hp).collect::<Vec<_>>(), [Some(64), Some(20)]);
}

/// Paragraph styles, anchor and insets survive our own round trip, in both
/// formats. ODF has no paragraph margin relative to the line height, so
/// `Spacing::Lines` survives pptx only.
#[test]
fn paragraph_styles_survive_a_snapshot() {
    use decks_core::engine::{Anchor, Bullet, Insets, ParaAlign, ParaStyle, Spacing, TextBody};
    let body = TextBody {
        paras: vec![
            ParaStyle { align: ParaAlign::Center, ..Default::default() },
            ParaStyle {
                level: 1,
                bullet: Bullet::Char("•".into()),
                margin_left: 36.0,
                indent: -36.0,
                space_before: Spacing::Lines(0.2),
                space_after: Spacing::Units(8.0),
                marker: decks_core::engine::MarkerStyle {
                    font: Some("Arial".into()),
                    size: Some(decks_core::engine::MarkerSize::Relative(0.75)),
                    color: Some("c00000".into()),
                },
                ..Default::default()
            },
            ParaStyle { bullet: Bullet::AutoNum { scheme: "alphaLcParenR".into(), start: 3 }, ..Default::default() },
        ],
        anchor: Anchor::Middle,
        insets: Some(Insets { left: 9.6, top: 4.8, right: 9.6, bottom: 4.8 }),
        autofit: Some(decks_core::engine::Autofit { font_scale: 0.625, line_reduction: 0.2 }),
    };
    let deck = deck_of(vec![slide_of(
        vec![SlideObject::TextBox {
            text: "a\nb\nc".into(),
            x: 10.0, y: 10.0, w: 300.0, h: 200.0,
            rotation: 0.0,
            runs: vec![],
            body: body.clone(),
        }],
        "",
        "#ffffff",
    )]);
    for kind in FORMATS {
        let back = through_a_snapshot(&deck, kind, "para-styles");
        let SlideObject::TextBox { body: got, text, .. } = &back.slides[0].objects[0] else {
            panic!("{kind}: not a text box")
        };
        assert_eq!(text, "a\nb\nc", "{kind}");
        assert_eq!(got.paras.len(), 3, "{kind}: {got:?}");
        assert_eq!(got.anchor, body.anchor, "{kind}");
        // ODF's shrink-to-fit has no stated scale; only pptx keeps it.
        if kind == "pptx" {
            assert_eq!(got.autofit, body.autofit);
        }
        for (a, b) in got.paras.iter().zip(&body.paras) {
            assert_eq!((a.align, a.level, &a.bullet, &a.marker), (b.align, b.level, &b.bullet, &b.marker), "{kind}");
            assert!((a.margin_left - b.margin_left).abs() < 1e-3 && (a.indent - b.indent).abs() < 1e-3, "{kind}: {a:?}");
            let units = |s: Spacing| match s {
                Spacing::Units(u) => Some(u),
                Spacing::Lines(_) => None,
            };
            match (kind, b.space_before) {
                ("odp", Spacing::Lines(_)) => assert_eq!(a.space_before, Spacing::Units(0.0), "odp: lines are dropped"),
                _ => assert_eq!(a.space_before, b.space_before, "{kind}"),
            }
            assert!((units(a.space_after).unwrap() - units(b.space_after).unwrap()).abs() < 0.02, "{kind}");
        }
        let (gi, bi) = (got.insets.unwrap(), body.insets.unwrap());
        assert!((gi.left - bi.left).abs() < 1e-3 && (gi.top - bi.top).abs() < 1e-3, "{kind}");
    }
}

/// A run's own typeface survives our round trip in both formats. It was
/// neither read, drawn nor written, so every run came back in the master's
/// font.
#[test]
fn a_runs_font_family_survives_a_snapshot() {
    let runs = vec![
        Run { text: "serif ".into(), style: RunStyle { font_family: Some("Liberation Serif".into()), ..RunStyle::default() } },
        Run { text: "plain".into(), style: RunStyle::default() },
    ];
    let deck = deck_of(vec![slide_of(
        vec![SlideObject::TextBox {
            text: "serif plain".into(),
            x: 10.0, y: 10.0, w: 300.0, h: 80.0,
            rotation: 0.0,
            runs,
            body: Default::default(),
        }],
        "",
        "#ffffff",
    )]);
    for kind in FORMATS {
        let back = through_a_snapshot(&deck, kind, "font-family");
        let SlideObject::TextBox { runs, .. } = &back.slides[0].objects[0] else { panic!("{kind}: not a text box") };
        let families: Vec<Option<&str>> = runs.iter().map(|r| r.style.font_family.as_deref()).collect();
        assert_eq!(families.first().copied().flatten(), Some("Liberation Serif"), "{kind}: {runs:?}");
        assert!(families.last().copied().flatten() != Some("Liberation Serif"), "{kind}: {runs:?}");
    }
}
