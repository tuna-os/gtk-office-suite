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

/// Attach the chart and conditional-formatting sidecars an xlsx package
/// carries to the first sheet. Both readers return empty for non-xlsx input,
/// so this is a no-op for ods/xls/csv rather than a special case at each
/// call site. Rows are auto-fitted to wrapped or resized text first, as
/// spreadsheets do on open.
pub(crate) fn attach_xlsx_sidecars(path: &str, sheets: &[Rc<RefCell<SheetModel>>]) {
    for sheet in sheets {
        crate::grid_render::fit_rows_to_content(&mut sheet.borrow_mut());
    }
    let Some(first) = sheets.first() else { return };
    let mut sheet = first.borrow_mut();
    sheet.charts = tables_core::io::read_charts_from_xlsx(path);
    sheet.cond_rules = tables_core::io::read_cond_rules_from_xlsx(path);
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

fn save_engine_to_xlsx(path: &str, state: &AppState) -> Result<(), String> {
    // Tables imports xls/ods/csv/tsv but writes only xlsx. Every save path —
    // Ctrl+S, Save As, and the close guard's "Save" button — funnels through
    // this one helper, so the refusal lives here rather than at each of the
    // three call sites: a call site that forgets the check then fails loudly
    // instead of silently destroying the user's file (#439).
    if !tables_core::io::is_writable_format(path) {
        return Err(unsupported_save_format_message(path));
    }
    let sheets: Vec<SheetModel> = state.sheets.iter().map(|s| s.borrow().clone()).collect();
    tables_core::io::save_sheets_to_xlsx_with_engine(path, &sheets, Some(&state.engine))
}

/// Save the workbook to `path`, the one way every save goes (Ctrl+S, Save
/// As, the close guard). The file is written now; a document at a remote
/// location (RFC-0003) is then uploaded without blocking, and if it changed
/// elsewhere the user picks Save as Copy, Overwrite or Reload.
/// `on_saved(path)` runs once the document is saved, with the path it now
/// is (a copy's, if they chose that). Reload reopens the server's version
/// through the app's `open`. Errors are shown; `on_saved` doesn't run.
pub(crate) fn save_workbook(
    parent: &impl IsA<gtk4::Widget>,
    path: &std::path::Path,
    state: &AppState,
    on_saved: impl FnOnce(std::path::PathBuf) + 'static,
) {
    if let Err(e) = save_engine_to_xlsx(&path.to_string_lossy(), state) {
        show_error(parent, "Error saving file", &e);
        return;
    }
    let window = parent.as_ref().root().and_downcast::<gtk4::Window>();
    let path_now = path.to_path_buf();
    suite_common::remote_io::finish_save(parent, path, move |outcome| {
        use suite_common::remote_io::SaveOutcome;
        match outcome {
            SaveOutcome::Saved => on_saved(path_now),
            SaveOutcome::SavedAs(copy) => on_saved(copy),
            SaveOutcome::Reload(fresh) => {
                if let Some(app) = window.and_then(|w| w.application()) {
                    app.open(&[gtk4::gio::File::for_path(fresh)], "");
                }
            }
            SaveOutcome::Cancelled | SaveOutcome::Failed => {}
        }
    });
}

fn show_error(parent: &impl IsA<gtk4::Widget>, heading: &str, body: &str) {
    use libadwaita::prelude::{AdwDialogExt, AlertDialogExt};
    let alert = libadwaita::AlertDialog::builder().heading(suite_common::i18n(heading)).body(body).build();
    alert.add_response("ok", &suite_common::i18n("OK"));
    alert.present(Some(parent));
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
            let heading = if for_save { "Error saving file" } else { "Error opening file" };
            if let Some(parent) = parent {
                show_error(parent, heading, &e);
            }
        })
        .ok()
}

pub(crate) fn autosave_bytes(state: &AppState) -> Result<Vec<u8>, String> {
    let sheets: Vec<SheetModel> = state.sheets.iter().map(|s| s.borrow().clone()).collect();
    tables_core::io::save_sheets_to_xlsx_bytes(&sheets, Some(&state.engine))
}

// ── Crash-recovery snapshots ─────────────────────────────────────────────
// One doc_id per open window, unique for the life of the process — it does
// not need to survive a restart, since recovery finds snapshots by scanning
// the state dir rather than by recomputing a prior id (see autosave.rs).
static NEXT_DOC_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) fn next_doc_id() -> String {
    let n = NEXT_DOC_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}-{n}", std::process::id())
}

pub(crate) fn autosave_state_dir() -> std::path::PathBuf {
    // XDG state dir, never a fixed name in /tmp (#829): see the shared helper.
    suite_common::autosave::state_dir("tables")
}
