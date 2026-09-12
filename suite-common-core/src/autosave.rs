// autosave.rs — Crash-recovery snapshot lifecycle, shared across Letters,
// Tables, and Decks.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// This module owns only the *lifecycle*: where a snapshot lives, how it's
// written/read/cleared, and how a leftover snapshot is recognized at next
// launch. It knows nothing about document formats — callers hand it opaque
// bytes (already serialized by the app's own `*-core` write path) and get
// opaque bytes back.
//
// There is deliberately no pidfile or lock file. A snapshot's mere presence
// at launch, before this session has written anything, IS the crash signal:
// a clean save or an explicit discard always clears its slot, so anything
// still there was orphaned by a save that never happened.

use crate::atomic_save::atomic_write_bytes;
use std::fs;
use std::path::{Path, PathBuf};

/// Snapshot envelope: one generation of one document in one file, so one
/// atomic write commits it.
///
/// The slot used to write bytes and metadata as two atomic writes. Each
/// was individually atomic, which is not the same as the pair being
/// atomic: a failure or a crash between them left the new bytes beside
/// the previous generation's identity, and recovery then offered the new
/// content under the old path and format. Observed directly under fault
/// injection — `"generation two"` paired with `/tmp/first.md`, kind `md`
/// — which is a silent wrong-file restore, worse than a visibly lost one
/// (#442, `docs/readiness-2026-09/recovery.md`).
///
/// Layout, all integers little-endian:
///
/// ```text
/// 0   11  magic, whose last byte is the format version
/// 11   4  kind length
/// 15   4  path length, or NO_PATH for a document that was never saved
/// 19   8  data length
/// 27   4  CRC-32 of the payload that follows
/// 31   …  kind bytes, then path bytes, then data bytes
/// ```
///
/// The path is stored as its native OS bytes on Unix, so a path holding a
/// newline or invalid UTF-8 round-trips exactly — the previous
/// newline-delimited text metadata corrupted both. The checksum catches a
/// truncated or damaged snapshot so recovery declines it instead of
/// handing a half file to a format reader.
mod envelope {
    use super::SnapshotMeta;
    use std::path::PathBuf;

    const MAGIC: &[u8; 11] = b"OFFICESNAP";
    const HEADER: usize = 31;
    /// Distinct from a zero-length path, which is not a valid identity.
    const NO_PATH: u32 = u32::MAX;

    pub fn encode(bytes: &[u8], meta: &SnapshotMeta) -> Result<Vec<u8>, String> {
        let kind = meta.kind.as_bytes();
        let path = match meta.original_path.as_deref() {
            None => None,
            Some(p) => Some(path_bytes(p)?),
        };
        let path_len = match &path {
            None => NO_PATH,
            Some(p) => u32::try_from(p.len()).map_err(|_| "snapshot path is too long".to_string())?,
        };

        let mut payload = Vec::with_capacity(kind.len() + path.as_ref().map_or(0, Vec::len) + bytes.len());
        payload.extend_from_slice(kind);
        if let Some(p) = &path {
            payload.extend_from_slice(p);
        }
        payload.extend_from_slice(bytes);

        let mut out = Vec::with_capacity(HEADER + payload.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(
            &u32::try_from(kind.len())
                .map_err(|_| "snapshot kind is too long".to_string())?
                .to_le_bytes(),
        );
        out.extend_from_slice(&path_len.to_le_bytes());
        out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        out.extend_from_slice(&crc32(&payload).to_le_bytes());
        out.extend_from_slice(&payload);
        Ok(out)
    }

    /// None for anything this build cannot vouch for: a foreign magic, a
    /// truncated file, a failed checksum. Recovery declines rather than
    /// guesses — the alternative is handing a damaged buffer to a format
    /// reader and calling the result the user's document.
    pub fn decode(raw: &[u8]) -> Option<(Vec<u8>, SnapshotMeta)> {
        if raw.len() < HEADER || &raw[..11] != MAGIC {
            return None;
        }
        let kind_len = u32::from_le_bytes(raw[11..15].try_into().ok()?) as usize;
        let path_field = u32::from_le_bytes(raw[15..19].try_into().ok()?);
        let data_len = usize::try_from(u64::from_le_bytes(raw[19..27].try_into().ok()?)).ok()?;
        let expected_crc = u32::from_le_bytes(raw[27..31].try_into().ok()?);

        let path_len = if path_field == NO_PATH { 0 } else { path_field as usize };
        let payload = raw.get(HEADER..)?;
        if payload.len() != kind_len.checked_add(path_len)?.checked_add(data_len)? {
            return None;
        }
        if crc32(payload) != expected_crc {
            return None;
        }

        let kind = std::str::from_utf8(&payload[..kind_len]).ok()?.to_string();
        let original_path = if path_field == NO_PATH {
            None
        } else {
            Some(path_from_bytes(&payload[kind_len..kind_len + path_len])?)
        };
        let data = payload[kind_len + path_len..].to_vec();
        Some((data, SnapshotMeta { original_path, kind }))
    }

    #[cfg(unix)]
    fn path_bytes(path: &std::path::Path) -> Result<Vec<u8>, String> {
        use std::os::unix::ffi::OsStrExt;
        Ok(path.as_os_str().as_bytes().to_vec())
    }

    /// Off Unix there is no lossless byte form for a path, so a
    /// non-UTF-8 one is rejected outright rather than mangled into
    /// something recovery would reopen as a different file.
    #[cfg(not(unix))]
    fn path_bytes(path: &std::path::Path) -> Result<Vec<u8>, String> {
        path.to_str()
            .map(|s| s.as_bytes().to_vec())
            .ok_or_else(|| "snapshot path is not valid Unicode on this platform".to_string())
    }

    #[cfg(unix)]
    fn path_from_bytes(bytes: &[u8]) -> Option<PathBuf> {
        use std::os::unix::ffi::OsStrExt;
        Some(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
    }

    #[cfg(not(unix))]
    fn path_from_bytes(bytes: &[u8]) -> Option<PathBuf> {
        Some(PathBuf::from(std::str::from_utf8(bytes).ok()?))
    }

    const CRC_TABLE: [u32; 256] = {
        let mut table = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut crc = i as u32;
            let mut bit = 0;
            while bit < 8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
                bit += 1;
            }
            table[i] = crc;
            i += 1;
        }
        table
    };

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in bytes {
            crc = CRC_TABLE[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
        }
        !crc
    }
}

/// Metadata persisted alongside a snapshot so recovery can reassociate it
/// with the original document — or offer it as a recovered "Untitled"
/// document if it was never saved anywhere.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotMeta {
    pub original_path: Option<PathBuf>,
    /// App-defined hint (e.g. a format extension) describing how to
    /// interpret the snapshot bytes. Opaque to this module.
    pub kind: String,
}

/// One durable autosave slot rooted at `state_dir`, for a document
/// identified by `doc_id`. `doc_id` must be stable for the life of the
/// in-memory document but does not need to be predictable across restarts —
/// recovery works by scanning `state_dir` for whatever is there, not by
/// recomputing the id that made it.
pub struct AutosaveSlot {
    state_dir: PathBuf,
    doc_id: String,
}

const DATA_SUFFIX: &str = ".snapshot";
const META_SUFFIX: &str = ".snapshot.meta";

impl AutosaveSlot {
    pub fn new(state_dir: impl Into<PathBuf>, doc_id: impl Into<String>) -> Self {
        Self { state_dir: state_dir.into(), doc_id: doc_id.into() }
    }

    fn data_path(&self) -> PathBuf {
        self.state_dir.join(format!("{}{DATA_SUFFIX}", self.doc_id))
    }

    fn meta_path(&self) -> PathBuf {
        self.state_dir.join(format!("{}{META_SUFFIX}", self.doc_id))
    }

    /// Write (or overwrite) the snapshot atomically, creating `state_dir`
    /// if it doesn't exist yet. Never call this after `mark_clean()` on a
    /// real save without first calling `clear()` — a snapshot represents
    /// *unsaved* state; leaving a stale one around after a save would
    /// falsely trigger recovery next launch.
    pub fn write(&self, bytes: &[u8], meta: &SnapshotMeta) -> Result<(), String> {
        fs::create_dir_all(&self.state_dir)
            .map_err(|e| format!("Cannot create {}: {e}", self.state_dir.display()))?;
        // One atomic write, so the bytes and the identity that interprets
        // them commit together or not at all. See `mod envelope`.
        let encoded = envelope::encode(bytes, meta)?;
        atomic_write_bytes(&self.data_path(), &encoded)?;
        // A snapshot written by an older build left a separate metadata
        // file beside this one. `read` prefers the envelope regardless, so
        // this is housekeeping rather than correctness — and best-effort
        // for the same reason: failing a snapshot over a leftover file
        // would trade a real problem for a cosmetic one.
        let legacy = self.meta_path();
        if legacy.exists() {
            let _ = fs::remove_file(&legacy);
        }
        Ok(())
    }

    /// Remove the snapshot — call this on a successful real save and on an
    /// explicit discard. Missing files are not an error: nothing to do.
    pub fn clear(&self) -> Result<(), String> {
        for p in [self.data_path(), self.meta_path()] {
            if p.exists() {
                fs::remove_file(&p).map_err(|e| format!("Cannot remove {}: {e}", p.display()))?;
            }
        }
        Ok(())
    }

    /// Read back the snapshot bytes and metadata, if both files are present
    /// and well-formed.
    pub fn read(&self) -> Option<(Vec<u8>, SnapshotMeta)> {
        let raw = fs::read(self.data_path()).ok()?;
        if let Some(found) = envelope::decode(&raw) {
            return Some(found);
        }
        // A snapshot left by a build that wrote the two-file layout. Read
        // it rather than discard it: the user crashed on the old build and
        // upgraded, and their unsaved work is in there. New writes replace
        // it with an envelope.
        self.read_legacy_pair(raw)
    }

    fn read_legacy_pair(&self, bytes: Vec<u8>) -> Option<(Vec<u8>, SnapshotMeta)> {
        let meta_raw = fs::read_to_string(self.meta_path()).ok()?;
        let mut lines = meta_raw.splitn(2, '\n');
        let path_line = lines.next().unwrap_or_default();
        let kind = lines.next().unwrap_or_default().to_string();
        let original_path = if path_line.is_empty() { None } else { Some(PathBuf::from(path_line)) };
        Some((bytes, SnapshotMeta { original_path, kind }))
    }
}

/// Scan `state_dir` for snapshots left behind by a crash — call this once
/// at app launch, before the session's own autosave timer has run. Returns
/// the `doc_id` of each complete (data + meta) snapshot found; a data file
/// with no matching meta file (write interrupted mid-way through the two
/// atomic writes) is skipped rather than offered for recovery, since there's
/// no way to tell the caller what format it's in.
///
/// **Newest snapshot first, and in a total order.** This used to return
/// `read_dir` order, which is whatever the filesystem hands back: two
/// launches from the same state directory could offer the user's orphans in
/// different orders, and its own test had to sort the result to assert
/// anything — the tell that the function had no order to assert. It matters
/// because a caller that can only reopen one document at a time (Tables and
/// Decks hold one per window) is choosing *which* unsaved work the user gets
/// back, and "whichever the directory listed first" is not a choice anybody
/// made. Ties on mtime — two snapshots written inside one filesystem
/// timestamp tick, which a crash makes likely — break on `doc_id`, so the
/// order is total rather than merely usually-stable.
/// Has a real save already captured everything this snapshot holds?
///
/// A snapshot is cleared when the document is saved, but that clear can
/// fail — a read-only state directory, a full disk — and the failure used to
/// be discarded (`let _ = slot.clear()`). The stale snapshot then sat there
/// and the next launch offered the user's *already-saved* work back as
/// "recovered": a dialog about losing nothing, for a document safely on
/// disk. Whatever the user chose, the snapshot was not cleared then either,
/// so it came back every launch.
///
/// Reporting the failed clear would not have helped much — it happens at the
/// moment of a *successful* save, about a temporary file the user cannot act
/// on. What they actually experience is the false offer, so that is what is
/// suppressed: if the file the snapshot describes is strictly newer than the
/// snapshot, the save that produced it came after, and the snapshot has
/// nothing left to recover.
///
/// Every uncertain case offers the snapshot anyway. A document never saved
/// to a path (`original_path: None`) has nothing to compare against; a path
/// that no longer exists may have been moved rather than saved; equal
/// timestamps are ambiguous at one-second filesystem granularity; and an
/// unreadable snapshot mtime is no evidence at all. Offering work the user
/// does not need costs them one dialog. Discarding work they do need costs
/// them the work.
fn superseded_by_a_real_save(meta: &SnapshotMeta, written: Option<std::time::SystemTime>) -> bool {
    let Some(written) = written else { return false };
    let Some(path) = meta.original_path.as_ref() else { return false };
    let Ok(saved) = fs::metadata(path).and_then(|m| m.modified()) else { return false };
    saved > written
}

pub fn find_orphaned_snapshots(state_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(state_dir) else { return Vec::new() };
    let mut found: Vec<(std::time::SystemTime, String)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter_map(|name| name.strip_suffix(DATA_SUFFIX).map(str::to_string))
        .filter_map(|doc_id| {
            let slot = AutosaveSlot::new(state_dir, doc_id.clone());
            // Readable, not merely present. A snapshot whose envelope is
            // truncated or fails its checksum is not offered for recovery,
            // and neither is a legacy data file whose metadata never
            // landed: there is no way to tell the caller what format it is
            // in.
            let (_bytes, meta) = slot.read()?;
            // `None` when the mtime cannot be read. Kept distinct from
            // "very old" on purpose: such a snapshot sorts as oldest rather
            // than being dropped — unreadable metadata about recoverable
            // bytes is a reason to offer it last, not to discard it — and
            // it is never treated as superseded, because a timestamp
            // comparison against a missing timestamp is not evidence.
            let written = fs::metadata(slot.data_path()).and_then(|m| m.modified()).ok();
            if superseded_by_a_real_save(&meta, written) {
                return None;
            }
            Some((written.unwrap_or(std::time::UNIX_EPOCH), doc_id))
        })
        .collect();
    found.sort_by(|(a_time, a_id), (b_time, b_id)| {
        b_time.cmp(a_time).then_with(|| a_id.cmp(b_id))
    });
    found.into_iter().map(|(_written, doc_id)| doc_id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::atomic_save::fault;

    /// Write a snapshot, then make the file it describes look saved *after*
    /// it. Helper for the supersession cases below.
    fn snapshot_then_touch_saved_file(
        offset: std::time::Duration,
        forward: bool,
    ) -> (tempfile::TempDir, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        let saved = dir.path().join("report.md");
        std::fs::write(&saved, b"the saved document").unwrap();
        let meta = SnapshotMeta { original_path: Some(saved.clone()), kind: "md".into() };
        let slot = AutosaveSlot::new(dir.path().to_path_buf(), "doc-1");
        slot.write(b"unsaved edits", &meta).unwrap();

        let written = std::fs::metadata(slot.data_path()).unwrap().modified().unwrap();
        let stamp = if forward { written + offset } else { written - offset };
        std::fs::File::options()
            .write(true)
            .open(&saved)
            .unwrap()
            .set_modified(stamp)
            .unwrap();

        let found = find_orphaned_snapshots(dir.path());
        (dir, found)
    }

    /// The false-recovery offer this rule exists to stop: the document was
    /// saved, the clear of its snapshot failed, and the next launch used to
    /// offer already-saved work back as "recovered".
    #[test]
    fn a_snapshot_the_saved_file_has_overtaken_is_not_offered() {
        let (_dir, found) = snapshot_then_touch_saved_file(std::time::Duration::from_secs(5), true);
        assert!(
            found.is_empty(),
            "a save newer than the snapshot leaves nothing to recover, so it must \
             not be offered; got {found:?}"
        );
    }

    /// The ordinary dirty-document case, which must keep working: edits made
    /// after the last save.
    #[test]
    fn a_snapshot_newer_than_the_saved_file_is_still_offered() {
        let (_dir, found) = snapshot_then_touch_saved_file(std::time::Duration::from_secs(5), false);
        assert_eq!(found, vec!["doc-1".to_string()], "unsaved edits must still be offered");
    }

    /// One-second filesystem granularity makes equal timestamps ambiguous,
    /// and ambiguity is resolved toward the user's work.
    #[test]
    fn an_equally_timed_snapshot_is_offered_rather_than_assumed_saved() {
        let (_dir, found) = snapshot_then_touch_saved_file(std::time::Duration::ZERO, true);
        assert_eq!(
            found, vec!["doc-1".to_string()],
            "equal timestamps are not evidence of a later save"
        );
    }

    /// A path that has gone may have been moved or deleted rather than
    /// saved over, so the snapshot is still the only copy of that work.
    #[test]
    fn a_snapshot_whose_file_no_longer_exists_is_still_offered() {
        let dir = tempfile::tempdir().unwrap();
        let meta = SnapshotMeta {
            original_path: Some(dir.path().join("moved-away.md")),
            kind: "md".into(),
        };
        AutosaveSlot::new(dir.path().to_path_buf(), "doc-1")
            .write(b"unsaved edits", &meta)
            .unwrap();
        assert_eq!(find_orphaned_snapshots(dir.path()), vec!["doc-1".to_string()]);
    }

    /// A document that was never saved anywhere has nothing to compare
    /// against — the case crash recovery matters most for.
    #[test]
    fn a_never_saved_snapshot_is_always_offered() {
        let dir = tempfile::tempdir().unwrap();
        let meta = SnapshotMeta { original_path: None, kind: "md".into() };
        AutosaveSlot::new(dir.path().to_path_buf(), "doc-1")
            .write(b"never saved anywhere", &meta)
            .unwrap();
        assert_eq!(find_orphaned_snapshots(dir.path()), vec!["doc-1".to_string()]);
    }

    /// A snapshot is one generation of one document: its bytes and the
    /// identity needed to interpret them. A failure anywhere in the write
    /// must leave the slot holding one whole generation or nothing —
    /// never the new bytes beside the previous generation's identity,
    /// which recovery would reopen as the wrong file in the wrong format.
    ///
    /// This is the test that caught the two-atomic-write slot: it read
    /// back `"generation two"` paired with `/tmp/first.md`, kind `md`.
    #[test]
    fn no_failure_in_a_snapshot_write_can_pair_two_generations() {
        let first = SnapshotMeta { original_path: Some(PathBuf::from("/tmp/first.md")), kind: "md".into() };
        let second = SnapshotMeta { original_path: Some(PathBuf::from("/tmp/second.odt")), kind: "odt".into() };

        for boundary in fault::ALL {
            for arrival in 1..=3 {
                let dir = tempfile::tempdir().unwrap();
                let slot = AutosaveSlot::new(dir.path(), "doc-1");
                slot.write(b"generation one", &first).unwrap();

                let result = {
                    let _armed = fault::arm_nth(boundary, arrival);
                    slot.write(b"generation two", &second)
                };

                match slot.read() {
                    // No snapshot at all is a valid outcome: recovery
                    // offers nothing rather than something wrong.
                    None => {}
                    Some((bytes, meta)) => {
                        let whole = (bytes == b"generation one" && meta == first)
                            || (bytes == b"generation two" && meta == second);
                        assert!(
                            whole,
                            "{boundary:?}/{arrival} produced a mismatched generation: \
                             {:?} paired with {meta:?} (write returned {result:?})",
                            String::from_utf8_lossy(&bytes)
                        );
                    }
                }
            }
        }
    }

    /// Identities the newline-delimited text metadata could not survive.
    /// A path holding a newline used to be read back truncated at it —
    /// recovery would then restore into a different file — and a
    /// non-UTF-8 path was mangled by `to_string_lossy` into one that does
    /// not exist.
    #[cfg(unix)]
    #[test]
    fn awkward_paths_round_trip_exactly() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let awkward = [
            PathBuf::from("/tmp/two\nlines.md"),
            PathBuf::from(OsStr::from_bytes(b"/tmp/not\xffutf8.md")),
            PathBuf::from("/tmp/\u{0441}\u{043f}\u{0438}\u{0441}\u{043e}\u{043a} \u{1f4dd}.odt"),
        ];
        for path in awkward {
            let dir = tempfile::tempdir().unwrap();
            let slot = AutosaveSlot::new(dir.path(), "doc-1");
            let meta = SnapshotMeta { original_path: Some(path.clone()), kind: "md".into() };
            slot.write(b"unsaved work", &meta).unwrap();

            let (bytes, read_back) = slot.read().expect("snapshot should be readable");
            assert_eq!(bytes, b"unsaved work");
            assert_eq!(read_back.original_path.as_deref(), Some(path.as_path()));
        }
    }

    /// A snapshot that lost bytes to a full disk or a damaged filesystem
    /// is declined, not handed to a format reader as the user's document.
    #[test]
    fn a_damaged_snapshot_is_declined_rather_than_half_restored() {
        let dir = tempfile::tempdir().unwrap();
        let slot = AutosaveSlot::new(dir.path(), "doc-1");
        slot.write(b"unsaved work", &SnapshotMeta { original_path: None, kind: "md".into() }).unwrap();
        let path = dir.path().join("doc-1.snapshot");

        let whole = fs::read(&path).unwrap();
        fs::write(&path, &whole[..whole.len() - 3]).unwrap();
        assert!(slot.read().is_none(), "a truncated snapshot must not be offered");
        assert!(find_orphaned_snapshots(dir.path()).is_empty());

        let mut flipped = whole.clone();
        *flipped.last_mut().unwrap() ^= 0xFF;
        fs::write(&path, &flipped).unwrap();
        assert!(slot.read().is_none(), "a corrupted snapshot must not be offered");
    }

    /// A snapshot written by the previous two-file build still recovers:
    /// the user crashed on the old build and upgraded, and their unsaved
    /// work is sitting in that pair.
    #[test]
    fn a_snapshot_from_the_previous_layout_is_still_recovered() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("doc-1.snapshot"), b"old unsaved work").unwrap();
        fs::write(dir.path().join("doc-1.snapshot.meta"), "/tmp/report.md\nmd").unwrap();

        let slot = AutosaveSlot::new(dir.path(), "doc-1");
        let (bytes, meta) = slot.read().expect("the legacy pair is readable");
        assert_eq!(bytes, b"old unsaved work");
        assert_eq!(meta.original_path, Some(PathBuf::from("/tmp/report.md")));
        assert_eq!(meta.kind, "md");
        assert_eq!(find_orphaned_snapshots(dir.path()), vec!["doc-1".to_string()]);

        // Writing replaces it, leaving no stale metadata to be preferred
        // over the generation just committed.
        slot.write(b"new", &SnapshotMeta { original_path: None, kind: "odt".into() }).unwrap();
        assert!(!dir.path().join("doc-1.snapshot.meta").exists());
        let (bytes, meta) = slot.read().unwrap();
        assert_eq!(bytes, b"new");
        assert_eq!(meta, SnapshotMeta { original_path: None, kind: "odt".into() });
    }

    #[test]
    fn write_then_read_round_trips_bytes_and_meta() {
        let dir = tempfile::tempdir().unwrap();
        let slot = AutosaveSlot::new(dir.path(), "doc-1");
        let meta = SnapshotMeta { original_path: Some(PathBuf::from("/home/x/report.xlsx")), kind: "xlsx".into() };
        slot.write(b"snapshot bytes", &meta).unwrap();

        let (bytes, read_meta) = slot.read().expect("snapshot should be readable");
        assert_eq!(bytes, b"snapshot bytes");
        assert_eq!(read_meta, meta);
    }

    #[test]
    fn write_creates_state_dir_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nested/state");
        let slot = AutosaveSlot::new(&nested, "doc-1");
        slot.write(b"x", &SnapshotMeta { original_path: None, kind: "md".into() }).unwrap();
        assert!(nested.exists());
    }

    #[test]
    fn unnamed_document_snapshot_has_no_original_path() {
        let dir = tempfile::tempdir().unwrap();
        let slot = AutosaveSlot::new(dir.path(), "doc-1");
        slot.write(b"x", &SnapshotMeta { original_path: None, kind: "md".into() }).unwrap();
        let (_, meta) = slot.read().unwrap();
        assert_eq!(meta.original_path, None);
    }

    #[test]
    fn clear_removes_both_files_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let slot = AutosaveSlot::new(dir.path(), "doc-1");
        slot.write(b"x", &SnapshotMeta { original_path: None, kind: "md".into() }).unwrap();
        slot.clear().unwrap();
        assert!(slot.read().is_none());
        // Clearing an already-clear slot must not error.
        slot.clear().unwrap();
    }

    /// Write a snapshot and stamp its data file with an explicit mtime, so
    /// ordering is asserted against a known age rather than against however
    /// fast the test machine happens to be.
    fn snapshot_written_at(dir: &Path, doc_id: &str, seconds_ago: u64) {
        let slot = AutosaveSlot::new(dir, doc_id);
        slot.write(doc_id.as_bytes(), &SnapshotMeta { original_path: None, kind: "md".into() }).unwrap();
        let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 - seconds_ago);
        let file = fs::File::options().write(true).open(slot.data_path()).unwrap();
        file.set_times(fs::FileTimes::new().set_modified(when)).unwrap();
    }

    #[test]
    fn find_orphaned_snapshots_lists_doc_ids_with_a_complete_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        snapshot_written_at(dir.path(), "a", 20);
        snapshot_written_at(dir.path(), "b", 10);

        assert_eq!(find_orphaned_snapshots(dir.path()), vec!["b".to_string(), "a".to_string()]);
    }

    /// A caller that can reopen only one document is choosing which unsaved
    /// work the user gets back. It must be the most recent, not whichever
    /// the filesystem listed first.
    #[test]
    fn the_newest_snapshot_is_offered_first() {
        let dir = tempfile::tempdir().unwrap();
        snapshot_written_at(dir.path(), "oldest", 300);
        snapshot_written_at(dir.path(), "newest", 1);
        snapshot_written_at(dir.path(), "middle", 60);

        assert_eq!(
            find_orphaned_snapshots(dir.path()),
            vec!["newest".to_string(), "middle".to_string(), "oldest".to_string()],
        );
    }

    /// A crash writes snapshots milliseconds apart, so two can share one
    /// filesystem timestamp. The order still has to be total: otherwise the
    /// nondeterminism simply moves from the directory listing to the clock.
    #[test]
    fn snapshots_written_in_the_same_tick_are_ordered_by_doc_id() {
        let dir = tempfile::tempdir().unwrap();
        for doc_id in ["window-3", "window-1", "window-2"] {
            snapshot_written_at(dir.path(), doc_id, 5);
        }

        assert_eq!(
            find_orphaned_snapshots(dir.path()),
            vec!["window-1".to_string(), "window-2".to_string(), "window-3".to_string()],
        );
    }

    /// The order is a property of the snapshots, not of the walk that found
    /// them: the same directory read twice must answer the same way.
    #[test]
    fn the_order_does_not_depend_on_the_directory_listing() {
        let dir = tempfile::tempdir().unwrap();
        for (index, doc_id) in ["d", "a", "c", "b"].iter().enumerate() {
            snapshot_written_at(dir.path(), doc_id, index as u64 * 10);
        }

        let first = find_orphaned_snapshots(dir.path());
        assert_eq!(first, vec!["d".to_string(), "a".to_string(), "c".to_string(), "b".to_string()]);
        assert_eq!(find_orphaned_snapshots(dir.path()), first, "same directory, different answer");
    }

    #[test]
    fn find_orphaned_snapshots_ignores_data_file_missing_its_meta() {
        let dir = tempfile::tempdir().unwrap();
        // Simulate a write interrupted between the two atomic writes.
        fs::write(dir.path().join("half.snapshot"), b"partial").unwrap();
        assert!(find_orphaned_snapshots(dir.path()).is_empty());
    }

    #[test]
    fn find_orphaned_snapshots_on_missing_dir_is_empty_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        assert_eq!(find_orphaned_snapshots(&missing), Vec::<String>::new());
    }

    #[test]
    fn clearing_after_a_real_save_leaves_no_orphan_for_next_launch() {
        let dir = tempfile::tempdir().unwrap();
        let slot = AutosaveSlot::new(dir.path(), "doc-1");
        slot.write(b"dirty state", &SnapshotMeta { original_path: None, kind: "md".into() }).unwrap();
        // ... a real save happens here, then:
        slot.clear().unwrap();
        assert!(find_orphaned_snapshots(dir.path()).is_empty());
    }
}
