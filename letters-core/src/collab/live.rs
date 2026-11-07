// SPDX-License-Identifier: GPL-3.0-or-later
//! The editor's side of a live session (RFC-0005 P5), without GTK: what
//! sits between the live model's ops and the bytes of one update, and what
//! a remote update does to the offsets the editor holds.
//!
//! [`Shared`] is a [`Replica`] plus what the shell has already sent:
//!
//! - a local action is [`Shared::record`]ed (all its ops or none, so a
//!   refused one leaves nothing half shared), and [`Shared::outgoing`] is
//!   the delta to publish for it;
//! - a peer's update is [`Shared::import`]ed: a dry run into a scratch copy
//!   first, then the live replica, and the document the replica now holds.
//!   Nothing that came from a peer is ever in [`Shared::outgoing`], so an
//!   update is never echoed back.
//!
//! # Offsets
//!
//! A [`TextEdit`] list says what a peer did to the edit sequence
//! (`edit::sequence_text`), in the offsets the model counts. The editor
//! keeps its caret and selection as offsets into the same sequence, and
//! [`map_offset`] / [`map_selection`] move them over what a peer did, so a
//! remote edit never moves the caret off the character it was on. The one
//! choice is a peer's insert at exactly the caret: it goes before the
//! caret ([`Bias::After`]), as it goes before the user's own text in
//! [`Rebaser`](super::Rebaser) and as `GtkTextBuffer`'s insert mark behaves,
//! so a user typing at a point that a peer also types at keeps typing after
//! both.

use super::{CollabError, ImportReport, Replica, TextEdit};
use crate::edit::{sequence_text, Op};
use crate::model::Document;

/// Which side of a peer's insert at exactly the same offset a position
/// stays on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bias {
    /// The position stays before text a peer inserted there.
    Before,
    /// The position moves after text a peer inserted there.
    After,
}

/// `offset` after the peer's `edits` (sequential: each is in the offsets the
/// one before it leaves). An offset inside deleted text goes to where the
/// deletion was; an insert strictly before it moves it; an insert at it
/// moves it only for [`Bias::After`].
pub fn map_offset(mut offset: usize, edits: &[TextEdit], bias: Bias) -> usize {
    for edit in edits {
        match *edit {
            TextEdit::Insert { at, len } => {
                if at < offset || (at == offset && bias == Bias::After) {
                    offset += len;
                }
            }
            TextEdit::Delete { at, len } => {
                if offset >= at + len {
                    offset -= len;
                } else if offset > at {
                    offset = at;
                }
            }
        }
    }
    offset
}

/// A selection, as `(insert, bound)` (the caret end and the other end), after
/// the peer's `edits`. A caret moves as [`Bias::After`]. A selection keeps
/// text a peer typed at its edges out: its start moves after an insert there
/// and its end stays before one, and it keeps its direction.
pub fn map_selection(insert: usize, bound: usize, edits: &[TextEdit]) -> (usize, usize) {
    if insert == bound {
        let at = map_offset(insert, edits, Bias::After);
        return (at, at);
    }
    // Neither edge can pass the other: an insert at the start moves it only
    // as far as the text it was before, a delete brings them together.
    let (start, end) = (insert.min(bound), insert.max(bound));
    let (start, end) = (map_offset(start, edits, Bias::After), map_offset(end, edits, Bias::Before));
    if insert <= bound {
        (start, end)
    } else {
        (end, start)
    }
}

/// What a peer's update did, for the editor to show.
#[derive(Debug)]
pub struct Remote {
    /// The document the replica now holds, `None` when the update added
    /// nothing (every op was already here, or it waits for ones before it).
    pub document: Option<Document>,
    /// What changed: the text edits to move offsets over, and what to do to
    /// the undo history ([`ImportReport::rebase`]).
    pub report: ImportReport,
    /// Local changes that had not been sent when the update arrived. They
    /// are not lost: publish them (before anything else).
    pub unsent: Option<Vec<u8>>,
}

/// One replica in one session, and what of it the others have.
pub struct Shared {
    replica: Replica,
    /// The replica's version the last time everything in it was either sent
    /// or from a peer.
    sent: Vec<u8>,
}

impl Shared {
    /// Start a session from `doc` as peer `peer`; with the genesis, the
    /// snapshot a joiner starts from. A document with content the encoding
    /// does not replicate is refused ([`CollabError::Unsupported`]) and
    /// nothing is started.
    pub fn start(peer: u64, doc: &Document) -> Result<(Shared, Vec<u8>), CollabError> {
        let replica = Replica::new(peer, doc)?;
        let genesis = replica.export_snapshot()?;
        let sent = replica.version_bytes();
        Ok((Shared { replica, sent }, genesis))
    }

    /// Join through the genesis `snapshot`: `base` is the file the session
    /// started from (its parts the document does not carry are kept), and
    /// the document is what the replica holds. `peer` must be one this
    /// document has never seen.
    pub fn join(peer: u64, snapshot: &[u8], base: &Document) -> Result<(Shared, Document), CollabError> {
        let (replica, document) = Replica::from_snapshot(peer, snapshot, base)?;
        let sent = replica.version_bytes();
        Ok((Shared { replica, sent }, document))
    }

    /// Record one user action: `ops`, the first applied to `before` (the
    /// editor's document, in step with this replica) and each further one to
    /// what the one before it left. All or nothing: a refused op
    /// ([`CollabError::Unsupported`]) leaves the replica as it was.
    pub fn record(&mut self, before: &Document, ops: &[Op]) -> Result<(), CollabError> {
        self.replica.record_all(before, ops)
    }

    /// What this replica has of its own that the others lack, as one update,
    /// or `None` when there is nothing. Taking it counts it as sent.
    pub fn outgoing(&mut self) -> Result<Option<Vec<u8>>, CollabError> {
        if self.replica.is_at(&self.sent)? {
            return Ok(None);
        }
        let delta = self.replica.export_since(&self.sent)?;
        self.sent = self.replica.version_bytes();
        Ok(Some(delta))
    }

    /// Take in a peer's update, `doc` being the editor's document now. The
    /// update is dry-run first; when it is refused the replica is as it was
    /// ([`Replica::import_updates`]).
    pub fn import(&mut self, update: &[u8], doc: &Document) -> Result<Remote, CollabError> {
        // What the editor has not sent yet goes out as its own update, so
        // that everything the replica holds after the import is either sent
        // or from a peer and the peer's changes are never sent back.
        let unsent = self.outgoing()?;
        let report = self.replica.import_updates(update)?;
        self.sent = self.replica.version_bytes();
        let document = if report.changed() { Some(self.replica.rebuild(doc)?) } else { None };
        Ok(Remote { document, report, unsent })
    }

    /// The document's whole state, for a joiner that has none.
    pub fn snapshot(&self) -> Result<Vec<u8>, CollabError> {
        self.replica.export_snapshot()
    }

    /// The replica, for a check a test or the shell makes against it.
    pub fn replica(&self) -> &Replica {
        &self.replica
    }
}

/// Whether the document a join built (`joined`) is the one the session's
/// first version (`file`, read with the editor's own reader) says: the same
/// paragraphs with the same text. Formatting is not compared, because a
/// saved file need not hold what Loro's marks do (a file format keeps its
/// own subset); the text is what a wrong genesis would get wrong.
pub fn same_text(joined: &Document, file: &Document) -> Result<(), String> {
    let (a, b) = (sequence_text(joined), sequence_text(file));
    if a == b {
        return Ok(());
    }
    let at = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
    Err(format!(
        "the shared text and the file's text first differ at character {at} ({} characters against {})",
        a.chars().count(),
        b.chars().count()
    ))
}

#[cfg(test)]
mod tests;
