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

#[test]
fn rewriting_the_fuzz_seed_decks_gives_consistent_packages() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fuzz/corpus");
    let dir = tempfile::tempdir().unwrap();
    let mut checked = 0;
    for (sub, ext) in [("decks_pptx", "pptx"), ("decks_odp", "odp")] {
        for entry in std::fs::read_dir(root.join(sub)).unwrap() {
            let src = entry.unwrap().path();
            // Seeds that aren't decks (the corpus holds edge cases).
            if src.extension().and_then(|e| e.to_str()) != Some(ext) {
                continue;
            }
            let Ok(deck) = decks_core::read_deck(src.to_str().unwrap()) else { continue };
            let out = dir.path().join(format!("{}.{ext}", src.file_stem().unwrap().to_string_lossy()));
            decks_core::write_deck(out.to_str().unwrap(), &deck).unwrap();
            assert_eq!(suite_common_core::carry::problems(&out), Vec::<String>::new(), "{}", src.display());
            checked += 1;
        }
    }
    assert!(checked >= 3, "only {checked} seed decks read");
}
