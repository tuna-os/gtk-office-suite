#![allow(deprecated)] // quick_xml 0.38 API surface used by the pptx reader/writer

//! engine — pptx deck read/write and the deck presentation data model.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Split by concern (issue #247):
//! - `model.rs` — [`Deck`], [`Slide`], [`MasterSlide`], [`SlideObject`]
//! - `parse.rs` — [`read_pptx`], [`parse_master_shapes`], xml helpers
//! - `write.rs` — [`write_pptx`], [`write_pptx_bytes`], shape writers
//! - `notes.rs` — speaker-notes XML + text extraction
//! - `placeholders.rs` — layout/master geometry a slide placeholder inherits
//! - `shape.rs` — preset shapes: geometry kind, fill and outline
//! - `table.rs` — tables: grid, cells and the default table style
//! - `text_body.rs` — paragraph styles, anchor and insets of a text box
//! - `text_xml.rs` — the pptx text styles a paragraph inherits

mod model;
mod notes;
mod parse;
mod placeholders;
pub mod shape;
pub mod table;
mod shape_xml;
pub mod text_body;
mod text_xml;
mod write;

pub use model::{Deck, MasterSlide, Slide, SlideObject, Transition};
pub use text_body::{Anchor, Autofit, Bullet, Insets, MarkerSize, MarkerStyle, ParaAlign, ParaGeometry, ParaStyle, Spacing, TextBody};
// `SlideObject::TextBox` carries `Vec<Run>`, so `Run`/`RunStyle` are already
// part of this crate's public surface; re-exported so a consumer can name
// the type it is handed without depending on letters-core directly.
pub use letters_core::model::{Run, RunStyle};
pub use parse::{parse_master_shapes, parse_master_shapes_scaled, parse_slide_size, read_pptx, SlideScale};
pub use write::{write_pptx, write_pptx_bytes};

// odp.rs and the shape parsers share these quick_xml helpers.
pub(crate) use parse::{resolve_general_ref, unescape_text};
