// SPDX-License-Identifier: GPL-3.0-or-later
//
// toc.rs — a table of contents: generated from the headings, updated in
// place.
//
// A table of contents is a run of consecutive paragraphs marked as its
// entries (`ParaStyle::toc`, the entry's heading level). Each entry is the
// heading's title, a tab and the page the heading starts on; the layout
// draws the tab as a right-aligned dot leader, as Word and LibreOffice do.
// It is generated, never edited entry by entry: inserting one and updating
// one replace its paragraphs whole (`Op::SetParagraphs`, a block op with an
// exact inverse), so each is one undo step.
//
// Page numbers need the document laid out, and the table of contents is in
// the document it paginates: adding entries can push headings to later
// pages. `settle` updates until the numbers stop changing. The paginator is
// the caller's (the pango layout in the app; anything in tests).
//
// In .docx it is Word's TOC field (docx_toc.rs); in .odt a
// `text:table-of-content` index. Either way another application can update
// it, and its entries reopen as entries here.

use crate::edit::{self, Op};
use crate::model::{Document, ParaStyle, Paragraph, Run};

/// Heading levels a table of contents lists: 1 to 3, Word's default
/// (`TOC \o "1-3"`).
pub const LEVELS: u8 = 3;

/// Each level's indent past the first, in points: LibreOffice's "Contents
/// 2".."Contents 10" styles step 0.499 cm.
pub const INDENT_PT: f64 = 14.15;

/// The one entry of a table of contents with no headings to list (Word's
/// wording).
pub const NO_ENTRIES: &str = "No table of contents entries found.";

/// The tables of contents in `doc`: each a range of paragraph indexes.
pub fn blocks(doc: &Document) -> Vec<std::ops::Range<usize>> {
    let mut out: Vec<std::ops::Range<usize>> = Vec::new();
    for (i, p) in doc.paragraphs.iter().enumerate() {
        if p.style.toc.is_none() {
            continue;
        }
        match out.last_mut() {
            Some(r) if r.end == i => r.end = i + 1,
            _ => out.push(i..i + 1),
        }
    }
    out
}

/// The headings a table of contents lists: (level, title, paragraph).
pub fn headings(doc: &Document) -> Vec<(u8, String, usize)> {
    crate::review::table_of_contents(doc)
        .into_iter()
        .filter(|e| e.level <= LEVELS && doc.paragraphs[e.paragraph].style.toc.is_none())
        .map(|e| (e.level, e.title.replace(crate::layout::OBJECT, "").trim().to_string(), e.paragraph))
        .filter(|(_, title, _)| !title.is_empty())
        .collect()
}

/// An entry paragraph.
fn entry(level: u8, title: &str, page: Option<usize>) -> Paragraph {
    let text = match page {
        Some(page) => format!("{title}\t{}", page + 1),
        None => title.to_string(),
    };
    let runs = vec![Run::plain(text)];
    Paragraph { style: ParaStyle { toc: Some(level), left_indent_pt: INDENT_PT * f64::from(level - 1), ..Default::default() }, runs }
}

/// The entries for `doc`'s headings. `pages[p]` is the page (from 0) that
/// paragraph `p` starts on, if known.
pub fn entries(doc: &Document, pages: &[Option<usize>]) -> Vec<Paragraph> {
    let found = headings(doc);
    if found.is_empty() {
        return vec![entry(1, NO_ENTRIES, None)];
    }
    found.into_iter().map(|(level, title, p)| entry(level, &title, pages.get(p).copied().flatten())).collect()
}

/// Insert a table of contents before paragraph `para`, without page
/// numbers yet (`settle` adds them).
pub fn insert(doc: &Document, para: usize) -> Vec<Op> {
    let mut para = para.min(doc.paragraphs.len());
    // Not between two cells of a table: before it.
    let table = |i: usize| doc.paragraphs.get(i).and_then(|p| p.style.table_cell).map(|c| c.table);
    while para > 0 && table(para).is_some() && table(para) == table(para - 1) {
        para -= 1;
    }
    vec![Op::SetParagraphs { para, remove: 0, insert: entries(doc, &[]) }]
}

/// Regenerate every table of contents from the headings, with `pages` as
/// the page numbers. Empty when they are already current.
pub fn update(doc: &Document, pages: &[Option<usize>]) -> Vec<Op> {
    let fresh = entries(doc, pages);
    blocks(doc)
        .into_iter()
        .rev()
        .filter(|b| doc.paragraphs[b.clone()] != fresh[..])
        .map(|b| Op::SetParagraphs { para: b.start, remove: b.len(), insert: fresh.clone() })
        .collect()
}

/// `first` (say, an insert), then updates until the page numbers settle:
/// the ops that do it all, from `doc`, in order. `paginate` gives each
/// paragraph's page, as `entries` takes them.
pub fn settle(doc: &Document, first: Vec<Op>, paginate: impl Fn(&Document) -> Vec<Option<usize>>) -> Vec<Op> {
    let mut scratch = doc.clone();
    if edit::apply_all(&mut scratch, &first).is_err() {
        return Vec::new();
    }
    let mut ops = first;
    // Entries only ever push headings later, so this settles fast; the
    // bound is for a document that flips between two layouts.
    for _ in 0..4 {
        let step = update(&scratch, &paginate(&scratch));
        if step.is_empty() || edit::apply_all(&mut scratch, &step).is_err() {
            break;
        }
        ops.extend(step);
    }
    ops
}

/// A document with three headings over three pages and a table of contents
/// at the top, as `settle` leaves it (for tests and the render lab).
#[doc(hidden)]
pub fn sample_document() -> Document {
    let mut doc = Document::from_plain_text("Introduction\nWhy it matters.\nMethod\nHow it was done.\nDetails\nThe fine print.\nResults\nWhat came of it.");
    for (i, level) in [(0, 1), (2, 1), (4, 2), (6, 1)] {
        doc.paragraphs[i].style.heading = Some(level);
    }
    for i in [2, 6] {
        doc.paragraphs[i].style.page_break_before = true;
    }
    let ops = settle(&doc, insert(&doc, 0), sample_pages);
    edit::apply_all(&mut doc, &ops).expect("applies");
    doc
}

/// Pages for `sample_document`: a new page at each page break.
#[doc(hidden)]
pub fn sample_pages(doc: &Document) -> Vec<Option<usize>> {
    let mut page = 0;
    doc.paragraphs
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if i > 0 && p.style.page_break_before {
                page += 1;
            }
            Some(page)
        })
        .collect()
}

#[cfg(test)]
mod tests;
