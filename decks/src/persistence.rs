// persistence.rs — where a Decks window's crash-recovery snapshots live.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Split out of window.rs (#168, #285), which had grown past the 1,800-line
// ceiling the release contract enforces. The same split as
// `tables/src/persistence.rs`: the per-window identity deciding where an
// autosave snapshot lands, and the format it is written in.

// ── Crash-recovery snapshots ─────────────────────────────────────────────
// Unique across launches, not only within one (see new_doc_id).
pub(crate) fn next_doc_id() -> String {
    suite_common::autosave::new_doc_id()
}

pub(crate) fn autosave_state_dir() -> std::path::PathBuf {
    // XDG state dir, never a fixed name in /tmp (#829): see the shared helper.
    suite_common::autosave::state_dir("decks")
}

pub(crate) fn autosave_format_hint(path: &Option<String>) -> String {
    match path {
        Some(p) if p.to_lowercase().ends_with(".odp") => "odp".to_string(),
        _ => "pptx".to_string(),
    }
}

// ── Autosave: one crash-recovery snapshot attempt ──────────────────────────
// Shared by the manual `autosave-now` action and the unattended timer so
// the two cannot drift apart. Serialization AND slot-write failures both
// reach the notifier: a snapshot that silently never lands is work the
// user believes is protected. The prior valid generation stays — the
// slot's single atomic write only ever replaces it on success (DCK-003).
// The bytes are the versioned recovery payload (canonical state, assets
// and source context), not bare package output.
pub(crate) fn autosave_once(
    controller: &decks_core::DecksController,
    dirty: &std::cell::Cell<bool>,
    slot: &suite_common::autosave::AutosaveSlot,
    notices: &suite_common::autosave_notice::AutosaveNotifier,
    path: Option<String>,
) {
    if !dirty.get() {
        return;
    }
    let kind = autosave_format_hint(&path);
    match decks_core::recovery::encode_snapshot(controller, &kind) {
        Ok(bytes) => {
            let meta = suite_common::autosave::SnapshotMeta {
                original_path: path.map(std::path::PathBuf::from),
                kind,
            };
            notices.record(slot.write(&bytes, &meta));
        }
        Err(reason) => notices.record(Err(reason)),
    }
}

/// Read package-shaped snapshot bytes back through the package readers —
/// the migration path for snapshots written before the versioned payload
/// (and the fallback for payloads from a newer version). The scratch file
/// carries the format's extension because the readers dispatch on it,
/// exactly as the old recovery path's temp file did; it is removed before
/// this returns, so no provenance may pin it.
pub(crate) fn read_legacy_package(
    state_dir: &std::path::Path,
    bytes: &[u8],
    ext: &str,
) -> Result<decks_core::engine::Deck, String> {
    let tmp = suite_common::autosave::recovery_scratch_path(state_dir, ext);
    std::fs::write(&tmp, bytes).map_err(|e| format!("could not stage the snapshot for recovery: {e}"))?;
    let deck = decks_core::read_deck(&tmp.to_string_lossy()).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    // Best-effort, as before: a deck that decoded must still recover even
    // if its staging file cannot be unlinked.
    let _ = std::fs::remove_file(&tmp);
    Ok(deck)
}
