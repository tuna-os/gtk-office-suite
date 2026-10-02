//! Page breaks on screen, in print and in PDF are one set (#1280).
//!
//! Print Layout, Print and Export as PDF all draw one `Typeset`'s pages
//! (`letters/src/doc_tab.rs::typeset_for`), so the breaks it reports are
//! theirs. This checks the other half: the PDF it writes, read back with
//! poppler, has those pages, each holding exactly the text the breaks
//! say. The document is built so that the breaks depend on run sizes and
//! paragraph spacing, with multi-byte text before every break, where a
//! byte offset taken for a character offset would land mid-word.
#![cfg(feature = "render")]

use letters_core::layout::pango::Typeset;
use letters_core::layout::LayoutOptions;
use letters_core::model::{Document, ParaStyle, Paragraph, Run, RunStyle};

fn document() -> Document {
    let mut d = Document::from_plain_text("");
    d.paragraphs = (0..60)
        .map(|i| {
            let big = RunStyle { font_size_hp: Some(if i % 3 == 0 { 40 } else { 24 }), ..Default::default() };
            Paragraph {
                style: ParaStyle { space_before_pt: (i % 4) as f64 * 9.0, space_after_pt: (i % 5) as f64 * 6.0, ..Default::default() },
                runs: vec![
                    Run { text: format!("¶{i} Ünïcödé — 中文テキスト ✨ "), style: big },
                    Run::plain("plain words that wrap across the line more than once in a long enough paragraph of text."),
                ],
            }
        })
        .collect();
    d
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn the_pdf_has_the_typesets_pages_with_the_text_its_breaks_say() {
    let doc = document();
    let text: Vec<char> = doc.to_plain_text().chars().collect();
    let typeset = Typeset::new(doc, LayoutOptions::default());
    let breaks = typeset.page_breaks();
    let pages = typeset.tree().pages.len();
    assert!(pages >= 3, "the document should run to several pages, got {pages}");
    assert_eq!(breaks.len(), pages);
    assert!(breaks.windows(2).all(|w| w[0] < w[1]), "breaks must advance: {breaks:?}");

    let dir = tempfile::tempdir().unwrap();
    let pdf = dir.path().join("doc.pdf");
    typeset.write_pdf(&pdf).expect("write the PDF");
    let run = |args: &[&str]| -> String {
        let out = std::process::Command::new(args[0]).args(&args[1..]).output().unwrap_or_else(|e| {
            panic!("{} is needed to read the PDF back (poppler-utils): {e}", args[0])
        });
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let info = run(&["pdfinfo", pdf.to_str().unwrap()]);
    let pdf_pages: usize = info.lines().find_map(|l| l.strip_prefix("Pages:")).unwrap().trim().parse().unwrap();
    assert_eq!(pdf_pages, pages, "the PDF has a different number of pages");

    for (i, &start) in breaks.iter().enumerate() {
        let end = breaks.get(i + 1).copied().unwrap_or(text.len());
        let want: String = text[start..end].iter().collect();
        let n = (i + 1).to_string();
        // -raw: the content stream's order, which is drawing order; the
        // default reading order splits a line mixing font sizes.
        let got = run(&["pdftotext", "-raw", "-f", &n, "-l", &n, "-enc", "UTF-8", pdf.to_str().unwrap(), "-"]);
        assert_eq!(squash(&got), squash(&want), "page {} differs from the text between its breaks", i + 1);
        // Every page after the first starts after multi-byte text: the
        // break is a character offset into it, not a byte offset.
        if i > 0 {
            assert!(text[..start].iter().any(|c| c.len_utf8() > 1));
        }
    }
}
