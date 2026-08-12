// review.rs — review annotations and deterministic navigation for Letters.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// This is deliberately GTK-free.  A view may render these ranges as tags or
// a sidebar, while the document/import layers keep the same semantics.

use crate::model::Document;
use serde::{Deserialize, Serialize};

pub type ReviewId = u64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextRange {
    pub start: usize,
    pub end: usize,
}

impl TextRange {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end: end.max(start) }
    }

    pub fn len(&self) -> usize { self.end.saturating_sub(self.start) }
    pub fn is_empty(&self) -> bool { self.start == self.end }

    /// Rebase an annotation after one buffer edit at `at`.
    pub fn rebase(&mut self, at: usize, removed: usize, inserted: usize) {
        let removed_end = at.saturating_add(removed);
        let delta = inserted as isize - removed as isize;
        let shift = |offset: usize| -> usize {
            if offset >= removed_end {
                offset.saturating_add_signed(delta)
            } else if offset > at {
                at.saturating_add(inserted)
            } else {
                offset
            }
        };
        self.start = shift(self.start);
        self.end = shift(self.end).max(self.start);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub id: ReviewId,
    pub range: TextRange,
    pub author: String,
    pub text: String,
    pub resolved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeKind { Insert, Delete }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackedChange {
    pub id: ReviewId,
    /// Insertions cover their visible text. Deletions are zero-width at the
    /// location where the removed text should be restored.
    pub range: TextRange,
    pub author: String,
    pub text: String,
    pub kind: ChangeKind,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewState {
    #[serde(default = "first_review_id")]
    next_id: ReviewId,
    pub comments: Vec<Comment>,
    pub changes: Vec<TrackedChange>,
}

fn first_review_id() -> ReviewId { 1 }

impl ReviewState {
    pub fn new() -> Self { Self { next_id: 1, ..Default::default() } }

    fn id(&mut self) -> ReviewId {
        let id = self.next_id.max(1);
        self.next_id = id.saturating_add(1);
        id
    }

    pub fn add_comment(&mut self, range: TextRange, author: impl Into<String>, text: impl Into<String>) -> ReviewId {
        let id = self.id();
        self.comments.push(Comment { id, range, author: author.into(), text: text.into(), resolved: false });
        id
    }

    pub fn add_change(&mut self, range: TextRange, kind: ChangeKind, author: impl Into<String>, text: impl Into<String>) -> ReviewId {
        let id = self.id();
        self.changes.push(TrackedChange { id, range, kind, author: author.into(), text: text.into() });
        id
    }

    pub fn resolve_comment(&mut self, id: ReviewId, resolved: bool) -> bool {
        self.comments.iter_mut().find(|c| c.id == id).map(|c| { c.resolved = resolved; true }).unwrap_or(false)
    }

    /// Return the next unresolved comment in document order, wrapping once.
    pub fn next_comment(&self, cursor: usize) -> Option<&Comment> {
        let mut comments: Vec<&Comment> = self.comments.iter().filter(|c| !c.resolved).collect();
        comments.sort_by_key(|c| (c.range.start, c.id));
        if let Some(comment) = comments.iter().copied().find(|c| c.range.start >= cursor) {
            Some(comment)
        } else {
            comments.into_iter().next()
        }
    }

    /// Accept or reject a revision and remove it from the pending queue.
    /// Rejecting an insertion removes visible text; rejecting a deletion
    /// restores its saved text. Accepting leaves the document as-is.
    pub fn decide_change(&mut self, doc: &mut Document, id: ReviewId, accept: bool) -> bool {
        let Some(index) = self.changes.iter().position(|c| c.id == id) else { return false };
        let change = self.changes.remove(index);
        if !accept {
            match change.kind {
                ChangeKind::Insert => doc.delete_range(change.range.start, change.range.end),
                ChangeKind::Delete => doc.insert_text(change.range.start, &change.text),
            }
        }
        true
    }

    pub fn rebase_after_edit(&mut self, at: usize, removed: usize, inserted: usize) {
        for c in &mut self.comments { c.range.rebase(at, removed, inserted); }
        for c in &mut self.changes { c.range.rebase(at, removed, inserted); }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TocEntry {
    pub paragraph: usize,
    pub level: u8,
    pub title: String,
    pub offset: usize,
}

/// Build a TOC from explicit heading levels or conventional named styles.
/// Paragraph order is the only ordering input, making updates deterministic.
pub fn table_of_contents(doc: &Document) -> Vec<TocEntry> {
    let mut offset = 0;
    let mut entries = Vec::new();
    for (paragraph, p) in doc.paragraphs.iter().enumerate() {
        let level = p.style.heading.filter(|n| (1..=6).contains(n)).or_else(|| named_style_level(p.style.named_style.as_deref()));
        if let Some(level) = level {
            let title = p.text().trim().to_string();
            if !title.is_empty() { entries.push(TocEntry { paragraph, level, title, offset }); }
        }
        offset += p.char_len() + 1;
    }
    entries
}

fn named_style_level(name: Option<&str>) -> Option<u8> {
    let name = name?.trim();
    let lower = name.to_ascii_lowercase();
    if lower == "title" { return Some(1); }
    if lower == "subtitle" { return Some(2); }
    let suffix = lower.strip_prefix("heading ").or_else(|| lower.strip_prefix("heading"))?;
    suffix.trim().parse::<u8>().ok().filter(|n| (1..=6).contains(n))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BidiDirection { #[default] Ltr, Rtl }

impl BidiDirection {
    pub fn opposite(self) -> Self { match self { Self::Ltr => Self::Rtl, Self::Rtl => Self::Ltr } }
}

/// Resolve the paragraph base direction from the first strong character.
/// The ranges cover the RTL scripts supported by Pango without treating
/// numbers or punctuation as a direction signal.
pub fn base_direction(text: &str, fallback: BidiDirection) -> BidiDirection {
    for ch in text.chars() {
        let u = ch as u32;
        if is_rtl(u) { return BidiDirection::Rtl; }
        if is_ltr(u) { return BidiDirection::Ltr; }
    }
    fallback
}

fn is_rtl(u: u32) -> bool {
    (0x0590..=0x08ff).contains(&u) || (0xfb1d..=0xfdff).contains(&u) || (0xfe70..=0xfeff).contains(&u) || (0x10800..=0x10fff).contains(&u)
}

fn is_ltr(u: u32) -> bool {
    (0x0041..=0x005a).contains(&u) || (0x0061..=0x007a).contains(&u) || (0x00c0..=0x02af).contains(&u) || (0x0370..=0x052f).contains(&u)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ParaStyle;

    #[test]
    fn comments_navigate_in_document_order_and_wrap() {
        let mut state = ReviewState::new();
        let late = state.add_comment(TextRange::new(20, 22), "a", "late");
        let early = state.add_comment(TextRange::new(2, 4), "b", "early");
        assert_eq!(state.next_comment(0).unwrap().id, early);
        assert_eq!(state.next_comment(5).unwrap().id, late);
        assert_eq!(state.next_comment(30).unwrap().id, early);
    }

    #[test]
    fn rejecting_changes_has_expected_document_effect() {
        let mut doc = Document::from_plain_text("one two");
        let mut state = ReviewState::new();
        let insertion = state.add_change(TextRange::new(4, 8), ChangeKind::Insert, "a", "two");
        assert!(state.decide_change(&mut doc, insertion, false));
        assert_eq!(doc.to_plain_text(), "one ");
        let deletion = state.add_change(TextRange::new(4, 4), ChangeKind::Delete, "a", "two");
        assert!(state.decide_change(&mut doc, deletion, false));
        assert_eq!(doc.to_plain_text(), "one two");
    }

    #[test]
    fn toc_uses_heading_and_named_styles() {
        let mut doc = Document::from_plain_text("Title\nIntro\nChapter");
        doc.paragraphs[0].style.named_style = Some("Title".into());
        doc.paragraphs[2].style = ParaStyle { named_style: Some("Heading 2".into()), ..Default::default() };
        assert_eq!(table_of_contents(&doc), vec![
            TocEntry { paragraph: 0, level: 1, title: "Title".into(), offset: 0 },
            TocEntry { paragraph: 2, level: 2, title: "Chapter".into(), offset: 12 },
        ]);
    }

    #[test]
    fn bidi_uses_first_strong_character_and_fallback() {
        assert_eq!(base_direction("123 שלום", BidiDirection::Ltr), BidiDirection::Rtl);
        assert_eq!(base_direction("123", BidiDirection::Rtl), BidiDirection::Rtl);
        assert_eq!(BidiDirection::Rtl.opposite(), BidiDirection::Ltr);
    }
}
