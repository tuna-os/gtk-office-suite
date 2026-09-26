// SPDX-License-Identifier: GPL-3.0-or-later
//
// track.rs — tracked changes as marks (docs/LETTERS-REVIEW-WORKFLOWS.md).
//
// A tracked change is a mark on the text it covers (`RunStyle::revision`,
// `MarkKey::Revision`): an insertion marks the inserted text, a deletion
// marks the deleted text, which stays in the document until the deletion
// is accepted. So a change moves with its text through any edit (no offset
// list to repair), and every step below is ordinary edit ops, each with
// its exact inverse (ADR 0011): recording, accepting and rejecting all
// undo.
//
// - While tracking, typing inserts text marked as an insertion by the
//   author (`tracked`), and deleting marks text as deleted instead of
//   removing it. Deleting your own pending insertion removes it, as Word
//   does: there is nothing to review.
// - Accepting an insertion clears its mark; accepting a deletion removes
//   the text. Rejecting does the opposite.
//
// Limits: paragraph breaks are not tracked (a tracked deletion across a
// break marks the text on both sides and keeps the break); formatting
// changes are not tracked.

use crate::edit::{self, locate, MarkKey, Op};
use crate::layout::is_object;
use crate::model::{Document, Revision, RevisionKind, RunStyle};

/// The value that sets `MarkKey::Revision` to `revision`.
fn mark(revision: Option<Revision>) -> RunStyle {
    RunStyle { revision, ..Default::default() }
}

/// One stretch of one tracked change: sequence offsets `start..end`
/// within a paragraph.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub start: usize,
    pub end: usize,
    pub revision: Revision,
    /// The text it covers (an object counts as its placeholder char).
    pub text: String,
}

/// Every tracked change in `doc`, in document order: runs with the same
/// revision next to each other are one change.
pub fn changes(doc: &Document) -> Vec<Change> {
    let mut out: Vec<Change> = Vec::new();
    let mut pos = 0;
    for (pi, p) in doc.paragraphs.iter().enumerate() {
        if pi > 0 {
            pos += 1;
        }
        for r in &p.runs {
            let n = if is_object(r) { 1 } else { r.text.chars().count() };
            if let Some(rev) = &r.style.revision {
                let text = if is_object(r) { crate::layout::OBJECT.to_string() } else { r.text.clone() };
                match out.last_mut() {
                    Some(last) if last.end == pos && &last.revision == rev => {
                        last.end += n;
                        last.text.push_str(&text);
                    }
                    _ => out.push(Change { start: pos, end: pos + n, revision: rev.clone(), text }),
                }
            }
            pos += n;
        }
    }
    out
}

/// The ops that make `ops` (a user's edit on `doc`) a tracked edit by
/// `author` at `date`: inserted runs are marked as an insertion, and each
/// deletion marks the text as deleted instead, except text that is already
/// this author's pending insertion, which is removed. `None` if an op is
/// not a text edit (the caller applies it untracked).
pub fn tracked(doc: &Document, ops: &[Op], author: &str, date: &str) -> Option<Vec<Op>> {
    let insertion = Revision { kind: RevisionKind::Insert, author: author.to_string(), date: date.to_string(), under: None };
    let deletion = Revision { kind: RevisionKind::Delete, ..insertion.clone() };
    let mut scratch = doc.clone();
    let mut out = Vec::new();
    for op in ops {
        let converted: Vec<Op> = match op {
            Op::Insert { at, content } => {
                // Typing on from one's own pending insertion continues it
                // (one change, dated when it began), as Word does.
                let rev = match revision_before(&scratch, *at) {
                    Some(r) if r.kind == RevisionKind::Insert && r.author == author => r,
                    _ => insertion.clone(),
                };
                let mut content = content.clone();
                for p in &mut content {
                    for r in &mut p.runs {
                        r.style.revision = Some(rev.clone());
                    }
                }
                vec![Op::Insert { at: *at, content }]
            }
            Op::Delete { at, len } => deletion_ops(&scratch, *at, at + len, &deletion),
            Op::Mark { .. } | Op::SetParaStyle { .. } | Op::SetComment { .. } => vec![op.clone()],
            Op::SetParagraphs { .. } => return None,
        };
        for c in &converted {
            edit::apply(&mut scratch, c).ok()?;
        }
        out.extend(converted);
    }
    Some(out)
}

/// The revision of the char just before sequence offset `at`, in the same
/// paragraph.
fn revision_before(doc: &Document, at: usize) -> Option<Revision> {
    let (p, off) = locate(doc, at)?;
    let mut pos = 0;
    for r in &doc.paragraphs[p].runs {
        let n = if is_object(r) { 1 } else { r.text.chars().count() };
        if off > pos && off <= pos + n {
            return r.style.revision.clone();
        }
        pos += n;
    }
    None
}

/// A tracked deletion of `start..end`: from the end backwards (so earlier
/// offsets stay valid), the author's own insertions are removed and the
/// rest is marked deleted (someone else's pending insertion keeps its
/// insertion `under` the deletion). Breaks are kept, and text already
/// marked deleted stays as it is.
fn deletion_ops(doc: &Document, start: usize, end: usize, deletion: &Revision) -> Vec<Op> {
    // (from, to, the revision to set, or None to remove the text)
    let mut spans: Vec<(usize, usize, Option<Revision>)> = Vec::new();
    let mut pos = 0;
    for (pi, p) in doc.paragraphs.iter().enumerate() {
        if pi > 0 {
            pos += 1;
        }
        for r in &p.runs {
            let n = if is_object(r) { 1 } else { r.text.chars().count() };
            let (a, b) = (pos.max(start), (pos + n).min(end));
            pos += n;
            if a >= b {
                continue;
            }
            let target = match &r.style.revision {
                Some(v) if v.kind == RevisionKind::Delete => continue,
                Some(v) if v.author == deletion.author => None,
                Some(v) => Some(Revision { under: Some(Box::new(v.clone())), ..deletion.clone() }),
                None => Some(deletion.clone()),
            };
            match spans.last_mut() {
                Some(last) if last.1 == a && last.2 == target => last.1 = b,
                _ => spans.push((a, b, target)),
            }
        }
    }
    spans
        .into_iter()
        .rev()
        .map(|(a, b, target)| match target {
            None => Op::Delete { at: a, len: b - a },
            Some(rev) => Op::Mark { start: a, end: b, key: MarkKey::Revision, value: mark(Some(rev)) },
        })
        .collect()
}

/// The ops that accept (`accept`) or reject every change overlapping
/// `start..end` (whole changes, not parts of them): an accepted insertion
/// loses its mark and an accepted deletion is removed; a rejected insertion
/// is removed and a rejected deletion goes back to what it deleted (plain
/// text, or the pending insertion under it). From the end backwards, so
/// each op's offsets hold when it is applied.
pub fn resolve(doc: &Document, start: usize, end: usize, accept: bool) -> Vec<Op> {
    let mut ops: Vec<Op> = changes(doc)
        .into_iter()
        .filter(|c| c.start < end.max(start + 1) && c.end > start)
        .map(|c| {
            let len = c.end - c.start;
            match (c.revision.kind, accept) {
                (RevisionKind::Insert, true) => Op::Mark { start: c.start, end: c.end, key: MarkKey::Revision, value: mark(None) },
                (RevisionKind::Insert, false) | (RevisionKind::Delete, true) => Op::Delete { at: c.start, len },
                (RevisionKind::Delete, false) => {
                    Op::Mark { start: c.start, end: c.end, key: MarkKey::Revision, value: mark(c.revision.under.map(|u| *u)) }
                }
            }
        })
        .collect();
    ops.reverse();
    ops
}

/// The ops that accept or reject every change in `doc`, including an
/// insertion that a rejected deletion brings back.
pub fn resolve_all(doc: &Document, accept: bool) -> Vec<Op> {
    let mut scratch = doc.clone();
    let mut out = Vec::new();
    for _ in 0..3 {
        let ops = resolve(&scratch, 0, edit::doc_len(&scratch), accept);
        if ops.is_empty() || edit::apply_all(&mut scratch, &ops).is_err() {
            break;
        }
        out.extend(ops);
    }
    out
}

/// The change at sequence offset `at` (touching it counts), if any.
pub fn change_at(doc: &Document, at: usize) -> Option<Change> {
    changes(doc).into_iter().find(|c| c.start <= at && at <= c.end)
}

/// Whether `doc` holds any tracked change (a save to a format that cannot
/// carry them says so).
pub fn has_changes(doc: &Document) -> bool {
    doc.paragraphs.iter().flat_map(|p| &p.runs).any(|r| r.style.revision.is_some())
}

/// `doc` as it reads with every change accepted, for word counts and plain
/// exports that cannot show changes.
pub fn accepted(doc: &Document) -> Document {
    let mut out = doc.clone();
    let _ = edit::apply_all(&mut out, &resolve_all(doc, true));
    out
}

/// Sequence offset `at` rounded into the document (for callers that hold a
/// caret past the end).
pub fn clamp(doc: &Document, at: usize) -> usize {
    if locate(doc, at).is_some() { at } else { edit::doc_len(doc) }
}

/// Now, as a tracked change's date: UTC to the second, as Word and ODF
/// store it.
pub fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// The colour a tracked change by `author` is drawn in (16-bit RGB): one of
/// a few distinct dark hues, the same for an author every time (a hash of
/// the name, not the order authors appear in).
pub fn author_color(author: &str) -> (u16, u16, u16) {
    const COLORS: [(u16, u16, u16); 6] = [
        (0xb3b3, 0x1111, 0x2222), // red
        (0x1a1a, 0x5555, 0xb4b4), // blue
        (0x1111, 0x7777, 0x2222), // green
        (0x8888, 0x2222, 0xaaaa), // purple
        (0xa6a6, 0x5555, 0x0000), // orange
        (0x0000, 0x7777, 0x7777), // teal
    ];
    let hash = author.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ u32::from(b));
    COLORS[hash as usize % COLORS.len()]
}

/// `author_color` as "#rrggbb".
pub fn author_hex(author: &str) -> String {
    let (r, g, b) = author_color(author);
    format!("#{:02x}{:02x}{:02x}", r >> 8, g >> 8, b >> 8)
}

/// A paragraph with an insertion, a deletion and a deletion of someone
/// else's insertion, for the format round-trip and LibreOffice oracle
/// tests.
#[doc(hidden)]
pub fn sample_document() -> Document {
    let rev = |kind, author: &str| Revision { kind, author: author.into(), date: "2026-09-25T20:30:00Z".into(), under: None };
    let styled = |text: &str, revision: Revision| crate::model::Run { text: text.into(), style: mark(Some(revision)) };
    let mut d = Document::from_plain_text("");
    d.paragraphs[0].runs = vec![
        crate::model::Run::plain("Keep "),
        styled("added ", rev(RevisionKind::Insert, "Ada Lovelace")),
        styled("gone ", rev(RevisionKind::Delete, "Grace Hopper")),
        styled("twice", Revision { under: Some(Box::new(rev(RevisionKind::Insert, "Ada Lovelace"))), ..rev(RevisionKind::Delete, "Grace Hopper") }),
        crate::model::Run::plain(" end."),
    ];
    d
}

#[cfg(test)]
mod tests;
