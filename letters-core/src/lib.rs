// letters-core — GTK-free document engine for Letters.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// See DESIGN.md for the model, invariants, and addressing scheme.

pub mod docx;
pub mod fragment;
pub mod markdown;
pub mod model;
pub mod odt;
pub mod session;
pub mod review;

pub use model::{Alignment, Document, ListKind, Paragraph, ParaStyle, Run, RunStyle, StylePatch};
pub use session::DocumentSession;
pub use review::{Comment, Revision, RevisionDecision, RevisionKind, ReviewState, TextDirection, TocEntry};
