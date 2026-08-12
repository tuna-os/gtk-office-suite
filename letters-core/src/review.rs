//! GTK-free review and navigation primitives for Letters.
//!
//! Ranges use Unicode scalar offsets, matching `Document`/GtkTextBuffer
//! addressing. Format adapters can map these records to DOCX/ODT parts once
//! their upstream APIs and fixtures are admitted.

use crate::model::Document;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevisionKind { Insert, Delete }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevisionDecision { #[default] Pending, Accepted, Rejected }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revision {
    pub id: u64,
    pub author: String,
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub kind: RevisionKind,
    pub decision: RevisionDecision,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub id: u64,
    pub author: String,
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub resolved: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewState {
    pub revisions: Vec<Revision>,
    pub comments: Vec<Comment>,
}

impl ReviewState {
    pub fn accept_revision(&mut self, id: u64) -> bool {
        self.set_revision(id, RevisionDecision::Accepted)
    }

    pub fn reject_revision(&mut self, id: u64) -> bool {
        self.set_revision(id, RevisionDecision::Rejected)
    }

    pub fn resolve_comment(&mut self, id: u64) -> bool {
        if let Some(comment) = self.comments.iter_mut().find(|c| c.id == id) {
            comment.resolved = true;
            true
        } else { false }
    }

    pub fn next_revision(&self, offset: usize, forward: bool) -> Option<&Revision> {
        let mut items: Vec<&Revision> = self.revisions.iter()
            .filter(|r| r.decision == RevisionDecision::Pending)
            .collect();
        items.sort_by_key(|r| (r.start, r.id));
        if forward {
            items.into_iter().find(|r| r.start > offset).or_else(|| {
                self.revisions.iter().filter(|r| r.decision == RevisionDecision::Pending)
                    .min_by_key(|r| (r.start, r.id))
            })
        } else {
            items.into_iter().rev().find(|r| r.start < offset).or_else(|| {
                self.revisions.iter().filter(|r| r.decision == RevisionDecision::Pending)
                    .max_by_key(|r| (r.start, r.id))
            })
        }
    }

    pub fn next_comment(&self, offset: usize, forward: bool) -> Option<&Comment> {
        let mut items: Vec<&Comment> = self.comments.iter().filter(|c| !c.resolved).collect();
        items.sort_by_key(|c| (c.start, c.id));
        if forward {
            items.into_iter().find(|c| c.start > offset).or_else(|| self.comments.iter()
                .filter(|c| !c.resolved).min_by_key(|c| (c.start, c.id)))
        } else {
            items.into_iter().rev().find(|c| c.start < offset).or_else(|| self.comments.iter()
                .filter(|c| !c.resolved).max_by_key(|c| (c.start, c.id)))
        }
    }

    fn set_revision(&mut self, id: u64, decision: RevisionDecision) -> bool {
        if let Some(revision) = self.revisions.iter_mut().find(|r| r.id == id) {
            revision.decision = decision;
            true
        } else { false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextDirection { Ltr, Rtl, Neutral }

/// Find the first strong directional character, matching the editor's
/// paragraph-level fallback when no explicit bidi paragraph style exists.
pub fn direction_of(text: &str) -> TextDirection {
    for ch in text.chars() {
        let code = ch as u32;
        if matches!(code, 0x0590..=0x08ff | 0xfb1d..=0xfdff | 0xfe70..=0xfefc) {
            return TextDirection::Rtl;
        }
        if ch.is_alphabetic() { return TextDirection::Ltr; }
    }
    TextDirection::Neutral
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TocEntry { pub paragraph: usize, pub level: u8, pub title: String }

/// Build a stable TOC from heading styles in document order. Empty headings
/// and invalid levels are excluded; no layout or renderer state is involved.
pub fn table_of_contents(document: &Document) -> Vec<TocEntry> {
    document.paragraphs.iter().enumerate().filter_map(|(paragraph, p)| {
        let level = p.style.heading?;
        if !(1..=6).contains(&level) { return None; }
        let title = p.text().trim().to_string();
        (!title.is_empty()).then_some(TocEntry { paragraph, level, title })
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Document, Paragraph, Run};

    #[test]
    fn review_navigation_wraps_and_accept_reject_is_explicit() {
        let mut state = ReviewState { revisions: vec![
            Revision { id: 2, author: "b".into(), start: 20, end: 21, text: "x".into(), kind: RevisionKind::Delete, decision: RevisionDecision::Pending },
            Revision { id: 1, author: "a".into(), start: 5, end: 5, text: "y".into(), kind: RevisionKind::Insert, decision: RevisionDecision::Pending },
        ], comments: vec![] };
        assert_eq!(state.next_revision(20, true).unwrap().id, 1);
        assert!(state.accept_revision(1));
        assert_eq!(state.next_revision(0, true).unwrap().id, 2);
        assert!(state.reject_revision(2));
        assert!(state.next_revision(0, true).is_none());
    }

    #[test]
    fn toc_is_document_ordered_and_direction_uses_strong_text() {
        let document = Document { paragraphs: vec![
            Paragraph { style: Default::default(), runs: vec![Run::plain("body")] },
            Paragraph { style: crate::model::ParaStyle { heading: Some(2), ..Default::default() }, runs: vec![Run::plain("Chapter")] },
            Paragraph { style: crate::model::ParaStyle { heading: Some(1), ..Default::default() }, runs: vec![Run::plain("مقدمة")] },
        ], ..Default::default() };
        let toc = table_of_contents(&document);
        assert_eq!(toc.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), vec!["Chapter", "مقدمة"]);
        assert_eq!(direction_of("123 שלום"), TextDirection::Rtl);
        assert_eq!(direction_of("123 abc"), TextDirection::Ltr);
        assert_eq!(direction_of("123 —"), TextDirection::Neutral);
    }
}
