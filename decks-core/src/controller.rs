// controller.rs — canonical slide-list state and controller for Decks.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// First slice of issue #103 (GTK-free canonical controllers): owns the
// slide list, master list, undo history, and dirty flag as a single unit
// so `decks/src/window.rs` stops holding four separately-cloned Rcs in
// lockstep by convention. Object-level editing (add/move/delete object,
// text edits) still goes through decks_core::undo commands directly from
// window.rs for now — narrowing this to slide-list management only keeps
// the slice small enough to land fully wired in one pass rather than as
// a half-adopted abstraction.

use std::cell::{Cell, RefCell};
use std::rc::Rc;


use crate::engine::{MasterSlide, Slide, SlideObject};
use crate::ops::Op;
use suite_common_core::ops::History;

/// How the selected objects are being worked on (DCK-005). The mode is
/// session state owned by the controller, not a document edit: changing
/// it never dirties the deck or touches history.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionMode {
    /// Objects are selected as whole boxes.
    #[default]
    Object,
    /// A text range inside one text box is being edited.
    Text,
    /// A table cell of one table object is current.
    TableCell,
    /// Navigation is scoped inside one group.
    GroupScope,
}

/// A validated caret or range inside one text object: byte offsets that
/// are Unicode scalar boundaries of that object's current text, with
/// `start <= end`. Session state, never persistence: undo that changes
/// the text revalidates or drops it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextRange {
    pub object_id: u64,
    pub start: usize,
    pub end: usize,
}

/// What the window shows as current: stable ids throughout (DCK-005).
/// The slide, the objects on it in selection order, the primary object
/// inspectors and keyboard commands act on, and how they are being
/// worked on. Owned by the controller; the shell derives view indices
/// from it at the point of use and never stores positions of its own.
/// Selection-only movement is not a document edit: it never dirties the
/// deck, clears redo, or bumps the revision.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionState {
    pub slide_id: Option<u64>,
    pub object_ids: Vec<u64>,
    pub primary_object_id: Option<u64>,
    pub mode: SelectionMode,
    pub text_range: Option<TextRange>,
}

impl SelectionState {
    pub fn none() -> Self {
        SelectionState { slide_id: None, object_ids: Vec::new(), primary_object_id: None, mode: SelectionMode::Object, text_range: None }
    }

    /// Slide `slide_id` current, object `object` (if any) selected on it.
    pub fn single(slide_id: u64, object: Option<u64>) -> Self {
        SelectionState {
            slide_id: Some(slide_id),
            object_ids: object.into_iter().collect(),
            primary_object_id: object,
            mode: SelectionMode::Object,
            text_range: None,
        }
    }
}

/// What one controller transaction did (DCK-005): whether it changed the
/// document, which stable ids it touched, the repaired selection and the
/// revision the document now stands at. Reads use snapshots and borrows
/// that cannot mutate behind history; GTK signals go out after every
/// borrow is dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxnOutcome {
    pub applied: bool,
    pub changed_slides: Vec<u64>,
    pub changed_objects: Vec<u64>,
    pub revision: u64,
    pub selection: SelectionState,
}

/// Why an in-place text edit could not be committed (DCK-007).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSessionError {
    /// No text session is open.
    NoSession,
    /// Its text box was deleted, or the view it was on closed.
    Gone,
    /// Its text box was changed by something else meanwhile.
    Changed,
}

impl std::fmt::Display for TextSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            TextSessionError::NoSession => "No text is being edited",
            TextSessionError::Gone => "The text box being edited is no longer on the slide",
            TextSessionError::Changed => "The text box was changed while it was being edited",
        })
    }
}

pub struct DecksController {
    pub slides: Rc<RefCell<Vec<Slide>>>,
    pub masters: Rc<RefCell<Vec<MasterSlide>>>,
    /// The deck's declared slide size, in EMU (`Deck.page_size`, DCK-006):
    /// installed at open/recovery, saved back by `deck()`. No edit path
    /// changes it in this cutover; it rides the session, not the history.
    pub page_size: Rc<Cell<Option<(f64, f64)>>>,
    /// The deck's retained themes (`Deck.themes`, DCK-006): installed at
    /// open/recovery, saved back by `deck()`. Masters name these by
    /// stable id; edits to masters (SetMaster) carry their `theme_id`
    /// through history, but no edit path retargets themes yet.
    pub themes: Rc<RefCell<Vec<crate::engine::theme::Theme>>>,
    pub dirty: Rc<Cell<bool>>,
    /// The deck's on-disk path, or `None` for an unsaved new document.
    /// Canonical document identity (#103) — window.rs reads/writes this
    /// shared cell rather than tracking its own copy (mirrors
    /// tables_core::controller::WorkbookController::file_path).
    pub file_path: Rc<RefCell<Option<String>>>,
    /// Every edit's inverse ops (ops.rs). Applying, undoing and redoing
    /// through it are the only ways the controller changes the slides,
    /// and each marks the deck dirty.
    history: RefCell<History<Op>>,
    /// The selection before each recorded history step, in step order:
    /// undo restores the selection the undone step saw, redo the one it
    /// left. Merged keystrokes (notes word typing) share one entry, as
    /// their ops share one step. Repaired against the slides on restore,
    /// never trusted blindly.
    undo_sels: RefCell<Vec<SelectionState>>,
    redo_sels: RefCell<Vec<SelectionState>>,
    /// Whether the previous apply extended a notes word-typing step: the
    /// controller's mirror of the history's merge flag, so the stacks
    /// above stay in step with coalesced records.
    merge_chain: Cell<bool>,
    /// The master view (`edit_master`): the deck's slides and history while
    /// the master is in their place.
    master_edit: RefCell<Option<MasterEdit>>,
    /// The imported revision's provenance (DCK-002): carried parts, loss
    /// findings and opaque inventory pinned to the opened bytes, plus the
    /// dependencies edits recorded since. Session-owned, never widget
    /// state, and never reconstructed from the filename alone. `None`
    /// for new documents and recovered snapshots with no original bytes.
    /// Keyed by stable slide ids (DCK-005), never positions.
    source_context: RefCell<Option<crate::provenance::DeckSourceContext>>,
    /// Monotonic session clock, bumped by every applied, undone or redone
    /// step. The save dialog binds its consent to this alongside the
    /// source fingerprint: an edit between the question and the
    /// confirmation un-confirms it (DCK-002).
    revision: Cell<u64>,
    /// The revision the last successful save wrote, if any. Marking saved
    /// is conditional on the revision actually committed
    /// (`acknowledge_save`): an edit (or undo/redo) after the output was
    /// staged leaves the deck dirty. `None` means unsaved since open.
    saved_revision: Cell<Option<u64>>,
    /// Controller-owned selection (DCK-005): stable ids, never positions.
    selection: RefCell<SelectionState>,
    /// The in-place text edit under way, if any (DCK-007): a working copy
    /// of one text box with its own undo. It is not the document: commit
    /// records it as one history step, cancel drops it without trace, and
    /// document undo/redo are refused while it is open so a text
    /// session's Ctrl+Z can never undo a slide.
    text_session: RefCell<Option<crate::rich_text::TextSession>>,
}

/// What the master view put aside, to put back on Done.
struct MasterEdit {
    /// The master being edited, by stable id.
    master_id: u64,
    slides: Vec<Slide>,
    history: History<Op>,
    selection: SelectionState,
    undo_sels: Vec<SelectionState>,
    redo_sels: Vec<SelectionState>,
    merge_chain: bool,
    /// The slide the view was opened from, by stable id, to return to.
    from_slide_id: u64,
}

/// The master as the stand-in slide the master view edits: its
/// decorations as the slide's objects, its background as the slide's.
fn master_as_slide(m: &MasterSlide) -> Slide {
    Slide { hidden: false,
        title: m.name.clone(),
        background: m.background.clone(),
        background_picture: None,
        objects: m.shapes.clone(),
        notes: String::new(),
        // No master: its decorations are the objects being edited, and
        // must not be drawn a second time underneath.
        master_id: None,
        transition: Default::default(),
        builds: Vec::new(),
        ids: Default::default(),
        layout_id: None,
    }
}

/// `m` with the stand-in slide's edits. The master's id, name, font, size
/// and layouts are not editable decorations: they survive untouched, so
/// the Done transaction only ever replaces background and shapes.
fn master_from_slide(m: &MasterSlide, s: &Slide) -> MasterSlide {
    MasterSlide { background: s.background.clone(), shapes: s.objects.clone(), ..m.clone() }
}

/// The slide ids and object ids a deck holds, for transaction reporting.
fn id_sets(slides: &[Slide]) -> (std::collections::BTreeSet<u64>, std::collections::BTreeSet<u64>) {
    (
        slides.iter().map(|s| s.ids.slide).collect(),
        slides.iter().flat_map(|s| s.ids.objects.iter().copied()).collect(),
    )
}

/// The slide ids one op group names, where still present afterwards.
fn ops_slide_ids(ops: &[Op]) -> Vec<u64> {
    ops.iter().filter_map(|op| match op {
        Op::InsertSlide { .. } => None,
        Op::DeleteSlide { slide }
        | Op::MoveSlide { slide, .. }
        | Op::SetSlide { slide, .. }
        | Op::InsertObject { slide, .. }
        | Op::DeleteObject { slide, .. }
        | Op::MoveObject { slide, .. }
        | Op::SetObject { slide, .. } => Some(*slide),
        Op::SetMaster { .. } => None,
    }).collect()
}

/// New ids the transaction minted, plus the named ids it touched that are
/// still present: what a view should refresh.
fn changed_ids(before: &std::collections::BTreeSet<u64>, after: &std::collections::BTreeSet<u64>, named: Vec<u64>) -> Vec<u64> {
    let mut out: Vec<u64> = after.difference(before).copied().chain(named.into_iter().filter(|id| after.contains(id))).collect();
    out.sort_unstable();
    out.dedup();
    out
}

impl DecksController {
    pub fn new(slides: Vec<Slide>, masters: Vec<MasterSlide>) -> Self {
        let mut slides = slides;
        let mut masters = masters;
        crate::ops::ensure_all_ids(&mut slides, &mut masters);
        let selection = slides.first().map(|s| SelectionState::single(s.ids.slide, None)).unwrap_or_else(SelectionState::none);
        Self {
            slides: Rc::new(RefCell::new(slides)),
            masters: Rc::new(RefCell::new(masters)),
            page_size: Rc::new(Cell::new(None)),
            themes: Rc::new(RefCell::new(Vec::new())),
            dirty: Rc::new(Cell::new(false)),
            file_path: Rc::new(RefCell::new(None)),
            history: RefCell::new(History::default()),
            undo_sels: RefCell::new(Vec::new()),
            redo_sels: RefCell::new(Vec::new()),
            merge_chain: Cell::new(false),
            master_edit: RefCell::new(None),
            source_context: RefCell::new(None),
            revision: Cell::new(0),
            saved_revision: Cell::new(None),
            selection: RefCell::new(selection),
            text_session: RefCell::new(None),
        }
    }

    /// The session's edit clock: every applied, undone or redone step
    /// bumps it. The save dialog binds consent to this value alongside
    /// the source fingerprint.
    pub fn revision(&self) -> u64 {
        self.revision.get()
    }

    /// The revision a save should bind its consent and output to: the
    /// current revision at the moment output starts. Committing the save
    /// must go through [`DecksController::acknowledge_save`], which
    /// refuses to mark clean when an edit landed meanwhile.
    pub fn checkpoint(&self) -> u64 {
        self.revision.get()
    }

    /// The revision the last successful save wrote, if any.
    pub fn saved_revision(&self) -> Option<u64> {
        self.saved_revision.get()
    }

    /// Mark the current revision saved, after the writer and the
    /// destination commit both succeed. Prefer `acknowledge_save` for
    /// asynchronous output, where the committed revision may be stale.
    pub fn mark_saved(&self) {
        self.saved_revision.set(Some(self.revision.get()));
        self.dirty.set(false);
    }

    /// Mark `checkpoint` saved when it is still the current revision.
    /// Returns whether the deck is now clean: an applied, undone or
    /// redone step after the output was staged leaves it dirty, and the
    /// save must be re-staged rather than silently marked clean.
    pub fn acknowledge_save(&self, checkpoint: u64) -> bool {
        if self.revision.get() == checkpoint {
            self.mark_saved();
            true
        } else {
            false
        }
    }

    /// Install the deck just read together with its source context, only
    /// after a successful import — model, report and provenance land as
    /// one session, never piecemeal. Ids the reader left at 0 are
    /// assigned once here; the selection returns to the first slide and
    /// the save clock restarts unsaved.
    pub fn install_opened_deck(
        &self,
        deck: crate::engine::Deck,
        context: crate::provenance::DeckSourceContext,
    ) {
        *self.slides.borrow_mut() = deck.slides;
        *self.masters.borrow_mut() = deck.masters;
        self.page_size.set(deck.page_size);
        *self.themes.borrow_mut() = deck.themes;
        crate::ops::ensure_all_ids(&mut self.slides.borrow_mut(), &mut self.masters.borrow_mut());
        *self.source_context.borrow_mut() = Some(context);
        *self.history.borrow_mut() = History::default();
        self.undo_sels.borrow_mut().clear();
        self.redo_sels.borrow_mut().clear();
        self.merge_chain.set(false);
        self.revision.set(0);
        self.saved_revision.set(None);
        let first = self.slides.borrow().first().map(|s| SelectionState::single(s.ids.slide, None)).unwrap_or_else(SelectionState::none);
        *self.selection.borrow_mut() = first;
        // A replaced deck has nothing for an open text edit to commit to.
        *self.text_session.borrow_mut() = None;
    }

    /// Clone of the session's source context for a recovery snapshot, if
    /// the session holds one. Snapshots carry the pin, never the live
    /// session's cell: encoding must not observe an edit landing halfway
    /// through it.
    pub fn source_context_snapshot(&self) -> Option<crate::provenance::DeckSourceContext> {
        self.source_context.borrow().clone()
    }

    /// Install a recovered session as one transaction: the deck, its
    /// source context (or none, for legacy snapshots that carry none and
    /// must make no source-loss claims), and the revision the snapshot
    /// pinned — never piecemeal. Recovered work is dirty even when its
    /// last persisted revision matches an old source, and history starts
    /// empty: the snapshot holds committed state, not the undo past.
    pub fn install_recovered_session(
        &self,
        deck: crate::engine::Deck,
        context: Option<crate::provenance::DeckSourceContext>,
        revision: u64,
    ) {
        *self.slides.borrow_mut() = deck.slides;
        *self.masters.borrow_mut() = deck.masters;
        self.page_size.set(deck.page_size);
        *self.themes.borrow_mut() = deck.themes;
        crate::ops::ensure_all_ids(&mut self.slides.borrow_mut(), &mut self.masters.borrow_mut());
        *self.source_context.borrow_mut() = context;
        *self.history.borrow_mut() = History::default();
        self.undo_sels.borrow_mut().clear();
        self.redo_sels.borrow_mut().clear();
        self.merge_chain.set(false);
        self.revision.set(revision);
        self.saved_revision.set(None);
        self.dirty.set(true);
        let first = self.slides.borrow().first().map(|s| SelectionState::single(s.ids.slide, None)).unwrap_or_else(SelectionState::none);
        *self.selection.borrow_mut() = first;
        *self.text_session.borrow_mut() = None;
    }

    /// Drop the session's provenance: recovery adopts a snapshot's bytes,
    /// not its source, so the next save falls back to inspecting the live
    /// original instead of pinning a deleted scratch file.
    pub fn clear_source_context(&self) {
        *self.source_context.borrow_mut() = None;
    }

    /// The live source measured against the imported revision.
    pub fn source_state(&self) -> crate::provenance::SourceState {
        self.source_context
            .borrow()
            .as_ref()
            .map(|c| c.source_state())
            .unwrap_or(crate::provenance::SourceState::NoContext)
    }

    /// The carried parts a save may write: the imported revision's, and
    /// only while its source still resolves to that revision.
    pub fn carried_for_save(&self) -> suite_common_core::carry::Carried {
        self.source_context
            .borrow()
            .as_ref()
            .map(|c| c.carried_for_save())
            .unwrap_or_default()
    }

    /// Re-pin provenance to the bytes just written, after the writer and
    /// the destination commit both succeed. Mutations retire with the old
    /// revision: the file on disk is the model's own now.
    pub fn repin_after_save(&self, path: &str) {
        *self.source_context.borrow_mut() = Some(crate::provenance::DeckSourceContext::capture(
            std::path::Path::new(path),
        ));
    }

    /// What the imported revision holds that no save can rebuild, for the
    /// save dialog's precise-consent wording. Reads the stored scan, never
    /// the live path.
    pub fn stored_source_loss(&self) -> Option<suite_common_core::interop::CompatibilityReport> {
        self.source_context
            .borrow()
            .as_ref()
            .map(|c| c.stored_source_loss().clone())
    }

    /// Names of the parts the imported revision carried, for naming what
    /// a missing or replaced source puts at stake.
    pub fn imported_carried_names(&self) -> Vec<String> {
        self.source_context
            .borrow()
            .as_ref()
            .map(|c| c.imported_carried_names())
            .unwrap_or_default()
    }

    /// Whether the imported revision held anything a save cannot rebuild
    /// from the model.
    pub fn has_unrecoverable_content(&self) -> bool {
        self.source_context
            .borrow()
            .as_ref()
            .is_some_and(|c| c.has_unrecoverable_content())
    }

    /// File name of the provenance source, for dialog copy.
    pub fn provenance_source_name(&self) -> Option<String> {
        self.source_context
            .borrow()
            .as_ref()
            .and_then(|c| c.provenance_source_name())
    }

    /// Whether `target` is the file the context was captured from.
    pub fn provenance_source_matches(&self, target: &str) -> bool {
        self.source_context.borrow().as_ref().is_some_and(|c| {
            c.provenance_source_matches(std::path::Path::new(target))
        })
    }

    /// Carried part names a save to `target` will not keep (the whole set
    /// on a cross-format Save As, none otherwise), for the dialog.
    pub fn carried_names_dropped_for(&self, target: &str) -> Vec<String> {
        self.source_context
            .borrow()
            .as_ref()
            .map(|c| c.carried_names_dropped_for(std::path::Path::new(target)))
            .unwrap_or_default()
    }

    /// The stored provenance facts for test introspection (`snapshot.rs`),
    /// or `None` when the session holds no context.
    pub fn provenance_facts(&self) -> Option<crate::provenance::ProvenanceFacts> {
        self.source_context.borrow().as_ref().map(|c| c.facts())
    }

    // ── The master view ───────────────────────────────────────────────
    //
    // Keynote's Edit Master: the master of one slide takes the slides'
    // place, as one slide whose objects are its decorations, and every
    // editing command works on it as on a slide, with its own undo. Done
    // (`finish_master`) puts the deck back and records the whole edit as
    // one `SetMaster` step; `cancel_master` puts everything back with no
    // step. The temporary editing scope is explicit: the view holds the
    // deck's slides, history and selection aside, and either Done or
    // Cancel restores the slides, history and selection — cancel restores
    // the original document and history exactly. Saving meanwhile saves
    // the deck with the master as edited so far (`deck`).

    /// The master one slide uses: its `master_id`, or the deck's first
    /// master when it names none. `None` with no masters at all.
    fn master_id_for_slide(&self, slide_id: u64) -> Option<u64> {
        let slides = self.slides.borrow();
        let slide = slides.iter().find(|s| s.ids.slide == slide_id)?;
        if let Some(id) = slide.master_id {
            return self.masters.borrow().iter().any(|m| m.id == id).then_some(id);
        }
        self.masters.borrow().first().map(|m| m.id)
    }

    /// Open the master view on the master slide `slide_id` uses. Returns
    /// the master's stable id, or `None` when the view is already open or
    /// there is no such master.
    pub fn edit_master(&self, slide_id: u64) -> Option<u64> {
        if self.master_edit.borrow().is_some() || self.text_session.borrow().is_some() {
            return None;
        }
        let master_id = self.master_id_for_slide(slide_id)?;
        let masters = self.masters.borrow();
        let master = masters.iter().find(|m| m.id == master_id)?.clone();
        drop(masters);
        let mut stand_in = master_as_slide(&master);
        crate::ops::ensure_all_ids(std::slice::from_mut(&mut stand_in), &mut []);
        let stand_in_id = stand_in.ids.slide;
        let slides = std::mem::replace(&mut *self.slides.borrow_mut(), vec![stand_in]);
        let history = std::mem::take(&mut *self.history.borrow_mut());
        let selection = std::mem::take(&mut *self.selection.borrow_mut());
        let undo_sels = std::mem::take(&mut *self.undo_sels.borrow_mut());
        let redo_sels = std::mem::take(&mut *self.redo_sels.borrow_mut());
        let merge_chain = self.merge_chain.replace(false);
        *self.selection.borrow_mut() = SelectionState::single(stand_in_id, None);
        *self.master_edit.borrow_mut() = Some(MasterEdit { master_id, slides, history, selection, undo_sels, redo_sels, merge_chain, from_slide_id: slide_id });
        Some(master_id)
    }

    /// The master being edited, by stable id, while the master view is open.
    pub fn editing_master(&self) -> Option<u64> {
        self.master_edit.borrow().as_ref().map(|e| e.master_id)
    }

    /// Close the master view: the slides come back, and the master's edits
    /// go into the deck's history as one step. Returns the id of the slide
    /// the view was opened from.
    pub fn finish_master(&self) -> Option<u64> {
        let edit = self.master_edit.borrow_mut().take()?;
        let stand_in = std::mem::replace(&mut *self.slides.borrow_mut(), edit.slides);
        *self.history.borrow_mut() = edit.history;
        *self.undo_sels.borrow_mut() = edit.undo_sels;
        *self.redo_sels.borrow_mut() = edit.redo_sels;
        self.merge_chain.set(edit.merge_chain);
        *self.selection.borrow_mut() = SelectionState::single(edit.from_slide_id, None);
        self.repair_selection();
        if let Some(s) = stand_in.first() {
            let old = self.masters.borrow().iter().find(|m| m.id == edit.master_id).cloned();
            if let Some(old) = old {
                let new = master_from_slide(&old, s);
                if format!("{new:?}") != format!("{old:?}") {
                    self.apply_ops(vec![Op::SetMaster { id: edit.master_id, master: Box::new(new) }]);
                }
            }
        }
        Some(edit.from_slide_id)
    }

    /// Close the master view without keeping anything: the original
    /// slides, history and selection come back exactly, and no history
    /// step is recorded. Returns the id of the slide the view was opened
    /// from.
    pub fn cancel_master(&self) -> Option<u64> {
        let edit = self.master_edit.borrow_mut().take()?;
        *self.slides.borrow_mut() = edit.slides;
        *self.history.borrow_mut() = edit.history;
        *self.undo_sels.borrow_mut() = edit.undo_sels;
        *self.redo_sels.borrow_mut() = edit.redo_sels;
        self.merge_chain.set(edit.merge_chain);
        *self.selection.borrow_mut() = edit.selection;
        self.repair_selection();
        Some(edit.from_slide_id)
    }

    /// The deck as it would be saved: while the master view is open, the
    /// deck's own slides with the master as edited so far.
    pub fn deck(&self) -> crate::engine::Deck {
        let edit = self.master_edit.borrow();
        let mut masters = self.masters.borrow().clone();
        let slides = match edit.as_ref() {
            Some(e) => {
                if let (Some(m), Some(s)) = (masters.iter().find(|m| m.id == e.master_id).cloned(), self.slides.borrow().first().cloned()) {
                    if let Some(slot) = masters.iter_mut().find(|m| m.id == e.master_id) {
                        *slot = master_from_slide(&m, &s);
                    }
                }
                e.slides.clone()
            }
            None => self.slides.borrow().clone(),
        };
        crate::engine::Deck { slides, masters, page_size: self.page_size.get(), themes: self.themes.borrow().clone() }
    }

    pub fn slide_count(&self) -> usize {
        self.slides.borrow().len()
    }

    // ── Controller-owned selection ──────────────────────────────────
    //
    // The window never stores slide or object positions of its own: it
    // reads this state and derives view indices (sidebar rows, canvas hit
    // order) at the point of use. Moving the selection is session state,
    // not a document edit.

    /// The current selection, by stable ids.
    pub fn selection(&self) -> SelectionState {
        self.selection.borrow().clone()
    }

    /// Replace the selection with `sel`, repaired against the slides now
    /// present. Never dirties the deck, clears redo, or bumps the
    /// revision: selection-only movement is not a document edit.
    pub fn set_selection(&self, sel: SelectionState) {
        *self.selection.borrow_mut() = self.repaired(sel);
    }

    /// Make slide `slide_id` current with nothing selected on it. Returns
    /// whether the slide exists.
    pub fn select_slide(&self, slide_id: u64) -> bool {
        if self.slides.borrow().iter().all(|s| s.ids.slide != slide_id) {
            return false;
        }
        *self.selection.borrow_mut() = SelectionState::single(slide_id, None);
        true
    }

    /// Select `object_ids` (in order) on slide `slide_id`, with `primary`
    /// the object inspectors act on, worked on as `mode`. Unknown ids are
    /// dropped; `primary` falls back to the first id kept. Returns
    /// whether the slide exists.
    pub fn select_objects(&self, slide_id: u64, object_ids: &[u64], primary: Option<u64>, mode: SelectionMode) -> bool {
        if self.slides.borrow().iter().all(|s| s.ids.slide != slide_id) {
            return false;
        }
        let slides = self.slides.borrow();
        let slide = slides.iter().find(|s| s.ids.slide == slide_id).expect("checked above");
        let kept: Vec<u64> = object_ids.iter().copied().filter(|id| slide.ids.objects.contains(id)).collect();
        let primary = primary.filter(|p| kept.contains(p)).or_else(|| kept.first().copied());
        let mode = if kept.is_empty() { SelectionMode::Object } else { mode };
        *self.selection.borrow_mut() = SelectionState { slide_id: Some(slide_id), object_ids: kept, primary_object_id: primary, mode, text_range: None };
        true
    }

    /// Point the text caret or range at `start..end` of object
    /// `object_id`'s text on the selected slide, for text mode. The
    /// bounds must be Unicode scalar boundaries of that object's current
    /// text with `start <= end`; anything else (a non-text object, a
    /// mid-character split, an unknown id) is rejected and leaves the
    /// selection exactly as it was.
    pub fn set_text_range(&self, object_id: u64, start: usize, end: usize) -> bool {
        let sel = self.selection.borrow().clone();
        let Some(slide_id) = sel.slide_id else { return false };
        let slides = self.slides.borrow();
        let Some(slide) = slides.iter().find(|s| s.ids.slide == slide_id) else { return false };
        let Some(at) = slide.object_index(object_id) else { return false };
        let text = match &slide.objects[at] {
            SlideObject::TextBox { text, .. } => text.clone(),
            _ => return false,
        };
        drop(slides);
        if start > end || end > text.len() || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return false;
        }
        *self.selection.borrow_mut() = SelectionState {
            slide_id: Some(slide_id),
            object_ids: vec![object_id],
            primary_object_id: Some(object_id),
            mode: SelectionMode::Text,
            text_range: Some(TextRange { object_id, start, end }),
        };
        true
    }

    /// Repair `sel` against the slides now present: a deleted slide falls
    /// back to the first slide, deleted objects (and a primary among
    /// them) are pruned, an empty object list resets the mode, and a text
    /// range whose object or bounds are gone is dropped. Pure: callers
    /// store the result.
    fn repaired(&self, sel: SelectionState) -> SelectionState {
        let slides = self.slides.borrow();
        let slide_id = sel.slide_id.filter(|id| slides.iter().any(|s| s.ids.slide == *id)).or_else(|| slides.first().map(|s| s.ids.slide));
        let Some(slide_id) = slide_id else { return SelectionState::none() };
        let slide = slides.iter().find(|s| s.ids.slide == slide_id).expect("checked above");
        let object_ids: Vec<u64> = sel.object_ids.into_iter().filter(|id| slide.ids.objects.contains(id)).collect();
        let primary_object_id = sel.primary_object_id.filter(|p| object_ids.contains(p)).or_else(|| object_ids.first().copied());
        let mut mode = if object_ids.is_empty() { SelectionMode::Object } else { sel.mode };
        let text_range = match (mode, sel.text_range) {
            (SelectionMode::Text, Some(r)) if object_ids.contains(&r.object_id) => {
                let valid = slide.object_index(r.object_id).map(|at| match &slide.objects[at] {
                    SlideObject::TextBox { text, .. } => r.start <= r.end && r.end <= text.len() && text.is_char_boundary(r.start) && text.is_char_boundary(r.end),
                    _ => false,
                }).unwrap_or(false);
                if valid { Some(r) } else { mode = SelectionMode::Object; None }
            }
            _ => None,
        };
        SelectionState { slide_id: Some(slide_id), object_ids, primary_object_id, mode, text_range }
    }

    /// Repair the owned selection against the slides now present (after
    /// an apply, undo, redo or master-view round trip) and return it.
    /// Undo restores identities and selection: an undone insert prunes
    /// what it took away instead of pointing past the end.
    pub fn repair_selection(&self) -> SelectionState {
        let repaired = self.repaired(self.selection.borrow().clone());
        *self.selection.borrow_mut() = repaired.clone();
        repaired
    }

    // ── View indices, derived only ────────────────────────────────
    //
    // GTK widgets (sidebar rows, canvas hit order, combo rows) are
    // positional. Every index below is derived from a stable id at the
    // point of use; nothing persists or communicates one.

    /// The position of slide `slide_id`, if it is still in the deck.
    pub fn slide_index(&self, slide_id: u64) -> Option<usize> {
        self.slides.borrow().iter().position(|s| s.ids.slide == slide_id)
    }

    /// The z-position of object `object_id` on slide `slide_id`.
    pub fn object_index(&self, slide_id: u64, object_id: u64) -> Option<usize> {
        self.slides.borrow().iter().find(|s| s.ids.slide == slide_id)?.object_index(object_id)
    }

    /// The id of the slide at view position `index`.
    pub fn slide_id_at(&self, index: usize) -> Option<u64> {
        self.slides.borrow().get(index).map(|s| s.ids.slide)
    }

    /// The id of the object at z-position `object` on the slide at view
    /// position `slide`.
    pub fn object_id_at(&self, slide: usize, object: usize) -> Option<u64> {
        self.slides.borrow().get(slide)?.ids.objects.get(object).copied()
    }

    /// The view position of the selected slide, if it is still present.
    pub fn selected_slide_index(&self) -> Option<usize> {
        let sel = self.selection.borrow();
        let id = sel.slide_id?;
        drop(sel);
        self.slide_index(id)
    }

    /// The z-positions of the selected objects, in selection order.
    pub fn selected_object_indices(&self) -> Vec<usize> {
        let sel = self.selection.borrow().clone();
        let Some(slide_id) = sel.slide_id else { return Vec::new() };
        let slides = self.slides.borrow();
        let Some(slide) = slides.iter().find(|s| s.ids.slide == slide_id) else { return Vec::new() };
        sel.object_ids.into_iter().filter_map(|id| slide.object_index(id)).collect()
    }

    /// The z-position of the primary object, if still selected.
    pub fn primary_object_index(&self) -> Option<usize> {
        let sel = self.selection.borrow().clone();
        let (slide_id, object_id) = (sel.slide_id?, sel.primary_object_id?);
        drop(sel);
        self.object_index(slide_id, object_id)
    }

    /// Insert a new slide at view position `at` (typically the slide
    /// count) and return the new slide's stable id. The inserted slide's
    /// incoming ids are discarded and minted fresh, so a duplicate or
    /// paste is a slide of its own. `None` in the master view.
    pub fn add_slide(&self, at: usize, slide: Slide) -> Option<u64> {
        // The master view has one slide, the master: no others.
        if self.master_edit.borrow().is_some() {
            return None;
        }
        let at = at.min(self.slides.borrow().len());
        let mut slide = slide;
        slide.ids = Default::default();
        // Fresh ids are minted from the session space at apply time; read
        // the new slide's id back by position afterwards.
        if !self.apply_ops(vec![Op::InsertSlide { at, slide: Box::new(slide) }]) {
            return None;
        }
        self.slide_id_at(at)
    }

    /// Delete slide `slide_id`. Returns false in the master view, for an
    /// unknown slide, or for the only slide (deletion is a no-op — Decks
    /// always keeps at least one slide). The owned selection is repaired
    /// to what is still there.
    pub fn delete_slide(&self, slide_id: u64) -> bool {
        if self.master_edit.borrow().is_some() {
            return false;
        }
        {
            let slides = self.slides.borrow();
            if slides.len() <= 1 || slides.iter().all(|s| s.ids.slide != slide_id) {
                return false;
            }
        }
        self.apply_ops(vec![Op::DeleteSlide { slide: slide_id }])
    }

    /// Put a copy of slide `slide_id` (its objects, notes, transition,
    /// builds, master and layout) right after it, as one undo step, and
    /// return the copy's stable id. `None` in the master view or for an
    /// unknown slide. Duplicate allocates fresh ids once: undo and redo
    /// keep the copy's identity.
    pub fn duplicate_slide(&self, slide_id: u64) -> Option<u64> {
        if self.master_edit.borrow().is_some() {
            return None;
        }
        let (at, copy) = {
            let slides = self.slides.borrow();
            let at = slides.iter().position(|s| s.ids.slide == slide_id)?;
            (at, slides[at].clone())
        };
        self.add_slide(at + 1, copy)
    }

    /// Move slide `slide_id` one place up (earlier). False when it is
    /// already first or unknown. The selection follows the id, not the
    /// position.
    pub fn move_slide_up(&self, slide_id: u64) -> bool {
        let at = match self.slide_index(slide_id) {
            Some(at) if at > 0 => at,
            _ => return false,
        };
        self.apply_ops(vec![Op::MoveSlide { slide: slide_id, to: at - 1 }])
    }

    /// Move slide `slide_id` one place down (later). False when it is
    /// already last or unknown.
    pub fn move_slide_down(&self, slide_id: u64) -> bool {
        let at = match self.slide_index(slide_id) {
            Some(at) => at,
            None => return false,
        };
        if at + 1 >= self.slides.borrow().len() {
            return false;
        }
        self.apply_ops(vec![Op::MoveSlide { slide: slide_id, to: at + 1 }])
    }

    /// Every slide, object, master, layout and placeholder given its id
    /// first. Reads that need ids call this before borrowing.
    fn ids_ready(&self) {
        crate::ops::ensure_all_ids(&mut self.slides.borrow_mut(), &mut self.masters.borrow_mut());
    }

    /// Apply `ops` as one undo step, all or nothing, and report what the
    /// transaction did: whether it changed the document, which stable ids
    /// it touched, the repaired selection and the new revision. Invalid,
    /// no-op and cancelled changes record nothing: they do not dirty the
    /// document, clear redo, or bump the revision.
    pub fn apply_ops_txn(&self, ops: Vec<Op>) -> TxnOutcome {
        self.apply_ops_inner(ops, false)
    }

    /// The `merged` flag is for notes word typing (`set_notes`): when the
    /// ops coalesce into the previous history step, the selection entry
    /// for that step is already on the stack.
    fn apply_ops_inner(&self, ops: Vec<Op>, merged: bool) -> TxnOutcome {
        let pre_selection = self.selection.borrow().clone();
        let failed = TxnOutcome { applied: false, changed_slides: Vec::new(), changed_objects: Vec::new(), revision: self.revision.get(), selection: pre_selection.clone() };
        if ops.is_empty() {
            return failed;
        }
        let before = id_sets(&self.slides.borrow());
        let inverses = crate::ops::with_doc(&mut self.slides.borrow_mut(), &mut self.masters.borrow_mut(), |doc| {
            suite_common_core::ops::apply_all(doc, &ops)
        });
        let Ok(inverses) = inverses else { return failed };
        if inverses.is_empty() {
            // Every op addressed something deleted: nothing happened.
            return failed;
        }
        // One user action, one step.
        self.history.borrow_mut().record(inverses);
        if !merged {
            self.undo_sels.borrow_mut().push(pre_selection);
        }
        self.redo_sels.borrow_mut().clear();
        self.merge_chain.set(false);
        if let Some(context) = self.source_context.borrow_mut().as_mut() {
            context.note_ops(&ops);
        }
        self.revision.set(self.revision.get().wrapping_add(1));
        self.dirty.set(true);
        let after = id_sets(&self.slides.borrow());
        let selection = self.repair_selection();
        TxnOutcome {
            applied: true,
            changed_slides: changed_ids(&before.0, &after.0, ops_slide_ids(&ops)),
            changed_objects: changed_ids(&before.1, &after.1, Vec::new()),
            revision: self.revision.get(),
            selection,
        }
    }

    /// Apply `ops` as one undo step, all or nothing.
    /// Returns false (and records nothing) when there is nothing to do or
    /// the group doesn't apply. The step's undo is the group's inverse ops.
    pub fn apply_ops(&self, ops: Vec<Op>) -> bool {
        self.apply_ops_txn(ops).applied
    }

    /// Edit the objects of slide `slide_id` in place (same count) as one
    /// undo step: one `SetObject` per object `edit` changes. The closure
    /// still sees positional objects, but the caller names the slide by
    /// stable id and the ops name every object by stable id.
    pub fn edit_objects(&self, slide_id: u64, edit: impl FnOnce(&mut Vec<SlideObject>)) -> bool {
        let ops = {
            self.ids_ready();
            let slides = self.slides.borrow();
            let Some(si) = slides.iter().position(|s| s.ids.slide == slide_id) else { return false };
            let slide = &slides[si];
            let mut objects = slide.objects.clone();
            edit(&mut objects);
            if objects.len() != slide.objects.len() {
                return false;
            }
            crate::ops::set_objects(&slides, si, &objects)
        };
        self.apply_ops(ops)
    }

    /// Crop the picture `object_id` so it fills its box without distorting
    /// it, the overflow cut equally from both sides (PowerPoint's Crop to
    /// Fill), or, with `fill` false, show all of it again. One undo step;
    /// `false` when it is not a picture or nothing changes.
    pub fn crop_picture(&self, slide_id: u64, object_id: u64, fill: bool) -> bool {
        let at = match self.object_index(slide_id, object_id) {
            Some(at) => at,
            None => return false,
        };
        self.edit_objects(slide_id, |objects| {
            if let Some(SlideObject::Image { path, w, h, crop, .. }) = objects.get_mut(at) {
                *crop = if fill {
                    match crate::image_px::natural_size_pt(path) {
                        Some((iw, ih)) => crate::engine::Crop::fill(iw, ih, *w, *h),
                        None => *crop,
                    }
                } else {
                    crate::engine::Crop::default()
                };
            }
        })
    }

    /// Append `object` to slide `slide_id` and return its fresh stable id.
    /// Paste and duplicate allocate fresh ids once: redo keeps them.
    /// `None` for an unknown slide.
    pub fn add_object(&self, slide_id: u64, object: SlideObject) -> Option<u64> {
        let op = {
            self.ids_ready();
            let slides = self.slides.borrow();
            let slide = slides.iter().find(|s| s.ids.slide == slide_id)?;
            Op::InsertObject {
                slide: slide_id,
                at: slide.objects.len(),
                id: crate::ops::next_id(&slides),
                object: Box::new(object),
            }
        };
        let id = match &op {
            Op::InsertObject { id, .. } => *id,
            _ => unreachable!(),
        };
        if self.apply_ops(vec![op]) { Some(id) } else { None }
    }

    /// Delete object `object_id` of slide `slide_id` (a tombstone: see
    /// ops.rs). The op takes the object from the slide itself, so a
    /// caller holding a stale copy cannot retarget it.
    pub fn delete_object(&self, slide_id: u64, object_id: u64) -> bool {
        if self.object_index(slide_id, object_id).is_none() {
            return false;
        }
        self.apply_ops(vec![Op::DeleteObject { slide: slide_id, id: object_id }])
    }

    pub fn move_object(&self, slide_id: u64, object_id: u64, dx: f64, dy: f64) {
        let at = match self.object_index(slide_id, object_id) {
            Some(at) => at,
            None => return,
        };
        self.edit_objects(slide_id, |o| {
            if let Some(obj) = o.get_mut(at) {
                crate::undo::offset_object(obj, dx, dy);
            }
        });
    }

    /// Replace the object's text with `new_text`, keeping the surrounding
    /// run and paragraph styles. The canonical object, not a caller's
    /// stale snapshot, owns the diff and the undo: the common prefix and
    /// suffix stay, and the middle is one `RichText::replace`.
    pub fn change_text(&self, slide_id: u64, object_id: u64, new_text: String) {
        self.edit_text(slide_id, object_id, |rt| {
            let text = &rt.text;
            if *text == new_text {
                return false;
            }
            let start: usize = text.chars().zip(new_text.chars())
                .take_while(|(a, b)| a == b).map(|(c, _)| c.len_utf8()).sum();
            let suffix: usize = text[start..].chars().rev().zip(new_text[start..].chars().rev())
                .take_while(|(a, b)| a == b).map(|(c, _)| c.len_utf8()).sum();
            let end = text.len() - suffix;
            rt.replace(start, end, &new_text[start..new_text.len() - suffix], None).unwrap_or(false)
        });
    }

    /// Replace bytes `start..end` of text box `object_id`'s text with
    /// `text` (DCK-007), in the style of the run the range starts in, as
    /// one undo step. Unaffected runs and paragraph styles stay as they
    /// are. `false`, recording nothing, for an invalid range, a non-text
    /// object or no change.
    pub fn replace_text_range(&self, slide_id: u64, object_id: u64, start: usize, end: usize, text: &str) -> bool {
        self.edit_text(slide_id, object_id, |rt| rt.replace(start, end, text, None).unwrap_or(false))
    }

    /// Apply `edit` to bytes `start..end` of text box `object_id` as far
    /// as its scope reaches (characters, the paragraphs touched, or the
    /// box: `FormatEdit::scope`), as one undo step. A range edit never
    /// widens to the whole box; `format_objects` is the whole-box command.
    pub fn format_text_range(&self, slide_id: u64, object_id: u64, start: usize, end: usize, edit: &crate::format::FormatEdit) -> bool {
        self.edit_text(slide_id, object_id, |rt| rt.format(start, end, edit).unwrap_or(false))
    }

    /// One text edit of one box as one undo step: `edit` changes a copy
    /// of its text and says whether it changed anything. Refused while a
    /// text session is open on that box, whose commit would otherwise
    /// overwrite the edit.
    fn edit_text(&self, slide_id: u64, object_id: u64, edit: impl FnOnce(&mut crate::rich_text::RichText) -> bool) -> bool {
        if self.text_session.borrow().as_ref().is_some_and(|s| s.object_id == object_id) {
            return false;
        }
        let Some(at) = self.object_index(slide_id, object_id) else { return false };
        let rt = {
            let slides = self.slides.borrow();
            slides.iter().find(|s| s.ids.slide == slide_id).and_then(|s| s.objects.get(at)).and_then(crate::rich_text::RichText::of)
        };
        let Some(mut rt) = rt else { return false };
        if !edit(&mut rt) {
            return false;
        }
        self.edit_objects(slide_id, |o| {
            if let Some(obj) = o.get_mut(at) {
                rt.write_to(obj);
            }
        })
    }

    /// Open an in-place text edit of text box `object_id` (DCK-007): a
    /// working copy with its own undo, the caret at its end, the
    /// selection in text mode. Nothing in the document changes until
    /// [`DecksController::commit_text_session`]. `false` when a session is
    /// already open or the object is not a text box on that slide.
    pub fn begin_text_session(&self, slide_id: u64, object_id: u64) -> bool {
        if self.text_session.borrow().is_some() {
            return false;
        }
        let Some(at) = self.object_index(slide_id, object_id) else { return false };
        let rt = {
            let slides = self.slides.borrow();
            slides.iter().find(|s| s.ids.slide == slide_id).and_then(|s| s.objects.get(at)).and_then(crate::rich_text::RichText::of)
        };
        let Some(rt) = rt else { return false };
        let session = crate::rich_text::TextSession::new(slide_id, object_id, self.editing_master(), rt);
        let (start, end) = session.range();
        self.select_objects(slide_id, &[object_id], Some(object_id), SelectionMode::Object);
        self.set_text_range(object_id, start, end);
        *self.text_session.borrow_mut() = Some(session);
        true
    }

    /// Whether an in-place text edit is open.
    pub fn has_text_session(&self) -> bool {
        self.text_session.borrow().is_some()
    }

    /// Read the open text session. `f` must not call back into the
    /// controller.
    pub fn text_session<R>(&self, f: impl FnOnce(&crate::rich_text::TextSession) -> R) -> Option<R> {
        self.text_session.borrow().as_ref().map(f)
    }

    /// Edit the open text session (type, format, move the caret, its own
    /// undo). The document, its history and its dirty state are never
    /// touched. `f` must not call back into the controller.
    pub fn edit_text_session<R>(&self, f: impl FnOnce(&mut crate::rich_text::TextSession) -> R) -> Option<R> {
        self.text_session.borrow_mut().as_mut().map(f)
    }

    /// Record the open text session as one undo step, end it, and leave
    /// its box selected in object mode. `Ok(false)` when it changed
    /// nothing: nothing is recorded or dirtied. When its box is gone or
    /// was changed underneath it, the session stays open and the error
    /// says why, so the editor can keep focus and show it.
    pub fn commit_text_session(&self) -> Result<bool, TextSessionError> {
        let Some(session) = self.text_session.borrow_mut().take() else { return Err(TextSessionError::NoSession) };
        let target = self.session_target(&session);
        let at = match target {
            Ok(at) => at,
            Err(e) => {
                *self.text_session.borrow_mut() = Some(session);
                return Err(e);
            }
        };
        // The step's undo restores this selection: the box, as it was
        // selected before the edit began.
        self.select_objects(session.slide_id, &[session.object_id], Some(session.object_id), SelectionMode::Object);
        if !session.changed() {
            return Ok(false);
        }
        let working = session.working().clone();
        Ok(self.edit_objects(session.slide_id, |o| {
            if let Some(obj) = o.get_mut(at) {
                working.write_to(obj);
            }
        }))
    }

    /// End the open text session without changing anything: the
    /// document, its history, dirty state and revision are exactly as
    /// before it began. Returns whether one was open.
    pub fn cancel_text_session(&self) -> bool {
        let Some(session) = self.text_session.borrow_mut().take() else { return false };
        if self.slide_index(session.slide_id).is_some() {
            self.select_objects(session.slide_id, &[session.object_id], Some(session.object_id), SelectionMode::Object);
        } else {
            self.repair_selection();
        }
        true
    }

    /// Where the session's box is now, if it is still the box the
    /// session opened: same view (deck or master), same slide, same text.
    fn session_target(&self, session: &crate::rich_text::TextSession) -> Result<usize, TextSessionError> {
        if self.editing_master() != session.master {
            return Err(TextSessionError::Gone);
        }
        let at = self.object_index(session.slide_id, session.object_id).ok_or(TextSessionError::Gone)?;
        let slides = self.slides.borrow();
        let current = slides.iter().find(|s| s.ids.slide == session.slide_id).and_then(|s| s.objects.get(at)).and_then(crate::rich_text::RichText::of);
        match current {
            Some(rt) if rt == *session.original() => Ok(at),
            Some(_) => Err(TextSessionError::Changed),
            None => Err(TextSessionError::Gone),
        }
    }

    pub fn resize_object(&self, slide_id: u64, object_id: u64, new_bounds: (f64, f64, f64, f64)) {
        let at = match self.object_index(slide_id, object_id) {
            Some(at) => at,
            None => return,
        };
        self.edit_objects(slide_id, |o| {
            if let Some(obj) = o.get_mut(at) {
                let (x, y, w, h) = new_bounds;
                crate::undo::set_obj_bounds(obj, x, y, w, h);
            }
        });
    }

    pub fn rotate_object(&self, slide_id: u64, object_id: u64, new_angle: f64) {
        let at = match self.object_index(slide_id, object_id) {
            Some(at) => at,
            None => return,
        };
        self.edit_objects(slide_id, |o| {
            if let Some(obj) = o.get_mut(at) {
                crate::undo::set_obj_rotation(obj, new_angle);
            }
        });
    }

    /// Align the objects `object_ids` names on slide `slide_id` as one
    /// undo step. Unknown ids are ignored; fewer than two known objects
    /// is a no-op.
    pub fn align_objects(&self, slide_id: u64, object_ids: &[u64], mode: crate::undo::AlignMode) {
        use crate::undo::{obj_bounds, AlignMode};
        let slides = self.slides.borrow();
        let Some(slide) = slides.iter().find(|s| s.ids.slide == slide_id) else { return };
        let mut resolved: Vec<(u64, (f64, f64, f64, f64))> = object_ids.iter().filter_map(|id| {
            slide.object_index(*id).map(|at| (*id, obj_bounds(&slide.objects[at])))
        }).collect();
        if resolved.len() < 2 { return; }
        resolved.sort_by_key(|r| r.0);
        resolved.dedup_by(|a, b| a.0 == b.0);
        if resolved.len() < 2 { return; }
        let bounds_list: Vec<(f64, f64, f64, f64)> = resolved.iter().map(|(_, b)| *b).collect();

        let target = match mode {
            AlignMode::Left => bounds_list.iter().map(|b| b.0).fold(f64::INFINITY, f64::min),
            AlignMode::Top => bounds_list.iter().map(|b| b.1).fold(f64::INFINITY, f64::min),
            AlignMode::Right => bounds_list.iter().map(|b| b.0 + b.2).fold(f64::NEG_INFINITY, f64::max),
            AlignMode::Bottom => bounds_list.iter().map(|b| b.1 + b.3).fold(f64::NEG_INFINITY, f64::max),
            AlignMode::Center => bounds_list.iter().map(|b| b.0 + b.2 / 2.0).sum::<f64>() / bounds_list.len() as f64,
            AlignMode::Middle => bounds_list.iter().map(|b| b.1 + b.3 / 2.0).sum::<f64>() / bounds_list.len() as f64,
        };

        let ids: Vec<u64> = resolved.iter().map(|(id, _)| *id).collect();
        let new_positions: Vec<(f64, f64)> = bounds_list.iter().map(|&(x, y, w, h)| {
            match mode {
                AlignMode::Left => (target, y),
                AlignMode::Right => (target - w, y),
                AlignMode::Center => (target - w / 2.0, y),
                AlignMode::Top => (x, target),
                AlignMode::Bottom => (x, target - h),
                AlignMode::Middle => (x, target - h / 2.0),
            }
        }).collect();
        drop(slides);
        self.place_objects(slide_id, &ids, &new_positions);
    }

    /// Move the objects `object_ids` names to `positions`
    /// (bounding-box origins) as one undo step.
    fn place_objects(&self, slide_id: u64, object_ids: &[u64], positions: &[(f64, f64)]) {
        let ats: Vec<Option<usize>> = object_ids.iter().map(|id| self.object_index(slide_id, *id)).collect();
        self.edit_objects(slide_id, |o| {
            for (at, &(x, y)) in ats.iter().zip(positions) {
                if let Some(at) = at {
                    if let Some(obj) = o.get_mut(*at) {
                        crate::undo::set_obj_origin(obj, x, y);
                    }
                }
            }
        });
    }

    pub fn distribute_objects(&self, slide_id: u64, object_ids: &[u64], mode: crate::undo::DistributeMode) {
        use crate::undo::{obj_bounds, DistributeMode};
        let slides = self.slides.borrow();
        let Some(slide) = slides.iter().find(|s| s.ids.slide == slide_id) else { return };
        let mut items: Vec<(u64, (f64, f64, f64, f64))> = object_ids.iter().filter_map(|id| {
            slide.object_index(*id).map(|at| (*id, obj_bounds(&slide.objects[at])))
        }).collect();
        items.sort_by_key(|item| item.0);
        items.dedup_by(|a, b| a.0 == b.0);
        if items.len() < 3 { return; }

        if mode == DistributeMode::Horizontal {
            items.sort_by(|a, b| a.1.0.partial_cmp(&b.1.0).unwrap());
            let min_x = items.first().unwrap().1.0;
            let max_x = items.last().unwrap().1.0;
            let total_span = max_x - min_x;
            let step = total_span / (items.len() - 1) as f64;
            let new_pos: Vec<(f64, f64)> = items.iter().enumerate().map(|(idx, item)| {
                (min_x + idx as f64 * step, item.1.1)
            }).collect();
            let sorted_ids: Vec<u64> = items.iter().map(|i| i.0).collect();
            drop(slides);
            self.place_objects(slide_id, &sorted_ids, &new_pos);
        } else {
            items.sort_by(|a, b| a.1.1.partial_cmp(&b.1.1).unwrap());
            let min_y = items.first().unwrap().1.1;
            let max_y = items.last().unwrap().1.1;
            let total_span = max_y - min_y;
            let step = total_span / (items.len() - 1) as f64;
            let new_pos: Vec<(f64, f64)> = items.iter().enumerate().map(|(idx, item)| {
                (item.1.0, min_y + idx as f64 * step)
            }).collect();
            let sorted_ids: Vec<u64> = items.iter().map(|i| i.0).collect();
            drop(slides);
            self.place_objects(slide_id, &sorted_ids, &new_pos);
        }
    }

    /// Move object `object_id` in the z-order as one undo step. Returns
    /// whether it moved. The selection needs no follow: the id is
    /// unchanged by the move.
    pub fn z_order_object(&self, slide_id: u64, object_id: u64, op: crate::undo::ZOrderOp) -> bool {
        let at = match self.object_index(slide_id, object_id) {
            Some(at) => at,
            None => return false,
        };
        let len = self.slides.borrow().iter().find(|s| s.ids.slide == slide_id).map(|s| s.objects.len()).unwrap_or(0);
        let to = crate::undo::z_order_index(at, len, op);
        if to == at {
            return false;
        }
        self.apply_ops(vec![Op::MoveObject { slide: slide_id, id: object_id, to }])
    }

    /// Apply one Format inspector edit to the objects `object_ids` names
    /// on slide `slide_id`, as one undo step. Returns whether anything
    /// changed.
    pub fn format_objects(&self, slide_id: u64, object_ids: &[u64], edit: &crate::format::FormatEdit) -> bool {
        let ats: Vec<usize> = {
            let slides = self.slides.borrow();
            let Some(slide) = slides.iter().find(|s| s.ids.slide == slide_id) else { return false };
            let mut seen = Vec::new();
            let mut ats = Vec::new();
            for id in object_ids {
                if seen.contains(id) {
                    continue;
                }
                seen.push(*id);
                if let Some(at) = slide.object_index(*id) {
                    ats.push(at);
                }
            }
            ats
        };
        self.edit_objects(slide_id, |o| {
            for at in ats {
                if let Some(obj) = o.get_mut(at) {
                    crate::format::apply_edit(obj, edit);
                }
            }
        })
    }

    /// Set object `object_id`'s build in (`out` false) or out on slide
    /// `slide_id`, or remove it (`effect` None), as one undo step. A new
    /// build goes last in the slide's build order. The build names the
    /// object by stable id, so reordering never retargets it. Returns
    /// whether anything changed.
    pub fn set_build(&self, slide_id: u64, object_id: u64, out: bool, effect: Option<crate::builds::BuildEffect>) -> bool {
        use crate::builds::Build;
        let (old, new) = {
            let slides = self.slides.borrow();
            let Some(slide) = slides.iter().find(|s| s.ids.slide == slide_id) else { return false };
            if !slide.ids.objects.contains(&object_id) {
                return false;
            }
            let old = slide.builds.clone();
            let mut new = old.clone();
            match (new.iter_mut().find(|b| b.object == object_id && b.out == out), effect) {
                (Some(b), Some(e)) => b.effect = e,
                (Some(_), None) => new.retain(|b| !(b.object == object_id && b.out == out)),
                (None, Some(e)) => new.push(Build { object: object_id, effect: e, out }),
                (None, None) => {}
            }
            (old, new)
        };
        if old == new {
            return false;
        }
        self.set_slide_props(slide_id, |p| p.builds = new)
    }

    /// Set slide `slide_id`'s speaker notes to `notes`, as the notes pane
    /// does on every change. Typing a word is one undo step, as in a text
    /// editor: a letter or digit added to a run of them joins that run's
    /// step, and anything else (a space, a deletion, a paste) starts one.
    pub fn set_notes(&self, slide_id: u64, notes: &str) -> bool {
        let typing = match self.slides.borrow().iter().find(|s| s.ids.slide == slide_id) {
            Some(s) => one_word_char_added(&s.notes, notes),
            None => return false,
        };
        // A word character joins the previous word step (one selection
        // entry for the whole word, as one history step); anything else
        // starts a step of its own.
        let merged = typing && self.merge_chain.get();
        self.history.borrow_mut().set_merge(typing);
        let changed = self.set_slide_props_inner(slide_id, merged, |p| p.notes = notes.to_string());
        if !changed {
            // Nothing recorded, so the mark must not reach the next step.
            self.history.borrow_mut().set_merge(false);
            self.merge_chain.set(false);
        } else {
            self.merge_chain.set(typing);
        }
        changed
    }

    /// Put slide `slide_id` on layout `layout_id` of its master (Format ▸
    /// Layout): its placeholder boxes move to the layout's places and it
    /// gains an empty box for each place it had nothing for
    /// (`layouts::arrange`). One undo step. False for an unknown slide,
    /// an unknown layout, or in the master view.
    pub fn apply_layout(&self, slide_id: u64, layout_id: u64) -> bool {
        if self.master_edit.borrow().is_some() {
            return false;
        }
        self.ids_ready();
        let target: crate::layouts::Layout = {
            let slides = self.slides.borrow();
            let s = match slides.iter().find(|s| s.ids.slide == slide_id) {
                Some(s) => s.clone(),
                None => return false,
            };
            let masters = self.masters.borrow();
            let master = match s.master_id.and_then(|id| masters.iter().find(|m| m.id == id)) {
                Some(m) => m.clone(),
                None => match masters.first() {
                    Some(m) => m.clone(),
                    None => return false,
                },
            };
            match master.layouts.iter().find(|l| l.id == layout_id) {
                Some(l) => l.clone(),
                None => return false,
            }
        };
        self.apply_arranged_layout(slide_id, &target)
    }

    /// Put slide `slide_id` on the standard layout at view position `at`
    /// (the picker's row for a master that has no layouts yet): the
    /// master gains the standard six with fresh stable ids and the slide
    /// takes the chosen one, all as one undo step. This is the only
    /// positional layout entry point, for masters with nothing to name
    /// yet; every other layout reference is a stable id.
    pub fn apply_standard_layout(&self, slide_id: u64, at: usize) -> bool {
        if self.master_edit.borrow().is_some() {
            return false;
        }
        self.ids_ready();
        let (master_id, standard) = {
            let slides = self.slides.borrow();
            let s = match slides.iter().find(|s| s.ids.slide == slide_id) {
                Some(s) => s.clone(),
                None => return false,
            };
            let masters = self.masters.borrow();
            let master = match s.master_id.and_then(|id| masters.iter().find(|m| m.id == id)) {
                Some(m) => m.clone(),
                None => match masters.first() {
                    Some(m) => m.clone(),
                    None => return false,
                },
            };
            if !master.layouts.is_empty() {
                return false;
            }
            (master.id, crate::layouts::standard())
        };
        // Fresh stable ids for the six, minted from the session space now
        // so the target below names the id the op will store.
        let mut layouts = standard;
        {
            let slides = self.slides.borrow();
            let masters = self.masters.borrow();
            let mut next = crate::ops::next_session_id(&slides, &masters);
            for l in layouts.iter_mut() {
                l.id = next;
                next += 1;
                for p in l.placeholders.iter_mut() {
                    p.id = next;
                    next += 1;
                }
            }
        }
        let target = match layouts.get(at) {
            Some(l) => l.clone(),
            None => return false,
        };
        let mut master = match self.masters.borrow().iter().find(|m| m.id == master_id).cloned() {
            Some(m) => m,
            None => return false,
        };
        master.layouts = layouts;
        // The SetMaster op goes first; the arranged-slide ops follow in
        // the same group, so undo takes the added layouts away too.
        let mut ops = vec![Op::SetMaster { id: master_id, master: Box::new(master) }];
        match self.arranged_ops(slide_id, master_id, &target) {
            Some(more) => ops.extend(more),
            None => return false,
        }
        self.apply_ops(ops)
    }

    /// The ops that put slide `slide_id` on `target`: new boxes appended
    /// with fresh ids, in-place boxes as `SetObject`s, and the slide's
    /// master/layout ids. `None` for an unknown slide. Pure: the caller
    /// applies the returned group (possibly with preceding ops) as one
    /// transaction.
    fn arranged_ops(&self, slide_id: u64, master_id: u64, target: &crate::layouts::Layout) -> Option<Vec<Op>> {
        let slides = self.slides.borrow();
        let si = slides.iter().position(|s| s.ids.slide == slide_id)?;
        let s = &slides[si];
        let arranged = crate::layouts::arrange(s, target);
        // The boxes the slide has, in place; then the new ones.
        let kept = s.objects.len();
        let mut ops = Vec::new();
        let mut props = crate::ops::SlideProps::of(s);
        props.layout_id = Some(target.id);
        props.master_id = Some(master_id);
        if props != crate::ops::SlideProps::of(s) {
            ops.push(Op::SetSlide { slide: slide_id, props: Box::new(props) });
        }
        ops.extend(crate::ops::set_objects(&slides, si, &arranged[..kept]));
        let first = crate::ops::next_id(&slides);
        for (k, (o, id)) in arranged[kept..].iter().zip(first..).enumerate() {
            ops.push(Op::InsertObject { slide: slide_id, at: kept + k, id, object: Box::new(o.clone()) });
        }
        Some(ops)
    }

    /// Apply the arranged-layout ops for `target` as one undo step.
    fn apply_arranged_layout(&self, slide_id: u64, target: &crate::layouts::Layout) -> bool {
        let ops = {
            let slides = self.slides.borrow();
            let s = match slides.iter().find(|s| s.ids.slide == slide_id) {
                Some(s) => s.clone(),
                None => return false,
            };
            let masters = self.masters.borrow();
            let master = match s.master_id.and_then(|id| masters.iter().find(|m| m.id == id)) {
                Some(m) => m.clone(),
                None => match masters.first() {
                    Some(m) => m.clone(),
                    None => return false,
                },
            };
            match self.arranged_ops(slide_id, master.id, target) {
                Some(ops) => ops,
                None => return false,
            }
        };
        self.apply_ops(ops)
    }

    /// Change slide `slide_id`'s properties as one undo step.
    fn set_slide_props(&self, slide_id: u64, change: impl FnOnce(&mut crate::ops::SlideProps)) -> bool {
        self.set_slide_props_inner(slide_id, false, change)
    }

    /// As `set_slide_props`, sharing the history step (and the selection
    /// entry) with the previous step when `merged`.
    fn set_slide_props_inner(&self, slide_id: u64, merged: bool, change: impl FnOnce(&mut crate::ops::SlideProps)) -> bool {
        let op = {
            self.ids_ready();
            let slides = self.slides.borrow();
            let Some(slide) = slides.iter().find(|s| s.ids.slide == slide_id) else { return false };
            let old = crate::ops::SlideProps::of(slide);
            let mut new = old.clone();
            change(&mut new);
            if new == old {
                return false;
            }
            Op::SetSlide { slide: slide_id, props: Box::new(new) }
        };
        self.apply_ops_inner(vec![op], merged).applied
    }

    /// Set how slide `slide_id` arrives when presented, as one undo step.
    /// Returns whether anything changed.
    pub fn set_transition(&self, slide_id: u64, transition: crate::engine::Transition) -> bool {
        self.set_slide_props(slide_id, |p| p.transition = transition)
    }

    /// What the inspector shows for object `object_id` on slide `slide_id`.
    /// Edit the chart at object `object_id` of slide `slide_id`: one undo
    /// step, or none when the edit changes nothing.
    pub fn edit_chart(&self, slide_id: u64, object_id: u64, edit: &crate::engine::chart::ChartEdit) -> bool {
        let at = match self.object_index(slide_id, object_id) {
            Some(at) => at,
            None => return false,
        };
        self.edit_objects(slide_id, |o| {
            if let Some(SlideObject::Chart { chart, .. }) = o.get_mut(at) {
                chart.apply(edit);
            }
        })
    }

    pub fn object_format(&self, slide_id: u64, object_id: u64) -> Option<crate::format::ObjectFormat> {
        let slides = self.slides.borrow();
        let slide = slides.iter().find(|s| s.ids.slide == slide_id)?;
        let at = slide.object_index(object_id)?;
        slide.objects.get(at).map(crate::format::ObjectFormat::of)
    }

    /// Undo one step. The document, the revision and the selection all go
    /// back: the selection the undone step saw is restored (repaired
    /// against what is present), so an undone insert cannot leave the
    /// window pointing past the end. Refused while a text session is open:
    /// its own undo is the session's (`TextSession::undo`).
    pub fn undo(&self) -> bool {
        if self.has_text_session() {
            return false;
        }
        let pre = self.selection.borrow().clone();
        let mut history = self.history.borrow_mut();
        let done = crate::ops::with_doc(&mut self.slides.borrow_mut(), &mut self.masters.borrow_mut(), |doc| {
            history.undo(doc)
        })
        .is_some();
        if done {
            self.redo_sels.borrow_mut().push(pre);
            match self.undo_sels.borrow_mut().pop() {
                Some(sel) => self.set_selection(sel),
                None => {
                    self.repair_selection();
                }
            }
            if let Some(context) = self.source_context.borrow_mut().as_mut() {
                context.note_history_step();
            }
            self.revision.set(self.revision.get().wrapping_add(1));
            self.dirty.set(true);
            self.merge_chain.set(false);
        } else {
            // Nothing to undo, or the step no longer fits and the history
            // dropped that step alone: the selection entry goes with it, so
            // the stacks stay in step with the history.
            self.undo_sels.borrow_mut().pop();
        }
        done
    }

    /// Redo one step. Unlike undo, redo applies onto the current
    /// selection, which stays where the user put it (repaired to what
    /// still exists); the step goes back on the undo stack, so undoing
    /// it restores the selection it saw.
    pub fn redo(&self) -> bool {
        if self.has_text_session() {
            return false;
        }
        let pre = self.selection.borrow().clone();
        let mut history = self.history.borrow_mut();
        let done = crate::ops::with_doc(&mut self.slides.borrow_mut(), &mut self.masters.borrow_mut(), |doc| {
            history.redo(doc)
        })
        .is_some();
        if done {
            // The redone step goes onto the undo stack with the selection
            // it saw, so a later undo of it restores that; but redo
            // itself applies onto the current selection, which stays
            // where the user put it (repaired to what still exists).
            // Undo is the direction that restores selection.
            self.undo_sels.borrow_mut().push(pre);
            self.redo_sels.borrow_mut().pop();
            self.repair_selection();
            if let Some(context) = self.source_context.borrow_mut().as_mut() {
                context.note_history_step();
            }
            self.revision.set(self.revision.get().wrapping_add(1));
            self.dirty.set(true);
            self.merge_chain.set(false);
        } else {
            // As for undo: the dropped step's entry goes, the others stay.
            self.redo_sels.borrow_mut().pop();
        }
        done
    }

    pub fn can_undo(&self) -> bool {
        self.history.borrow().can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.borrow().can_redo()
    }
}

/// Whether `new` is `old` with one letter or digit typed into it.
fn one_word_char_added(old: &str, new: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    if b.len() != a.len() + 1 {
        return false;
    }
    let at = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    b[at].is_alphanumeric() && a[at..] == b[at + 1..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Slide;

    fn slide(title: &str) -> Slide {
        Slide { hidden: false, title: title.into(), background: "#fff".into(), background_picture: None, objects: vec![], notes: String::new(), master_id: None, transition: Default::default(), builds: Vec::new(), ids: Default::default(), layout_id: None }
    }

    fn rect(x: f64, y: f64) -> SlideObject {
        SlideObject::Rect { x, y, w: 10.0, h: 10.0, rotation: 0.0 }
    }

    /// The stable id of the slide at view position `i`.
    fn sid(c: &DecksController, i: usize) -> u64 {
        c.slide_id_at(i).unwrap()
    }

    /// The stable id of the object at z-position `oi` on the slide at
    /// view position `si`.
    fn oid(c: &DecksController, si: usize, oi: usize) -> u64 {
        c.object_id_at(si, oi).unwrap()
    }

    /// Each chart edit is one undo step, and one that changes nothing
    /// makes none.
    #[test]
    fn each_chart_edit_is_one_undo_step() {
        use crate::engine::chart::{ChartEdit, ChartKind};
        let c = DecksController::new(vec![slide("S1")], vec![]);
        let s = sid(&c, 0);
        c.add_object(s, crate::insert::chart(ChartKind::Bar));
        let o = oid(&c, 0, 0);
        let chart = |c: &DecksController| match &c.slides.borrow()[0].objects[0] {
            SlideObject::Chart { chart, .. } => chart.clone(),
            _ => panic!("expected a chart"),
        };
        let inserted = chart(&c);
        assert!(c.edit_chart(s, o, &ChartEdit::Value(1, 9.0)));
        assert!(c.edit_chart(s, o, &ChartEdit::Kind(ChartKind::Pie)));
        assert!(!c.edit_chart(s, o, &ChartEdit::Kind(ChartKind::Pie)), "no change, no step");
        assert!(!c.edit_chart(s, u64::MAX, &ChartEdit::AddPoint), "no object there");
        assert_eq!((chart(&c).kind, chart(&c).points[1].1), (ChartKind::Pie, 9.0));
        assert!(c.undo());
        assert_eq!((chart(&c).kind, chart(&c).points[1].1), (ChartKind::Bar, 9.0), "one undo, the type");
        assert!(c.undo());
        assert_eq!(chart(&c), inserted, "a second, the value");
        assert!(c.redo());
        assert_eq!(chart(&c).points[1].1, 9.0);
    }

    #[test]
    fn add_object_appends_to_slide_and_marks_dirty() {
        let c = DecksController::new(vec![slide("S1")], vec![]);
        let s = sid(&c, 0);
        let id = c.add_object(s, rect(1.0, 2.0)).unwrap();
        assert_eq!(c.slides.borrow()[0].objects.len(), 1);
        assert_eq!(c.object_id_at(0, 0), Some(id));
        assert!(c.dirty.get());
    }

    #[test]
    fn move_object_offsets_position() {
        let c = DecksController::new(vec![slide("S1")], vec![]);
        let s = sid(&c, 0);
        c.add_object(s, rect(1.0, 2.0));
        let o = oid(&c, 0, 0);
        c.move_object(s, o, 5.0, -1.0);
        let slides = c.slides.borrow();
        match slides[0].objects[0] {
            SlideObject::Rect { x, y, .. } => {
                assert_eq!(x, 6.0);
                assert_eq!(y, 1.0);
            }
            _ => panic!("expected Rect"),
        }
    }

    #[test]
    fn delete_object_removes_it_and_undo_restores_it() {
        let c = DecksController::new(vec![slide("S1")], vec![]);
        let s = sid(&c, 0);
        c.add_object(s, rect(0.0, 0.0));
        let o = oid(&c, 0, 0);
        assert!(c.delete_object(s, o));
        assert!(c.slides.borrow()[0].objects.is_empty());
        assert!(!c.delete_object(s, o), "already deleted");
        assert!(c.undo());
        assert_eq!(c.slides.borrow()[0].objects.len(), 1);
        // Undo brings the same identity back, not a fresh one.
        assert_eq!(c.object_id_at(0, 0), Some(o));
    }

    #[test]
    fn change_text_updates_textbox_and_undo_reverts() {
        let mut s = slide("S1");
        s.objects.push(SlideObject::TextBox { text: "old".into(), x: 0.0, y: 0.0, w: 10.0, h: 10.0, runs: vec![], rotation: 0.0, body: Default::default() });
        let c = DecksController::new(vec![s], vec![]);
        let (s, o) = (sid(&c, 0), oid(&c, 0, 0));
        c.change_text(s, o, "new".into());
        {
            let slides = c.slides.borrow();
            match &slides[0].objects[0] {
                SlideObject::TextBox { text, .. } => assert_eq!(text, "new"),
                _ => panic!("expected TextBox"),
            }
        }
        assert!(c.undo());
        let slides = c.slides.borrow();
        match &slides[0].objects[0] {
            SlideObject::TextBox { text, .. } => assert_eq!(text, "old"),
            _ => panic!("expected TextBox"),
        }
    }

    fn styled_text_controller(runs: Vec<letters_core::model::Run>, body: crate::engine::TextBody) -> DecksController {
        let mut s = slide("Styled");
        s.objects.push(SlideObject::TextBox {
            text: runs.iter().map(|r| r.text.as_str()).collect(),
            x: 12.0, y: 34.0, w: 240.0, h: 120.0, rotation: 15.0, runs, body,
        });
        DecksController::new(vec![s], vec![])
    }

    fn text_state(c: &DecksController) -> (String, Vec<letters_core::model::Run>, crate::engine::TextBody) {
        match &c.slides.borrow()[0].objects[0] {
            SlideObject::TextBox { text, runs, body, .. } => (text.clone(), runs.clone(), body.clone()),
            _ => panic!("expected TextBox"),
        }
    }

    #[test]
    fn change_text_preserves_styled_spans_and_exact_undo_redo() {
        use letters_core::model::{Run, RunStyle};
        let bold = RunStyle { bold: true, ..Default::default() };
        let italic = RunStyle { italic: true, ..Default::default() };
        let c = styled_text_controller(vec![
            Run::plain("Keep "),
            Run { text: "ca".into(), style: bold.clone() },
            Run { text: "fé".into(), style: bold.clone() },
            Run { text: " 東京😀".into(), style: italic.clone() },
        ], Default::default());
        let before = format!("{:?}", c.slides.borrow()[0].objects[0]);
        // A stale caller snapshot must not become the source of style offsets or undo.
        let (s, o) = (sid(&c, 0), oid(&c, 0, 0));
        c.change_text(s, o, "Keep cañon 東京😀".into());
        let (text, runs, _) = text_state(&c);
        assert_eq!(text, "Keep cañon 東京😀");
        assert_eq!(runs, vec![
            Run::plain("Keep "),
            Run { text: "cañon".into(), style: bold },
            Run { text: " 東京😀".into(), style: italic },
        ]);
        assert_eq!(runs.iter().map(|r| r.text.as_str()).collect::<String>(), text);
        let after = format!("{:?}", c.slides.borrow()[0].objects[0]);
        assert!(c.undo());
        assert_eq!(format!("{:?}", c.slides.borrow()[0].objects[0]), before);
        assert!(!c.can_undo());
        assert!(c.redo());
        assert_eq!(format!("{:?}", c.slides.borrow()[0].objects[0]), after);
    }

    #[test]
    fn change_text_unicode_insert_delete_and_replace_across_run_boundaries() {
        use letters_core::model::{Run, RunStyle};
        let bold = RunStyle { bold: true, ..Default::default() };
        let italic = RunStyle { italic: true, ..Default::default() };
        let original = vec![
            Run { text: "é😀".into(), style: bold.clone() },
            Run { text: "東京".into(), style: italic.clone() },
            Run::plain("e\u{301}!"),
        ];
        let cases = [
            ("前é😀東京e\u{301}!", vec![
                Run { text: "前é😀".into(), style: bold.clone() }, original[1].clone(), original[2].clone(),
            ]),
            ("é😀東京e\u{301}!後", vec![
                original[0].clone(), original[1].clone(), Run::plain("e\u{301}!後"),
            ]),
            ("Replacement", vec![Run { text: "Replacement".into(), style: bold.clone() }]),
            ("é😀界東京e\u{301}!", vec![
                original[0].clone(), Run { text: "界東京".into(), style: italic.clone() }, original[2].clone(),
            ]),
            ("é京e\u{301}!", vec![
                Run { text: "é".into(), style: bold.clone() },
                Run { text: "京".into(), style: italic.clone() }, original[2].clone(),
            ]),
            ("é🦀京e\u{301}!", vec![
                Run { text: "é🦀".into(), style: bold.clone() },
                Run { text: "京".into(), style: italic.clone() }, original[2].clone(),
            ]),
            ("ê😀東京e\u{301}!", vec![
                Run { text: "ê😀".into(), style: bold.clone() }, original[1].clone(), original[2].clone(),
            ]),
            ("é😀東京e\u{302}!", vec![
                original[0].clone(), original[1].clone(), Run::plain("e\u{302}!"),
            ]),
        ];
        for (new_text, expected) in cases {
            let c = styled_text_controller(original.clone(), Default::default());
            let before = text_state(&c);
            let (s, o) = (sid(&c, 0), oid(&c, 0, 0));
            c.change_text(s, o, new_text.into());
            assert_eq!(text_state(&c).1, expected, "{new_text}");
            assert_eq!(text_state(&c).0, new_text);
            assert!(c.undo());
            assert_eq!(text_state(&c), before);
            assert!(c.redo());
            assert_eq!(text_state(&c).1, expected);
        }
    }

    #[test]
    fn change_text_empty_box_keeps_typing_style_and_noop_keeps_history() {
        use letters_core::model::{Run, RunStyle};
        let style = RunStyle { bold: true, font_size_hp: Some(48), ..Default::default() };
        let c = styled_text_controller(vec![Run { text: "Hi".into(), style: style.clone() }], Default::default());
        let before = text_state(&c);
        let (s, o) = (sid(&c, 0), oid(&c, 0, 0));
        c.change_text(s, o, "Hi".into());
        assert!(!c.dirty.get());
        assert!(!c.can_undo());
        c.change_text(s, o, String::new());
        assert_eq!(text_state(&c).1, vec![Run { text: String::new(), style: style.clone() }]);
        c.change_text(s, o, "😀".into());
        assert_eq!(text_state(&c).1, vec![Run { text: "😀".into(), style }]);
        assert!(c.undo());
        assert!(text_state(&c).0.is_empty());
        assert!(c.undo());
        assert_eq!(text_state(&c), before);
        c.change_text(s, o, "Hi".into());
        assert!(c.can_redo(), "no-op must not discard redo");
        assert!(c.redo());
        assert!(c.redo());
        assert_eq!(text_state(&c).0, "😀");
    }

    #[test]
    fn change_text_remaps_paragraph_styles_with_newlines_and_undo() {
        use crate::engine::text_body::{Anchor, ParaStyle, TextBody};
        use letters_core::model::Run;
        let first = ParaStyle { margin_left: 10.0, ..Default::default() };
        let second = ParaStyle { margin_left: 20.0, ..Default::default() };
        let third = ParaStyle { margin_left: 30.0, ..Default::default() };
        let c = styled_text_controller(vec![Run::plain("one\ntwo\n三")], TextBody {
            paras: vec![first.clone(), second.clone(), third.clone()],
            anchor: Anchor::Middle, ..Default::default()
        });
        let before = text_state(&c);
        let (s, o) = (sid(&c, 0), oid(&c, 0, 0));
        c.change_text(s, o, "one\nnew\ntwo\n三".into());
        let inserted = text_state(&c);
        assert_eq!(inserted.1.iter().map(|r| r.text.as_str()).collect::<String>(), inserted.0);
        assert_eq!(inserted.2.paras, vec![first.clone(), second.clone(), second.clone(), third.clone()]);
        assert_eq!(inserted.2.anchor, Anchor::Middle);
        c.change_text(s, o, "one\nnewtwo\n三".into());
        assert_eq!(text_state(&c).2.paras, vec![first, second, third]);
        assert!(c.undo());
        assert_eq!(text_state(&c), inserted);
        assert!(c.undo());
        assert_eq!(text_state(&c), before);
        assert!(c.redo());
        assert_eq!(text_state(&c), inserted);
    }

    #[test]
    fn add_slide_appends_and_marks_dirty() {
        let c = DecksController::new(vec![slide("S1")], vec![]);
        assert!(!c.dirty.get());
        let id = c.add_slide(1, slide("S2")).unwrap();
        assert_eq!(c.slide_index(id), Some(1));
        assert_eq!(c.slide_count(), 2);
        assert_eq!(c.slides.borrow()[1].title, "S2");
        assert!(c.dirty.get());
        // Redo after undo keeps the slide's identity.
        assert!(c.undo());
        assert!(c.redo());
        assert_eq!(c.slide_id_at(1), Some(id));
    }

    #[test]
    fn delete_slide_removes_and_repairs_selection() {
        let c = DecksController::new(vec![slide("S1"), slide("S2"), slide("S3")], vec![]);
        let doomed = sid(&c, 1);
        c.select_slide(doomed);
        assert!(c.delete_slide(doomed));
        assert_eq!(c.slide_count(), 2);
        assert_eq!(c.slides.borrow()[1].title, "S3");
        // The selection fell back to a slide that still exists.
        assert_ne!(c.selection().slide_id, Some(doomed));
        assert!(c.slide_index(c.selection().slide_id.unwrap()).is_some());
    }

    #[test]
    fn delete_slide_refuses_to_empty_deck() {
        let c = DecksController::new(vec![slide("Only")], vec![]);
        assert!(!c.delete_slide(sid(&c, 0)));
        assert_eq!(c.slide_count(), 1);
    }

    #[test]
    fn crop_to_fill_trims_the_long_side_and_reset_shows_it_all() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("wide.png");
        // A 400×200 PNG header is all the size needs.
        let mut head = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        head.extend_from_slice(&400u32.to_be_bytes());
        head.extend_from_slice(&200u32.to_be_bytes());
        std::fs::write(&png, head).unwrap();
        let mut s = slide("S1");
        s.objects.push(SlideObject::Image {
            path: png.to_string_lossy().into(), x: 0.0, y: 0.0, w: 100.0, h: 100.0, rotation: 0.0,
            crop: Default::default(),
        });
        let c = DecksController::new(vec![s], vec![]);
        let (s, o) = (sid(&c, 0), oid(&c, 0, 0));
        let crop = |c: &DecksController| match &c.slides.borrow()[0].objects[0] {
            SlideObject::Image { crop, .. } => *crop,
            _ => unreachable!(),
        };
        assert!(c.crop_picture(s, o, true));
        let fill = crop(&c);
        assert!((fill.left - 0.25).abs() < 1e-9 && (fill.right - 0.25).abs() < 1e-9, "a 2:1 picture in a square box loses a quarter each side: {fill:?}");
        assert_eq!((fill.top, fill.bottom), (0.0, 0.0));
        assert!(!c.crop_picture(s, o, true), "already filled: no step");
        assert!(c.crop_picture(s, o, false));
        assert!(crop(&c).is_none());
        assert!(c.undo());
        assert_eq!(crop(&c), fill, "one undo puts the crop back");
    }

    #[test]
    fn duplicating_a_slide_copies_it_after_itself_as_one_undo_step() {
        let mut first = slide("One");
        first.notes = "say this".into();
        first.objects.push(rect(5.0, 6.0));
        let c = DecksController::new(vec![first.clone(), slide("Two")], vec![]);
        let orig = sid(&c, 0);
        let copy = c.duplicate_slide(orig).unwrap();
        assert_eq!(c.slide_index(copy), Some(1));
        {
            let s = c.slides.borrow();
            assert_eq!(s.len(), 3);
            assert_eq!((&s[1].title, &s[1].notes), (&first.title, &first.notes));
            assert_eq!(format!("{:?}", s[1].objects), format!("{:?}", first.objects), "the same objects");
            assert_eq!(s[2].title, "Two", "the slide after it moves down");
            assert_ne!(s[1].ids.slide, s[0].ids.slide, "the copy is a slide of its own");
            assert!(s[1].ids.objects.iter().all(|id| !s[0].ids.objects.contains(id)), "with objects of its own");
        }
        assert_eq!(c.duplicate_slide(u64::MAX), None);
        assert!(c.undo());
        {
            let s = c.slides.borrow();
            assert_eq!(s.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["One", "Two"], "one undo removes the copy");
        }
        assert!(c.redo());
        // Redo keeps the copy's identity: it is the same slide, not a new one.
        assert_eq!(c.slide_id_at(1), Some(copy));
    }

    #[test]
    fn move_slide_up_and_down_swap_neighbors() {
        let c = DecksController::new(vec![slide("S1"), slide("S2")], vec![]);
        let (first, second) = (sid(&c, 0), sid(&c, 1));
        assert!(!c.move_slide_up(first));
        assert!(!c.move_slide_down(second));
        assert!(c.move_slide_down(first));
        assert_eq!(c.slide_index(first), Some(1));
        assert_eq!(c.slides.borrow()[0].title, "S2");
        assert!(c.move_slide_up(first));
        assert_eq!(c.slide_index(first), Some(0));
        assert_eq!(c.slides.borrow()[0].title, "S1");
        // The selection follows the id through the reorder.
        c.select_slide(first);
        assert!(c.move_slide_down(first));
        assert_eq!(c.selection().slide_id, Some(first));
    }

    /// Type `text` into the first slide's notes one character at a time,
    /// as the notes pane reports it.
    fn type_notes(c: &DecksController, text: &str) {
        let s = sid(c, 0);
        for ch in text.chars() {
            let mut now = c.slides.borrow()[0].notes.clone();
            now.push(ch);
            c.set_notes(s, &now);
        }
    }

    #[test]
    fn typed_notes_undo_a_word_at_a_time() {
        let c = DecksController::new(vec![slide("S1")], vec![]);
        type_notes(&c, "hello world");
        assert_eq!(c.slides.borrow()[0].notes, "hello world");
        assert!(c.dirty.get(), "a notes edit makes the deck dirty");
        assert!(c.undo());
        assert_eq!(c.slides.borrow()[0].notes, "hello ", "the last word goes in one step");
        assert!(c.undo());
        assert_eq!(c.slides.borrow()[0].notes, "hello", "the space is its own step");
        assert!(c.undo());
        assert_eq!(c.slides.borrow()[0].notes, "", "and so is the first word");
        assert!(!c.can_undo());
        assert!(c.redo() && c.redo() && c.redo());
        assert_eq!(c.slides.borrow()[0].notes, "hello world");
    }

    #[test]
    fn a_notes_paste_or_deletion_is_its_own_step() {
        let c = DecksController::new(vec![slide("S1")], vec![]);
        let s = sid(&c, 0);
        type_notes(&c, "ab");
        assert!(c.set_notes(s, "ab pasted text"));
        assert!(c.set_notes(s, "ab pasted tex"));
        assert!(!c.set_notes(s, "ab pasted tex"), "no change, no step");
        type_notes(&c, "t");
        assert!(c.undo());
        assert_eq!(c.slides.borrow()[0].notes, "ab pasted tex");
        assert!(c.undo());
        assert_eq!(c.slides.borrow()[0].notes, "ab pasted text");
        assert!(c.undo());
        assert_eq!(c.slides.borrow()[0].notes, "ab");
        assert!(c.undo());
        assert_eq!(c.slides.borrow()[0].notes, "");
    }

    #[test]
    fn a_word_char_is_one_letter_or_digit_typed_anywhere() {
        assert!(one_word_char_added("", "a"));
        assert!(one_word_char_added("hllo", "hello"));
        assert!(one_word_char_added("abc", "abc7"));
        assert!(!one_word_char_added("abc", "abc "));
        assert!(!one_word_char_added("abc", "ab"));
        assert!(!one_word_char_added("abc", "abxy"));
        assert!(!one_word_char_added("abc", "xbcd"));
    }

    fn decorated_master() -> MasterSlide {
        MasterSlide {
            // The controller stamps the session id at construction.
            id: 0,
            name: "Theme".into(),
            background: "#0b3d6b".into(),
            default_font: "Sans".into(),
            shapes: vec![SlideObject::Rect { x: 0.0, y: 0.0, w: 28.0, h: 540.0, rotation: 0.0 }],
            theme_id: None,
            color_map: Default::default(),
            layouts: Vec::new(),
        }
    }

    #[test]
    fn the_master_view_edits_the_master_and_done_is_one_undo_step() {
        let c = DecksController::new(vec![slide("S1"), slide("S2")], vec![decorated_master()]);
        let (first, second) = (sid(&c, 0), sid(&c, 1));
        let master_id = c.masters.borrow()[0].id;
        c.add_object(first, SlideObject::Rect { x: 5.0, y: 5.0, w: 5.0, h: 5.0, rotation: 0.0 });
        assert_eq!(c.edit_master(first), Some(master_id));
        assert_eq!(c.editing_master(), Some(master_id));
        assert_eq!(c.slide_count(), 1, "the master stands in for the slides");
        assert_eq!(c.slides.borrow()[0].objects.len(), 1, "its decorations are the objects");
        assert!(!c.can_undo(), "the view has its own history");
        // Edits, each undoable inside the view.
        let stand_in = sid(&c, 0);
        c.add_object(stand_in, SlideObject::Rect { x: 900.0, y: 0.0, w: 60.0, h: 60.0, rotation: 0.0 });
        c.move_object(stand_in, oid(&c, 0, 0), 10.0, 0.0);
        assert!(c.undo());
        assert_eq!(crate::undo::obj_bounds(&c.slides.borrow()[0].objects[0]).0, 0.0);
        assert!(c.redo());
        // Saving now saves the deck, with the master as edited.
        let saved = c.deck();
        assert_eq!(saved.slides.len(), 2);
        assert_eq!(saved.masters[0].shapes.len(), 2);
        assert_eq!(c.finish_master(), Some(first));
        assert_eq!(c.editing_master(), None);
        assert_eq!(c.slide_count(), 2, "the slides are back");
        assert_eq!(c.masters.borrow()[0].shapes.len(), 2);
        assert_eq!(c.slides.borrow()[0].objects.len(), 1, "the slide's own objects are untouched");
        assert_eq!(sid(&c, 1), second, "slide identities survive the round trip");
        // One step undoes the whole master edit; the slide edit before it
        // is still in the history.
        assert!(c.undo());
        assert_eq!(format!("{:?}", c.masters.borrow()[0]), format!("{:?}", decorated_master_stamped(&c, master_id)));
        assert!(c.undo());
        assert!(c.slides.borrow()[0].objects.is_empty());
        assert!(c.redo() && c.redo());
        assert_eq!(c.masters.borrow()[0].shapes.len(), 2);
    }

    /// `decorated_master` with the session id the controller stamped.
    fn decorated_master_stamped(c: &DecksController, id: u64) -> MasterSlide {
        let mut m = decorated_master();
        m.id = id;
        // Layouts the master view may have added are not decorations.
        m.layouts = c.masters.borrow()[0].layouts.clone();
        m
    }

    #[test]
    fn a_master_view_with_no_change_records_nothing() {
        let c = DecksController::new(vec![slide("S1")], vec![decorated_master()]);
        let s = sid(&c, 0);
        c.edit_master(s);
        assert_eq!(c.edit_master(s), None, "one master view at a time");
        c.finish_master();
        assert!(!c.can_undo());
        assert!(c.finish_master().is_none());
    }

    #[test]
    fn master_edit_cancel_restores_history() {
        let c = DecksController::new(vec![slide("S1")], vec![decorated_master()]);
        let s = sid(&c, 0);
        c.add_object(s, rect(1.0, 2.0));
        assert!(c.edit_master(s).is_some());
        let stand_in = sid(&c, 0);
        c.add_object(stand_in, rect(9.0, 9.0));
        assert!(c.can_undo(), "the view has its own history");
        assert_eq!(c.cancel_master(), Some(s));
        assert_eq!(c.editing_master(), None);
        assert_eq!(c.slide_count(), 1);
        assert_eq!(c.slides.borrow()[0].objects.len(), 1, "the view's objects are gone");
        assert_eq!(c.masters.borrow()[0].shapes.len(), 1, "the master is untouched");
        // The original history is back exactly: one step undoes the slide edit.
        assert!(c.undo());
        assert!(c.slides.borrow()[0].objects.is_empty());
        assert!(!c.can_undo());
        assert_eq!(c.selection().slide_id, Some(s));
    }

    #[test]
    fn applying_a_layout_is_one_step_and_a_bare_master_gets_the_standard_ones() {
        use crate::layouts::Placeholder;
        let mut s1 = slide("S1");
        s1.objects.push(SlideObject::TextBox {
            text: "Hello".into(),
            x: 1.0,
            y: 1.0,
            w: 10.0,
            h: 10.0,
            rotation: 0.0,
            runs: vec![],
            body: crate::engine::TextBody { placeholder: Some(Placeholder::Title), ..Default::default() },
        });
        let c = DecksController::new(vec![s1], vec![decorated_master()]);
        let s = sid(&c, 0);
        assert!(c.apply_standard_layout(s, 1), "Title and Content");
        assert_eq!(c.masters.borrow()[0].layouts.len(), 6);
        let layout_id = c.masters.borrow()[0].layouts[1].id;
        {
            let slides = c.slides.borrow();
            assert_eq!(slides[0].layout_id, Some(layout_id));
            assert_eq!(slides[0].objects.len(), 2, "the body is added");
            assert_eq!(crate::undo::obj_bounds(&slides[0].objects[0]), (80.0, 40.0, 800.0, 90.0), "the title moves");
        }
        // Deleting the added body and re-applying by stable id brings it back.
        let body = oid(&c, 0, 1);
        assert!(c.delete_object(s, body));
        assert_eq!(c.slides.borrow()[0].objects.len(), 1);
        assert!(c.apply_layout(s, layout_id));
        assert_eq!(c.slides.borrow()[0].objects.len(), 2, "the body is back");
        assert!(!c.apply_layout(s, u64::MAX), "no such layout");
        assert!(c.undo());
        assert!(c.undo());
        assert!(c.undo());
        assert!(c.masters.borrow()[0].layouts.is_empty(), "undo takes the added layouts too");
        assert_eq!(c.slides.borrow()[0].objects.len(), 1);
        assert_eq!(c.slides.borrow()[0].layout_id, None);
        assert!(!c.can_undo());
    }

    #[test]
    fn undo_redo_roundtrip_add_slide() {
        let c = DecksController::new(vec![slide("S1")], vec![]);
        c.add_slide(1, slide("S2"));
        assert_eq!(c.slide_count(), 2);
        assert!(c.undo());
        assert_eq!(c.slide_count(), 1);
        assert!(c.redo());
        assert_eq!(c.slide_count(), 2);
        assert!(!c.can_redo());
    }

    #[test]
    fn create_edit_save_reopen_flow() {
        // Full controller lifecycle: create, edit (add/reorder/delete),
        // write to a pptx, and confirm the reopened deck matches. Slide
        // titles aren't semantically stored in pptx by this reader/writer
        // (read_pptx synthesizes "Slide N" from position, matching what
        // real pptx slide titles usually are: placeholder text content,
        // not a plain field) — notes are, so that's the round-trip check.
        let mut s1 = slide("S1");
        s1.notes = "notes-1".into();
        let mut s2 = slide("S2");
        s2.notes = "notes-2".into();
        let mut s3 = slide("S3");
        s3.notes = "notes-3".into();
        let c = DecksController::new(vec![s1], vec![]);
        c.add_slide(1, s2);
        c.add_slide(2, s3);
        let first = sid(&c, 0);
        c.move_slide_down(first);
        c.delete_slide(sid(&c, 2));
        assert_eq!(c.slide_count(), 2);
        let notes: Vec<String> = c.slides.borrow().iter().map(|s| s.notes.clone()).collect();

        let deck = crate::engine::Deck {
            slides: c.slides.borrow().clone(),
            masters: c.masters.borrow().clone(),
            page_size: None,
            themes: Vec::new(),
        };
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().with_extension("pptx");
        crate::write_deck(path.to_str().unwrap(), &deck).unwrap();
        let reopened = crate::read_deck(path.to_str().unwrap()).unwrap();
        let reopened_notes: Vec<String> = reopened.slides.iter().map(|s| s.notes.clone()).collect();
        assert_eq!(reopened_notes, notes);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn resize_and_rotate_object_with_undo() {
        let mut s = slide("S1");
        s.objects.push(SlideObject::Rect { x: 10.0, y: 10.0, w: 50.0, h: 50.0, rotation: 0.0 });
        let c = DecksController::new(vec![s], vec![]);
        let (s, o) = (sid(&c, 0), oid(&c, 0, 0));
        c.resize_object(s, o, (20.0, 20.0, 100.0, 80.0));
        assert_eq!(crate::undo::obj_bounds(&c.slides.borrow()[0].objects[0]), (20.0, 20.0, 100.0, 80.0));
        assert!(c.undo());
        assert_eq!(crate::undo::obj_bounds(&c.slides.borrow()[0].objects[0]), (10.0, 10.0, 50.0, 50.0));

        c.rotate_object(s, o, 45.0);
        assert_eq!(c.slides.borrow()[0].objects[0].rotation(), 45.0);
        assert!(c.undo());
        assert_eq!(c.slides.borrow()[0].objects[0].rotation(), 0.0);
    }

    #[test]
    fn align_and_distribute_objects_with_undo() {
        use crate::undo::{AlignMode, DistributeMode};
        let mut s = slide("S1");
        s.objects.push(SlideObject::Rect { x: 10.0, y: 10.0, w: 20.0, h: 20.0, rotation: 0.0 });
        s.objects.push(SlideObject::Rect { x: 50.0, y: 30.0, w: 20.0, h: 20.0, rotation: 0.0 });
        s.objects.push(SlideObject::Rect { x: 100.0, y: 50.0, w: 20.0, h: 20.0, rotation: 0.0 });
        let c = DecksController::new(vec![s], vec![]);
        let s = sid(&c, 0);
        let ids = [oid(&c, 0, 0), oid(&c, 0, 1), oid(&c, 0, 2)];

        c.align_objects(s, &ids, AlignMode::Left);
        assert_eq!(c.slides.borrow()[0].objects[0].x(), 10.0);
        assert_eq!(c.slides.borrow()[0].objects[1].x(), 10.0);
        assert_eq!(c.slides.borrow()[0].objects[2].x(), 10.0);
        assert!(c.undo());
        assert_eq!(c.slides.borrow()[0].objects[2].x(), 100.0);

        c.distribute_objects(s, &ids, DistributeMode::Horizontal);
        assert_eq!(c.slides.borrow()[0].objects[0].x(), 10.0);
        assert_eq!(c.slides.borrow()[0].objects[1].x(), 55.0);
        assert_eq!(c.slides.borrow()[0].objects[2].x(), 100.0);
        assert!(c.undo());
        assert_eq!(c.slides.borrow()[0].objects[1].x(), 50.0);
    }

    #[test]
    fn z_order_object_with_undo() {
        use crate::undo::ZOrderOp;
        let mut s = slide("S1");
        s.objects.push(SlideObject::Rect { x: 10.0, y: 10.0, w: 20.0, h: 20.0, rotation: 0.0 });
        s.objects.push(SlideObject::Circle { x: 50.0, y: 50.0, r: 10.0, rotation: 0.0 });
        let c = DecksController::new(vec![s], vec![]);
        let (s, o) = (sid(&c, 0), oid(&c, 0, 0));

        assert!(c.z_order_object(s, o, ZOrderOp::BringToFront));
        assert!(matches!(c.slides.borrow()[0].objects[1], SlideObject::Rect { .. }));
        assert!(c.undo());
        assert!(matches!(c.slides.borrow()[0].objects[0], SlideObject::Rect { .. }));
        // The z-order move keeps the object's identity.
        assert_eq!(c.object_id_at(0, 0), Some(o));
    }

    #[test]
    fn a_format_edit_is_one_undo_step_and_a_no_op_is_none() {
        use crate::format::FormatEdit;
        let mut s = slide("S1");
        s.objects.push(SlideObject::Rect { x: 10.0, y: 10.0, w: 20.0, h: 20.0, rotation: 0.0 });
        s.objects.push(SlideObject::Rect { x: 40.0, y: 10.0, w: 20.0, h: 20.0, rotation: 0.0 });
        let c = DecksController::new(vec![s], vec![]);
        let s = sid(&c, 0);
        let (o1, o2) = (oid(&c, 0, 0), oid(&c, 0, 1));
        assert!(c.format_objects(s, &[o1, o2], &FormatEdit::Width(50.0)));
        assert_eq!(c.object_format(s, o2).unwrap().bounds.2, 50.0);
        assert!(!c.format_objects(s, &[o1, o2], &FormatEdit::Width(50.0)), "nothing to change");
        assert!(c.undo());
        assert_eq!(c.object_format(s, o1).unwrap().bounds.2, 20.0);
        assert_eq!(c.object_format(s, o2).unwrap().bounds.2, 20.0, "both in the one step");
        assert!(!c.can_undo());
        assert!(c.dirty.get());
    }

    #[test]
    fn a_slides_transition_is_set_as_one_undo_step() {
        use crate::engine::Transition;
        let c = DecksController::new(vec![slide("S1"), slide("S2")], vec![]);
        let second = sid(&c, 1);
        assert!(c.set_transition(second, Transition::MagicMove));
        assert_eq!(c.slides.borrow()[1].transition, Transition::MagicMove);
        assert!(!c.set_transition(second, Transition::MagicMove), "no change, no step");
        assert!(!c.set_transition(u64::MAX, Transition::Fade), "no such slide");
        assert!(c.undo());
        assert_eq!(c.slides.borrow()[1].transition, Transition::None);
        assert!(!c.can_undo());
    }

    #[test]
    fn builds_are_set_undoably_and_follow_deletes_and_reorders() {
        use crate::builds::{Build, BuildEffect};
        let mut s = slide("S1");
        for x in [0.0, 50.0, 100.0] {
            s.objects.push(SlideObject::Rect { x, y: 0.0, w: 20.0, h: 20.0, rotation: 0.0 });
        }
        let c = DecksController::new(vec![s], vec![]);
        let s = sid(&c, 0);
        let (o1, o2, o3) = (oid(&c, 0, 0), oid(&c, 0, 1), oid(&c, 0, 2));
        assert!(c.set_build(s, o3, false, Some(BuildEffect::Dissolve)));
        assert!(c.set_build(s, o1, false, Some(BuildEffect::Appear)));
        assert!(!c.set_build(s, o1, false, Some(BuildEffect::Appear)), "no change");
        let builds = || c.slides.borrow()[0].builds.clone();
        assert_eq!(builds().iter().map(|b| b.object).collect::<Vec<_>>(), [o3, o1], "in the order they were added");
        // Deleting the middle object drops only the builds that name it;
        // the build on the third object keeps its identity.
        assert!(c.delete_object(s, o2));
        assert_eq!(builds().iter().map(|b| b.object).collect::<Vec<_>>(), [o3, o1]);
        assert!(c.undo());
        assert_eq!(builds().iter().map(|b| b.object).collect::<Vec<_>>(), [o3, o1]);
        // Reordering never retargets a build: the same ids name the same boxes.
        assert!(c.z_order_object(s, o1, crate::undo::ZOrderOp::BringToFront));
        assert_eq!(builds(), vec![
            Build { object: o3, effect: BuildEffect::Dissolve, out: false },
            Build { object: o1, effect: BuildEffect::Appear, out: false },
        ]);
        assert!(c.undo());
        assert!(c.set_build(s, o3, false, None));
        assert_eq!(builds().len(), 1);
    }

    #[test]
    fn stable_selection_survives_reorder_and_undo() {
        let c = DecksController::new(vec![slide("S1"), slide("S2"), slide("S3")], vec![]);
        let (first, second, third) = (sid(&c, 0), sid(&c, 1), sid(&c, 2));
        c.add_object(second, rect(1.0, 1.0));
        let o = oid(&c, 1, 0);
        assert!(c.select_objects(second, &[o], Some(o), SelectionMode::Object));
        // Reordering moves positions, not the selection.
        assert!(c.move_slide_up(second));
        assert_eq!(c.selection(), SelectionState { slide_id: Some(second), object_ids: vec![o], primary_object_id: Some(o), mode: SelectionMode::Object, text_range: None });
        assert_eq!(c.slide_index(second), Some(0));
        // Deleting the selected slide repairs the selection; undo restores both.
        assert!(c.delete_slide(second));
        assert_ne!(c.selection().slide_id, Some(second));
        assert!(c.undo());
        assert_eq!(c.selection().slide_id, Some(second));
        assert_eq!(c.slide_index(second), Some(0));
        // Redo re-applies the delete with the same identity.
        assert!(c.redo());
        assert_eq!(c.slide_index(first), Some(0));
        assert_eq!(c.slide_index(third), Some(1));
        assert!(c.selection().slide_id.is_some_and(|id| id == first || id == third));
    }

    /// ADR 0011: a step that no longer fits is dropped alone. (Decks' ops
    /// treat a missing slide or object as a delete that won and do nothing;
    /// an out-of-range position is the error a step can meet.) The
    /// selection stacks mirror the history, so the dropped step's entry goes
    /// with it and the next undo restores the selection of the step it takes
    /// back.
    #[test]
    fn a_step_that_no_longer_fits_is_dropped_with_its_selection_entry_and_the_rest_stay() {
        let c = DecksController::new(vec![slide("S1"), slide("S2"), slide("S3")], vec![]);
        let third = sid(&c, 2);
        c.add_object(third, rect(1.0, 1.0));
        let o = oid(&c, 2, 0);
        assert!(c.select_objects(third, &[o], Some(o), SelectionMode::Object));
        let on_object = c.selection();
        c.move_object(third, o, 1.0, 0.0);
        assert!(c.select_slide(third));
        assert_ne!(c.selection(), on_object);
        assert!(c.move_slide_up(third), "its undo moves it back to position 2");
        // A peer removes the other two slides: position 2 is out of range.
        c.slides.borrow_mut().retain(|s| s.ids.slide == third);
        assert!(!c.undo(), "that step no longer fits");
        assert!(c.can_undo() && !c.can_redo(), "the steps before it stay, and the dropped one cannot be redone");
        assert!(c.undo(), "the move is next");
        assert_eq!(c.selection(), on_object, "with the selection that step saw, not the dropped step's");
    }

    #[test]
    fn save_ack_does_not_clean_newer_revision() {
        let c = DecksController::new(vec![slide("S1")], vec![]);
        assert!(!c.dirty.get());
        assert_eq!(c.saved_revision(), None);
        let s = sid(&c, 0);
        c.add_object(s, rect(1.0, 1.0));
        assert!(c.dirty.get());
        let staged = c.checkpoint();
        // An edit after staging leaves the ack stale: still dirty.
        c.move_object(s, oid(&c, 0, 0), 1.0, 0.0);
        assert!(!c.acknowledge_save(staged));
        assert!(c.dirty.get());
        // Acknowledging the current revision marks clean.
        assert!(c.acknowledge_save(c.checkpoint()));
        assert!(!c.dirty.get());
        assert_eq!(c.saved_revision(), Some(c.revision()));
        // Undo after a save dirties again.
        assert!(c.undo());
        assert!(c.dirty.get());
    }

    #[test]
    fn text_ranges_are_validated_session_state() {
        let mut s = slide("S1");
        s.objects.push(SlideObject::TextBox { text: "héllo".into(), x: 0.0, y: 0.0, w: 10.0, h: 10.0, runs: vec![], rotation: 0.0, body: Default::default() });
        s.objects.push(rect(50.0, 50.0));
        let c = DecksController::new(vec![s], vec![]);
        let (s, text_box, shape) = (sid(&c, 0), oid(&c, 0, 0), oid(&c, 0, 1));
        assert!(c.select_slide(s));
        // "héllo" is 6 bytes: h(1) é(2) l l o. Offset 2 is inside é.
        assert!(!c.set_text_range(text_box, 2, 3), "mid-character split rejected");
        assert!(!c.set_text_range(text_box, 3, 2), "start after end rejected");
        assert!(!c.set_text_range(text_box, 0, 99), "past the end rejected");
        assert!(!c.set_text_range(shape, 0, 0), "non-text object rejected");
        assert!(!c.set_text_range(u64::MAX, 0, 0), "unknown object rejected");
        assert!(c.set_text_range(text_box, 1, 3));
        assert_eq!(c.selection().mode, SelectionMode::Text);
        assert_eq!(c.selection().text_range, Some(TextRange { object_id: text_box, start: 1, end: 3 }));
        // Selection-only movement never dirties or bumps the revision.
        assert!(!c.dirty.get());
        let revision = c.revision();
        assert!(c.select_slide(s));
        assert_eq!(c.revision(), revision);
        assert_eq!(c.selection().text_range, None, "leaving text mode drops the range");
    }

    #[test]
    fn transactions_report_changed_ids_and_repaired_selection() {
        let c = DecksController::new(vec![slide("S1")], vec![]);
        let s = sid(&c, 0);
        let revision = c.revision();
        let id = crate::ops::next_id(&c.slides.borrow());
        let outcome = c.apply_ops_txn(vec![Op::InsertObject {
            slide: s,
            at: 0,
            id,
            object: Box::new(rect(3.0, 4.0)),
        }]);
        assert!(outcome.applied);
        assert_eq!(outcome.changed_slides, vec![s]);
        assert_eq!(outcome.changed_objects.len(), 1);
        assert_eq!(outcome.revision, revision + 1);
        assert_eq!(outcome.selection.slide_id, Some(s));
        // No-op and invalid groups record nothing.
        let quiet = c.apply_ops_txn(vec![]);
        assert!(!quiet.applied);
        assert_eq!(quiet.revision, outcome.revision);
        let unknown = c.apply_ops_txn(vec![Op::DeleteObject { slide: s, id: u64::MAX }]);
        assert!(!unknown.applied);
        assert!(!c.dirty.get() || c.revision() == outcome.revision, "no new revision");
    }

    /// DCK-007: an in-place text edit is one transaction. Typing and
    /// formatting inside it touch neither the document nor its history;
    /// commit records one step whose undo restores the text and the
    /// object selection; cancel leaves dirty, revision and redo exactly
    /// as they were; document undo is refused while it is open.
    #[test]
    fn a_text_session_commits_one_step_and_cancels_without_trace() {
        use crate::format::FormatEdit;
        use letters_core::model::{Run, RunStyle};
        let bold = RunStyle { bold: true, ..Default::default() };
        let c = styled_text_controller(vec![Run { text: "Alpha ".into(), style: bold.clone() }, Run { text: "Beta".into(), style: RunStyle::default() }], Default::default());
        let (s, o) = (sid(&c, 0), oid(&c, 0, 0));
        // An earlier, unrelated edit gives undo something to take back.
        c.move_object(s, o, 1.0, 0.0);
        let before = text_state(&c);
        let revision = c.revision();
        assert!(c.begin_text_session(s, o));
        assert!(!c.begin_text_session(s, o), "one session at a time");
        assert_eq!(c.selection().mode, SelectionMode::Text);
        c.edit_text_session(|t| {
            t.set_range(6, 10).unwrap();
            assert!(t.format(&FormatEdit::Italic(true)));
            t.replace(10, 10, "\nGamma").unwrap();
        });
        assert_eq!(text_state(&c), before, "the session is not the document");
        assert_eq!(c.revision(), revision);
        assert!(!c.undo(), "document undo is refused during a session");
        assert!(!c.redo());
        assert_eq!(c.object_format(s, o).unwrap().bounds.0, 13.0, "the earlier move was not undone");
        assert_eq!(c.commit_text_session(), Ok(true));
        assert!(!c.has_text_session());
        let (text, runs, _) = text_state(&c);
        assert_eq!(text, "Alpha Beta\nGamma");
        assert!(runs.iter().any(|r| r.text.starts_with("Beta") && r.style.italic), "{runs:?}");
        assert_eq!(c.selection().mode, SelectionMode::Object);
        assert!(c.undo(), "one step");
        assert_eq!(text_state(&c), before);
        assert_eq!(c.selection().primary_object_id, Some(o));
        assert!(c.redo());
        assert_eq!(text_state(&c).0, "Alpha Beta\nGamma");

        // Cancel: same text, same dirty flag, same revision, redo kept.
        assert!(c.undo());
        let (dirty, revision, redo) = (c.dirty.get(), c.revision(), c.can_redo());
        assert!(c.begin_text_session(s, o));
        c.edit_text_session(|t| t.replace(0, 0, "Discard ").unwrap());
        assert!(c.cancel_text_session());
        assert_eq!(text_state(&c), before);
        assert_eq!((c.dirty.get(), c.revision(), c.can_redo()), (dirty, revision, redo));
        // A session that changed nothing commits nothing.
        assert!(c.begin_text_session(s, o));
        assert_eq!(c.commit_text_session(), Ok(false));
        assert_eq!(c.revision(), revision);
        assert!(c.can_redo(), "a no-op commit keeps redo");
    }

    #[test]
    fn a_text_session_whose_box_vanished_or_changed_stays_open_with_an_error() {
        use letters_core::model::Run;
        let c = styled_text_controller(vec![Run::plain("one")], Default::default());
        let (s, o) = (sid(&c, 0), oid(&c, 0, 0));
        assert!(c.begin_text_session(s, o));
        c.edit_text_session(|t| t.replace(3, 3, " two").unwrap());
        // Something outside the session changes the box (automation).
        c.edit_objects(s, |objs| {
            if let SlideObject::TextBox { text, runs, .. } = &mut objs[0] {
                *text = "changed".into();
                runs.clear();
            }
        });
        assert_eq!(c.commit_text_session(), Err(TextSessionError::Changed));
        assert!(c.has_text_session(), "kept open so the editor can show the error");
        assert!(!c.replace_text_range(s, o, 0, 0, "x"), "no direct edit under an open session");
        assert!(c.cancel_text_session());
        assert!(c.begin_text_session(s, o));
        assert!(c.delete_object(s, o));
        assert_eq!(c.commit_text_session(), Err(TextSessionError::Gone));
        assert!(c.cancel_text_session());
        assert_eq!(c.commit_text_session(), Err(TextSessionError::NoSession));
    }

    #[test]
    fn range_commands_are_one_step_each_and_reject_bad_ranges() {
        use crate::format::FormatEdit;
        use letters_core::model::Run;
        let c = styled_text_controller(vec![Run::plain("héllo wörld")], Default::default());
        let (s, o) = (sid(&c, 0), oid(&c, 0, 0));
        assert!(!c.format_text_range(s, o, 2, 3, &FormatEdit::Bold(true)), "mid-character");
        assert!(!c.replace_text_range(s, o, 5, 2, "x"), "reversed");
        assert!(!c.can_undo(), "rejected commands record nothing");
        let world = "héllo ".len();
        assert!(c.format_text_range(s, o, world, world + "wörld".len(), &FormatEdit::Bold(true)));
        assert!(c.replace_text_range(s, o, 0, "héllo".len(), "hi"));
        let (text, runs, _) = text_state(&c);
        assert_eq!(text, "hi wörld");
        assert_eq!(runs.iter().filter(|r| r.style.bold).map(|r| r.text.as_str()).collect::<String>(), "wörld");
        assert!(c.undo());
        assert!(c.undo());
        assert_eq!(text_state(&c).0, "héllo wörld");
        assert!(text_state(&c).1.iter().all(|r| !r.style.bold));
    }
}
