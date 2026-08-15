// io — Spreadsheet file I/O: xlsx/ods/csv/tsv import, xlsx export.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Extracted from tables/src/window.rs so it is unit-testable without GTK.
// Split by domain (issue #247):
// - `load.rs` — xlsx/ods/csv/tsv import into the engine
// - `save.rs` — xlsx export (sheets -> xlsx bytes/file)
// - `charts.rs` — read embedded charts back from xlsx
// - `condrules.rs` — read conditional-formatting rules back from xlsx
// - `props.rs` — per-sheet properties (hidden rows/cols, page setup)

mod charts;
mod condrules;
mod load;
mod props;
mod save;

pub use charts::read_charts_from_xlsx;
pub use condrules::read_cond_rules_from_xlsx;
pub use load::{load_file_into_engine, load_xlsx_workbook, load_xlsx_workbook_with_report};
pub use props::{read_sheet_props_from_xlsx, SheetXlsxProps};
pub use save::{
    save_sheets_to_xlsx, save_sheets_to_xlsx_bytes, save_sheets_to_xlsx_with_engine,
    save_sheets_to_xlsx_with_opaque,
};
