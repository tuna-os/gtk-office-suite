// decks-core — GTK-free presentation core for Decks.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Owns the Deck/Slide/SlideObject model, PPTX read/write, and undo
// commands. The `decks` binary renders (Cairo/Pango) and wires signals.

pub mod engine;
pub mod undo;

pub use engine::{Deck, MasterSlide, Slide, SlideObject};
