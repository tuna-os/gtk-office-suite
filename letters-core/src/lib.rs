// letters-core — GTK-free document engine for Letters.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// See DESIGN.md for the model, invariants, and addressing scheme.

pub mod docx;
pub mod markdown;
pub mod model;

pub use model::{Alignment, Document, ListKind, Paragraph, ParaStyle, Run, RunStyle, StylePatch};
