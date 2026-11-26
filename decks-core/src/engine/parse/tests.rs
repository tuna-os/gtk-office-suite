// SPDX-License-Identifier: GPL-3.0-or-later
// Tests moved out of parse.rs (#1668).

use super::*;
use crate::engine::*;

/// Reopening a pptx reuses the picture it already unpacked instead of
/// leaving another temp file per picture per open (#455).
#[test]
fn reopening_a_pptx_does_not_unpack_its_picture_again() {
    let dir = tempfile::tempdir().unwrap();
    let png: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D',
        b'R', 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0, 0x90, 0x77, 0x53, 0xde, 0, 0, 0,
        12, b'I', b'D', b'A', b'T', 0x08, 0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0, 0, 0, 3, 0,
        1, 0x9e, 0xdd, 0x22, 0x71, 0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42,
        0x60, 0x82,
    ];
    let src = dir.path().join("pic.png");
    std::fs::write(&src, png).unwrap();
    let mut deck = Deck::new();
    deck.slides[0].objects.push(SlideObject::Image {
        path: src.to_string_lossy().to_string(),
        x: 10.0, y: 20.0, w: 30.0, h: 40.0,
        rotation: 0.0, crop: Default::default()
    });
    let path = dir.path().join("pic.pptx");
    write_pptx(path.to_str().unwrap(), &deck).unwrap();

    let unpacked = |deck: &Deck| {
        let found: Vec<String> = deck.slides[0].objects.iter().filter_map(|o| match o {
            SlideObject::Image { path, .. } => Some(path.clone()),
            _ => None,
        }).collect();
        assert_eq!(found.len(), 1, "expected one picture, got {found:?}");
        found[0].clone()
    };
    let first = unpacked(&read_pptx(path.to_str().unwrap()).unwrap());
    assert_eq!(std::fs::read(&first).unwrap(), png);
    for _ in 0..5 {
        assert_eq!(unpacked(&read_pptx(path.to_str().unwrap()).unwrap()), first);
    }
    let cache = suite_common_core::media_cache::process_dir().unwrap();
    assert!(Path::new(&first).starts_with(&cache), "{first} not in {cache:?}");
}

#[test]
fn test_pptx_roundtrip() {
    let mut deck = Deck::new();
    deck.slides[0].objects.push(SlideObject::TextBox {
        text: "Hello Slide".into(),
        x: 100.0, y: 100.0, w: 300.0, h: 50.0,
        runs: vec![],
        rotation: 0.0,
        body: Default::default(),
    });
    deck.slides[0].objects.push(SlideObject::Rect {
        x: 150.0, y: 200.0, w: 200.0, h: 100.0,
        rotation: 0.0,
    });
    deck.slides[0].objects.push(SlideObject::Circle {
        x: 400.0, y: 300.0, r: 50.0,
        rotation: 0.0,
    });

    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join("test_deck.pptx");
    let path_str = path.to_string_lossy();

    // Write
    let write_res = write_pptx(&path_str, &deck);
    assert!(write_res.is_ok(), "Write pptx failed: {:?}", write_res.err());

    // Read
    let read_res = read_pptx(&path_str);
    assert!(read_res.is_ok(), "Read pptx failed: {:?}", read_res.err());

    let read_deck = read_res.unwrap();
    assert_eq!(read_deck.slides.len(), 1);
    let slide = &read_deck.slides[0];
    assert_eq!(slide.objects.len(), 3);

    // Verify TextBox
    match &slide.objects[0] {
        SlideObject::TextBox { text, .. } => assert_eq!(text, "Hello Slide"),
        _ => panic!("Expected TextBox"),
    }

    // A rect and a circle come back as what the file holds: preset
    // shapes, in the colours they were written in.
    use crate::engine::shape::{Color, ShapeKind};
    match &slide.objects[1] {
        SlideObject::Shape { kind: ShapeKind::Rect, x, y, w, h, style, .. } => {
            assert!((x - 150.0).abs() < 0.1);
            assert!((y - 200.0).abs() < 0.1);
            assert!((w - 200.0).abs() < 0.1);
            assert!((h - 100.0).abs() < 0.1);
            assert_eq!(style.fill, Some(Color(0x4A, 0x90, 0xE2)));
        }
        other => panic!("Expected a rect shape, got {other:?}"),
    }

    match &slide.objects[2] {
        SlideObject::Shape { kind: ShapeKind::Ellipse, x, y, w, h, style, .. } => {
            // Circle (400, 300) r 50 is the box (350, 250, 100, 100).
            assert!((x - 350.0).abs() < 0.1 && (y - 250.0).abs() < 0.1);
            assert!((w - 100.0).abs() < 0.1 && (h - 100.0).abs() < 0.1);
            assert_eq!(style.fill, Some(Color(0xE0, 0x4F, 0x32)));
        }
        other => panic!("Expected an ellipse shape, got {other:?}"),
    }

    let _ = std::fs::remove_file(&path);
}
