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
