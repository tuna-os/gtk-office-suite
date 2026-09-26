// SPDX-License-Identifier: GPL-3.0-or-later
//
// comments.rs — comments on the text, as marks.
//
// A comment thread is a first comment and its replies (`model::Comment`,
// held in `Document::comments`). The first comment is anchored to the text
// it is about by a mark of its own on that text (`MarkKey::Comment(id)`,
// `RunStyle::comments`), so the anchor moves with edits as any mark does,
// two comments can cover the same text, and text typed inside a commented
// stretch is inside the comment (`edit::typing_style`). Replies are not
// marked: they share their thread's anchor.
//
// Everything here returns ops (`edit::Op`), so adding, replying,
// resolving and deleting are each one undo step with an exact inverse.
// A thread whose text was all deleted keeps its comments (an orphan, as
// Google Docs shows it) until it is deleted.

use crate::edit::{self, MarkKey, Op};
use crate::layout::is_object;
use crate::model::{Comment, Document, RunStyle};

/// Where a thread's anchor is: the sequence offsets of its first and past
/// its last marked char, and the text between.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// A comment thread: the first comment, its replies in id order, and its
/// anchor (None when its text was deleted).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thread {
    pub comment: Comment,
    pub replies: Vec<Comment>,
    pub anchor: Option<Anchor>,
}

/// The anchor of every marked comment id.
fn anchors(doc: &Document) -> std::collections::BTreeMap<u32, Anchor> {
    let mut out: std::collections::BTreeMap<u32, Anchor> = Default::default();
    let mut pos = 0;
    for (i, p) in doc.paragraphs.iter().enumerate() {
        if i > 0 {
            // A paragraph break inside an anchor is part of its text.
            for a in out.values_mut().filter(|a| a.end == pos) {
                a.text.push('\n');
                a.end += 1;
            }
            pos += 1;
        }
        for r in &p.runs {
            let text = if is_object(r) { crate::layout::OBJECT.to_string() } else { r.text.clone() };
            let n = text.chars().count();
            for id in &r.style.comments {
                let a = out.entry(*id).or_insert(Anchor { start: pos, end: pos, text: String::new() });
                if a.end < pos {
                    // Discontinuous (text moved out of it): the anchor is
                    // the whole extent.
                    let gap = edit::slice(doc, a.end, pos).map(|ps| ps.iter().map(|p| crate::layout::layout_text(&p.runs)).collect::<Vec<_>>().join("\n"));
                    a.text.push_str(&gap.unwrap_or_default());
                }
                a.text.push_str(&text);
                a.end = pos + n;
            }
            pos += n;
        }
    }
    // A trailing break the loop above took belongs to no text.
    for a in out.values_mut() {
        while a.text.ends_with('\n') && a.end > a.start {
            a.text.pop();
            a.end -= 1;
        }
    }
    out
}

/// The threads: anchored ones in document order, then orphans by id.
pub fn threads(doc: &Document) -> Vec<Thread> {
    let anchors = anchors(doc);
    let mut out: Vec<Thread> = doc
        .comments
        .iter()
        .filter(|c| c.parent.is_none())
        .map(|c| Thread {
            comment: c.clone(),
            replies: doc.comments.iter().filter(|r| r.parent == Some(c.id)).cloned().collect(),
            anchor: anchors.get(&c.id).cloned(),
        })
        .collect();
    out.sort_by_key(|t| (t.anchor.is_none(), t.anchor.as_ref().map_or(0, |a| a.start), t.comment.id));
    out
}

/// The thread of comment `id` (a reply's is its parent's).
pub fn thread(doc: &Document, id: u32) -> Option<Thread> {
    let root = doc.comments.iter().find(|c| c.id == id)?.parent.unwrap_or(id);
    threads(doc).into_iter().find(|t| t.comment.id == root)
}

/// The threads whose anchor holds the char at sequence offset `at` or the
/// one before it (the caret touching the text), in document order.
pub fn at(doc: &Document, at: usize) -> Vec<u32> {
    threads(doc)
        .into_iter()
        .filter(|t| t.anchor.as_ref().is_some_and(|a| a.start <= at && at <= a.end))
        .map(|t| t.comment.id)
        .collect()
}

/// The id a new comment takes: one past every id in use, bodies and marks.
pub fn next_id(doc: &Document) -> u32 {
    let marked = doc.paragraphs.iter().flat_map(|p| &p.runs).flat_map(|r| r.style.comments.iter().copied());
    doc.comments.iter().map(|c| c.id).chain(marked).max().map_or(1, |m| m + 1)
}

fn mark(id: u32, on: bool) -> RunStyle {
    RunStyle { comments: if on { vec![id] } else { Vec::new() }, ..Default::default() }
}

/// Comment `text` on sequence offsets `start..end`: the ops, and the new
/// comment's id. None for an empty range.
pub fn add(doc: &Document, start: usize, end: usize, author: &str, date: &str, text: &str) -> Option<(Vec<Op>, u32)> {
    let (start, end) = (start.min(end), start.max(end).min(edit::doc_len(doc)));
    if start == end {
        return None;
    }
    let id = next_id(doc);
    let comment = Comment { id, author: author.into(), date: date.into(), text: text.into(), resolved: false, parent: None };
    Some((vec![Op::SetComment { id, comment: Some(comment) }, Op::Mark { start, end, key: MarkKey::Comment(id), value: mark(id, true) }], id))
}

/// Reply `text` to the thread of comment `to`. None if there is no such
/// comment.
pub fn reply(doc: &Document, to: u32, author: &str, date: &str, text: &str) -> Option<(Vec<Op>, u32)> {
    let root = doc.comments.iter().find(|c| c.id == to)?.parent.unwrap_or(to);
    let id = next_id(doc);
    let comment = Comment { id, author: author.into(), date: date.into(), text: text.into(), resolved: false, parent: Some(root) };
    Some((vec![Op::SetComment { id, comment: Some(comment) }], id))
}

/// Mark the thread of comment `id` resolved (or open again). A resolved
/// thread keeps its anchor; views show it dimmed or not at all.
pub fn set_resolved(doc: &Document, id: u32, resolved: bool) -> Vec<Op> {
    let Some(t) = thread(doc, id) else { return Vec::new() };
    if t.comment.resolved == resolved {
        return Vec::new();
    }
    vec![Op::SetComment { id: t.comment.id, comment: Some(Comment { resolved, ..t.comment }) }]
}

/// Change comment `id`'s text.
pub fn edit_text(doc: &Document, id: u32, text: &str) -> Vec<Op> {
    match doc.comments.iter().find(|c| c.id == id) {
        Some(c) if c.text != text => vec![Op::SetComment { id, comment: Some(Comment { text: text.into(), ..c.clone() }) }],
        _ => Vec::new(),
    }
}

/// Delete comment `id`: a reply alone, or a whole thread with its anchor.
pub fn delete(doc: &Document, id: u32) -> Vec<Op> {
    let Some(c) = doc.comments.iter().find(|c| c.id == id) else { return Vec::new() };
    if c.parent.is_some() {
        return vec![Op::SetComment { id, comment: None }];
    }
    let mut ops = Vec::new();
    if let Some(a) = anchors(doc).get(&id) {
        ops.push(Op::Mark { start: a.start, end: a.end, key: MarkKey::Comment(id), value: mark(id, false) });
    }
    ops.extend(doc.comments.iter().filter(|r| r.parent == Some(id)).map(|r| Op::SetComment { id: r.id, comment: None }));
    ops.push(Op::SetComment { id, comment: None });
    ops
}

/// Two authors' comments on overlapping text, one resolved, one replied to
/// (for tests and the render lab).
#[doc(hidden)]
pub fn sample_document() -> Document {
    let mut doc = Document::from_plain_text("The quarterly figures look strong.\nRevenue grew in every region but one.");
    let date = "2026-09-26T09:15:00Z";
    for (start, end, author, text) in [(4, 21, "Ada Lovelace", "Which quarter?"), (14, 33, "Grace Hopper", "Source for this?")] {
        let (ops, _) = add(&doc, start, end, author, date, text).expect("a range");
        edit::apply_all(&mut doc, &ops).expect("applies");
    }
    let (ops, _) = reply(&doc, 1, "Grace Hopper", "2026-09-26T09:20:00Z", "Q3, see the appendix.").expect("comment 1");
    edit::apply_all(&mut doc, &ops).expect("applies");
    let (ops, _) = add(&doc, 35, 72, "Ada Lovelace", date, "Name the region.").expect("a range");
    edit::apply_all(&mut doc, &ops).expect("applies");
    let ops = set_resolved(&doc, 4, true);
    edit::apply_all(&mut doc, &ops).expect("applies");
    doc
}

#[cfg(test)]
mod tests;
