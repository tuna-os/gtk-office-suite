//! controller — canonical workbook state and user-facing edit controller.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Split into three files:
//! - `state.rs` — [`WorkbookState`] and the undo [`Command`]s that mutate it
//! - `controller.rs` — [`WorkbookController`], the edit facade GUI code drives
//! - `mod.rs` — module wiring

#[allow(clippy::module_inception)] // controller module contains a controller submodule (split of controller.rs)
mod controller;
mod state;

pub use controller::WorkbookController;
pub use state::WorkbookState;
