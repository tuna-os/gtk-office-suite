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
