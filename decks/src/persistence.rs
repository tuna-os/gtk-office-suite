// persistence.rs — where a Decks window's crash-recovery snapshots live.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of window.rs (#168, #285), which had grown past the 1,800-line
// ceiling the release contract enforces. The same split as
// `tables/src/persistence.rs`: the per-window identity deciding where an
// autosave snapshot lands, and the format it is written in.

// ── Crash-recovery snapshots ─────────────────────────────────────────────
static NEXT_DOC_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) fn next_doc_id() -> String {
    let n = NEXT_DOC_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{}-{n}", std::process::id())
}

pub(crate) fn autosave_state_dir() -> std::path::PathBuf {
    // glib::user_state_dir() needs the "v2_72" feature this workspace's
    // glib binding doesn't enable — do the XDG fallback ourselves.
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    base.join("decks")
}

pub(crate) fn autosave_format_hint(path: &Option<String>) -> String {
    match path {
        Some(p) if p.to_lowercase().ends_with(".odp") => "odp".to_string(),
        _ => "pptx".to_string(),
    }
}
