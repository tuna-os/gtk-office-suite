//! Performance budgets for Letters' document paths (#1208).
//!
//! performance-accessibility.md asks for representative small, medium and
//! large documents, p50/p95 latency with explicit budgets, and the sample
//! count and runtime recorded. Each measurement here prints its samples,
//! p50 and p95, and fails when p95 exceeds its budget. These are ordinary
//! integration tests in the PR lane, like tables-core's, so a change that
//! makes saving or typing in a long document slower can't merge unnoticed.
//!
//! Budgets are set well above what a CI runner takes, so they catch
//! accidental quadratic work, not noise. Measured on the development
//! container (debug build, 7 samples, the three tests in parallel) when
//! they were set, p50: large (5,000 paragraphs) DOCX save 3.8 s and open
//! 4.5 s, ODT save 0.3 s and open 0.3 s; 200 characters typed into it 0.9 s.
//!
//! Writing this found the DOCX save quadratic in list items: rdocx's
//! add_*_list_item clones the whole document on every call, so the large
//! document took 30 s to save. docx::write now allocates each list
//! definition once (#1208); the 15 s budget fails the old behaviour.

use std::hint::black_box;
use std::time::{Duration, Instant};

use letters_core::edit::{self, apply_all};
use letters_core::{Document, ListKind, ParaStyle, Paragraph, Run, RunStyle};

const SAMPLES: usize = 7;

struct Size {
    name: &'static str,
    paragraphs: usize,
}

const SMALL: Size = Size { name: "small", paragraphs: 20 };
const MEDIUM: Size = Size { name: "medium", paragraphs: 500 };
const LARGE: Size = Size { name: "large", paragraphs: 5_000 };

/// A document shaped like real writing: headings every twenty paragraphs,
/// mixed bold and italic runs, a bulleted list in each section.
fn document(size: &Size) -> Document {
    let mut doc = Document::default();
    doc.paragraphs.clear();
    for i in 0..size.paragraphs {
        let mut style = ParaStyle::default();
        let runs = if i % 20 == 0 {
            style.heading = Some(if i % 100 == 0 { 1 } else { 2 });
            vec![Run::plain(format!("Section {}", i / 20 + 1))]
        } else {
            if i % 20 >= 15 {
                style.list = ListKind::Bullet;
            }
            vec![
                Run::plain(format!("Paragraph {i} opens with plain text, ")),
                Run { text: "then a bold phrase".into(), style: RunStyle { bold: true, ..Default::default() } },
                Run::plain(", and closes with "),
                Run { text: "an italic aside.".into(), style: RunStyle { italic: true, ..Default::default() } },
            ]
        };
        doc.paragraphs.push(Paragraph { style, runs });
    }
    doc
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

fn budget(size: &Size, small_ms: u64, large_ms: u64) -> Duration {
    // Linear between the small and large budgets, by paragraph count.
    let t = (size.paragraphs as f64 - SMALL.paragraphs as f64) / (LARGE.paragraphs - SMALL.paragraphs) as f64;
    Duration::from_millis(small_ms + ((large_ms - small_ms) as f64 * t.clamp(0.0, 1.0)) as u64)
}

#[test]
fn docx_and_odt_save_and_open_stay_within_budget() {
    let dir = tempfile::tempdir().unwrap();
    for size in [&SMALL, &MEDIUM, &LARGE] {
        let doc = document(size);
        type Write = fn(&Document, &std::path::Path) -> Result<(), String>;
        type Read = fn(&str) -> Result<Document, String>;
        let formats: [(&str, Write, Read); 2] = [
            ("docx", |d, p| letters_core::docx::write(d, p), letters_core::docx::read),
            ("odt", |d, p| letters_core::odt::write(d, p), letters_core::odt::read),
        ];
        for (format, write, read) in formats {
            let path = dir.path().join(format!("{}.{format}", size.name));
            measure(&format!("{} {format} save", size.name), budget(size, 1_000, 15_000), || {
                write(&doc, &path).expect("save");
            });
            measure(&format!("{} {format} open", size.name), budget(size, 1_000, 15_000), || {
                let back = read(path.to_str().unwrap()).expect("open");
                assert_eq!(back.paragraphs.len(), doc.paragraphs.len(), "{format} lost paragraphs");
                black_box(back);
            });
        }
    }
}

#[test]
fn markdown_round_trip_stays_within_budget() {
    for size in [&SMALL, &MEDIUM, &LARGE] {
        let doc = document(size);
        let text = letters_core::markdown::serialize(&doc);
        measure(&format!("{} markdown serialize", size.name), budget(size, 100, 2_000), || {
            black_box(letters_core::markdown::serialize(&doc));
        });
        measure(&format!("{} markdown parse", size.name), budget(size, 100, 3_000), || {
            black_box(letters_core::markdown::parse(&text));
        });
    }
}

/// Typing is the latency a user feels most. 200 characters typed one at a
/// time near the end of the document, each through the model's edit op.
#[test]
fn typing_into_a_document_stays_within_budget() {
    for size in [&SMALL, &MEDIUM, &LARGE] {
        let base = document(size);
        measure(&format!("{} typing 200 characters", size.name), budget(size, 200, 3_000), || {
            let mut doc = base.clone();
            let start = edit::doc_len(&doc).saturating_sub(10);
            let typed = "the quick brown fox jumps over the lazy dog ".chars().cycle().take(200);
            for (at, c) in (start..).zip(typed) {
                let op = edit::typing(&doc, at, &c.to_string()).expect("typing op");
                apply_all(&mut doc, &[op]).expect("apply");
            }
            black_box(doc);
        });
    }
}
