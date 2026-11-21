// unreadable_slides.rs — a slide part the presentation names but the
// package does not contain must not vanish silently (#1977).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The fixture is a deck this crate's own writer produced, repackaged
// without its second slide part — the shape a truncated download or a
// broken export actually arrives in. The reader skips the slide; what is
// under test is that the skip is *recorded*: on the deck, in the open
// report, and in the save-time loss check, under one shared id, so the
// user hears about it before a save makes the loss permanent.

use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;

use suite_common_core::interop::FeatureDisposition;

/// A two-slide PPTX written by this crate, as (bytes, slide part names).
fn two_slide_pptx() -> (Vec<u8>, Vec<String>) {
    let mut deck = decks_core::Deck::default();
    let second = deck.slides[0].clone();
    deck.slides.push(second);
    let bytes = decks_core::write_deck_bytes("pptx", &deck).expect("write a pptx");
    let zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut parts: Vec<String> = zip
        .file_names()
        .map(|n| n.expect("member name").into_owned())
        .filter(|n| n.starts_with("ppt/slides/") && n.ends_with(".xml"))
        .collect();
    parts.sort();
    assert_eq!(parts.len(), 2, "the writer must emit two slide parts: {parts:?}");
    (bytes, parts)
}

/// The same package with `drop_part` removed; everything else byte-identical.
fn without_part(bytes: &[u8], drop_part: &str) -> Vec<u8> {
    let mut src = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let names: Vec<String> = src.file_names().map(|n| n.expect("member name").into_owned()).collect();
    let mut out = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(Cursor::new(&mut out));
        for name in &names {
            if name == drop_part || name.ends_with('/') {
                continue;
            }
            let mut data = Vec::new();
            src.by_name(name).unwrap().read_to_end(&mut data).unwrap();
            writer.start_file(name, SimpleFileOptions::default()).unwrap();
            writer.write_all(&data).unwrap();
        }
        writer.finish().unwrap();
    }
    out
}

fn write_temp(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::Builder::new().suffix(".pptx").tempfile().unwrap();
    file.write_all(bytes).unwrap();
    file.flush().unwrap();
    file
}

#[test]
fn a_missing_slide_part_is_recorded_not_silently_dropped() {
    let (bytes, parts) = two_slide_pptx();
    let dropped = parts[1].clone();
    let doctored = without_part(&bytes, &dropped);
    let file = write_temp(&doctored);
    let path = file.path().to_str().unwrap();

    // The deck still opens, with the readable slide — and the skip named.
    let deck = decks_core::read_deck(path).expect("a missing slide part must not fail the open");
    assert_eq!(deck.slides.len(), 1, "only the readable slide is in the deck");
    assert_eq!(deck.skipped_slides.len(), 1, "the skip must be recorded: {:?}", deck.skipped_slides);
    assert_eq!(deck.skipped_slides[0].part, dropped);
    assert!(deck.skipped_slides[0].reason.contains("missing"), "reason: {}", deck.skipped_slides[0].reason);

    // The open report names the slide under the shared id, as a loss the
    // user can refuse.
    let (_, report, _) = decks_core::read_deck_with_report(path).expect("open with report");
    let finding = report
        .features
        .iter()
        .find(|f| f.id == "unreadable-slides")
        .expect("the open report must name the unreadable slide");
    assert_eq!(finding.location, dropped);
    assert_eq!(finding.disposition, FeatureDisposition::WarnOnLoss);

    // And the save-time loss check, which re-inspects the file on disk,
    // makes the same finding under the same id.
    let loss = decks_core::loss::content_a_save_drops(path);
    let loss_finding = loss
        .features
        .iter()
        .find(|f| f.id == "unreadable-slides")
        .expect("the loss check must name the unreadable slide");
    assert_eq!(loss_finding.location, dropped);
    assert_eq!(loss_finding.disposition, FeatureDisposition::WarnOnLoss);
}

#[test]
fn a_complete_package_records_no_skipped_slides() {
    let (bytes, _) = two_slide_pptx();
    let file = write_temp(&bytes);
    let path = file.path().to_str().unwrap();

    let deck = decks_core::read_deck(path).expect("open the complete deck");
    assert_eq!(deck.slides.len(), 2);
    assert!(deck.skipped_slides.is_empty(), "nothing was skipped: {:?}", deck.skipped_slides);

    let loss = decks_core::loss::content_a_save_drops(path);
    assert!(
        !loss.features.iter().any(|f| f.id == "unreadable-slides"),
        "a complete package must not warn about unreadable slides: {:?}",
        loss.features
    );
}
