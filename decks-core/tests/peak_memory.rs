//! Peak memory budgets for an image-heavy deck (#1208).
//!
//! performance-accessibility.md row 2 asks for peak memory beside latency.
//! The 30-slide deck from the performance budgets (a ~360 KB picture per
//! slide, ~11 MB of pictures) is saved and reopened as PPTX and ODP, and
//! each operation's peak heap above what was live before it started is
//! printed and held to a budget. One test, so the harness's parallel
//! threads cannot add a neighbour's allocations to a measurement.
//!
//! Budgets are two to three times what the development container measured
//! (debug build) when they were set: PPTX save 12 MB and open 0.6 MB, ODP
//! save 21 MB and open 1 MB. Opening is small because pictures are not
//! decoded or held in the model; a reader that started keeping every
//! picture's bytes would cost ~11 MB here and fail.

use std::hint::black_box;

use suite_common_core::peak_heap::{peak_during, CountingAlloc};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

mod support;
use support::{deck, pictures};

const MB: usize = 1 << 20;

fn within(name: &str, budget_mb: usize, peak: usize) {
    eprintln!("{name}: peak heap {:.1} MB, budget {budget_mb} MB", peak as f64 / MB as f64);
    assert!(peak <= budget_mb * MB, "{name}: peak heap {peak} bytes is over its {budget_mb} MB budget");
}

#[test]
fn an_image_heavy_deck_saves_and_opens_within_its_memory_budget() {
    let dir = tempfile::tempdir().unwrap();
    let d = deck(30, dir.path());
    for (format, save_mb, open_mb) in [("pptx", 48, 16), ("odp", 64, 16)] {
        let path = dir.path().join(format!("deck.{format}"));
        let path = path.to_str().unwrap();
        let ((), peak) = peak_during(|| decks_core::write_deck(path, &d).expect("save"));
        within(&format!("30-slide {format} save"), save_mb, peak);
        let (back, peak) = peak_during(|| decks_core::read_deck(path).expect("open"));
        within(&format!("30-slide {format} open"), open_mb, peak);
        assert_eq!(pictures(&back), 30, "{format} lost pictures");
        black_box(back);
    }
}
