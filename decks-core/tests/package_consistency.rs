//! The packages Decks writes are consistent (#1274): every part has a
//! content type or a manifest entry, and nothing relates to a part that
//! isn't there. `suite_common_core::carry::problems` is the check.

use decks_core::engine::{Deck, SlideObject};

/// A valid 3x2 PNG.
const PNG: [u8; 78] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00,
    0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0x12, 0x16, 0xf1, 0x4d, 0x00, 0x00, 0x00, 0x15, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63,
    0x3c, 0x21, 0x27, 0xc7, 0xc0, 0xc0, 0xc0, 0xc0, 0xc0, 0xc0, 0xc4, 0x00, 0x03, 0x00, 0x13, 0x2e, 0x01, 0x08, 0x6a, 0xc0, 0x65, 0x61,
    0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

#[test]
fn pptx_and_odp_packages_are_consistent() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("dot.png");
    std::fs::write(&png, PNG).unwrap();
    let mut deck = Deck::new();
    deck.slides[0].notes = "notes".into();
    deck.slides[0].objects.push(SlideObject::Image { path: png.to_string_lossy().into_owned(), x: 10.0, y: 10.0, w: 100.0, h: 60.0, rotation: 0.0, crop: Default::default() });
    deck.slides[0].objects.push(SlideObject::Rect { x: 50.0, y: 250.0, w: 200.0, h: 90.0, rotation: 0.0 });
    for ext in ["pptx", "odp"] {
        let path = dir.path().join(format!("deck.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &deck).unwrap();
        assert_eq!(suite_common_core::carry::problems(&path), Vec::<String>::new(), "{ext}");
    }
}

/// Decks of several shapes are written, read back and written again: the
/// second write is of a package Decks wrote itself, the case every later
/// save of a presentation is.
#[test]
fn rewriting_written_decks_gives_consistent_packages() {
    let dir = tempfile::tempdir().unwrap();
    let mut empty = Deck::new();
    empty.slides[0].objects.clear();
    let mut several = Deck::new();
    several.slides[0].notes = "first".into();
    several.slides[0].objects.push(SlideObject::Rect { x: 50.0, y: 250.0, w: 200.0, h: 90.0, rotation: 15.0 });
    let mut second = several.slides[0].clone();
    second.notes = "second".into();
    several.slides.push(second.clone());
    several.slides.push(second);
    for (name, deck) in [("empty", empty), ("several", several)] {
        for ext in ["pptx", "odp"] {
            let first = dir.path().join(format!("{name}-0.{ext}"));
            decks_core::write_deck(first.to_str().unwrap(), &deck).unwrap();
            assert_eq!(suite_common_core::carry::problems(&first), Vec::<String>::new(), "{name}.{ext}");
            let reread = decks_core::read_deck(first.to_str().unwrap()).unwrap();
            let again = dir.path().join(format!("{name}-1.{ext}"));
            decks_core::write_deck(again.to_str().unwrap(), &reread).unwrap();
            assert_eq!(suite_common_core::carry::problems(&again), Vec::<String>::new(), "{name}.{ext} rewritten");
        }
    }
}

/// A deck Decks wrote — including its single-series chart — is consistent
/// and loses nothing (DCK-001 inventory row 9): the next save must not ask.
#[test]
fn a_written_single_series_chart_loses_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut deck = Deck::new();
    deck.slides[0].objects.push(SlideObject::Chart {
        x: 10.0,
        y: 10.0,
        w: 300.0,
        h: 200.0,
        rotation: 0.0,
        chart: decks_core::engine::chart::ChartData::sample(suite_common_core::charts::ChartKind::Bar),
    });
    for ext in ["pptx", "odp"] {
        let path = dir.path().join(format!("chart.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &deck).unwrap();
        assert_eq!(suite_common_core::carry::problems(&path), Vec::<String>::new(), "{ext}");
        let report = decks_core::loss::content_a_save_drops(path.to_str().unwrap());
        assert!(report.features.is_empty(), "{ext}: {:?}", report.features);
        assert!(!report.requires_confirmation());
    }
}

/// A package holding safe-family parts the model never reads, opened with
/// its provenance, edited on an unrelated slide and saved through the
/// single staged transaction: the parts survive byte for byte and the
/// saved package's content types and relationships agree (DCK-002).
/// Foreign bytes stand in for what our writer never emits.
fn pptx_with_unmodeled_safe_parts(
    dir: &tempfile::TempDir,
    name: &str,
) -> (std::path::PathBuf, Vec<(String, Vec<u8>)>) {
    use std::io::Write;
    let mut deck = Deck::new();
    deck.slides[0].objects.clear();
    let path = dir.path().join(name);
    decks_core::write_deck(path.to_str().unwrap(), &deck).unwrap();
    let parts = vec![
        ("customXml/item1.xml".to_string(), b"<data>kept</data>".to_vec()),
        ("docProps/thumbnail.jpeg".to_string(), b"preview".to_vec()),
    ];
    {
        let file = std::fs::File::options()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let mut zip = zip::ZipWriter::new_append(file).unwrap();
        let options = zip::write::SimpleFileOptions::default();
        for (name, body) in &parts {
            zip.start_file(name, options).unwrap();
            zip.write_all(body).unwrap();
        }
        zip.finish().unwrap();
    }
    (path, parts)
}

#[test]
fn opaque_dependency_survives_unrelated_edit() {
    let dir = tempfile::tempdir().unwrap();
    let (path, parts) = pptx_with_unmodeled_safe_parts(&dir, "opaque.pptx");
    let (deck, context) = decks_core::read_deck_with_context(path.to_str().unwrap()).unwrap();
    let controller = decks_core::DecksController::new(vec![], vec![]);
    controller.install_opened_deck(deck, context);
    // An unrelated edit: a new slide, touching nothing the parts refer to.
    controller.add_slide(
        1,
        decks_core::engine::Slide { hidden: false,
            title: "added".into(),
            background: "#ffffff".into(),
            background_picture: None,
            objects: vec![],
            notes: String::new(),
            master_id: None,
            transition: Default::default(),
            builds: vec![],
            ids: Default::default(),
            layout_id: None,
        },
    );
    let deck = controller.deck();
    decks_core::write_deck_with_context(
        path.to_str().unwrap(),
        &deck,
        &controller.carried_for_save(),
    )
    .unwrap();
    assert_eq!(
        suite_common_core::carry::problems(&path),
        Vec::<String>::new()
    );
    let reread = std::fs::read(&path).unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(reread)).unwrap();
    for (name, body) in &parts {
        let mut buf = Vec::new();
        use std::io::Read;
        archive
            .by_name(name)
            .unwrap()
            .read_to_end(&mut buf)
            .unwrap();
        assert_eq!(&buf, body, "{name} was not preserved byte for byte");
    }
    // And the edited model round-trips through the same file.
    let back = decks_core::read_deck(path.to_str().unwrap()).unwrap();
    assert_eq!(back.slides.len(), 2);
}

#[test]
fn deleting_target_cannot_leave_dangling_relationship() {
    let dir = tempfile::tempdir().unwrap();
    let png = dir.path().join("dot.png");
    std::fs::write(&png, PNG).unwrap();
    let mut deck = Deck::new();
    deck.slides[0].objects.push(SlideObject::Image {
        path: png.to_string_lossy().into_owned(),
        x: 10.0,
        y: 10.0,
        w: 100.0,
        h: 60.0,
        rotation: 0.0,
        crop: Default::default(),
    });
    deck.slides.push(deck.slides[0].clone());
    let path = dir.path().join("img.pptx");
    decks_core::write_deck(path.to_str().unwrap(), &deck).unwrap();
    let (deck, context) = decks_core::read_deck_with_context(path.to_str().unwrap()).unwrap();
    let controller = decks_core::DecksController::new(vec![], vec![]);
    controller.install_opened_deck(deck, context);
    // Delete the slide holding the image reference: the save must not
    // leave a relationship pointing at a part that is no longer there.
    assert!(controller.delete_slide(controller.slide_id_at(0).unwrap()));
    let out = dir.path().join("img-deleted.pptx");
    let deck = controller.deck();
    decks_core::write_deck_with_context(
        out.to_str().unwrap(),
        &deck,
        &controller.carried_for_save(),
    )
    .unwrap();
    assert_eq!(
        suite_common_core::carry::problems(&out),
        Vec::<String>::new()
    );
    decks_core::read_deck(out.to_str().unwrap()).unwrap();
}

#[test]
fn external_replacement_requires_copy() {
    let dir = tempfile::tempdir().unwrap();
    let (path, parts) = pptx_with_unmodeled_safe_parts(&dir, "orig.pptx");
    let (_, context) = decks_core::read_deck_with_context(path.to_str().unwrap()).unwrap();
    // Another writer replaces the source while it is open: the opened
    // revision's parts must never ride into a save of the replacement.
    let mut deck = Deck::new();
    deck.slides[0].objects.clear();
    decks_core::write_deck(path.to_str().unwrap(), &deck).unwrap();
    assert_eq!(context.source_state(), decks_core::SourceState::Replaced);
    assert!(context.carried_for_save().is_empty());
    // ... but the context still names what is at stake, for the dialog.
    assert_eq!(
        context.imported_carried_names(),
        parts.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>()
    );
    // The unverified-copy gate the dialog re-checks at confirm time: a
    // fresh destination passes, one created while the question was open
    // (or the source itself) does not.
    let fresh = dir.path().join("copy.pptx");
    let source = path.to_str().unwrap();
    let target = fresh.to_str().unwrap();
    assert!(decks_core::loss::can_save_unverified_copy(source, target));
    assert!(!decks_core::loss::can_save_unverified_copy(source, source));
    std::fs::write(&fresh, b"taken").unwrap();
    assert!(!decks_core::loss::can_save_unverified_copy(source, target));
}

#[test]
fn hostile_part_names_block_the_save() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let mut deck = Deck::new();
    deck.slides[0].objects.clear();
    let path = dir.path().join("hostile.pptx");
    decks_core::write_deck(path.to_str().unwrap(), &deck).unwrap();
    {
        let file = std::fs::File::options()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let mut zip = zip::ZipWriter::new_append(file).unwrap();
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("customXml/../evil.xml", options).unwrap();
        zip.write_all(b"<evil/>").unwrap();
        zip.finish().unwrap();
    }
    let (deck, context) = decks_core::read_deck_with_context(path.to_str().unwrap()).unwrap();
    // The boundary blocks instead of guessing: nothing is written.
    let before = std::fs::read(&path).unwrap();
    let err = decks_core::write_deck_with_context(
        path.to_str().unwrap(),
        &deck,
        &context.carried_for_save(),
    )
    .expect_err("hostile part was carried");
    assert!(err.contains("not a safe relative path"), "{err}");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "a blocked save wrote anyway"
    );
}
