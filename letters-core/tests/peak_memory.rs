//! Peak memory budgets for Letters' document paths (#1208).
//!
//! performance-accessibility.md row 2 asks for peak memory beside latency.
//! Each operation here runs on the large (5,000-paragraph) document from
//! the performance budgets, and its peak heap above what was live before
//! it started is printed and held to a budget. One test, so the harness's
//! parallel threads cannot add a neighbour's allocations to a measurement.
//!
//! Budgets are about twice what the development container measured
//! (debug build) when they were set: building the document 7 MB; DOCX save
//! 127 MB and open 89 MB; ODT save 4 MB and open 8 MB; Markdown parse 10 MB.
//! The DOCX save is the outlier: rdocx clones its document as paragraphs
//! go in, so it holds many times the model at once. The budget records
//! that as it is, so it can only get better.

use std::hint::black_box;

use letters_core::Document;
use suite_common_core::peak_heap::{peak_during, CountingAlloc};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

mod support;
use support::{document, LARGE};

const MB: usize = 1 << 20;

fn within(name: &str, budget_mb: usize, peak: usize) {
    eprintln!("{name}: peak heap {:.1} MB, budget {budget_mb} MB", peak as f64 / MB as f64);
    assert!(peak <= budget_mb * MB, "{name}: peak heap {peak} bytes is over its {budget_mb} MB budget");
}

#[test]
fn the_large_document_saves_and_opens_within_its_memory_budget() {
    let dir = tempfile::tempdir().unwrap();
    let (doc, peak) = peak_during(|| document(&LARGE));
    within("large build", 32, peak);
    type Write = fn(&Document, &std::path::Path) -> Result<(), String>;
    type Read = fn(&str) -> Result<Document, String>;
    let formats: [(&str, Write, Read, usize, usize); 2] = [
        ("docx", |d, p| letters_core::docx::write(d, p), letters_core::docx::read, 300, 200),
        ("odt", |d, p| letters_core::odt::write(d, p), letters_core::odt::read, 32, 32),
    ];
    for (format, write, read, save_mb, open_mb) in formats {
        let path = dir.path().join(format!("large.{format}"));
        let ((), peak) = peak_during(|| write(&doc, &path).expect("save"));
        within(&format!("large {format} save"), save_mb, peak);
        let (back, peak) = peak_during(|| read(path.to_str().unwrap()).expect("open"));
        within(&format!("large {format} open"), open_mb, peak);
        assert_eq!(back.paragraphs.len(), doc.paragraphs.len(), "{format} lost paragraphs");
        black_box(back);
    }
    let text = letters_core::markdown::serialize(&doc);
    let (back, peak) = peak_during(|| letters_core::markdown::parse(&text));
    within("large markdown parse", 48, peak);
    black_box(back);
}
