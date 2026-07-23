// decks-core — GTK-free presentation core for Decks.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Owns the Deck/Slide/SlideObject model, PPTX read/write, and undo
// commands. The `decks` binary renders (Cairo/Pango) and wires signals.

pub mod engine;
pub mod fragment;
pub mod odp;
pub mod undo;

pub use engine::{Deck, MasterSlide, Slide, SlideObject};

/// Read a presentation, dispatching on extension (.pptx or .odp).
pub fn read_deck(path: &str) -> Result<engine::Deck, String> {
    if path.to_lowercase().ends_with(".odp") {
        odp::read(path)
    } else {
        engine::read_pptx(path)
    }
}

/// Write a presentation, dispatching on extension (.pptx or .odp).
pub fn write_deck(path: &str, deck: &engine::Deck) -> Result<(), String> {
    if path.to_lowercase().ends_with(".odp") {
        odp::write(deck, path)
    } else {
        engine::write_pptx(path, deck)
    }
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
