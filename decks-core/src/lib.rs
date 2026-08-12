// decks-core — GTK-free presentation core for Decks.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Owns the Deck/Slide/SlideObject model, PPTX read/write, and undo
// commands. The `decks` binary renders (Cairo/Pango) and wires signals.

pub mod controller;
pub mod engine;
pub mod fragment;
pub mod odp;
pub mod presenter;
pub mod snapshot;
pub mod undo;

pub use controller::DecksController;
pub use engine::{Deck, MasterSlide, Slide, SlideObject};

/// Read a presentation, dispatching on extension (.pptx or .odp).
pub fn read_deck(path: &str) -> Result<engine::Deck, String> {
    if path.to_lowercase().ends_with(".odp") {
        odp::read(path)
    } else {
        engine::read_pptx(path)
    }
}

/// Read a deck while retaining unsupported package members for a safe save.
pub fn read_deck_with_report(path: &str) -> Result<(engine::Deck, suite_common_core::interop::CompatibilityReport, suite_common_core::interop::OpaquePackage), String> {
    let deck = read_deck(path)?;
    let format = if path.to_lowercase().ends_with(".odp") { "odp" } else { "pptx" };
    let recognized: &[&str] = if format == "odp" {
        &["mimetype", "META-INF/manifest.xml", "content.xml", "styles.xml", "settings.xml"]
    } else {
        &["[Content_Types].xml", "_rels/.rels", "ppt/presentation.xml", "ppt/_rels/presentation.xml.rels"]
    };
    let opaque = suite_common_core::interop::OpaquePackage::capture(path, recognized)?;
    let mut report = suite_common_core::interop::CompatibilityReport::new(format);
    for name in opaque.part_names() {
        report.record(suite_common_core::interop::UnsupportedFeature::new("uninterpreted-package-part", "Uninterpreted package part", name, suite_common_core::interop::FeatureDisposition::OpaquePassThrough, "will be copied through on an opaque save"));
    }
    Ok((deck, report, opaque))
}

/// Write a presentation, dispatching on extension (.pptx or .odp).
pub fn write_deck(path: &str, deck: &engine::Deck) -> Result<(), String> {
    if path.to_lowercase().ends_with(".odp") {
        odp::write(deck, path)
    } else {
        engine::write_pptx(path, deck)
    }
}

pub fn write_deck_with_opaque(path: &str, deck: &engine::Deck, opaque: &suite_common_core::interop::OpaquePackage) -> Result<(), String> {
    write_deck(path, deck)?;
    opaque.append_to(path)
}

/// Render a presentation to an in-memory buffer without touching disk, in
/// the format implied by `format_hint` ("odp" or anything else -> pptx) —
/// used for autosave snapshots, which have no real save path to dispatch on.
pub fn write_deck_bytes(format_hint: &str, deck: &engine::Deck) -> Result<Vec<u8>, String> {
    if format_hint.eq_ignore_ascii_case("odp") {
        odp::write_bytes(deck)
    } else {
        engine::write_pptx_bytes(deck)
    }
}
