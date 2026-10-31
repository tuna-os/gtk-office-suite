// SPDX-License-Identifier: GPL-3.0-or-later
//! Stored undo steps over a peer's edits (ADR 0011, "Stored inverses are
//! rebased on remote events").
//!
//! A step is the ops that undo (or redo) one of the user's own actions,
//! addressing the document as it is when the step is applied. A peer's
//! edits, a [`TextEdit`] list, changed that document under it. [`Rebaser`]
//! rewrites a step so it applies to the changed document, by operational
//! transformation of the text edits both ways:
//!
//! - an offset moves over the peer's inserts and deletes before it;
//! - a delete (the undo of typing) and a mark range (the undo of a format)
//!   skip text the peer inserted inside them, so undo never takes back
//!   other people's words, and lose the part the peer deleted;
//! - at one offset the peer's insert goes before the user's;
//! - a paragraph restyle whose paragraph break the peer deleted is
//!   dropped: that paragraph is no longer there to restyle;
//! - a step with nothing left is dropped, that step only.
//!
//! Steps of one chain are visited top first, and the peer's edits are
//! carried along: a step lower down addresses the document as it is once
//! the ones above it have been undone, so the peer's edits are transformed
//! over each step's own text effect as it goes. A step that is dropped
//! leaves the document without its undo, so what is carried is the peer's
//! edits after the step's forward edit.

use super::TextEdit;
use crate::edit::{seq_len, Op};
use crate::model::Paragraph;

/// Rewrites the steps of one chain, top first, over one import's edits.
#[derive(Clone, Debug)]
pub struct Rebaser {
    /// The peer's edits, in the coordinates of the document at the step
    /// about to be visited.
    remote: Vec<TextEdit>,
}

impl Rebaser {
    pub(super) fn new(edits: &[TextEdit]) -> Self {
        Rebaser { remote: edits.to_vec() }
    }

    /// `ops`, as they apply to the document the peer's edits made; `None`
    /// when nothing of the step is left to do.
    pub fn step(&mut self, ops: &[Op]) -> Option<Vec<Op>> {
        let mut remote = self.remote.clone();
        let mut out = Vec::with_capacity(ops.len());
        for op in ops {
            match op {
                Op::Insert { at, content } => {
                    let mine = [TextEdit::Insert { at: *at, len: content_len(content) }];
                    let (done, rest) = it_lists(&mine, &remote);
                    let Some(TextEdit::Insert { at, .. }) = done.first().copied() else { return self.drop_step(ops) };
                    out.push(Op::Insert { at, content: content.clone() });
                    remote = rest;
                }
                Op::Delete { at, len } => {
                    let (done, rest) = it_lists(&[TextEdit::Delete { at: *at, len: *len }], &remote);
                    out.extend(done.into_iter().filter_map(|edit| match edit {
                        TextEdit::Delete { at, len } => Some(Op::Delete { at, len }),
                        TextEdit::Insert { .. } => None,
                    }));
                    remote = rest;
                }
                Op::Mark { start, end, key, value } => {
                    let mut ranges = vec![(*start, *end)];
                    for edit in &remote {
                        ranges = ranges.into_iter().flat_map(|range| range_through(range, *edit)).collect();
                    }
                    out.extend(ranges.into_iter().map(|(start, end)| Op::Mark { start, end, key: *key, value: value.clone() }));
                }
                Op::SetParaStyle { at, style } => {
                    let Some(at) = paragraph_through(*at, &remote) else { return self.drop_step(ops) };
                    out.push(Op::SetParaStyle { at, style: style.clone() });
                }
                // Not text: nothing to move.
                other => out.push(other.clone()),
            }
        }
        self.remote = remote;
        (!out.is_empty()).then_some(out)
    }

    /// The step cannot be applied. It stays out of the history, so the
    /// document keeps its forward edit, and the steps below it, which
    /// address the document without it, must be moved over that: back out
    /// of the step's effect, then the peer's edits.
    fn drop_step(&mut self, ops: &[Op]) -> Option<Vec<Op>> {
        let mut carried: Vec<TextEdit> = ops
            .iter()
            .rev()
            .filter_map(|op| match op {
                Op::Insert { at, content } => Some(TextEdit::Delete { at: *at, len: content_len(content) }),
                Op::Delete { at, len } => Some(TextEdit::Insert { at: *at, len: *len }),
                _ => None,
            })
            .collect();
        carried.extend(self.remote.iter().copied());
        self.remote = carried;
        None
    }
}

/// The sequence length of `content` as `Op::Insert` inserts it.
fn content_len(content: &[Paragraph]) -> usize {
    content.iter().map(|p| seq_len(&p.runs)).sum::<usize>() + content.len().saturating_sub(1)
}

/// Where the paragraph holding offset `at` is after `edits`, or `None` when
/// the break before it was deleted (taken as `at - 1`; at the start of the
/// paragraph, where every producer addresses it, that is the break).
fn paragraph_through(mut at: usize, edits: &[TextEdit]) -> Option<usize> {
    for edit in edits {
        match *edit {
            TextEdit::Insert { at: p, len } => {
                if p <= at {
                    at += len;
                }
            }
            TextEdit::Delete { at: p, len } => {
                if at > 0 && p < at && at - 1 < p + len {
                    return None;
                }
                if at >= p + len {
                    at -= len;
                } else if at > p {
                    at = p;
                }
            }
        }
    }
    Some(at)
}

/// The pieces of `range` after `edit`, in the new coordinates and all at
/// once (a mark changes no offset): text inserted inside splits it, text
/// deleted leaves the rest.
fn range_through((start, end): (usize, usize), edit: TextEdit) -> Vec<(usize, usize)> {
    let pieces = match edit {
        TextEdit::Insert { at, len } => {
            if at <= start {
                vec![(start + len, end + len)]
            } else if at >= end {
                vec![(start, end)]
            } else {
                vec![(start, at), (at + len, end + len)]
            }
        }
        TextEdit::Delete { at, len } => {
            let mut pieces = Vec::new();
            if start < at {
                pieces.push((start, end.min(at)));
            }
            if end > at + len {
                pieces.push((start.max(at + len) - len, end - len));
            }
            pieces
        }
    };
    pieces.into_iter().filter(|(start, end)| end > start).collect()
}

/// `a` as it applies after `b` happened instead of before it (both from the
/// same document). When both insert at one offset, `b`'s text goes first if
/// `b_first`. A delete that `b` split gives its later piece first, so that
/// applied in order each is where it says.
fn through(a: TextEdit, b: TextEdit, b_first: bool) -> Vec<TextEdit> {
    use TextEdit::{Delete, Insert};
    if let Delete { len: 0, .. } = a {
        return Vec::new();
    }
    match (a, b) {
        (Insert { at, len }, Insert { at: p, len: n }) => {
            vec![Insert { at: if p < at || (p == at && b_first) { at + n } else { at }, len }]
        }
        (Insert { at, len }, Delete { at: p, len: n }) => {
            vec![Insert { at: if at <= p { at } else if at >= p + n { at - n } else { p }, len }]
        }
        (Delete { at, len }, Insert { at: p, len: n }) => {
            if p <= at {
                vec![Delete { at: at + n, len }]
            } else if p >= at + len {
                vec![Delete { at, len }]
            } else {
                vec![Delete { at: p + n, len: at + len - p }, Delete { at, len: p - at }]
            }
        }
        (Delete { at, len }, Delete { at: p, len: n }) => {
            let (a1, b1) = (at + len, p + n);
            let mut out = Vec::new();
            if a1 > b1 {
                let from = at.max(b1);
                out.push(Delete { at: from - n, len: a1 - from });
            }
            if at < p {
                out.push(Delete { at, len: a1.min(p) - at });
            }
            out
        }
    }
}

/// Transform the sequential edit lists `a` and `b`, which both start from
/// the same document, so that `a` then `b'` and `b` then `a'` give the same
/// document. Returns `(a', b')`. Where both insert at one offset, `b`'s
/// text goes first.
pub(super) fn it_lists(a: &[TextEdit], b: &[TextEdit]) -> (Vec<TextEdit>, Vec<TextEdit>) {
    match (a, b) {
        ([], _) => (Vec::new(), b.to_vec()),
        (_, []) => (a.to_vec(), Vec::new()),
        ([x], [y]) => (through(*x, *y, true), through(*y, *x, false)),
        ([x], [y, rest @ ..]) => {
            let (x2, y2) = it_lists(&[*x], &[*y]);
            let (x3, rest2) = it_lists(&x2, rest);
            (x3, y2.into_iter().chain(rest2).collect())
        }
        ([x, rest @ ..], _) => {
            let (x2, b2) = it_lists(&[*x], b);
            let (rest2, b3) = it_lists(rest, &b2);
            (x2.into_iter().chain(rest2).collect(), b3)
        }
    }
}
