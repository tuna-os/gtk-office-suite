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
use letters_core::layout::{Item, LayoutOptions, Page, Source};
use letters_core::model::{Document, ListKind, ParaStyle, Paragraph, Run, RunStyle, VertAlign};

fn document() -> Document {
    let mut d = Document::from_plain_text("");
    d.paragraphs = (0..60)
        .map(|i| {
            let big = RunStyle { font_size_hp: Some(if i % 3 == 0 { 40 } else { 24 }), ..Default::default() };
            Paragraph {
                style: ParaStyle { space_before_pt: (i % 4) as f64 * 9.0, space_after_pt: (i % 5) as f64 * 6.0, ..Default::default() },
                runs: vec![
                    // Multi-byte, but only in scripts every runner's fonts
                    // cover: a glyph with no font is drawn as a hex box,
                    // which pdftotext reads back as its hex digits.
                    Run { text: format!("¶{i} Ünïcödé — "), style: big },
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

/// A family nothing provides, for missing-font reporting alongside the
/// agreement run: the test fails only where it requires this face.
const MISSING_FAMILY: &str = "No Such Family XYZ";

/// A document heavy where agreement matters: styled body text with
/// super/subscripts, nested bulleted lists, two multi-row tables that split
/// across pages, footnotes, inline image frames without files (the loader
/// finds nothing and draws the frame), and one paragraph in a missing face.
///
/// Bullets only, no numbered lists: a bullet draws as a disc (no PDF text)
/// while a numbered marker draws its text, so with bullets the tree's line
/// text and the PDF's content stream agree exactly.
fn heavy_document() -> Document {
    let mut d = Document::from_plain_text("");
    d.paragraphs.clear();
    d.paragraphs.push(Paragraph {
        style: ParaStyle { heading: Some(1), ..Default::default() },
        runs: vec![Run::plain("Heavy pages")],
    });
    for i in 0..24 {
        d.paragraphs.push(Paragraph {
            style: ParaStyle {
                space_before_pt: (i % 3) as f64 * 6.0,
                space_after_pt: (i % 2) as f64 * 6.0,
                ..Default::default()
            },
            runs: vec![
                Run {
                    text: format!("¶{i} Section — "),
                    style: RunStyle { font_size_hp: if i % 4 == 0 { Some(28) } else { None }, bold: i % 5 == 0, ..Default::default() },
                },
                Run::plain("plain words that wrap across the line more than once in a long enough paragraph of text. "),
                Run { text: "2".into(), style: RunStyle { vert_align: Some(VertAlign::Superscript), ..Default::default() } },
                Run::plain(" and "),
                Run { text: "2".into(), style: RunStyle { vert_align: Some(VertAlign::Subscript), ..Default::default() } },
            ],
        });
    }
    for i in 0..12 {
        d.paragraphs.push(Paragraph {
            style: ParaStyle { list: ListKind::Bullet, list_level: (i % 3) as u8, ..Default::default() },
            runs: vec![Run::plain(format!("Level {} item {i}", (i % 3) + 1))],
        });
    }
    for _ in 0..2 {
        let id = d.insert_table_at(d.paragraphs.len(), 8, 3);
        for p in d.paragraphs.iter_mut() {
            if let Some(cell) = p.style.table_cell {
                if cell.table == id {
                    p.runs = vec![Run::plain(format!("R{}C{} cell text that wraps a little", cell.row + 1, cell.col + 1))];
                }
            }
        }
    }
    d.footnotes = vec![
        "First footnote: the note text drawn at the foot of its page.".into(),
        "Second footnote on the pages its references paginate to.".into(),
    ];
    for n in 0..6 {
        d.paragraphs.push(Paragraph {
            style: ParaStyle::default(),
            runs: vec![
                Run::plain(format!("Body paragraph {n} with its note at the end.")),
                Run { text: String::new(), style: RunStyle { footnote: Some(n % 2), ..Default::default() } },
            ],
        });
    }
    for i in 0..3 {
        d.paragraphs.push(Paragraph {
            style: ParaStyle::default(),
            runs: vec![
                Run::plain(format!("Figure {i}: ")),
                Run {
                    // One space: it vanishes in the squash, and the layout's
                    // object char is filtered from the tree text below.
                    text: " ".into(),
                    style: RunStyle {
                        image: Some(format!("missing-{i}.png")),
                        image_extent_emu: Some((914400 * 3, 914400 * 2)),
                        ..Default::default()
                    },
                },
            ],
        });
    }
    d.paragraphs.push(Paragraph {
        style: ParaStyle::default(),
        runs: vec![Run {
            text: "A paragraph in a missing face.".into(),
            style: RunStyle { font_family: Some(MISSING_FAMILY.into()), ..Default::default() },
        }],
    });
    d
}

/// The text `draw_page` puts into the PDF's content stream for `page`, in
/// order: body and footnote line text, and each footnote reference's number
/// where it is drawn. Image frames and bullet discs put no text in.
fn drawn_text(doc: &Document, page: &Page) -> String {
    fn note_number(doc: &Document, para: usize, ch: usize) -> Option<usize> {
        let mut at = 0;
        for r in &doc.paragraphs.get(para)?.runs {
            let len = if letters_core::layout::is_object(r) { 1 } else { r.text.chars().count() };
            if (at..at + len).contains(&ch) {
                return r.style.footnote;
            }
            at += len;
        }
        None
    }
    let mut out = String::new();
    for item in &page.items {
        match item {
            Item::Line { source: Source::Paragraph(_), text, .. } => {
                out.push_str(&text.chars().filter(|c| *c != letters_core::layout::OBJECT).collect::<String>());
            }
            Item::Line { source: Source::Footnote(_), text, .. } => out.push_str(text),
            Item::NoteRef { para, ch, .. } => {
                if let Some(n) = note_number(doc, *para, *ch) {
                    out.push_str(&(n + 1).to_string());
                }
            }
            _ => {}
        }
    }
    out
}

/// Every laid-out item sits on its page: finite coordinates, positive line
/// and box sizes, lines and boxes inside the page they are drawn on.
fn assert_bounds(typeset: &Typeset) {
    for page in &typeset.tree().pages {
        assert!(page.width_pt > 0.0 && page.height_pt > 0.0);
        for item in &page.items {
            match item {
                Item::Line { x_pt, top_pt, baseline_pt, height_pt, box_width_pt, .. } => {
                    for v in [x_pt, top_pt, baseline_pt, height_pt, box_width_pt] {
                        assert!(v.is_finite(), "a line with a non-finite box: {item:?}");
                    }
                    assert!(*height_pt > 0.0 && *box_width_pt > 0.0, "a line with no size: {item:?}");
                    assert!(*x_pt >= 0.0 && *x_pt <= page.width_pt, "a line off its page: {item:?}");
                    assert!(*top_pt >= 0.0 && *top_pt <= page.height_pt, "a line off its page: {item:?}");
                }
                Item::Image { x_pt, y_pt, width_pt, height_pt, .. }
                | Item::Cell { x_pt, y_pt, width_pt, height_pt, .. }
                | Item::ParaBox { x_pt, y_pt, width_pt, height_pt, .. }
                | Item::PageBorder { x_pt, y_pt, width_pt, height_pt, .. }
                | Item::TextBox { x_pt, y_pt, width_pt, height_pt, .. } => {
                    for v in [x_pt, y_pt, width_pt, height_pt] {
                        assert!(v.is_finite(), "a box with a non-finite edge: {item:?}");
                    }
                    assert!(*width_pt > 0.0 && *height_pt > 0.0, "a box with no size: {item:?}");
                    assert!(*x_pt >= 0.0 && *x_pt + *width_pt <= page.width_pt + 1.0, "a box off its page: {item:?}");
                    assert!(*y_pt >= 0.0 && *y_pt + *height_pt <= page.height_pt + 1.0, "a box off its page: {item:?}");
                }
                Item::Marker { x_pt, baseline_pt, .. }
                | Item::Chip { x_pt, baseline_pt, .. }
                | Item::NoteRef { x_pt, baseline_pt, .. } => {
                    assert!(x_pt.is_finite() && baseline_pt.is_finite(), "a mark off the numbers: {item:?}");
                }
                Item::Rule { x_pt, y_pt, width_pt } => {
                    assert!(x_pt.is_finite() && y_pt.is_finite() && width_pt.is_finite(), "a rule off the numbers: {item:?}");
                }
            }
        }
    }
}

/// The PDF at `pdf` is `typeset`: as many pages, each holding exactly the
/// text the tree draws on it — checked on every page, not just the count.
fn assert_pdf_matches_typeset(typeset: &Typeset, pdf: &std::path::Path) {
    let run = |args: &[&str]| -> String {
        let out = std::process::Command::new(args[0]).args(&args[1..]).output().unwrap_or_else(|e| {
            panic!("{} is needed to read the PDF back (poppler-utils): {e}", args[0])
        });
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let pages = typeset.tree().pages.len();
    assert!(pages >= 3, "the heavy document should run to several pages, got {pages}");
    let info = run(&["pdfinfo", pdf.to_str().unwrap()]);
    let pdf_pages: usize = info.lines().find_map(|l| l.strip_prefix("Pages:")).unwrap().trim().parse().unwrap();
    assert_eq!(pdf_pages, pages, "the PDF has a different number of pages");
    let bytes = std::fs::read(pdf).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    let page_objects = text.matches("/Type /Page").count() - text.matches("/Type /Pages").count();
    assert_eq!(page_objects, pages, "one PDF page per laid-out page");
    assert_bounds(typeset);
    for (i, page) in typeset.tree().pages.iter().enumerate() {
        let n = (i + 1).to_string();
        // -raw: the content stream's order, which is drawing order; the
        // default reading order splits a line mixing font sizes.
        let got = run(&["pdftotext", "-raw", "-f", &n, "-l", &n, "-enc", "UTF-8", pdf.to_str().unwrap(), "-"]);
        assert_eq!(squash(&got), squash(&drawn_text(typeset.document(), page)), "page {} differs from what the tree draws on it", i + 1);
    }
}

#[test]
fn pdf_matches_typeset_on_table_heavy_pages() {
    let mut d = heavy_document();
    let mut typeset = Typeset::new(d.clone(), LayoutOptions::default());
    let breaks = typeset.page_breaks();
    assert!(breaks.len() >= 3, "the heavy document should run to several pages");
    assert!(breaks.windows(2).all(|w| w[0] < w[1]), "breaks must advance: {breaks:?}");

    let dir = tempfile::tempdir().unwrap();
    let pdf = dir.path().join("heavy.pdf");
    typeset.write_pdf(&pdf).expect("write the PDF");
    assert_pdf_matches_typeset(&typeset, &pdf);

    // An edit past the tables: the incremental layout must agree again on
    // every page, with at least as many of them.
    for i in 0..10 {
        d.paragraphs.push(Paragraph {
            style: ParaStyle::default(),
            runs: vec![Run::plain(format!("Appended after the edit {i}, with enough words to wrap across the line more than once in a long paragraph."))],
        });
    }
    typeset.update(d, LayoutOptions::default());
    assert!(typeset.page_breaks().len() >= breaks.len(), "the edit must not lose pages");
    let pdf = dir.path().join("heavy-edited.pdf");
    typeset.write_pdf(&pdf).expect("write the edited PDF");
    assert_pdf_matches_typeset(&typeset, &pdf);

    // Missing-font reporting alongside the agreement run.
    assert!(
        typeset.missing_fonts_by_page().iter().any(|m| m.contains(&MISSING_FAMILY.to_string())),
        "the missing face is reported"
    );
}

#[test]
fn zoom_and_scale_do_not_move_breaks() {
    let d = heavy_document();
    let opts = LayoutOptions::default();
    let fresh = Typeset::new(d.clone(), opts.clone());
    let breaks = fresh.page_breaks();
    assert!(breaks.len() >= 3, "the heavy document should run to several pages");
    assert!(breaks.windows(2).all(|w| w[0] < w[1]), "breaks must advance: {breaks:?}");
    // The incremental path with identical inputs breaks identically.
    let mut relaid = Typeset::new(d.clone(), opts.clone());
    relaid.update(d, opts.clone());
    assert_eq!(relaid.page_breaks(), breaks, "re-layout must not move a break");
    assert_eq!(relaid.tree(), fresh.tree());
    // Zoom only scales the display (100% is 96/72 px per point): the stack
    // at 25%–400% holds the same pages, and each page's middle is still
    // that page.
    for pct in [25.0, 50.0, 100.0, 200.0, 400.0] {
        let stack = fresh.tree().page_stack(96.0 / 72.0 * pct / 100.0, 24.0);
        assert_eq!(stack.len(), breaks.len(), "page count at {pct}% zoom");
        for i in 0..stack.len() {
            let mid = (stack.top(i) + stack.top(i + 1)) / 2.0;
            assert_eq!(stack.nearest(mid), Some(i), "page {} at {pct}% zoom", i + 1);
        }
        assert_eq!(stack.visible(0.0, stack.height()), 0..stack.len(), "every page visible at {pct}% zoom");
    }
}
