//! controller — canonical workbook state and user-facing edit controller.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Split into files:
//! - `state.rs` — [`WorkbookState`], the workbook the ops change
//! - `collab.rs` — the workbook as a Loro document (feature `collab`, RFC-0001 Phase 2)
//! - `core.rs` — [`WorkbookController`] state ownership + undo facade
//! - `editing.rs` — cell edits, sheet mutations, paste, fill
//! - `format.rs` — cell styles and borders on the selection (the Format inspector)
//! - `ops.rs` — workbook operations with exact inverses (RFC-0001 Phase 0)
//! - `pivot.rs` — pivot-table registration/evaluation
//! - `sheets.rs` — the sheet bar's add/rename/move/delete, as ops
//! - `view.rs` — protection, names, filters, visibility, print, sort

#[cfg(feature = "collab")]
mod collab;
mod core;
mod editing;
mod format;
mod ops;
mod pivot;
mod sheets;
mod state;
mod view;

#[cfg(all(test, feature = "collab"))]
mod collab_tests;
#[cfg(test)]
mod ops_tests;
#[cfg(test)]
mod tests;

#[cfg(feature = "collab")]
pub use collab::Replica;
pub use core::WorkbookController;
pub use format::BorderPreset;
pub use ops::{apply, apply_all, blank_lines, Axis, CellContent, Line, Op, SheetImage, WorkbookImage};
pub use state::WorkbookState;
