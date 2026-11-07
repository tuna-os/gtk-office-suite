// SPDX-License-Identifier: GPL-3.0-or-later
//
// live/collab.rs — a tab's document as part of a live session (RFC-0005 P5,
// feature `collab`).
//
// The live model stays the one place a document changes. In a session it
// also owns the document's `Shared` (a Loro replica, `letters_core::collab`):
//
//   - a local action (typing, a mark, an undo) goes into the replica before it
//     goes into the document, all its ops or none, so one the encoding cannot
//     carry (a table, a footnote) is refused with the document untouched
//     (`record_shared`, `undo_shared`). Its delta is handed to the session
//     (`publish_shared`) once the buffer shows it;
//   - a peer's update goes into the replica (a dry run first), the document
//     becomes the one the replica holds, the stored undo steps are moved over
//     what the peer did (`ImportReport::rebase`), and the buffer is updated
//     like for any other change, except that the caret and the selection ride
//     over the peer's edits as offsets into the edit sequence
//     (`letters_core::collab::map_selection`) instead of going to the end of
//     the change. Nothing a peer sent is ever recorded as the user's own, so it
//     is not sent back and not on the undo stack.
//
// The hooks the session gives must not touch the live model: they run while it
// is borrowed.

use super::*;
use letters_core::collab::{map_selection, CollabError, Remote, Shared, Unsupported};

/// Why an edit of a shared document was not made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The edit touches content a shared document does not carry yet.
    Unsupported(Unsupported),
    /// The replica refused it for another reason.
    Other(String),
    /// This person may only read the document.
    ReadOnly,
    /// The document was replaced wholesale (a file loaded into the tab): the
    /// replica no longer describes it, so the session ends.
    Replaced,
}

impl From<CollabError> for Refusal {
    fn from(e: CollabError) -> Refusal {
        match e {
            CollabError::Unsupported(what) => Refusal::Unsupported(what),
            other => Refusal::Other(other.to_string()),
        }
    }
}

/// What the session gives the model when a tab joins it.
pub struct Hooks {
    /// This person may only read.
    pub read_only: bool,
    /// One local action's update, to send to the others.
    pub publish: Box<dyn Fn(Vec<u8>)>,
    /// An edit was refused (or the document replaced): tell the person.
    pub refused: Box<dyn Fn(&Refusal)>,
}

/// A tab's place in a session.
pub struct Link {
    shared: Shared,
    pub(super) hooks: Hooks,
}

/// What taking in a peer's update did.
#[derive(Debug, Default)]
pub struct Imported {
    /// The document changed.
    pub changed: bool,
    /// This person's own changes that had not been sent: send them first.
    pub unsent: Option<Vec<u8>>,
}

impl LiveModel {
    /// Make this tab's document part of the session `shared` belongs to (a
    /// `Shared::start` from this document, or a `Shared::join` whose document
    /// this model shows). The undo history starts over: a step recorded
    /// before the session has no part in it.
    pub fn attach_shared(&mut self, shared: Shared, hooks: Hooks) {
        self.history.clear();
        self.collab = Some(Link { shared, hooks });
    }

    /// Leave the session: the document is the one shown, edited as any other.
    pub fn detach_shared(&mut self) {
        self.collab = None;
    }

    /// Record the ops of one action in the replica; `false` (and the person
    /// told) when they are refused, with nothing written.
    pub(super) fn record_shared(&mut self, before: &Document, ops: &[Op]) -> bool {
        let Some(link) = self.collab.as_mut() else { return true };
        let result = if link.hooks.read_only { Err(Refusal::ReadOnly) } else { link.shared.record(before, ops).map_err(Refusal::from) };
        match result {
            Ok(()) => true,
            Err(refusal) => {
                (link.hooks.refused)(&refusal);
                false
            }
        }
    }

    /// Hand the session what the replica has recorded since the last time.
    pub(super) fn publish_shared(&mut self) {
        let Some(link) = self.collab.as_mut() else { return };
        match link.shared.outgoing() {
            Ok(Some(delta)) => (link.hooks.publish)(delta),
            Ok(None) => {}
            Err(e) => (link.hooks.refused)(&Refusal::from(e)),
        }
    }

    /// Undo or redo in a shared document. The step is taken on a copy first:
    /// the replica must accept its ops (they are ordinary edits of this
    /// person's, recorded and sent like any other) before the document, the
    /// history and the buffer change.
    pub(super) fn undo_shared(&mut self, buf: &gtk::TextBuffer, redo: bool) -> bool {
        let Some(link) = self.collab.as_mut() else { return false };
        if link.hooks.read_only {
            (link.hooks.refused)(&Refusal::ReadOnly);
            return false;
        }
        let old = self.doc.clone();
        let (mut history, mut doc) = (self.history.clone(), old.clone());
        let ops = if redo { history.redo(&mut doc) } else { history.undo(&mut doc) };
        let Some(ops) = ops else {
            // Nothing to undo, or the step no longer fitted and is gone (the
            // others stay): the document is as it was.
            self.history = history;
            return false;
        };
        if let Err(e) = link.shared.record(&old, &ops) {
            (link.hooks.refused)(&Refusal::from(e));
            return false;
        }
        self.history = history;
        self.doc = doc;
        self.revision += 1;
        self.project(buf, &old);
        self.publish_shared();
        true
    }

    /// Take in a peer's update (`Shared::import`: a dry run, then the live
    /// replica). On success the buffer shows what the peer did and the
    /// caret and the selection are on the characters they were on.
    pub fn import_remote(&mut self, buf: &gtk::TextBuffer, update: &[u8]) -> Result<Imported, CollabError> {
        let offset = |mark: &gtk::TextMark| buf.iter_at_mark(mark).offset().max(0) as usize;
        let (insert, bound) = (offset(&buf.get_insert()), offset(&buf.selection_bound()));
        let (insert, bound) = (self.sequence_offset(insert), self.sequence_offset(bound));
        let Some(link) = self.collab.as_mut() else {
            return Err(CollabError::Invalid("this document is not shared".into()));
        };
        let Remote { document, report, unsent } = link.shared.import(update, &self.doc)?;
        let changed = document.is_some();
        if let Some(new) = document {
            // The user's own undo steps move over what the peer did; the
            // peer's edits are never on this person's stack.
            report.rebase(&mut self.history);
            let keep = map_selection(insert, bound, &report.edits);
            let old = std::mem::replace(&mut self.doc, new);
            self.revision += 1;
            self.project_with(buf, &old, Some(keep));
        }
        Ok(Imported { changed, unsent })
    }
}
