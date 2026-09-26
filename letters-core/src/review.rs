// review.rs — table of contents and text direction for Letters.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// This is deliberately GTK-free.  A view may render these ranges as tags or
// a sidebar, while the document/import layers keep the same semantics.

use crate::model::Document;
use serde::{Deserialize, Serialize};

// Comments are marks on the text they cover (`crate::comments`), and so
// are tracked changes (`crate::track`): see docs/LETTERS-REVIEW-WORKFLOWS.md.

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
