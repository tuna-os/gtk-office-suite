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

mod support;
use support::{deck, pictures};

const SAMPLES: usize = 5;

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
