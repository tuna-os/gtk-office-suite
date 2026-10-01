//! Fixtures shared by the performance and peak-memory budgets (#1208).
#![allow(dead_code)] // each test binary uses its own subset

use letters_core::{Document, ListKind, ParaStyle, Paragraph, Run, RunStyle};

pub struct Size {
    pub name: &'static str,
    pub paragraphs: usize,
}

pub const SMALL: Size = Size { name: "small", paragraphs: 20 };
pub const MEDIUM: Size = Size { name: "medium", paragraphs: 500 };
pub const LARGE: Size = Size { name: "large", paragraphs: 5_000 };

/// A document shaped like real writing: headings every twenty paragraphs,
/// mixed bold and italic runs, a bulleted list in each section.
pub fn document(size: &Size) -> Document {
    let mut doc = Document::default();
    doc.paragraphs.clear();
    for i in 0..size.paragraphs {
        let mut style = ParaStyle::default();
        let runs = if i % 20 == 0 {
            style.heading = Some(if i % 100 == 0 { 1 } else { 2 });
            vec![Run::plain(format!("Section {}", i / 20 + 1))]
        } else {
            if i % 20 >= 15 {
                style.list = ListKind::Bullet;
            }
            vec![
                Run::plain(format!("Paragraph {i} opens with plain text, ")),
                Run { text: "then a bold phrase".into(), style: RunStyle { bold: true, ..Default::default() } },
                Run::plain(", and closes with "),
                Run { text: "an italic aside.".into(), style: RunStyle { italic: true, ..Default::default() } },
            ]
        };
        doc.paragraphs.push(Paragraph { style, runs });
    }
    doc
}
