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

use tables_core::sheet::SheetModel;

use crate::window::AppState;

/// Attach the chart and conditional-formatting sidecars an xlsx package
/// carries to the first sheet. Both readers return empty for non-xlsx input,
/// so this is a no-op for ods/xls/csv rather than a special case at each
/// call site.
pub(crate) fn attach_xlsx_sidecars(path: &str, sheets: &[Rc<RefCell<SheetModel>>]) {
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

pub(crate) fn save_engine_to_xlsx(path: &str, state: &AppState) -> Result<(), String> {
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
    // glib::user_state_dir() needs the "v2_72" feature (glib >= 2.72 at the
    // C level), which this workspace's glib binding doesn't enable — do the
    // XDG Base Directory fallback ourselves instead.
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    base.join("tables")
}
