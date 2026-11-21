// provenance.rs — session-owned package provenance (DCK-002).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// What the opened file held that the model does not, pinned to the exact
// bytes it was read from. Two halves, mirroring Letters' LET-001
// `ImportProvenance`:
//
// * the safe subset a save carries back (`Carried`: custom XML, thumbnail,
//   ODF settings — the families `suite_common_core::carry` proves safe);
// * the scan of what no save can keep (`loss::content_a_save_drops`);
// * plus the advertised-opaque inventory: the uninterpreted part names
//   `OpaquePackage` captured at open, so a save can be held to
//   byte-identical preservation of exactly that set.
//
// All three used to be re-read from the live source path at save/dialog
// time (`window.rs::save_deck` recaptured, `loss_ui` rescanned), so a file
// deleted or replaced while open either warned about the wrong bytes or
// carried bytes the user never opened. The controller owns this context;
// the model owns content. After a successful save it is re-pinned to the
// bytes just written, which are the model's own — a file Decks wrote has
// nothing left to warn about.
//
// Graph ownership (first-PR safe subset): the carried families survive any
// modeled edit, because nothing the writers regenerate refers to them and
// they refer to nothing the writers regenerate (the `carry` safety table).
// Everything else the model does not hold is never copied half-consistent:
// the stored loss report names it before a destructive save. Edits record
// their dependencies in `MutationSet` so later relationship families can
// invalidate precisely; an unknown dependency blocks preservation or asks
// for a lossy copy, never guesses. `MutationSet` only grows within a
// session — undo included — so history can never forget that unsupported
// content became unsafe.
//
// Deliberately not synced by `collab.rs`: provenance pins one session's
// opened bytes. Sharing it across peers would resurrect one machine's
// stale parts into another's save. Each session captures its own.
//
// Nothing here fetches or executes: external relationship targets are
// never resolved to bytes, and part names are never trusted as paths.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use suite_common_core::carry::Carried;
use suite_common_core::interop::{CompatibilityReport, OpaquePackage};

use crate::ops::Op;

/// Whether a carried part name escapes the package or names a remote
/// target. Part names came out of a hostile zip directory, so the save
/// boundary (`write_deck_with_context`) and the context's own check below
/// share this one verdict: block, never guess.
pub fn carried_name_is_hostile(name: &str) -> bool {
    use std::path::Component;
    name.starts_with('/')
        || name.starts_with('\\')
        || Path::new(name).components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        || name.contains("://")
        || name.contains('\\')
}

/// The members `read_deck_with_report` claims as interpreted, per format.
/// Anything else in the package is uninterpreted at open: the opaque
/// inventory names it, the loss scan classifies it, and only the `carry`
/// safe subset is ever written back. One list, shared by the report loader
/// and the context capture, so the two cannot drift apart.
pub fn recognized_parts(format: &str) -> &'static [&'static str] {
    if format == "odp" {
        &[
            "mimetype",
            "META-INF/manifest.xml",
            "content.xml",
            "styles.xml",
            "settings.xml",
        ]
    } else {
        &[
            "[Content_Types].xml",
            "_rels/.rels",
            "ppt/presentation.xml",
            "ppt/_rels/presentation.xml.rels",
        ]
    }
}

/// The live source path measured against the imported revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceState {
    /// Untitled documents, or a context that captured nothing: callers fall
    /// back to the old live read.
    NoContext,
    /// On-disk bytes still match the imported revision.
    Intact,
    /// The source cannot be read: deleted, renamed away, or unreadable.
    /// Carried parts are unavailable; the dialog must say the original is
    /// unrecoverable rather than warn about bytes that are not there.
    Missing,
    /// The source reads back with different bytes. Never carry from it:
    /// those are not the bytes the user opened.
    Replaced,
}

/// FNV-1a over the whole file: no new dependency for an open-vs-save
/// comparison made in the same process (same function as Letters'
/// session provenance). Length is compared too, so equal hashes of
/// different lengths cannot pass.
fn fingerprint(bytes: &[u8]) -> (u64, u64) {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    (bytes.len() as u64, hash)
}

/// Which modeled content changed since open, as stable identities — never
/// positions, which reordering invalidates. The first PR's safe carried
/// subset is independent of all of these (see the module docs); the set
/// exists so follow-up relationship families can invalidate exactly their
/// known dependents while unknown ones stay blocked.
#[derive(Clone, Debug, Default)]
pub struct MutationSet {
    /// Stable slide ids (`SlideIds::slide`) touched by any slide/object op.
    pub edited_slides: BTreeSet<u64>,
    /// A slide was inserted, deleted or moved: positional guesses about
    /// source parts are void.
    pub structure_changed: bool,
    /// A master was replaced: layout/theme-dependent content is suspect.
    pub masters_touched: bool,
    /// Undo or redo moved: the dependency knowledge stays conservative —
    /// stepping history never clears it.
    pub history_stepped: bool,
}

impl MutationSet {
    /// Record one applied op's dependencies. Exhaustive over `Op`: adding
    /// an op variant fails to compile here until its policy is chosen.
    pub fn note_op(&mut self, op: &Op) {
        match op {
            Op::InsertSlide { .. } => {
                self.structure_changed = true;
            }
            Op::DeleteSlide { slide } => {
                self.structure_changed = true;
                self.edited_slides.insert(*slide);
            }
            Op::MoveSlide { slide, .. } => {
                self.structure_changed = true;
                self.edited_slides.insert(*slide);
            }
            Op::SetSlide { slide, .. } => {
                self.edited_slides.insert(*slide);
            }
            Op::InsertObject { slide, .. }
            | Op::DeleteObject { slide, .. }
            | Op::MoveObject { slide, .. }
            | Op::SetObject { slide, .. } => {
                self.edited_slides.insert(*slide);
            }
            Op::SetMaster { .. } => {
                self.masters_touched = true;
            }
        }
    }

    pub fn note_ops(&mut self, ops: &[Op]) {
        for op in ops {
            self.note_op(op);
        }
    }

    /// Undo/redo moved: keep every dependency known so far.
    pub fn note_history_step(&mut self) {
        self.history_stepped = true;
    }

    pub fn is_empty(&self) -> bool {
        self.edited_slides.is_empty()
            && !self.structure_changed
            && !self.masters_touched
            && !self.history_stepped
    }

    /// How many dependency facts are held, for introspection/tests.
    pub fn len(&self) -> usize {
        self.edited_slides.len()
            + usize::from(self.structure_changed)
            + usize::from(self.masters_touched)
            + usize::from(self.history_stepped)
    }
}

/// The imported revision's carried parts + loss findings + opaque
/// inventory, pinned to the bytes at `source` (DCK-002).
#[derive(Clone, Debug, Default)]
pub struct DeckSourceContext {
    source: PathBuf,
    format: String,
    len: u64,
    hash: u64,
    /// False when the source could not even be read at open; the context
    /// then carries nothing and behaves as no context.
    has_bytes: bool,
    carried: Carried,
    source_loss: CompatibilityReport,
    /// Uninterpreted part names captured at open: the advertised-opaque
    /// set a save must preserve byte for byte.informative only — the
    /// write path carries the `carry` safe subset, never this list raw.
    opaque_names: Vec<String>,
    /// False when the opaque capture itself failed while the file was
    /// readable: the inventory above may be partial, so introspection must
    /// say so instead of advertising completeness.
    opaque_complete: bool,
    mutations: MutationSet,
}

impl DeckSourceContext {
    /// Pin the carried parts + loss findings + opaque inventory to the
    /// revision at `source` right now. The model must already have loaded
    /// successfully: context is installed together with it, never alone.
    /// Never fetches external targets or executes embedded content; every
    /// read below is a bounded local-package inspection.
    pub fn capture(source: &Path) -> Self {
        let format = if source.to_string_lossy().to_lowercase().ends_with(".odp") {
            "odp"
        } else {
            "pptx"
        };
        let (len, hash, has_bytes) = match std::fs::read(source) {
            Ok(bytes) => {
                let (len, hash) = fingerprint(&bytes);
                (len, hash, true)
            }
            Err(_) => (0, 0, false),
        };
        let path_str = source.to_string_lossy().into_owned();
        let (opaque_names, opaque_complete) =
            match OpaquePackage::capture(source, recognized_parts(format)) {
                Ok(opaque) => (opaque.part_names().map(str::to_string).collect(), true),
                Err(_) => (Vec::new(), false),
            };
        Self {
            source: source.to_path_buf(),
            format: format.to_string(),
            len,
            hash,
            has_bytes,
            carried: suite_common_core::carry::capture(source),
            source_loss: crate::loss::content_a_save_drops(&path_str),
            opaque_names,
            opaque_complete,
            mutations: MutationSet::default(),
        }
    }

    /// The format the source was opened as (`"odp"` or `"pptx"`).
    pub fn format(&self) -> &str {
        &self.format
    }

    /// Measure the live source path against the imported revision. Never
    /// carry bytes from a path that no longer resolves to it.
    pub fn source_state(&self) -> SourceState {
        if !self.has_bytes {
            return SourceState::NoContext;
        }
        match std::fs::read(&self.source) {
            Ok(bytes) => {
                let (len, hash) = fingerprint(&bytes);
                if len == self.len && hash == self.hash {
                    SourceState::Intact
                } else {
                    SourceState::Replaced
                }
            }
            Err(_) => SourceState::Missing,
        }
    }

    /// The carried parts a save may write: the imported revision's, and
    /// only while its source still resolves to that revision. Missing or
    /// replaced sources carry nothing — writing the old bytes over a
    /// changed file, or resurrecting a deleted file's parts unasked, would
    /// be a silent carry. Recorded edits never revoke the safe subset:
    /// the `carry` safety table holds it independent of every modeled
    /// change, which is what makes unrelated edits preserve it.
    pub fn carried_for_save(&self) -> Carried {
        match self.source_state() {
            SourceState::Intact => self.carried.clone(),
            _ => Carried::default(),
        }
    }

    /// Record the dependencies of applied ops (edits and history steps).
    pub fn note_ops(&mut self, ops: &[Op]) {
        self.mutations.note_ops(ops);
    }

    /// Undo/redo moved: keep every dependency known so far.
    pub fn note_history_step(&mut self) {
        self.mutations.note_history_step();
    }

    pub fn mutations(&self) -> &MutationSet {
        &self.mutations
    }

    /// The loss the imported revision's source was scanned with at open.
    /// The save dialog shows this instead of re-scanning the live path.
    pub fn stored_source_loss(&self) -> &CompatibilityReport {
        &self.source_loss
    }

    /// Whether `target` is the file the context was captured from: only
    /// then does a Save-a-Copy offer make sense (a Save As elsewhere
    /// already is one).
    pub fn provenance_source_matches(&self, target: &Path) -> bool {
        self.source == target
    }

    /// File name of the provenance source, for dialog copy.
    pub fn provenance_source_name(&self) -> Option<String> {
        self.source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
    }

    /// Names of the parts the imported revision carried, whatever the live
    /// source looks like now — for naming what's at stake, never for
    /// writing.
    pub fn imported_carried_names(&self) -> Vec<String> {
        self.carried.names().map(str::to_string).collect()
    }

    /// The advertised-opaque inventory captured at open, and whether the
    /// capture ran whole. Introspection and tests hold saves to exactly
    /// this set; a partial capture must never be advertised as complete.
    pub fn opaque_inventory(&self) -> (&[String], bool) {
        (&self.opaque_names, self.opaque_complete)
    }

    /// Whether the imported revision held anything a save cannot rebuild
    /// from the model: carried parts or scanned loss. Only then does a
    /// missing or replaced source change what the dialog must say; an
    /// ordinary package saves on without asking, as before. A partial
    /// opaque capture also counts: there may be uninterpreted content the
    /// inventory cannot name.
    pub fn has_unrecoverable_content(&self) -> bool {
        !self.carried.is_empty()
            || !self.source_loss.destructive_features().is_empty()
            || !self.opaque_complete
    }

    /// Whether saving to `target` crosses formats (`.pptx` to `.odp` or
    /// back). `Carried::apply` already drops parts whose family does not
    /// match the target, so the write needs no further gating — but the
    /// dialog must report the incompatible opaque content by name instead
    /// of dropping it silently.
    pub fn is_cross_format_save(&self, target: &Path) -> bool {
        fn family(path: &Path) -> Option<&'static str> {
            let ext = path.extension()?.to_string_lossy().to_lowercase();
            match ext.as_str() {
                "odp" => Some("odf"),
                "pptx" => Some("ooxml"),
                _ => None,
            }
        }
        match (family(&self.source), family(target)) {
            (Some(from), Some(to)) => from != to,
            _ => false,
        }
    }

    /// Carried part names a save to `target` will not keep: the whole set
    /// on a cross-format save (families never cross), none otherwise. For
    /// the dialog's precise-consent wording, not for the write path.
    pub fn carried_names_dropped_for(&self, target: &Path) -> Vec<String> {
        if self.is_cross_format_save(target) {
            self.imported_carried_names()
        } else {
            Vec::new()
        }
    }

    /// Fail-safe hygiene for the pinned carried names: they came out of a
    /// hostile zip directory, so a hostile one may hold an absolute path,
    /// a `..` escape, or a URI before the write path trusts them. The
    /// shared `Carried` only ever carries fixed safe families, but the
    /// check lives here, at the decks save boundary, where a failure
    /// blocks the save instead of guessing.
    pub fn check_carried_hygiene(&self) -> Result<(), String> {
        for name in self.carried.names() {
            if carried_name_is_hostile(name) {
                return Err(format!("save blocked: the original file names a carried part that is not a safe relative path ({name})"));
            }
        }
        Ok(())
    }

    /// The stored facts for test introspection (`snapshot.rs`).
    pub fn facts(&self) -> ProvenanceFacts {
        ProvenanceFacts {
            format: self.format.clone(),
            carried_names: self.imported_carried_names(),
            opaque_names: self.opaque_names.clone(),
            opaque_complete: self.opaque_complete,
            edited_slides: self.mutations.edited_slides.iter().copied().collect(),
            structure_changed: self.mutations.structure_changed,
            masters_touched: self.mutations.masters_touched,
            history_stepped: self.mutations.history_stepped,
            destructive_loss_ids: self
                .source_loss
                .destructive_features()
                .iter()
                .map(|f| f.id.clone())
                .collect(),
        }
    }
}

/// The imported revision's pinned identity and findings, cloned out of a
/// session for a recovery snapshot (`recovery.rs`). Plain data, no
/// serialization: the payload owns its encoding, and this stays readable
/// without it.
#[derive(Clone, Debug)]
pub struct PinnedRevision {
    pub source: PathBuf,
    pub format: String,
    pub len: u64,
    pub hash: u64,
    pub has_bytes: bool,
    pub source_loss: CompatibilityReport,
    pub opaque_names: Vec<String>,
    pub mutations: MutationSet,
}

impl DeckSourceContext {
    /// Clone the pinned revision out for a recovery snapshot.
    pub fn pinned_revision(&self) -> PinnedRevision {
        PinnedRevision {
            source: self.source.clone(),
            format: self.format.clone(),
            len: self.len,
            hash: self.hash,
            has_bytes: self.has_bytes,
            source_loss: self.source_loss.clone(),
            opaque_names: self.opaque_names.clone(),
            mutations: self.mutations.clone(),
        }
    }

    /// Rebuild a context for a recovered session from a stored pin
    /// (DCK-003). When the live path still resolves to the pinned
    /// revision — same length and content hash — the carried parts, loss
    /// scan and opaque inventory are re-captured from it, exactly as open
    /// would have, and the stored dependency set is kept. Anything else
    /// (missing, replaced, or never readable) keeps the STORED loss
    /// report and names, with the inventory marked incomplete: a changed
    /// file's content is never inferred safe, and the next save asks
    /// about the stored findings against the live file instead of
    /// silently carrying another revision's parts.
    ///
    /// The stored completeness flag is not taken: the intact path
    /// re-captures it from identical bytes, and the degraded path cannot
    /// vouch for another revision's inventory.
    pub fn recovered(pin: &PinnedRevision) -> Self {
        let intact = pin.has_bytes
            && std::fs::read(&pin.source).is_ok_and(|bytes| {
                let (live_len, live_hash) = fingerprint(&bytes);
                live_len == pin.len && live_hash == pin.hash
            });
        if intact {
            let mut recaptured = DeckSourceContext::capture(&pin.source);
            recaptured.mutations = pin.mutations.clone();
            return recaptured;
        }
        Self {
            source: pin.source.clone(),
            format: pin.format.clone(),
            len: pin.len,
            hash: pin.hash,
            has_bytes: pin.has_bytes,
            carried: Carried::default(),
            source_loss: pin.source_loss.clone(),
            opaque_names: pin.opaque_names.clone(),
            opaque_complete: false,
            mutations: pin.mutations.clone(),
        }
    }
}

/// Test introspection for `snapshot.rs`: the stored facts about the
/// imported revision, never a live filesystem read. `source_state` is
/// deliberately absent — snapshots assert what was captured, while the
/// save boundary measures the live path at write time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProvenanceFacts {
    pub format: String,
    pub carried_names: Vec<String>,
    pub opaque_names: Vec<String>,
    pub opaque_complete: bool,
    pub edited_slides: Vec<u64>,
    pub structure_changed: bool,
    pub masters_touched: bool,
    pub history_stepped: bool,
    /// Destructive findings in the stored loss scan, by stable id: what
    /// the next save asks about.
    pub destructive_loss_ids: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sample_deck_bytes(ext: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("deck.{ext}"));
        let mut deck = crate::engine::Deck::new();
        deck.slides[0].objects.clear();
        crate::write_deck(path.to_str().unwrap(), &deck).unwrap();
        (dir, path)
    }

    #[test]
    fn intact_source_carries_and_missing_or_replaced_carries_nothing() {
        let (dir, path) = sample_deck_bytes("pptx");
        let ctx = DeckSourceContext::capture(&path);
        assert_eq!(ctx.source_state(), SourceState::Intact);
        // A deck Decks wrote carries nothing and warns about nothing.
        assert!(!ctx.has_unrecoverable_content());
        // Deletion reads as missing, never as intact.
        std::fs::remove_file(&path).unwrap();
        assert_eq!(ctx.source_state(), SourceState::Missing);
        assert!(ctx.carried_for_save().is_empty());
        // Different bytes at the same path read as replaced, and their
        // parts are never carried into the save.
        std::fs::write(&path, b"entirely different bytes").unwrap();
        assert_eq!(ctx.source_state(), SourceState::Replaced);
        assert!(ctx.carried_for_save().is_empty());
        let _ = dir;
    }

    /// A pptx Decks wrote, plus safe-family parts our writer never emits:
    /// the imported revision carries them, as a foreign file would.
    fn pptx_with_carried_parts() -> (tempfile::TempDir, PathBuf, Vec<String>) {
        let (dir, path) = sample_deck_bytes("pptx");
        {
            let file = std::fs::File::options()
                .read(true)
                .write(true)
                .open(&path)
                .unwrap();
            let mut zip = zip::ZipWriter::new_append(file).unwrap();
            let options = zip::write::SimpleFileOptions::default();
            for (name, body) in [
                ("customXml/item1.xml", "<a/>"),
                ("docProps/thumbnail.jpeg", "preview"),
            ] {
                zip.start_file(name, options).unwrap();
                zip.write_all(body.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        let names = vec![
            "customXml/item1.xml".to_string(),
            "docProps/thumbnail.jpeg".to_string(),
        ];
        (dir, path, names)
    }

    #[test]
    fn unrelated_edits_keep_the_safe_subset_while_replacement_carries_nothing() {
        let (_dir, path, names) = pptx_with_carried_parts();
        let mut ctx = DeckSourceContext::capture(&path);
        assert_eq!(ctx.source_state(), SourceState::Intact);
        assert_eq!(ctx.imported_carried_names(), names);
        assert!(ctx.has_unrecoverable_content());
        // Recorded edits never revoke the safe subset: it is independent
        // of every modeled change by construction.
        ctx.note_ops(&[
            Op::DeleteSlide { slide: 3 },
            Op::SetMaster {
                id: 11,
                master: Box::new(crate::engine::MasterSlide {
                    id: 11,
                    name: String::new(),
                    background: String::new(),
                    default_font: String::new(),
                    shapes: vec![],
                    theme_id: None,
                    color_map: Default::default(),
                    layouts: vec![],
                }),
            },
        ]);
        assert!(!ctx.mutations().is_empty());
        assert_eq!(
            ctx.carried_for_save().names().collect::<Vec<_>>(),
            names.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        // A replaced source carries none of the opened revision's bytes.
        std::fs::write(&path, b"entirely different bytes").unwrap();
        assert_eq!(ctx.source_state(), SourceState::Replaced);
        assert!(ctx.carried_for_save().is_empty());
        // ... but the context still names what is at stake, for the dialog.
        assert_eq!(ctx.imported_carried_names(), names);
    }

    #[test]
    fn cross_format_save_names_every_dropped_carried_part() {
        let (_dir, path, names) = pptx_with_carried_parts();
        let ctx = DeckSourceContext::capture(&path);
        assert!(!ctx.is_cross_format_save(&path));
        assert!(ctx.carried_names_dropped_for(&path).is_empty());
        // Same-format saves keep the set; a cross-format Save As drops it
        // whole, and the dialog must name every part by name.
        let odp = path.with_extension("odp");
        assert!(ctx.is_cross_format_save(&odp));
        assert_eq!(ctx.carried_names_dropped_for(&odp), names);
    }
    #[test]
    fn every_op_variant_records_a_dependency() {
        use crate::engine::SlideObject;
        let slide = || {
            Box::new(crate::engine::Slide { hidden: false,
                title: String::new(),
                background: String::new(),
                background_picture: None,
                objects: vec![],
                notes: String::new(),
                master_id: None,
                transition: Default::default(),
                builds: vec![],
                ids: Default::default(),
                layout_id: None,
            })
        };
        let object = || {
            Box::new(SlideObject::Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
                rotation: 0.0,
            })
        };
        // One op per variant: each must leave a trace. Adding a variant
        // breaks the exhaustive `note_op` match until its policy is set.
        let ops = vec![
            Op::InsertSlide {
                at: 0,
                slide: slide(),
            },
            Op::DeleteSlide { slide: 7 },
            Op::MoveSlide { slide: 7, to: 0 },
            Op::SetSlide {
                slide: 7,
                props: Box::new(crate::ops::SlideProps::of(&slide())),
            },
            Op::InsertObject {
                slide: 7,
                at: 0,
                id: 9,
                object: object(),
            },
            Op::DeleteObject { slide: 7, id: 9 },
            Op::MoveObject {
                slide: 7,
                id: 9,
                to: 0,
            },
            Op::SetObject {
                slide: 7,
                id: 9,
                object: object(),
            },
            Op::SetMaster {
                id: 11,
                master: Box::new(crate::engine::MasterSlide {
                    id: 11,
                    name: String::new(),
                    background: String::new(),
                    default_font: String::new(),
                    shapes: vec![],
                    theme_id: None,
                    color_map: Default::default(),
                    layouts: vec![],
                }),
            },
        ];
        for op in &ops {
            let mut set = MutationSet::default();
            set.note_op(op);
            assert!(!set.is_empty(), "op left no dependency: {op:?}");
        }
        let mut set = MutationSet::default();
        set.note_ops(&ops);
        assert!(set.edited_slides.contains(&7));
        assert!(set.structure_changed);
        assert!(set.masters_touched);
        let mut undoing = MutationSet::default();
        undoing.note_history_step();
        assert!(!undoing.is_empty(), "history steps must stay conservative");
    }

    #[test]
    fn hostile_carried_names_block_the_save() {
        // A hostile directory entry smuggled into the safe families must
        // fail the write boundary, never ride into the saved package.
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hostile.pptx");
        {
            let file = std::fs::File::create(&path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            for (name, body) in [
                ("[Content_Types].xml", "<Types/>"),
                ("customXml/item1.xml", "<a/>"),
                ("customXml/../evil.xml", "<evil/>"),
            ] {
                zip.start_file(name, options).unwrap();
                zip.write_all(body.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        let ctx = DeckSourceContext::capture(&path);
        assert!(ctx
            .imported_carried_names()
            .iter()
            .any(|n| n.contains("..")));
        assert!(ctx.check_carried_hygiene().is_err());
    }
}
