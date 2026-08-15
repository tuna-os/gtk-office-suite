#![allow(deprecated)] // quick_xml 0.38 API surface used by the pptx reader/writer

//! engine — pptx deck read/write and the deck presentation data model.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Split by concern (issue #247):
//! - `model.rs` — [`Deck`], [`Slide`], [`MasterSlide`], [`SlideObject`]
//! - `parse.rs` — [`read_pptx`], [`parse_master_shapes`], xml helpers
//! - `write.rs` — [`write_pptx`], [`write_pptx_bytes`], shape writers
//! - `notes.rs` — speaker-notes XML + text extraction

mod model;
mod notes;
mod parse;
mod write;

pub use model::{Deck, MasterSlide, Slide, SlideObject};
pub use parse::{parse_master_shapes, read_pptx};
pub use write::{write_pptx, write_pptx_bytes};

// odp.rs and the shape parsers share these quick_xml helpers.
pub(crate) use parse::{resolve_general_ref, unescape_text};
