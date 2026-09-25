// Engine tests with MonoShaper: 12pt text is 6pt per char and 15pt per
// line, so an A4 page with 1in margins holds 75 chars × 46 lines.

use super::*;
use crate::model::{Alignment, ParaStyle, TableCell};

const LINE: f64 = 15.0;
const LINES_PER_PAGE: usize = 46;

fn opts() -> LayoutOptions {
    LayoutOptions::default()
}

fn lay(doc: &Document) -> RenderTree {
    layout(doc, &opts(), &mut MonoShaper)
}

fn doc_of(n: usize, text: &str) -> Document {
    let mut d = Document::new();
    d.paragraphs = (0..n).map(|_| Paragraph { style: ParaStyle::default(), runs: vec![Run::plain(text)] }).collect();
    d
}

/// Body lines per page.
fn lines_per_page(t: &RenderTree) -> Vec<usize> {
    t.pages.iter().map(|p| p.lines().count()).collect()
}

fn line_items(p: &Page) -> Vec<(usize, usize, f64, f64)> {
    p.items
        .iter()
        .filter_map(|i| match i {
            Item::Line { source: Source::Paragraph(q), line, x_pt, top_pt, .. } => Some((*q, *line, *x_pt, *top_pt)),
            _ => None,
        })
        .collect()
}

#[test]
fn an_empty_document_is_one_page_with_one_line() {
    let t = lay(&Document::new());
    assert_eq!(t.pages.len(), 1);
    assert_eq!(lines_per_page(&t), vec![1], "an empty paragraph still has a line (the caret needs one)");
}

#[test]
fn text_flows_onto_further_pages_at_the_bottom_margin() {
    let t = lay(&doc_of(100, "one line"));
    assert_eq!(lines_per_page(&t), vec![LINES_PER_PAGE, LINES_PER_PAGE, 100 - 2 * LINES_PER_PAGE]);
    // Every page starts at the top margin.
    for page in &t.pages {
        assert_eq!(line_items(page)[0].3, 72.0);
    }
    let last = line_items(&t.pages[0]).last().copied().unwrap();
    assert!(last.3 + LINE <= 841.9 - 72.0, "nothing is drawn into the bottom margin");
}

#[test]
fn long_paragraphs_wrap_at_the_text_width() {
    // 75 chars fit; a 100-char word-wrapped paragraph needs two lines.
    let words = "abcd ".repeat(20);
    let t = lay(&doc_of(1, words.trim_end()));
    let lines: Vec<String> = t.pages[0]
        .lines()
        .map(|i| match i { Item::Line { text, .. } => text.clone(), _ => unreachable!() })
        .collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].chars().count() <= 75);
    assert_eq!(lines.concat(), words.trim_end(), "lines partition the paragraph's text");
}

#[test]
fn a_page_break_starts_a_new_page_but_not_an_empty_first_one() {
    let mut d = doc_of(3, "x");
    d.paragraphs[0].style.page_break_before = true;
    d.paragraphs[2].style.page_break_before = true;
    let t = lay(&d);
    assert_eq!(lines_per_page(&t), vec![2, 1]);
    assert_eq!(t.page_of_paragraph(2), Some(1));
}

#[test]
fn paragraph_spacing_and_line_spacing_move_what_follows() {
    let mut d = doc_of(3, "x");
    d.paragraphs[1].style.space_before_pt = 24.0;
    d.paragraphs[1].style.space_after_pt = 6.0;
    d.paragraphs[1].style.line_spacing = 2.0;
    let tops: Vec<f64> = line_items(&lay(&d).pages[0]).iter().map(|l| l.3).collect();
    assert_eq!(tops, vec![72.0, 72.0 + LINE + 24.0, 72.0 + LINE + 24.0 + 2.0 * LINE + 6.0]);
}

#[test]
fn a_split_paragraph_keeps_two_lines_at_each_end() {
    let long = "abcd ".repeat(75); // 5 lines of 75 chars
    // 44 one-line paragraphs leave room for 2 lines: orphans are satisfied
    // and 3 lines go over.
    let mut d = doc_of(44, "x");
    d.paragraphs.push(Paragraph { style: ParaStyle::default(), runs: vec![Run::plain(long.trim_end())] });
    assert_eq!(lines_per_page(&lay(&d)), vec![46, 3]);

    // Room for one line only: one line would be an orphan, so the whole
    // paragraph moves.
    let mut d = doc_of(45, "x");
    d.paragraphs.push(Paragraph { style: ParaStyle::default(), runs: vec![Run::plain(long.trim_end())] });
    assert_eq!(lines_per_page(&lay(&d)), vec![45, 5]);

    // Room for 4 of 5 lines: one line would be a widow, so 3 + 2.
    let mut d = doc_of(42, "x");
    d.paragraphs.push(Paragraph { style: ParaStyle::default(), runs: vec![Run::plain(long.trim_end())] });
    assert_eq!(lines_per_page(&lay(&d)), vec![45, 2]);
}

#[test]
fn a_heading_is_kept_with_what_follows_it() {
    // 44 lines used; the 12pt×1.6 heading (24pt) fits, the body line
    // after it would not.
    let mut d = doc_of(44, "x");
    d.paragraphs.push(Paragraph {
        style: ParaStyle { heading: Some(1), ..Default::default() },
        runs: vec![Run::plain("Chapter")],
    });
    d.paragraphs.push(Paragraph { style: ParaStyle::default(), runs: vec![Run::plain("body")] });
    let t = lay(&d);
    assert_eq!(t.page_of_paragraph(44), Some(1), "the heading moved with its body");
    assert_eq!(t.page_of_paragraph(45), Some(1));
}

#[test]
fn list_items_hang_their_marker_before_indented_text() {
    let mut d = doc_of(4, "item");
    let kinds = [(ListKind::Bullet, 0), (ListKind::Bullet, 1), (ListKind::Numbered, 0), (ListKind::Numbered, 0)];
    for (p, (k, l)) in d.paragraphs.iter_mut().zip(kinds) {
        p.style.list = k;
        p.style.list_level = l;
    }
    let t = lay(&d);
    let markers: Vec<(String, f64)> = t.pages[0]
        .items
        .iter()
        .filter_map(|i| match i { Item::Marker { text, x_pt, .. } => Some((text.clone(), *x_pt)), _ => None })
        .collect();
    assert_eq!(
        markers,
        vec![("\u{2022}".into(), 72.0), ("\u{2022}".into(), 90.0), ("1.".into(), 72.0), ("2.".into(), 72.0)]
    );
    let xs: Vec<f64> = line_items(&t.pages[0]).iter().map(|l| l.2).collect();
    assert_eq!(xs, vec![90.0, 108.0, 90.0, 90.0], "text sits one hanging indent after its marker");
}

#[test]
fn indents_and_alignment_position_lines() {
    let mut d = doc_of(3, "abcd ".repeat(20).trim_end());
    d.paragraphs[0].style.first_line_indent_pt = 36.0;
    d.paragraphs[1].style.left_indent_pt = 72.0;
    d.paragraphs[2].style.alignment = Alignment::Right;
    let t = lay(&d);
    let l = line_items(&t.pages[0]);
    assert_eq!((l[0].2, l[1].2), (108.0, 72.0), "first-line indent only on the first line");
    assert_eq!((l[2].2, l[3].2), (144.0, 144.0), "left indent on every line");
    let (line1_x, width) = match &t.pages[0].items.iter().filter(|i| matches!(i, Item::Line { .. })).nth(5).unwrap() {
        Item::Line { x_pt, text, .. } => (*x_pt, text.trim_end().chars().count() as f64 * 6.0),
        _ => unreachable!(),
    };
    assert!((line1_x + width - (595.3 - 72.0)).abs() < 1e-6, "right-aligned lines end at the right margin");
}

#[test]
fn a_table_is_a_grid_of_cells_with_rows_as_tall_as_their_tallest_cell() {
    let mut d = doc_of(1, "after");
    let table = d.insert_table_at(0, 2, 3);
    let long = "abcd ".repeat(30);
    for p in &mut d.paragraphs {
        if p.style.table_cell == Some(TableCell { table, row: 0, col: 1 }) {
            p.runs = vec![Run::plain(long.trim_end())];
        } else if p.style.table_cell.is_some() {
            p.runs = vec![Run::plain("c")];
        }
    }
    let t = lay(&d);
    let cells: Vec<(u32, u32, f64, f64, f64, f64)> = t.pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Cell { row, col, x_pt, y_pt, width_pt, height_pt, .. } => Some((*row, *col, *x_pt, *y_pt, *width_pt, *height_pt)),
            _ => None,
        })
        .collect();
    assert_eq!(cells.len(), 6);
    let w = (595.3 - 144.0) / 3.0;
    assert!((cells[1].2 - (72.0 - CELL_PADDING_PT + w)).abs() < 1e-9 && (cells[1].4 - w).abs() < 1e-9, "equal columns");
    assert_eq!(cells[0].2, 72.0 - CELL_PADDING_PT, "cell text, not the rule, aligns with the margin");
    let tall = cells[1].5;
    assert!(tall > LINE * 2.0, "the long cell wraps: {tall}");
    assert_eq!(cells[3].5, LINE + CELL_RULE_PT, "a one-line row is its line plus a rule");
    assert!(cells[..3].iter().all(|c| c.5 == tall), "the whole row takes the tallest cell's height");
    assert_eq!(cells[3].3, 72.0 + tall, "row two starts below row one");
    // The paragraph after the table starts below it.
    let after = t.pages[0].items.iter().find_map(|i| match i {
        Item::Line { text, top_pt, .. } if text == "after" => Some(*top_pt),
        _ => None,
    });
    assert_eq!(after, Some(72.0 + tall + LINE + CELL_RULE_PT), "below both rows");
}

#[test]
fn headers_and_footers_repeat_with_page_numbers() {
    let mut d = doc_of(60, "x");
    d.header = Some("Report".into());
    d.footer = Some("Page {page} of {total}".into());
    let t = lay(&d);
    assert_eq!(t.pages.len(), 2);
    let texts = |p: &Page, src: Source| -> Vec<String> {
        p.items
            .iter()
            .filter_map(|i| match i { Item::Line { source, text, .. } if *source == src => Some(text.clone()), _ => None })
            .collect()
    };
    assert_eq!(texts(&t.pages[1], Source::Header), vec!["Report"]);
    assert_eq!(texts(&t.pages[1], Source::Footer), vec!["Page 2 of 2"]);
    let footer_top = t.pages[0].items.iter().find_map(|i| match i {
        Item::Line { source: Source::Footer, top_pt, height_pt, .. } => Some(top_pt + height_pt),
        _ => None,
    });
    assert!((footer_top.unwrap() - (841.9 - 36.0)).abs() < 1e-9, "footer bottom sits at the footer distance");
}

#[test]
fn columns_fill_left_to_right_before_the_next_page() {
    let mut d = doc_of(LINES_PER_PAGE + 1, "x");
    d.page = Some(PageGeometry { columns: 2, column_gap_pt: 18.0, ..PageGeometry::default() });
    let t = lay(&d);
    assert_eq!(t.pages.len(), 1);
    let l = line_items(&t.pages[0]);
    let col_w = (595.3 - 144.0 - 18.0) / 2.0;
    assert_eq!(l[LINES_PER_PAGE].2, 72.0 + col_w + 18.0, "the overflow line opens column two");
    assert_eq!(l[LINES_PER_PAGE].3, 72.0);
}

#[test]
fn the_render_tree_is_serialisable() {
    let t = lay(&doc_of(2, "hello"));
    let json = serde_json::to_string(&t).unwrap();
    let back: RenderTree = serde_json::from_str(&json).unwrap();
    assert_eq!(back.pages.len(), t.pages.len());
    assert_eq!(back.pages[0].items.len(), t.pages[0].items.len());
    // serde_json's default float parsing may differ in the last bit.
    assert!(matches!(&back.pages[0].items[1], Item::Line { text, top_pt, .. } if text == "hello" && *top_pt == 87.0));
}

#[test]
fn an_inline_image_takes_its_extent_and_sits_on_the_baseline() {
    let mut d = doc_of(2, "caption");
    d.paragraphs[1].runs = vec![Run {
        text: "alt text".into(),
        style: crate::model::RunStyle {
            image: Some("/nonexistent.png".into()),
            image_extent_emu: Some((2 * 914_400, 914_400)),
            ..Default::default()
        },
    }];
    let t = lay(&d);
    let images: Vec<(f64, f64, f64, f64)> = t.pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Image { x_pt, y_pt, width_pt, height_pt, .. } => Some((*x_pt, *y_pt, *width_pt, *height_pt)),
            _ => None,
        })
        .collect();
    assert_eq!(images, vec![(72.0, 72.0 + LINE, 144.0, 72.0)], "2in x 1in at the margin, under the caption");
    let line = t.pages[0].lines().nth(1).unwrap();
    let Item::Line { text, height_pt, .. } = line else { unreachable!() };
    assert_eq!(text, "\u{FFFC}", "the image is one object char, not its alt text");
    assert!(*height_pt >= 72.0, "the line grows to hold the image: {height_pt}");
}

#[test]
fn an_image_wider_than_the_text_box_is_scaled_to_fit() {
    let run = Run {
        text: String::new(),
        style: crate::model::RunStyle {
            image: Some("x".into()),
            image_extent_emu: Some((12700 * 900, 12700 * 300)),
            ..Default::default()
        },
    };
    assert_eq!(image_size_pt(&run, 450.0), (450.0, 150.0));
}

#[test]
fn the_document_base_font_sets_body_text_size() {
    let mut d = doc_of(1, "x");
    d.base_font = crate::model::BaseFont { family: Some("Carlito".into()), size_hp: Some(22) };
    let t = lay(&d);
    let Item::Line { height_pt, .. } = t.pages[0].lines().next().unwrap() else { unreachable!() };
    assert_eq!(*height_pt, 11.0 * 1.25, "11pt body text from the document, not the 12pt default");
    assert_eq!(LayoutOptions::default().for_document(&d).font_family, "Carlito");
}

#[test]
fn the_larger_of_space_after_and_space_before_separates_paragraphs() {
    let mut d = doc_of(3, "x");
    for p in &mut d.paragraphs {
        p.style.space_after_pt = 10.0;
    }
    d.paragraphs[1].style.space_before_pt = 24.0;
    d.paragraphs[2].style.space_before_pt = 4.0;
    let tops: Vec<f64> = line_items(&lay(&d).pages[0]).iter().map(|l| l.3).collect();
    assert_eq!(tops, vec![72.0, 72.0 + LINE + 24.0, 72.0 + 2.0 * LINE + 24.0 + 10.0]);
}

/// A shaper that counts the paragraphs it shapes.
struct Counting(usize);

impl Shaper for Counting {
    fn shape(&mut self, req: &ShapeRequest<'_>) -> Vec<LineBox> {
        self.0 += 1;
        MonoShaper.shape(req)
    }
}

/// Incremental relayout: after typing in one paragraph of a long document,
/// only that paragraph is shaped again, and the result is the full layout.
#[test]
fn relayout_reshapes_only_the_edited_paragraph() {
    let mut d = doc_of(200, "x");
    for (i, p) in d.paragraphs.iter_mut().enumerate() {
        p.runs = vec![Run::plain(format!("paragraph {i} of body text that wraps once or twice at this width"))];
    }
    let opts = opts();
    let mut cache = ShapeCache::default();
    let mut counting = Counting(0);
    relayout(&d, &opts, &mut counting, &mut cache);
    cache.prune();
    let first = counting.0;
    assert!(first >= 200, "everything is shaped once: {first}");

    let op = crate::edit::typing(&d, 5, "x").unwrap();
    crate::edit::apply(&mut d, &op).unwrap();
    let tree = relayout(&d, &opts, &mut counting, &mut cache);
    assert_eq!(cache.misses, 1, "only the edited paragraph is shaped again");
    assert_eq!(counting.0, first + 1);
    cache.prune();
    assert_eq!(tree, lay(&d), "the incremental layout is the full layout");
}

/// A paragraph ending in a reference to footnote `n`.
fn with_note(text: &str, n: usize) -> Paragraph {
    Paragraph {
        style: ParaStyle::default(),
        runs: vec![Run::plain(text), Run { text: String::new(), style: crate::model::RunStyle { footnote: Some(n), ..Default::default() } }],
    }
}

#[test]
fn footnotes_sit_at_the_foot_of_the_page_that_references_them() {
    let mut d = doc_of(2, "body");
    d.paragraphs.insert(1, with_note("noted", 0));
    d.footnotes = vec!["The note.".into()];
    let t = lay(&d);
    let page = &t.pages[0];
    // The reference is drawn as a superscript number after "noted".
    let refs: Vec<f64> = page.items.iter().filter_map(|i| match i { Item::NoteRef { x_pt, .. } => Some(*x_pt), _ => None }).collect();
    assert_eq!(refs, [72.0 + 5.0 * 6.0]);
    // The note: its number at body size (12 pt: 15 pt tall) and 10 pt
    // text on one line ending at the bottom margin, under a rule a quarter
    // of the text width.
    let foot = 841.9 - 72.0;
    let note: Vec<(f64, f64)> = page
        .items
        .iter()
        .filter_map(|i| match i { Item::Line { source: Source::Footnote(0), top_pt, height_pt, .. } => Some((*top_pt, *height_pt)), _ => None })
        .collect();
    assert_eq!(note.len(), 1);
    assert!((note[0].1 - 15.0).abs() < 1e-9 && (note[0].0 + note[0].1 - foot).abs() < 1e-6, "{note:?}");
    let rule = page.items.iter().find_map(|i| match i { Item::Rule { y_pt, width_pt, .. } => Some((*y_pt, *width_pt)), _ => None }).unwrap();
    assert!((rule.0 - (note[0].0 - NOTE_RULE_GAP_PT)).abs() < 1e-9);
    assert!((rule.1 - (595.3 - 144.0) * NOTE_RULE_FRACTION).abs() < 0.1);
}

#[test]
fn a_line_moves_to_the_next_page_with_its_note_when_they_do_not_both_fit() {
    // The page holds 46 body lines. 45 lines, then a line whose note (and
    // the separator) no longer fits below it: that line and its note go
    // to page 2 together.
    let mut d = doc_of(45, "x");
    d.paragraphs.push(with_note("noted", 0));
    d.footnotes = vec!["The note.".into()];
    let t = lay(&d);
    assert_eq!(t.page_of_paragraph(45), Some(1), "the referencing line moved on");
    let on = |p: usize| t.pages[p].items.iter().any(|i| matches!(i, Item::Line { source: Source::Footnote(0), .. }));
    assert!(!on(0) && on(1), "the note is on the reference's page");
    // Without the note, the line would have fitted on page 1.
    d.footnotes.clear();
    d.paragraphs[45] = doc_of(1, "noted").paragraphs.remove(0);
    assert_eq!(lay(&d).page_of_paragraph(45), Some(0));
}

#[test]
fn a_smart_chip_is_one_object_drawn_as_a_pill() {
    let mut d = doc_of(1, "Due ");
    let date = crate::chips::date_chip(crate::chips::NaiveDate::from_ymd_opt(2026, 10, 3).unwrap());
    d.paragraphs[0].runs.push(date);
    d.paragraphs[0].runs.push(Run::plain(" ok"));
    assert_eq!(layout_text(&d.paragraphs[0].runs), "Due \u{FFFC} ok");
    let t = lay(&d);
    let chips: Vec<(usize, usize, f64)> = t.pages[0]
        .items
        .iter()
        .filter_map(|i| match i { Item::Chip { para, ch, x_pt, .. } => Some((*para, *ch, *x_pt)), _ => None })
        .collect();
    // After "Due " (4 chars × 6 pt) from the left margin.
    assert_eq!(chips, [(0, 4, 72.0 + 24.0)]);
    // The pill holds the label open: " ok" starts after it.
    let label_w = "3 Oct 2026".len() as f64 * 6.0 + 2.0 * (CHIP_PAD_PT + CHIP_GAP_PT);
    let line = t.pages[0].lines().next().unwrap();
    if let Item::Line { text, .. } = line {
        assert_eq!(text, "Due \u{FFFC} ok");
    }
    let mut shaper = MonoShaper;
    let o = opts();
    let req = paragraph_request(&d.paragraphs[0], 400.0, &o);
    let lb = &shaper.shape(&req)[0];
    assert!((lb.width_pt - (7.0 * 6.0 + label_w)).abs() < 1e-9, "{}", lb.width_pt);
}

#[test]
fn title_subtitle_and_quotes_have_their_own_looks() {
    let mut d = doc_of(5, "text");
    d.paragraphs[0].style.named_style = Some("Title".into());
    d.paragraphs[1].style.named_style = Some("Subtitle".into());
    d.paragraphs[2].style.block_quote = true;
    // A heading level wins over a named style.
    d.paragraphs[3].style = ParaStyle { heading: Some(2), named_style: Some("Title".into()), ..Default::default() };
    let t = lay(&d);
    let lines = line_items(&t.pages[0]);
    let tops: Vec<f64> = lines.iter().map(|l| l.3).collect();
    let heights: Vec<f64> = tops.windows(2).map(|w| w[1] - w[0]).collect();
    // MonoShaper lines are 1.25 × size tall.
    assert!((heights[0] - 12.0 * 26.0 / 11.0 * 1.25).abs() < 1e-6, "Title: {heights:?}");
    assert!((heights[1] - 12.0 * 15.0 / 11.0 * 1.25).abs() < 1e-6, "Subtitle: {heights:?}");
    assert!((heights[2] - LINE).abs() < 1e-6, "a quote is body-sized");
    assert!((heights[3] - 12.0 * heading_scale(2) * 1.25).abs() < 1e-6, "heading 2: {heights:?}");
    // The quote is indented; the rest start at the margin.
    let xs: Vec<f64> = lines.iter().map(|l| l.2).collect();
    assert_eq!(xs[2] - xs[4], QUOTE_INDENT_PT);
    assert_eq!(xs[0], xs[4]);
    assert_eq!(paragraph_look(&d.paragraphs[4].style), Look::Body);
}
