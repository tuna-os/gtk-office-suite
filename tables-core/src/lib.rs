// tables-core — GTK-free spreadsheet core for Tables.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Owns the engine (IronCalc wrapper), sheet model, undo commands, file I/O
// (xlsx/ods/csv/tsv in, xlsx out), and Typst export. The `tables` binary
// renders and wires signals; it must not grow logic that belongs here.

pub mod controller;
pub mod engine;
pub mod export;
pub mod fragment;
pub mod io;
pub mod sheet;
pub mod snapshot;
pub mod undo;

pub use engine::TablesEngine;
pub use sheet::SheetModel;
