//! controller — canonical workbook state and user-facing edit controller.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Split into seven files:
//! - `state.rs` — [`WorkbookState`] and the undo [`Command`]s that mutate it
//! - `core.rs` — [`WorkbookController`] state ownership + undo facade
//! - `editing.rs` — cell edits, sheet mutations, paste, fill
//! - `format.rs` — cell styles and borders on the selection (the Format inspector)
//! - `ops.rs` — workbook operations with exact inverses (RFC-0001 Phase 0)
//! - `pivot.rs` — pivot-table registration/evaluation
//! - `view.rs` — protection, names, filters, visibility, print, sort

mod core;
mod editing;
mod format;
mod ops;
mod pivot;
mod state;
mod view;

#[cfg(test)]
mod ops_tests;
#[cfg(test)]
mod tests;

pub use core::WorkbookController;
pub use format::BorderPreset;
pub use ops::{apply, apply_all, blank_lines, Axis, CellContent, Line, Op, SheetImage, WorkbookImage};
pub use state::WorkbookState;
