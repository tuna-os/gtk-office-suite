// recovery.rs — versioned crash-recovery payload (DCK-003).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// What the autosave slot holds for one dirty deck, and how a later launch
// turns it back into an editing session.
//
// The slot used to hold raw package bytes (`write_deck_bytes` output): a
// rebuilt PPTX/ODP, which silently shed everything the interchange formats
// cannot carry — stable identities, layouts the reader re-mints, the
// session's source context (so the next save inspected the live original
// with no memory of what it had held), and pictures whose files had moved.
// Recovery then reinstalled the bytes with no context and claimed nothing,
// which the save dialog read as a clean bill of health.
//
// The slot now holds one versioned envelope:
//
// * the envelope magic plus one JSON object whose only forever-stable
//   fields are the version and the package-shaped bytes. A newer
//   version's envelope still yields its package; bytes with no envelope
//   at all ARE a package. Either way the existing package-shaped
//   snapshots stay readable migration input, decoded by the same readers
//   as ever.
// * `SnapshotV1` — the canonical semantic state: the full `Deck` (every
//   identity, master, layout, build and style, immune to writer/reader
//   asymmetry by construction), the source context needed to make the
//   next save safe, and the captured picture bytes with a list of
//   whatever could not be captured.
//
// There is deliberately no shared recovery service here: the shared
// envelope owns lifecycle (where a snapshot lives, atomicity, orphans),
// Decks owns this payload. A future shared envelope version (SH-003) can
// wrap it; nothing here assumes one.
//
// Undo history is not persisted: the recovered session starts with all
// committed edits and identities, dirty, with empty history.

use std::collections::BTreeMap;
use std::path::PathBuf;

use base64::engine::Engine as _;

use crate::controller::DecksController;
use crate::engine::Deck;
use crate::provenance::{DeckSourceContext, MutationSet, PinnedRevision};
use suite_common_core::interop::CompatibilityReport;

/// The payload version this build writes. Decoding accepts exactly this;
/// anything newer falls back to the envelope's package bytes (which the
/// header layout keeps extractable), never to a guess about the body.
pub const RECOVERY_VERSION: u32 = 1;

/// Largest snapshot this build will attempt to decode, in bytes. Snapshots
/// live beside the deck's pictures, so this has to admit a photo-heavy
/// deck while still refusing a corrupt length field's gigabyte promise
/// before allocation.
pub const MAX_SNAPSHOT_BYTES: usize = 256 * 1024 * 1024;

/// Largest single picture captured into a snapshot. Past this the picture
/// is listed in `unresolved_assets` instead: the snapshot still protects
/// the edits, and the next save reports the picture like any other
/// missing media.
pub const MAX_ASSET_BYTES: usize = 32 * 1024 * 1024;

/// Envelope magic, version included: `DECKSNAP` + two ASCII digits.
const MAGIC: &[u8; 10] = b"DECKSNAP01";

/// The only shape that must parse forever: version plus the package-shaped
/// fallback. Never add a required field here; everything versioned lives
/// in `SnapshotV1`. (`hint` is deliberately absent: the envelope metadata
/// beside the slot already names the format, and a newer version must be
/// able to rename anything except these two.)
#[derive(serde::Serialize, serde::Deserialize)]
struct SnapshotHead {
    v: u32,
    package: Option<String>,
}

/// The v1 body: canonical state plus everything the package cannot carry.
#[derive(serde::Serialize, serde::Deserialize)]
struct SnapshotV1 {
    v: u32,
    hint: String,
    package: Option<String>,
    rev: u64,
    deck: Deck,
    context: Option<RecoveryContext>,
    assets: Vec<RecoveryAsset>,
    unresolved: Vec<String>,
}

/// The imported revision's provenance, as stored: enough to reinstall a
/// context that makes the next save as safe as the crashed session's was.
/// Carried part *bytes* are not stored — when the live source still
/// resolves to the pinned revision they are re-captured from it, exactly
/// as open would have; when it does not, no stored bytes could make
/// writing them safe, and the stored loss report plus the incomplete
/// inventory say so instead.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct RecoveryContext {
    format: String,
    source_len: u64,
    source_hash: u64,
    has_bytes: bool,
    /// Native path bytes (Unix), so a non-UTF-8 original reinstalls
    /// exactly. `source_str` is the portable fallback.
    source_unix_bytes: Option<Vec<u8>>,
    source_str: Option<String>,
    source_loss: CompatibilityReport,
    opaque_names: Vec<String>,
    edited_slides: Vec<u64>,
    structure_changed: bool,
    masters_touched: bool,
    history_stepped: bool,
}

/// Which model node a captured picture belongs to. Slides address their
/// objects by stable id; master decorations have no object ids, so they
/// use their z-index within their master.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
enum AssetOwner {
    Slide(u64),
    Master(u64),
    /// A slide's background picture, by stable slide id (`object` is 0).
    /// Snapshots written before it existed never name it, so they decode
    /// unchanged.
    SlideBackground(u64),
}

/// One picture's bytes, captured at snapshot time so recovery does not
/// depend on the file still being where it was.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
struct RecoveryAsset {
    owner: AssetOwner,
    /// Stable object id (slides) or z-index (masters).
    object: u64,
    /// The model path at capture time, for matching and for honest
    /// reporting when the bytes cannot be restaged.
    path: String,
    bytes_b64: String,
}

impl RecoveryContext {
    fn from_pinned(pinned: &PinnedRevision) -> Self {
        #[cfg(unix)]
        let unix_bytes = {
            use std::os::unix::ffi::OsStrExt;
            Some(pinned.source.as_os_str().as_bytes().to_vec())
        };
        #[cfg(not(unix))]
        let unix_bytes = None;
        Self {
            format: pinned.format.clone(),
            source_len: pinned.len,
            source_hash: pinned.hash,
            has_bytes: pinned.has_bytes,
            source_unix_bytes: unix_bytes,
            source_str: pinned.source.to_str().map(str::to_string),
            source_loss: pinned.source_loss.clone(),
            opaque_names: pinned.opaque_names.clone(),
            edited_slides: pinned.mutations.edited_slides.iter().copied().collect(),
            structure_changed: pinned.mutations.structure_changed,
            masters_touched: pinned.mutations.masters_touched,
            history_stepped: pinned.mutations.history_stepped,
        }
    }

    /// The original path, exactly on Unix, UTF-8-only elsewhere. `None`
    /// off Unix for a non-UTF-8 original: the caller reinstalls no context
    /// rather than pin a lossy guess of the source.
    fn source_path(&self) -> Option<PathBuf> {
        #[cfg(unix)]
        if let Some(bytes) = &self.source_unix_bytes {
            use std::os::unix::ffi::OsStrExt;
            return Some(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)));
        }
        self.source_str.as_ref().map(PathBuf::from)
    }

    fn mutations(&self) -> MutationSet {
        MutationSet {
            edited_slides: self.edited_slides.iter().copied().collect(),
            structure_changed: self.structure_changed,
            masters_touched: self.masters_touched,
            history_stepped: self.history_stepped,
        }
    }
}

/// What one slot's bytes decode to.
pub enum SnapshotKind {
    /// A versioned payload: install deck, context and revision as one
    /// session. Never claims more than the payload holds — see
    /// `RecoveredSession.context`.
    Versioned(Box<RecoveredSession>),
    /// Package-shaped bytes: a legacy snapshot, or a newer version's
    /// fallback. Open with the package readers; they carry no context, so
    /// the session makes no source-loss claims about them.
    Package(Vec<u8>),
    /// Not a snapshot this build can vouch for. Try the next orphan;
    /// never hand the bytes to a format reader as the user's document.
    Unreadable,
}

/// A decoded v1 payload, ready to install: the document, the context that
/// makes its next save safe (or `None` when the payload holds none, which
/// the save dialog reads as no claims), and the capture gaps to tell the
/// user about exactly once.
pub struct RecoveredSession {
    pub deck: Deck,
    pub context: Option<DeckSourceContext>,
    pub revision: u64,
    pub format_hint: String,
    pub unresolved_assets: Vec<String>,
}

/// Capture the controller's current revision and deck as one versioned
/// snapshot.
///
/// The revision is read before and after the synchronous clone: both run
/// on the GTK thread, so they cannot disagree today, and the check is what
/// stops a future background serialization from misattributing a newer
/// revision to these bytes. Serialization failure (a geometry gone NaN, an
/// unreadable picture at package time) is an `Err` for the notifier, never
/// a half-written slot: the caller writes nothing on `Err`, so the prior
/// valid generation stays.
pub fn encode_snapshot(controller: &DecksController, format_hint: &str) -> Result<Vec<u8>, String> {
    let rev = controller.checkpoint();
    let deck = controller.deck();
    if rev != controller.checkpoint() {
        return Err("the deck changed while its snapshot was being written; retrying next tick".to_string());
    }
    let mut unresolved = Vec::new();
    let assets = capture_assets(&deck, &mut unresolved);
    // The interchange fallback: best-effort. A deck whose package cannot
    // be built (a picture's file is gone) still snapshots canonically —
    // the edits are worth protecting, the missing picture is listed in
    // `unresolved`, and the next save reports it like any missing media.
    // `None` here only removes the fallback, never the recovery.
    let package = match crate::write_deck_bytes(format_hint, &deck) {
        Ok(bytes) => Some(base64::engine::general_purpose::STANDARD.encode(&bytes)),
        Err(reason) => {
            unresolved.push(format!("no interchange fallback in this snapshot: {reason}"));
            None
        }
    };
    let context = controller
        .source_context_snapshot()
        .map(|ctx| RecoveryContext::from_pinned(&ctx.pinned_revision()));
    let snapshot = SnapshotV1 {
        v: RECOVERY_VERSION,
        hint: format_hint.to_string(),
        package,
        rev,
        deck,
        context,
        assets,
        unresolved,
    };
    let mut json = serde_json::to_vec(&snapshot)
        .map_err(|e| format!("could not snapshot the deck: {e}"))?;
    let mut out = Vec::with_capacity(MAGIC.len() + 8 + json.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(json.len() as u64).to_le_bytes());
    out.append(&mut json);
    Ok(out)
}

/// Decode one slot's bytes. Legacy package-shaped snapshots (anything
/// without the envelope magic) come back as `Package`: the migration
/// input, readable by the same readers as ever.
pub fn decode_snapshot(bytes: &[u8]) -> SnapshotKind {
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return SnapshotKind::Unreadable;
    }
    let Some(body) = strip_envelope(bytes) else {
        // No envelope: a legacy package-shaped snapshot.
        return SnapshotKind::Package(bytes.to_vec());
    };
    let head: SnapshotHead = match serde_json::from_slice(body) {
        Ok(head) => head,
        Err(_) => return SnapshotKind::Unreadable,
    };
    if head.v != RECOVERY_VERSION {
        // Explicit version handling: a newer payload's body is opaque to
        // this build, but its header still yields the package fallback.
        // Anything else is declined, never guessed at.
        return match head.package {
            Some(pkg) => decode_package_b64(&pkg),
            None => SnapshotKind::Unreadable,
        };
    }
    let snapshot: SnapshotV1 = match serde_json::from_slice(body) {
        Ok(snapshot) => snapshot,
        Err(_) => {
            return match head.package {
                Some(pkg) => decode_package_b64(&pkg),
                None => SnapshotKind::Unreadable,
            };
        }
    };
    debug_assert_eq!(snapshot.v, RECOVERY_VERSION, "the version gate above let a non-v1 body through");
    SnapshotKind::Versioned(Box::new(rebuild_session(snapshot)))
}

/// Split the envelope magic and length off, returning the JSON body.
/// `None` is not corruption — it is how legacy snapshots are recognized.
fn strip_envelope(bytes: &[u8]) -> Option<&[u8]> {
    const HEADER: usize = 10 + 8;
    if bytes.len() < HEADER || &bytes[..MAGIC.len()] != MAGIC {
        return None;
    }
    let len = usize::try_from(u64::from_le_bytes(bytes[10..HEADER].try_into().ok()?)).ok()?;
    let body = bytes.get(HEADER..)?;
    (body.len() == len).then_some(body)
}

fn decode_package_b64(pkg: &str) -> SnapshotKind {
    match base64::engine::general_purpose::STANDARD.decode(pkg) {
        Ok(bytes) => SnapshotKind::Package(bytes),
        Err(_) => SnapshotKind::Unreadable,
    }
}

/// Turn a parsed v1 payload into an installable session: restage picture
/// bytes, reinstall the source context, fill any zeroed identities.
fn rebuild_session(mut snapshot: SnapshotV1) -> RecoveredSession {
    // A missing interchange fallback always travels with a listed reason:
    // the package only fails to build for a cause the capture already
    // recorded (an unreadable picture).
    debug_assert!(snapshot.package.is_some() || !snapshot.unresolved.is_empty());
    crate::ops::ensure_all_ids(&mut snapshot.deck.slides, &mut snapshot.deck.masters);
    let mut unresolved = std::mem::take(&mut snapshot.unresolved);
    restage_assets(&mut snapshot.deck, &snapshot.assets, &mut unresolved);
    let context = snapshot.context.as_ref().and_then(|stored| {
        Some(DeckSourceContext::recovered(&PinnedRevision {
            source: stored.source_path()?,
            format: stored.format.clone(),
            len: stored.source_len,
            hash: stored.source_hash,
            has_bytes: stored.has_bytes,
            source_loss: stored.source_loss.clone(),
            opaque_names: stored.opaque_names.clone(),
            mutations: stored.mutations(),
        }))
    });
    RecoveredSession {
        deck: snapshot.deck,
        context,
        revision: snapshot.rev,
        format_hint: snapshot.hint,
        unresolved_assets: unresolved,
    }
}

/// Read every picture the deck names, bounded. One entry per unreadable
/// picture goes to `unresolved`; the snapshot still protects the edits.
fn capture_assets(deck: &Deck, unresolved: &mut Vec<String>) -> Vec<RecoveryAsset> {
    let mut by_path: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut read_bytes = |path: &str| -> Option<Vec<u8>> {
        if let Some(held) = by_path.get(path) {
            return Some(held.clone());
        }
        match std::fs::read(path) {
            Ok(bytes) if bytes.len() <= MAX_ASSET_BYTES => {
                by_path.insert(path.to_string(), bytes.clone());
                Some(bytes)
            }
            Ok(bytes) => {
                unresolved.push(format!(
                    "{path} is {} bytes, past the {}-byte snapshot capture limit; it is not held in the snapshot",
                    bytes.len(),
                    MAX_ASSET_BYTES,
                ));
                None
            }
            Err(e) => {
                unresolved.push(format!("{path} could not be captured into the snapshot: {e}"));
                None
            }
        }
    };
    let mut assets = Vec::new();
    let mut capture = |owner: AssetOwner, object: u64, path: &str| {
        if let Some(bytes) = read_bytes(path) {
            assets.push(RecoveryAsset {
                owner,
                object,
                path: path.to_string(),
                bytes_b64: base64::engine::general_purpose::STANDARD.encode(&bytes),
            });
        }
    };
    for slide in &deck.slides {
        if let Some(picture) = &slide.background_picture {
            capture(AssetOwner::SlideBackground(slide.ids.slide), 0, &picture.path);
        }
        for (index, object) in slide.objects.iter().enumerate() {
            if let crate::engine::SlideObject::Image { path, .. } = object {
                let id = slide.ids.objects.get(index).copied().unwrap_or(0);
                capture(AssetOwner::Slide(slide.ids.slide), id, path);
            }
        }
    }
    for master in &deck.masters {
        for (index, object) in master.shapes.iter().enumerate() {
            if let crate::engine::SlideObject::Image { path, .. } = object {
                capture(AssetOwner::Master(master.id), index as u64, path);
            }
        }
    }
    assets
}

/// Point recovered pictures at bytes the snapshot holds. A picture whose
/// file still holds exactly the captured bytes keeps its path; anything
/// else is restaged through the content-addressed media cache (so an
/// imported picture restages to the path it already has) and repointed.
/// Whatever cannot be restaged keeps its path and is reported: the next
/// save's missing-media preflight names it, as it would a picture lost
/// any other way.
fn restage_assets(deck: &mut Deck, assets: &[RecoveryAsset], unresolved: &mut Vec<String>) {
    for asset in assets {
        let bytes = match base64::engine::general_purpose::STANDARD.decode(&asset.bytes_b64) {
            Ok(bytes) => bytes,
            Err(_) => {
                unresolved.push(format!("{}: its snapshot bytes do not decode; keeping its path", asset.path));
                continue;
            }
        };
        let slot: Option<&mut String> = match asset.owner {
            AssetOwner::Slide(slide_id) => deck
                .slides
                .iter_mut()
                .find(|s| s.ids.slide == slide_id)
                .and_then(|s| {
                    s.ids.objects.iter().position(|id| *id == asset.object).and_then(|i| s.objects.get_mut(i))
                })
                .and_then(|o| match o {
                    crate::engine::SlideObject::Image { path, .. } => Some(path),
                    _ => None,
                }),
            AssetOwner::SlideBackground(slide_id) => deck
                .slides
                .iter_mut()
                .find(|s| s.ids.slide == slide_id)
                .and_then(|s| s.background_picture.as_mut())
                .map(|p| &mut p.path),
            AssetOwner::Master(master_id) => deck
                .masters
                .iter_mut()
                .find(|m| m.id == master_id)
                .and_then(|m| m.shapes.get_mut(asset.object as usize))
                .and_then(|o| match o {
                    crate::engine::SlideObject::Image { path, .. } => Some(path),
                    _ => None,
                }),
        };
        let Some(slot) = slot else {
            unresolved.push(format!("{}: its object is gone from the recovered deck; keeping its path", asset.path));
            continue;
        };
        if std::fs::read(&*slot).is_ok_and(|on_disk| on_disk == bytes) {
            continue;
        }
        match suite_common_core::media_cache::persist(&bytes) {
            Ok(staged) => {
                *slot = staged.to_string_lossy().into_owned();
            }
            Err(e) => {
                unresolved.push(format!("{} could not be restaged after recovery: {e}", asset.path));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_deck(text: &str) -> Deck {
        let mut deck = Deck {
            slides: vec![crate::engine::Slide { hidden: false,
                title: String::new(),
                background: String::new(),
                background_picture: None,
                objects: vec![crate::engine::SlideObject::TextBox {
                    text: text.into(),
                    x: 10.0, y: 10.0, w: 200.0, h: 50.0,
                    rotation: 0.0,
                    runs: vec![],
                    body: Default::default(),
                }],
                notes: String::new(),
                master_id: None,
                transition: Default::default(),
                builds: Vec::new(),
                ids: Default::default(),
                layout_id: None,
            }],
            masters: vec![],
            page_size: None,
            themes: Vec::new(),
            skipped_slides: Vec::new(),
        };
        crate::ops::ensure_all_ids(&mut deck.slides, &mut deck.masters);
        deck
    }

    /// The envelope round-trips a bare session; legacy bytes stay packages.
    #[test]
    fn envelope_round_trip_and_legacy_recognition() {
        let controller = DecksController::new(vec![], vec![]);
        let deck = text_deck("kept");
        controller.install_opened_deck(deck, DeckSourceContext::capture(std::path::Path::new("/nonexistent/deck.pptx")));
        let bytes = encode_snapshot(&controller, "pptx").expect("encode");
        let SnapshotKind::Versioned(back) = decode_snapshot(&bytes) else {
            panic!("a snapshot this build wrote must decode as versioned");
        };
        assert_eq!(back.revision, controller.revision());
        let crate::engine::SlideObject::TextBox { text, .. } = &back.deck.slides[0].objects[0] else {
            panic!("the text box came back as another kind");
        };
        assert_eq!(text, "kept");
        // No envelope: legacy package input.
        let package = crate::write_deck_bytes("pptx", &back.deck).expect("package");
        let SnapshotKind::Package(legacy) = decode_snapshot(&package) else {
            panic!("package-shaped bytes must decode as a package");
        };
        assert_eq!(legacy, package);
        // Oversize input is declined, never allocated-against.
        assert!(matches!(decode_snapshot(&vec![0u8; MAX_SNAPSHOT_BYTES + 1]), SnapshotKind::Unreadable));
    }

    /// A newer version's body is opaque but its package still recovers.
    #[test]
    fn newer_version_falls_back_to_its_package() {
        let package = vec![0x50, 0x4b, 0x03, 0x04];
        let head = SnapshotHead { v: RECOVERY_VERSION + 1, package: Some(base64::engine::general_purpose::STANDARD.encode(&package)) };
        let mut out = Vec::new();
        let json = serde_json::to_vec(&head).unwrap();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(json.len() as u64).to_le_bytes());
        out.extend_from_slice(&json);
        let SnapshotKind::Package(back) = decode_snapshot(&out) else {
            panic!("a newer version must fall back to its package");
        };
        assert_eq!(back, package);
    }

    /// A slide's background picture is held in the snapshot, so a crash
    /// after its file is gone still recovers it (DCK-004).
    #[test]
    fn a_background_picture_is_captured_and_restaged() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("bg.png");
        let bytes = b"background picture bytes".to_vec();
        std::fs::write(&src, &bytes).unwrap();
        let mut deck = text_deck("over a picture");
        let fill = crate::engine::FillRect { left: -0.01, right: -0.01, ..Default::default() };
        deck.slides[0].background_picture = Some(crate::engine::BackgroundPicture { path: src.to_string_lossy().into_owned(), fill });
        let controller = DecksController::new(vec![], vec![]);
        controller.install_opened_deck(deck, DeckSourceContext::capture(std::path::Path::new("/nonexistent/deck.pptx")));
        let encoded = encode_snapshot(&controller, "pptx").expect("encode");
        std::fs::remove_file(&src).unwrap();
        let SnapshotKind::Versioned(back) = decode_snapshot(&encoded) else {
            panic!("a snapshot this build wrote must decode as versioned");
        };
        assert!(back.unresolved_assets.is_empty(), "{:?}", back.unresolved_assets);
        let picture = back.deck.slides[0].background_picture.as_ref().expect("the background picture");
        assert_eq!(picture.fill, fill);
        assert_eq!(std::fs::read(&picture.path).unwrap(), bytes);
    }
}
