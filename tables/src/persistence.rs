// persistence.rs — how a Tables window's in-memory state becomes a file.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of window.rs (#168, #285), which had grown past the 2,300-line
// ceiling the release contract enforces. The format work itself lives in
// `tables_core::io`; this is the adapter layer between it and `AppState`,
// plus the per-window crash-recovery identity that decides where an autosave
// snapshot lands.
//
// Grouped by what they are for — turning this window's state into bytes on
// disk — not to reach a line count.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use tables_core::sheet::SheetModel;

use crate::window::AppState;

/// Finish an opened workbook for display: rows are auto-fitted to wrapped
/// or resized text, as spreadsheets do on open. Charts and conditional
/// formats used to be attached here; `load_workbook` reads them itself now,
/// so every opener gets them, not only the window.
pub(crate) fn attach_xlsx_sidecars(_path: &str, sheets: &[Rc<RefCell<SheetModel>>]) {
    for sheet in sheets {
        crate::grid_render::fit_rows_to_content(&mut sheet.borrow_mut());
    }
}

/// Message shown when a save target is a format Tables cannot write.
pub(crate) fn unsupported_save_format_message(path: &str) -> String {
    let name = std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());
    format!(
        "{}\n\n{}: {} → {}",
        suite_common::i18n(
            "Tables can open this file but cannot write it. Saving would replace it with an \
             Excel workbook under its current name, leaving a file that no longer matches its \
             own extension."
        ),
        suite_common::i18n("Use Save As instead"),
        name,
        tables_core::io::xlsx_save_as_name(path),
    )
}

/// `source` is the file the document came from, if it has one: its safe
/// unmodelled parts (a thumbnail, custom XML, custom properties) are
/// carried into the saved file (#1274, `suite_common::carry`). They are
/// read before the write, which may replace that very file.
pub(crate) fn save_engine_to_xlsx(path: &str, source: Option<&str>, state: &AppState) -> Result<(), String> {
    // Tables imports xls/ods/csv/tsv but writes only xlsx. Every save path —
    // Ctrl+S, Save As, and the close guard's "Save" button — funnels through
    // this one helper, so the refusal lives here rather than at each of the
    // three call sites: a call site that forgets the check then fails loudly
    // instead of silently destroying the user's file (#439).
    if !tables_core::io::is_writable_format(path) {
        return Err(unsupported_save_format_message(path));
    }
    let carried = source.map(|s| suite_common::carry::capture(std::path::Path::new(s))).unwrap_or_default();
    let sheets: Vec<SheetModel> = state.sheets.iter().map(|s| s.borrow().clone()).collect();
    // The carried parts go in with the same atomic replace (#1274).
    carried.write_with(std::path::Path::new(path), |p| {
        let p = p.to_str().ok_or_else(|| "save path is not UTF-8".to_string())?;
        tables_core::io::save_sheets_to_xlsx_with_engine(p, &sheets, Some(&state.engine))
    })?;
    // A document at a remote location (RFC-0003) is written to its staged
    // copy above and uploaded here; a local path needs nothing.
    suite_common::locations::commit_save(std::path::Path::new(path))
}

/// The local path for a location a dialog or the desktop handed over:
/// its own path, or a staged copy of a remote one (RFC-0003). `for_save`
/// stages a destination to write rather than downloading it. A location
/// that can't be used is reported in a dialog, never silently ignored.
pub(crate) fn local_path(
    file: &gtk4::gio::File,
    for_save: bool,
    parent: Option<&impl IsA<gtk4::Widget>>,
) -> Option<std::path::PathBuf> {
    let staged = if for_save {
        suite_common::locations::save_location(file)
    } else {
        suite_common::locations::open_location(file)
    };
    staged
        .map_err(|e| {
            let heading = if for_save { "Could Not Save File" } else { "Could Not Open File" };
            use libadwaita::prelude::{AdwDialogExt, AlertDialogExt};
            let alert = libadwaita::AlertDialog::builder().heading(suite_common::i18n(heading)).body(&e).build();
            alert.add_response("ok", &suite_common::i18n("_OK"));
            alert.present(parent);
        })
        .ok()
}

pub(crate) fn autosave_bytes(state: &AppState) -> Result<Vec<u8>, String> {
    let sheets: Vec<SheetModel> = state.sheets.iter().map(|s| s.borrow().clone()).collect();
    tables_core::io::save_sheets_to_xlsx_bytes(&sheets, Some(&state.engine))
}

// ── Crash-recovery snapshots ─────────────────────────────────────────────
// One doc_id per open window. It never needs to be recomputed after a
// restart (recovery scans the state dir for whatever is there), but it
// must differ from every earlier launch's (see autosave::new_doc_id).
// Unique across launches, not only within one (see new_doc_id).
pub(crate) fn next_doc_id() -> String {
    suite_common::autosave::new_doc_id()
}

pub(crate) fn autosave_state_dir() -> std::path::PathBuf {
    // XDG state dir, never a fixed name in /tmp (#829): see the shared helper.
    suite_common::autosave::state_dir("tables")
}
