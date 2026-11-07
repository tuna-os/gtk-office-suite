// letters-core — GTK-free document engine for Letters.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// See DESIGN.md for the model, invariants, and addressing scheme.

pub mod chips;
#[cfg(feature = "collab")]
pub mod collab;
pub mod comments;
pub mod docx;
mod docx_anchors;
mod docx_chips;
mod docx_comments;
mod docx_toc;
mod docx_revisions;
mod docx_fields;
mod docx_text_boxes;
pub mod edit;
pub mod fragment;
pub mod layout;
pub mod lists;
pub mod loss;
pub mod markdown;
pub mod model;
pub mod odt;
pub mod review;
pub mod save;
pub mod session;
pub mod sections;
pub mod structured;
pub mod table_text;
pub mod toc;
pub mod track;
pub mod word_count;

pub use model::{Alignment, BorderStyle, CellEdges, CellFill, CellSpan, CellVAlign, VAlign, DocSection, Document, HfFloat, HfLine, HfStory, LineHeight, ListKind, ListLabel, NumberFormat, OpenEdges, PageGeometry, Paragraph, ParagraphLayout, ParagraphRange, ParaBorder, ParaBorders, ParaStyle, Comment, Revision, RevisionKind, RowHeight, Run, RunStyle, StylePatch, TabAlign, TabLeader, TabStop, TableCell, style_readout, CellPadding};
pub use review::{base_direction, table_of_contents, BidiDirection, TocEntry};
pub use session::DocumentSession;
pub use structured::StructuredEditor;
pub use word_count::editor_word_count;
