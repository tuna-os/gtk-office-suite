// letters-core — GTK-free document engine for Letters.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// See DESIGN.md for the model, invariants, and addressing scheme.

pub mod docx;
pub mod fragment;
pub mod markdown;
pub mod model;
pub mod odt;
pub mod review;
pub mod session;
pub mod structured;

pub use model::{Alignment, Document, ListKind, PageGeometry, Paragraph, ParagraphLayout, ParaStyle, Run, RunStyle, StylePatch, TableCell};
pub use review::{base_direction, table_of_contents, BidiDirection, ChangeKind, Comment, ReviewId, ReviewState, TextRange, TocEntry, TrackedChange};
pub use session::DocumentSession;
pub use structured::StructuredEditor;
