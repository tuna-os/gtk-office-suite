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
    /// The magic without its trailing version byte: what identifies a file
    /// as *an* envelope, whatever version wrote it.
    const MAGIC_PREFIX: &[u8] = b"OFFICESNAP";
    const HEADER: usize = 31;

    /// Whether this is an envelope at all — as opposed to the raw document
    /// bytes the two-file layout stored.
    ///
    /// `decode` answering `None` means "this build cannot read it", which is
    /// not the same as "this is not one". The difference matters because the
    /// caller's fallback is to treat the bytes as a legacy document, and
    /// doing that to an envelope hands its 31-byte header to a format reader
    /// as though it were the user's file.
    pub fn looks_like_an_envelope(raw: &[u8]) -> bool {
        raw.starts_with(MAGIC_PREFIX)
    }
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
/// Ownership marker. A zero-byte file whose only job is to carry an
/// advisory lock; `find_orphaned_snapshots` keys off DATA_SUFFIX, so these
/// are never mistaken for recoverable content.
const LOCK_SUFFIX: &str = ".lock";

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

    /// Take over recovered content: write it to this slot, and report
    /// whether the orphan slot it came from may now be cleared.
    ///
    /// The order is the point. All three apps used to clear the orphan as
    /// soon as the content was in memory and leave the next autosave tick to
    /// write a replacement — up to a minute later at the shipped interval,
    /// and never at all in a Letters that shipped with its timer switched
    /// off. A crash inside that window lost work that had just survived a
    /// crash, which is the one thing crash recovery must not do.
    ///
    /// Returning `false` keeps the orphan on disk, so work whose replacement
    /// snapshot could not be written is still offered on the next launch.
    /// The cost is that a clean save this session will not clear that orphan
    /// — the close path clears the window's own slot, not the one it
    /// recovered from — so the same content can be offered once more. Work
    /// offered twice is recoverable; work silently dropped is not.
    pub fn adopt_recovered(&self, bytes: &[u8], meta: &SnapshotMeta) -> bool {
        self.write(bytes, meta).is_ok()
    }

    /// Mark this process as the live owner of the slot, so another launch
    /// does not offer this window's open document as a crash recovery.
    ///
    /// Keep the returned guard for the window's lifetime. `None` means
    /// another live process already owns the slot, which for a freshly
    /// allocated doc_id should not happen and is worth not overwriting.
    /// A slot that cannot be locked at all — no directory, a read-only
    /// state dir — yields a guard anyway: autosave still has to work
    /// where locking does not, and the cost is only that a second launch
    /// may offer a document that is already open.
    pub fn claim(&self) -> Option<SnapshotOwner> {
        if fs::create_dir_all(&self.state_dir).is_err() {
            return None;
        }
        let path = lock_path(&self.state_dir, &self.doc_id);
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .ok()?;
        match try_lock_exclusive(&file) {
            Ok(true) => Some(SnapshotOwner { _file: file }),
            Ok(false) => None,
            // Could not ask. Autosave must still run, so claim it.
            Err(_) => Some(SnapshotOwner { _file: file }),
        }
    }

    /// Remove the snapshot — call this on a successful real save and on an
    /// explicit discard. Missing files are not an error: nothing to do.
    pub fn clear(&self) -> Result<(), String> {
        // The ownership marker belongs to the snapshot, so it goes when the
        // snapshot does. Best-effort: a leftover zero-byte .lock is inert
        // (nothing holds it, so it asserts no liveness), while failing the
        // clear over it would turn a tidy-up into a reported error.
        let _ = fs::remove_file(lock_path(&self.state_dir, &self.doc_id));
        for p in [self.data_path(), self.meta_path()] {
            if p.exists() {
                fs::remove_file(&p).map_err(|e| format!("Cannot remove {}: {e}", p.display()))?;
            }
        }
        Ok(())
    }

    /// Clear the slot, and say so on stderr if it could not be cleared.
    ///
    /// Returns whether it reported a failure, which is what makes the
    /// behaviour testable — the message itself is a side effect.
    ///
    /// Every call site in the three apps used to be `let _ = slot.clear();`.
    /// A failed clear happens at the moment of a *successful* save, about a
    /// temporary file the user cannot act on, so #666 deliberately did not
    /// put a toast in front of someone whose save just worked, and removed
    /// the consequence instead: `find_orphaned_snapshots` no longer offers a
    /// snapshot the saved file has overtaken. That left the failure with no
    /// trace at all, which is the wrong end of the trade — a full or
    /// read-only state directory is exactly what somebody diagnosing "why is
    /// this directory filling up" needs to see. A log line is that trace,
    /// and it is the house convention for a diagnostic nobody can act on
    /// mid-session (see the `GSettings write failed` reports).
    pub fn clear_or_report(&self) -> bool {
        match self.clear() {
            Ok(()) => false,
            Err(reason) => {
                eprintln!(
                    "autosave: could not clear the crash snapshot for {}: {reason}. \
                     The snapshot is left behind; it will not be offered as a \
                     recovery once the saved file is newer, but the state \
                     directory keeps the file.",
                    self.doc_id,
                );
                true
            }
        }
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
        // An envelope this build could not decode is not legacy content, and
        // must not be read as though it were. Without this the CRC check
        // above is defeated by a leftover sidecar: a damaged envelope fails
        // `decode`, falls through to here, and — if a `.snapshot.meta` from
        // an older build happens to sit beside it — is handed back *whole*,
        // header and all, wearing that sidecar's path and kind. Recovery
        // then writes those bytes to a temp file and asks a format reader to
        // open them as the user's document.
        //
        // The sidecar survives because `write` removes it best-effort
        // (`let _ = fs::remove_file`), so a read-only or full state
        // directory — the failure modes autosave already expects — leaves it
        // in place next to a perfectly good envelope. It only needs the
        // envelope to be damaged later for the fallback to start lying.
        //
        // The same guard covers the forward direction: a snapshot from a
        // newer build carries a version byte this one does not know, so
        // `decode` declines it, and declining is right — but it must decline
        // as "an envelope I cannot read" rather than be reinterpreted as a
        // document.
        if envelope::looks_like_an_envelope(&bytes) {
            return None;
        }
        let meta_raw = fs::read_to_string(self.meta_path()).ok()?;
        let mut lines = meta_raw.splitn(2, '\n');
        let path_line = lines.next().unwrap_or_default();
        let kind = lines.next().unwrap_or_default().to_string();
        let original_path = if path_line.is_empty() { None } else { Some(PathBuf::from(path_line)) };
        Some((bytes, SnapshotMeta { original_path, kind }))
    }
}

/// How often a still-failing autosave says so again, counted in failed
/// attempts. With the shipped 30-second timer that is roughly every five
/// minutes: often enough that somebody who missed the first notice learns
/// before losing an afternoon, rare enough not to be the reason they stop
/// reading notices.
const REPEAT_EVERY: u32 = 10;

/// Whether an autosave failure has been told to the user yet.
///
/// Every autosave write site in the three apps used to read
/// `let _ = slot.write(&bytes, &meta);`. A snapshot write can fail for
/// ordinary reasons — a read-only home, a full disk, a sandbox denying the
/// state directory — and when it did, autosave silently did nothing for the
/// rest of the session while the user went on believing their unsaved work
/// was protected. They found out at the crash, which is the one moment the
/// feature exists for.
///
/// Reporting every failure instead is no better: autosave runs on a timer,
/// so a permanently unwritable directory would raise a notice every thirty
/// seconds, and a notice that appears 120 times an hour is one nobody reads.
/// This keeps the decision — first failure of a streak, a reminder every
/// `REPEAT_EVERY` after that, and one notice when it starts working again —
/// out of the three GUI call sites and somewhere it can be tested.
#[derive(Debug, Default)]
pub struct AutosaveNotices {
    consecutive_failures: u32,
}

impl AutosaveNotices {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a failed snapshot write. Returns the message to show, or
    /// `None` when this failure is one the user has already been told about.
    pub fn failed(&mut self, reason: &str) -> Option<String> {
        self.consecutive_failures += 1;
        let first = self.consecutive_failures == 1;
        let reminder = self.consecutive_failures.is_multiple_of(REPEAT_EVERY);
        if !first && !reminder {
            return None;
        }
        // Names the consequence before the cause: "could not write" invites
        // a shrug, "your unsaved work is not being protected" does not.
        Some(format!(
            "Autosave is failing — unsaved work is not being protected. Save manually. ({reason})"
        ))
    }

    /// Record a successful snapshot write. Returns a message only if the
    /// user was previously told autosave was failing, so an ordinary session
    /// stays silent.
    pub fn succeeded(&mut self) -> Option<String> {
        let was_failing = self.consecutive_failures > 0;
        self.consecutive_failures = 0;
        was_failing.then(|| "Autosave is working again.".to_string())
    }

    /// Whether autosave is currently failing — for a caller that wants to
    /// show state rather than an event.
    pub fn is_failing(&self) -> bool {
        self.consecutive_failures > 0
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
/// Proof that this process is the live owner of a snapshot slot.
///
/// Hold one of these for as long as the window is open. Dropping it — or
/// the process dying, however abruptly — releases the claim, because the
/// kernel owns the release rather than our shutdown path. That is the
/// whole reason for an advisory lock here instead of writing a pid into a
/// file: a pid has to be interpreted, and it is wrong exactly when it
/// matters, after a SIGKILL, when the number may already belong to
/// something else.
pub struct SnapshotOwner {
    /// Held open because closing it would release the lock. Never read.
    _file: fs::File,
}

/// Try to take an advisory exclusive lock without blocking.
///
/// `Ok(true)` means we hold it, `Ok(false)` that someone else does, and
/// `Err` that the question could not be asked.
///
/// What to do with that last case is the caller's, and the two callers
/// answer it oppositely on purpose. `has_a_live_owner` treats it as "no
/// live owner", because autosave must still run. `atomic_save`'s sweep
/// treats it as "leave it alone", because the action it gates is a
/// deletion. Both are the same rule — failing to prove something is safe
/// is not proof that it is — pointed at different consequences.
#[cfg(unix)]
pub(crate) fn try_lock_exclusive(file: &fs::File) -> std::io::Result<bool> {
    use std::os::unix::io::AsRawFd;
    // flock locks the open file description, so two descriptors on the
    // same file conflict even inside one process. That is what makes this
    // usable from a second window in the same app.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        return Ok(true);
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        // EAGAIN and EWOULDBLOCK are the same value on Linux; both mean
        // "held by someone else", which is an answer rather than a fault.
        Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN => Ok(false),
        _ => Err(err),
    }
}

#[cfg(not(unix))]
pub(crate) fn try_lock_exclusive(_file: &fs::File) -> std::io::Result<bool> {
    // No advisory locking wired up off Unix. Claiming always "succeeds"
    // and liveness is never asserted, which degrades to the behaviour
    // before ownership existed: snapshots are offered.
    Ok(true)
}

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

/// Is some other live window holding this snapshot open?
///
/// The bias is deliberate and one-directional: this returns `true` only
/// when the lock is demonstrably held by someone, and `false` for every
/// other outcome — no lock file (nothing ever claimed it, which is every
/// snapshot written before ownership existed), an unreadable lock path, a
/// platform without locking. A wrong `true` hides recoverable work
/// forever, because a snapshot that is never offered is never cleared
/// either; a wrong `false` costs the user one recovery dialog for a
/// document that is already open. Those are not the same mistake.
fn has_a_live_owner(state_dir: &Path, doc_id: &str) -> bool {
    let path = lock_path(state_dir, doc_id);
    if !path.exists() {
        return false;
    }
    let Ok(file) = fs::OpenOptions::new().read(true).write(true).open(&path) else {
        return false;
    };
    // Taking the lock proves nobody held it; dropping `file` at the end of
    // this scope releases what we just took.
    !matches!(try_lock_exclusive(&file), Ok(true) | Err(_))
}

fn lock_path(state_dir: &Path, doc_id: &str) -> PathBuf {
    state_dir.join(format!("{doc_id}{LOCK_SUFFIX}"))
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
            // A window that is still open is not a crash to recover from.
            // Since a live window always keeps a snapshot on disk now, a
            // second launch would otherwise offer the first window's
            // unsaved document back — reopening work that is already open,
            // in a second window, from a snapshot that is still being
            // rewritten underneath it.
            if has_a_live_owner(state_dir, &doc_id) {
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

    /// The failure branch is the one worth pinning: when the replacement
    /// snapshot cannot be written, the caller must be told to keep the
    /// orphan. Reasoning about that in a comment is not the same as
    /// checking it.
    #[test]
    fn adopting_recovered_content_reports_failure_so_the_orphan_survives() {
        let meta = SnapshotMeta { original_path: None, kind: "md".into() };
        // A regular file where the state directory should be: the write
        // cannot succeed, for root either.
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("not-a-directory");
        std::fs::write(&blocker, b"occupied").unwrap();
        let slot = AutosaveSlot::new(blocker.join("state"), "doc-1");
        assert!(
            !slot.adopt_recovered(b"recovered work", &meta),
            "a failed write must report false so the caller keeps the orphan"
        );
    }

    #[test]
    fn adopting_recovered_content_leaves_it_readable_from_the_new_slot() {
        let meta = SnapshotMeta { original_path: None, kind: "md".into() };
        let dir = tempfile::tempdir().unwrap();
        let slot = AutosaveSlot::new(dir.path().to_path_buf(), "doc-1");
        assert!(slot.adopt_recovered(b"recovered work", &meta));
        let (bytes, read_back) = slot.read().expect("the adopted snapshot must read back");
        assert_eq!(bytes, b"recovered work");
        assert_eq!(read_back.kind, "md");
    }
    use super::*;

    use crate::atomic_save::fault;

    /// The assumption the whole ownership design rests on: flock is per
    /// open file description, so a second descriptor conflicts even inside
    /// one process. If that were false, `has_a_live_owner` would always say
    /// "free" and every check below would pass while protecting nothing.
    #[test]
    fn a_claimed_slot_reads_as_live_from_a_second_descriptor() {
        let dir = tempfile::tempdir().unwrap();
        let slot = AutosaveSlot::new(dir.path().to_path_buf(), "doc-1");
        let owner = slot.claim().expect("a fresh slot should be claimable");
        assert!(
            has_a_live_owner(dir.path(), "doc-1"),
            "a held claim must read as live, or nothing here protects anything"
        );
        drop(owner);
        assert!(
            !has_a_live_owner(dir.path(), "doc-1"),
            "releasing the claim must make the slot recoverable again"
        );
    }

    /// The bug this exists for: recovery adopts content into the live
    /// window's own slot, so an open window always has a snapshot on disk,
    /// and a second launch used to offer it back.
    #[test]
    fn a_live_windows_snapshot_is_not_offered_for_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let meta = SnapshotMeta { original_path: None, kind: "md".into() };
        let slot = AutosaveSlot::new(dir.path().to_path_buf(), "doc-1");
        slot.write(b"work in an open window", &meta).unwrap();
        let _owner = slot.claim().unwrap();
        assert!(
            find_orphaned_snapshots(dir.path()).is_empty(),
            "a document open in another window is not a crash to recover"
        );
    }

    /// The crash case, which is the whole point: the process dies without
    /// running any cleanup, the kernel drops the lock, and the work is
    /// offered. Dropping the guard while leaving the snapshot behind is
    /// what an abrupt exit leaves.
    #[test]
    fn work_from_a_dead_window_is_still_offered() {
        let dir = tempfile::tempdir().unwrap();
        let meta = SnapshotMeta { original_path: None, kind: "md".into() };
        let slot = AutosaveSlot::new(dir.path().to_path_buf(), "doc-1");
        slot.write(b"unsaved work", &meta).unwrap();
        drop(slot.claim().unwrap());
        assert_eq!(
            find_orphaned_snapshots(dir.path()),
            vec!["doc-1".to_string()],
            "a released claim means the owner is gone and the work is recoverable"
        );
    }

    /// Every snapshot written before ownership existed has no lock file,
    /// and must still be offered.
    #[test]
    fn a_snapshot_with_no_lock_file_is_offered() {
        let dir = tempfile::tempdir().unwrap();
        let meta = SnapshotMeta { original_path: None, kind: "md".into() };
        AutosaveSlot::new(dir.path().to_path_buf(), "doc-1")
            .write(b"from an older build", &meta)
            .unwrap();
        assert!(!lock_path(dir.path(), "doc-1").exists());
        assert_eq!(find_orphaned_snapshots(dir.path()), vec!["doc-1".to_string()]);
    }

    /// Liveness that cannot be determined must not hide work: an
    /// unopenable lock path reads as "no live owner", never as "held".
    #[test]
    fn an_unusable_lock_path_does_not_hide_the_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let meta = SnapshotMeta { original_path: None, kind: "md".into() };
        AutosaveSlot::new(dir.path().to_path_buf(), "doc-1")
            .write(b"unsaved work", &meta)
            .unwrap();
        // A directory where the lock file should be: open() fails with
        // EISDIR, for root as well as anyone else.
        fs::create_dir(lock_path(dir.path(), "doc-1")).unwrap();
        assert!(!has_a_live_owner(dir.path(), "doc-1"));
        assert_eq!(find_orphaned_snapshots(dir.path()), vec!["doc-1".to_string()]);
    }

    /// Clearing a slot takes its ownership marker with it, so a state
    /// directory does not silt up with zero-byte files.
    #[test]
    fn clearing_a_slot_removes_its_lock_file() {
        let dir = tempfile::tempdir().unwrap();
        let meta = SnapshotMeta { original_path: None, kind: "md".into() };
        let slot = AutosaveSlot::new(dir.path().to_path_buf(), "doc-1");
        slot.write(b"work", &meta).unwrap();
        drop(slot.claim().unwrap());
        assert!(lock_path(dir.path(), "doc-1").exists());
        slot.clear().unwrap();
        assert!(!lock_path(dir.path(), "doc-1").exists());
    }

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

    /// The CRC check must hold whatever else is lying around in the state
    /// directory. `a_damaged_snapshot_is_declined_rather_than_half_restored`
    /// proves it does with a clean directory; this proves the leftover
    /// sidecar from an older build cannot talk the reader out of it.
    #[test]
    fn a_damaged_envelope_is_declined_even_beside_a_stale_legacy_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let slot = AutosaveSlot::new(dir.path(), "doc-1");
        slot.write(
            b"the real workbook",
            &SnapshotMeta { original_path: Some(PathBuf::from("/home/x/books.xlsx")), kind: "xlsx".into() },
        )
        .unwrap();

        // Damage the payload, leaving the magic and version intact, so the
        // envelope is recognisably one and fails only its checksum.
        let path = dir.path().join("doc-1.snapshot");
        let mut raw = fs::read(&path).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0xFF;
        fs::write(&path, &raw).unwrap();

        // A sidecar from a previous build, of the kind `write` removes only
        // best-effort — a read-only or full state directory leaves it.
        fs::write(dir.path().join("doc-1.snapshot.meta"), "/tmp/old-notes.md\nmd").unwrap();

        assert!(
            slot.read().is_none(),
            "a damaged envelope was handed back as a document because a stale \
             sidecar was sitting next to it",
        );
        assert_eq!(
            find_orphaned_snapshots(dir.path()),
            Vec::<String>::new(),
            "and it must not be offered for recovery either",
        );
    }

    /// A snapshot from a *newer* build: same magic, a version byte this one
    /// does not know. Declining is the only safe answer — the layout may
    /// have changed — but it has to decline as an envelope rather than be
    /// re-read as legacy content.
    #[test]
    fn a_snapshot_from_a_newer_build_is_declined_not_reinterpreted() {
        let dir = tempfile::tempdir().unwrap();
        let slot = AutosaveSlot::new(dir.path(), "doc-1");
        slot.write(b"content", &SnapshotMeta { original_path: None, kind: "xlsx".into() }).unwrap();

        let path = dir.path().join("doc-1.snapshot");
        let mut raw = fs::read(&path).unwrap();
        raw[10] = raw[10].wrapping_add(1); // the version byte
        fs::write(&path, &raw).unwrap();
        fs::write(dir.path().join("doc-1.snapshot.meta"), "/tmp/old.md\nmd").unwrap();

        assert!(
            slot.read().is_none(),
            "a newer build's snapshot was reinterpreted as a legacy document",
        );
        assert_eq!(find_orphaned_snapshots(dir.path()), Vec::<String>::new());
    }

    /// Genuine legacy content still works, which is what stops the guard
    /// above from being a blanket refusal. The bytes here are a document,
    /// not an envelope, so the sidecar is the only description of them
    /// there is.
    #[test]
    fn the_guard_does_not_block_real_legacy_content() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("doc-1.snapshot"), b"# notes\n\nplain markdown").unwrap();
        fs::write(dir.path().join("doc-1.snapshot.meta"), "/tmp/report.md\nmd").unwrap();

        let slot = AutosaveSlot::new(dir.path(), "doc-1");
        let (bytes, meta) = slot.read().expect("legacy content is still readable");
        assert_eq!(bytes, b"# notes\n\nplain markdown");
        assert_eq!(meta.kind, "md");
        assert_eq!(find_orphaned_snapshots(dir.path()), vec!["doc-1".to_string()]);
    }

    /// Recovering the same work twice must not offer it twice. The sequence
    /// is the one `recover_from_snapshot` performs: adopt the orphan's
    /// content into this window's own claimed slot, then clear the orphan.
    /// A second pass has to come back empty — the adopted copy belongs to a
    /// live window, and the orphan is gone.
    #[test]
    fn a_recovered_snapshot_is_not_offered_again() {
        let dir = tempfile::tempdir().unwrap();
        let orphan = AutosaveSlot::new(dir.path(), "crashed-1");
        let meta = SnapshotMeta { original_path: Some(PathBuf::from("/home/x/report.odt")), kind: "odt".into() };
        orphan.write(b"unsaved work", &meta).unwrap();
        assert_eq!(find_orphaned_snapshots(dir.path()), vec!["crashed-1".to_string()]);

        let mine = AutosaveSlot::new(dir.path(), "recovering-1");
        let _claim = mine.claim().expect("the recovering window claims its own slot");
        assert!(mine.adopt_recovered(b"unsaved work", &meta));
        orphan.clear().unwrap();

        assert_eq!(
            find_orphaned_snapshots(dir.path()),
            Vec::<String>::new(),
            "the recovered work was offered a second time, which is how one \
             crash becomes two copies of the same document",
        );
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

    /// A clear that cannot happen must leave a trace rather than nothing.
    ///
    /// `remove_file` on a directory fails even as root, which is how this
    /// arranges a failing clear without needing a read-only mount: the
    /// snapshot path is a directory, so it exists and cannot be removed.
    #[test]
    fn a_clear_that_fails_is_reported_rather_than_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let slot = AutosaveSlot::new(dir.path(), "doc-1");
        fs::create_dir_all(dir.path().join("doc-1.snapshot")).unwrap();

        assert!(slot.clear().is_err(), "precondition: this clear cannot succeed");
        assert!(
            slot.clear_or_report(),
            "a failed clear must report, or a full state directory leaves no trace",
        );
    }

    /// And a clear that works says nothing, so the report means something
    /// when it does appear.
    #[test]
    fn a_clear_that_works_is_silent() {
        let dir = tempfile::tempdir().unwrap();
        let slot = AutosaveSlot::new(dir.path(), "doc-1");
        slot.write(b"work", &SnapshotMeta { original_path: None, kind: "md".into() }).unwrap();

        assert!(!slot.clear_or_report(), "a successful clear must not report");
        // And with nothing left to remove it is still silent, because
        // clearing an already-clear slot is not a failure.
        assert!(!slot.clear_or_report(), "clearing twice must not report");
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

    /// The first failure must be reported: silence here is the defect this
    /// type exists to fix.
    #[test]
    fn the_first_failure_is_reported() {
        let mut notices = AutosaveNotices::new();
        let said = notices.failed("Permission denied").expect("the first failure must be told");
        assert!(said.contains("not being protected"), "names the consequence: {said}");
        assert!(said.contains("Permission denied"), "carries the reason: {said}");
        assert!(notices.is_failing());
    }

    /// And the second through ninth must not: the timer fires every thirty
    /// seconds, and a notice shown 120 times an hour is one nobody reads.
    #[test]
    fn a_failure_already_reported_is_not_reported_again() {
        let mut notices = AutosaveNotices::new();
        notices.failed("disk full").unwrap();
        for attempt in 2..REPEAT_EVERY {
            assert!(
                notices.failed("disk full").is_none(),
                "attempt {attempt} repeated a notice the user already has",
            );
        }
    }

    /// Somebody who missed the first notice must still find out, so a
    /// standing failure reminds them — on a schedule, not on every attempt.
    #[test]
    fn a_standing_failure_reminds_the_user_periodically() {
        let mut notices = AutosaveNotices::new();
        let reported: Vec<u32> = (1..=REPEAT_EVERY * 3)
            .filter(|_| notices.failed("read-only file system").is_some())
            .collect();
        // The attempts, not just how many: a count passes for an off-by-one
        // that reminds on the wrong schedule, which is how a "roughly every
        // five minutes" claim quietly becomes something else.
        assert_eq!(reported, vec![1, REPEAT_EVERY, REPEAT_EVERY * 2, REPEAT_EVERY * 3]);
    }

    /// A session where autosave works says nothing at all. A feature that
    /// announces its own success every thirty seconds is noise.
    #[test]
    fn success_is_silent_unless_it_follows_a_failure() {
        let mut notices = AutosaveNotices::new();
        for _ in 0..5 {
            assert!(notices.succeeded().is_none(), "a working autosave is quiet");
        }
        notices.failed("Permission denied").unwrap();
        assert_eq!(notices.succeeded().as_deref(), Some("Autosave is working again."));
        assert!(!notices.is_failing());
    }

    /// Recovery resets the streak, so a second outage is announced as its
    /// own rather than being swallowed as part of the first.
    #[test]
    fn a_second_outage_is_reported_like_the_first() {
        let mut notices = AutosaveNotices::new();
        notices.failed("disk full").unwrap();
        notices.succeeded().unwrap();
        assert!(
            notices.failed("disk full").is_some(),
            "the user was told it recovered, so they must be told it broke again",
        );
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

