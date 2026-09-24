//! controller — canonical workbook state and user-facing edit controller.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Split into six files:
//! - `state.rs` — [`WorkbookState`] and the undo [`Command`]s that mutate it
//! - `core.rs` — [`WorkbookController`] state ownership + undo facade
//! - `editing.rs` — cell edits, sheet mutations, paste, fill
//! - `format.rs` — cell styles and borders on the selection (the Format inspector)
//! - `pivot.rs` — pivot-table registration/evaluation
//! - `view.rs` — protection, names, filters, visibility, print, sort

mod core;
mod editing;
mod format;
mod pivot;
mod state;
mod view;

#[cfg(test)]
mod tests;

pub use core::WorkbookController;
pub use format::BorderPreset;
pub use state::WorkbookState;
