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
        | SlideObject::Image { rotation, .. } => *rotation,
    };
    let anchor_of = |o: &SlideObject| match o {
        SlideObject::Rect { x, y, .. }
        | SlideObject::TextBox { x, y, .. }
        | SlideObject::Image { x, y, .. }
        | SlideObject::Circle { x, y, .. } => (*x, *y),
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
