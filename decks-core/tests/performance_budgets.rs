//! Performance budgets for an image-heavy deck (#1208).
//!
//! performance-accessibility.md row 1 asks for image-heavy decks among the
//! representative documents, and row 2 for p50/p95 budgets. Pictures are
//! what makes a real deck heavy: each slide here carries a title, a text
//! box and a ~360 KB picture (400×300, uncompressed, so the bytes are what
//! a photo costs), and the deck is saved and reopened as PPTX and ODP.
//!
//! Budgets are set well above what a CI runner takes, so they catch
//! accidental per-picture work, not noise. Measured on the development
//! container (debug build, 5 samples) when they were set, p50 for the
//! 30-slide deck: PPTX save 0.08 s, open 0.12 s; ODP save 0.07 s, open
//! 0.08 s.
//!
//! Writing this found the PPTX writer deflating every picture again: a
//! 30-slide save took 2.8 s against ODP's 0.07 s. Pictures are stored now,
//! as ODP's already were, and the 1.5 s budget fails the old behaviour.

use std::hint::black_box;
use std::time::{Duration, Instant};

use decks_core::engine::{Deck, SlideObject};

const SAMPLES: usize = 5;

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// A w×h RGB PNG whose image data is stored uncompressed, so its size on
/// disk is its pixel count: the weight of a photograph, without an encoder.
fn picture(w: u32, h: u32, seed: u32) -> Vec<u8> {
    let mut raw = Vec::with_capacity(((w * 3 + 1) * h) as usize);
    let mut state = seed.wrapping_mul(2_654_435_761).max(1);
    for _ in 0..h {
        raw.extend([0u8]); // each row starts with its filter byte: none
        for _ in 0..w * 3 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            raw.push(state as u8);
        }
    }
    // zlib stream of stored deflate blocks, then Adler-32.
    let mut z = vec![0x78, 0x01];
    for (i, block) in raw.chunks(65_535).enumerate() {
        let last = (i + 1) * 65_535 >= raw.len();
        z.push(u8::from(last));
        let len = block.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        png.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        png.extend_from_slice(&body);
        png.extend_from_slice(&crc32(&body).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    png
}

fn deck(slides: usize, dir: &std::path::Path) -> Deck {
    let mut d = Deck::new();
    let proto = d.slides[0].clone();
    d.slides = (0..slides)
        .map(|i| {
            let path = dir.join(format!("photo-{i}.png"));
            std::fs::write(&path, picture(400, 300, i as u32 + 1)).unwrap();
            let mut s = proto.clone();
            s.title = format!("Slide {}", i + 1);
            s.objects = vec![
                SlideObject::TextBox {
                    text: format!("Point {} of the talk", i + 1),
                    x: 60.0,
                    y: 40.0,
                    w: 500.0,
                    h: 60.0,
                    rotation: 0.0,
                    runs: vec![],
                    body: Default::default(),
                },
                SlideObject::Image { path: path.to_string_lossy().into(), x: 60.0, y: 120.0, w: 400.0, h: 300.0, rotation: 0.0 },
            ];
            s
        })
        .collect();
    d
}

fn measure<F: FnMut()>(name: &str, budget: Duration, mut operation: F) {
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        operation();
        samples.push(start.elapsed());
    }
    samples.sort_unstable();
    let p50 = samples[SAMPLES / 2];
    let p95 = samples[(SAMPLES * 95).div_ceil(100) - 1];
    eprintln!("{name}: p50={p50:?} p95={p95:?} budget={budget:?} samples={samples:?}");
    assert!(p95 <= budget, "{name}: p95 {p95:?} is over its {budget:?} budget; samples={samples:?}");
}

fn pictures(d: &Deck) -> usize {
    d.slides.iter().flat_map(|s| &s.objects).filter(|o| matches!(o, SlideObject::Image { .. })).count()
}

#[test]
fn an_image_heavy_deck_saves_and_opens_within_budget() {
    let dir = tempfile::tempdir().unwrap();
    for (slides, save_budget, open_budget) in [(5, 1_000, 1_000), (30, 1_500, 2_000)] {
        let d = deck(slides, dir.path());
        for format in ["pptx", "odp"] {
            let path = dir.path().join(format!("deck-{slides}.{format}"));
            let path = path.to_str().unwrap();
            measure(&format!("{slides}-slide {format} save"), Duration::from_millis(save_budget), || {
                decks_core::write_deck(path, &d).expect("save");
            });
            measure(&format!("{slides}-slide {format} open"), Duration::from_millis(open_budget), || {
                let back = decks_core::read_deck(path).expect("open");
                assert_eq!(pictures(&back), slides, "{format} lost pictures");
                black_box(back);
            });
        }
    }
}
