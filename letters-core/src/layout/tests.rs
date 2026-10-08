// Engine tests with MonoShaper: 12pt text is 6pt per char and 15pt per
// line, so an A4 page with 1in margins holds 75 chars × 46 lines.

use super::*;
use crate::model::{Alignment, CellFill, ParaStyle, RowHeight, TableCell};

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

/// A table is drawn with the file's column widths: a narrow number column
/// beside a wide text one, as Word and LibreOffice draw an agenda. Widths
/// wider than the column box are scaled to fit, and a table whose column
/// count changed since it was read falls back to equal columns.
#[test]
fn a_table_takes_the_files_column_widths() {
    let cells_x_w = |d: &Document| -> Vec<(f64, f64)> {
        lay(d).pages[0]
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Cell { row: 0, x_pt, width_pt, .. } => Some((*x_pt, *width_pt)),
                _ => None,
            })
            .collect()
    };
    let mut d = doc_of(1, "after");
    let table = d.insert_table_at(0, 1, 2);
    d.table_columns.insert(table, vec![36.0, 400.0]);
    let x0 = 72.0 - CELL_PADDING_PT;
    assert_eq!(cells_x_w(&d), [(x0, 36.0), (x0 + 36.0, 400.0)]);

    let box_w = 595.3 - 144.0;
    d.table_columns.insert(table, vec![box_w, box_w]);
    let got = cells_x_w(&d);
    assert!((got[0].1 - box_w / 2.0).abs() < 1e-9 && (got[1].0 - (x0 + box_w / 2.0)).abs() < 1e-9, "scaled to fit: {got:?}");

    d.table_columns.insert(table, vec![36.0, 200.0, 100.0]);
    let got = cells_x_w(&d);
    assert!(got.iter().all(|(_, w)| (w - box_w / 2.0).abs() < 1e-9), "three widths for two columns: equal columns, {got:?}");
}

/// A row with a height from the file is at least that tall (Word's
/// `atLeast`), or exactly that tall (`exact`) whatever it holds; rows
/// without one size to their content.
#[test]
fn a_table_row_takes_the_files_height() {
    let heights = |d: &Document| -> Vec<f64> {
        lay(d).pages[0]
            .items
            .iter()
            .filter_map(|i| match i { Item::Cell { col: 0, height_pt, .. } => Some(*height_pt), _ => None })
            .collect()
    };
    let mut d = doc_of(1, "after");
    let table = d.insert_table_at(0, 3, 1);
    let one_line = LINE + CELL_RULE_PT;
    d.table_rows.insert(table, vec![Some(RowHeight { pt: 40.0, exact: false }), None, Some(RowHeight { pt: 5.0, exact: true })]);
    assert_eq!(heights(&d), [40.0, one_line, 5.0]);
    // A minimum below the content's height leaves the row its content's.
    d.table_rows.insert(table, vec![Some(RowHeight { pt: 5.0, exact: false })]);
    assert_eq!(heights(&d), [one_line; 3]);
    // Inserting and deleting rows moves the heights with their rows.
    d.table_rows.insert(table, vec![Some(RowHeight { pt: 40.0, exact: false }), None, Some(RowHeight { pt: 30.0, exact: false })]);
    assert!(d.insert_table_rows(table, 1, 1));
    assert_eq!(heights(&d), [40.0, one_line, one_line, 30.0]);
    assert!(d.delete_table_rows(table, 0, 1));
    assert_eq!(heights(&d), [one_line, one_line, 30.0]);
    // A minimum taller than the space left splits the row at the foot of
    // the page, as Word and LibreOffice do, rather than moving it on.
    let foot = 841.9 - 72.0;
    d.table_rows.insert(table, vec![None, Some(RowHeight { pt: 2000.0, exact: false })]);
    let t = lay(&d);
    let rows: Vec<(f64, f64)> = t.pages[0].items.iter().filter_map(|i| match i { Item::Cell { col: 0, y_pt, height_pt, .. } => Some((*y_pt, *height_pt)), _ => None }).collect();
    assert_eq!(rows.len(), 2, "the tall row stays on the first page: {rows:?}");
    assert!((rows[1].0 + rows[1].1 - foot).abs() < 1e-6, "{rows:?}");
}

/// A row taller than the space left splits at the foot of the page, as
/// Word and LibreOffice split it: the lines that fit stay, the rest
/// continue at the top of the next page in the same cell, and nothing is
/// drawn into the bottom margin. Moved whole, a tall answer box left a
/// page of blank space, and one taller than a page ran off it.
#[test]
fn a_tall_row_splits_across_pages() {
    let mut d = doc_of(1, "after");
    let table = d.insert_table_at(0, 2, 1);
    for p in d.paragraphs.iter_mut() {
        if p.style.table_cell == Some(TableCell { table, row: 0, col: 0 }) {
            p.runs = vec![Run::plain("first")];
        }
    }
    let at = d.paragraphs.iter().position(|p| p.style.table_cell == Some(TableCell { table, row: 1, col: 0 })).unwrap();
    let lines: Vec<Paragraph> = (0..80).map(|n| Paragraph {
        style: ParaStyle { table_cell: Some(TableCell { table, row: 1, col: 0 }), ..Default::default() },
        runs: vec![Run::plain(format!("line {n}"))],
    }).collect();
    d.paragraphs.splice(at..=at, lines);
    let t = lay(&d);
    let foot = 841.9 - 72.0;
    let cells = |page: usize| -> Vec<(u32, f64, f64)> {
        t.pages[page].items.iter().filter_map(|i| match i { Item::Cell { row, y_pt, height_pt, .. } => Some((*row, *y_pt, *height_pt)), _ => None }).collect()
    };
    let first = cells(0);
    assert_eq!(first.iter().map(|c| c.0).collect::<Vec<_>>(), [0, 1], "the tall row starts on the first page: {first:?}");
    assert!((first[1].1 + first[1].2 - foot).abs() < 1e-6, "its first piece runs to the foot: {first:?}");
    let second = cells(1);
    assert_eq!(second.first().map(|c| (c.0, c.1)), Some((1, 72.0)), "and continues at the top of the next: {second:?}");
    let texts = |page: usize| -> Vec<String> {
        t.pages[page].items.iter().filter_map(|i| match i { Item::Line { text, top_pt, height_pt, .. } => { assert!(top_pt + height_pt <= foot + 1e-6, "{text} in the margin"); Some(text.clone()) } _ => None }).collect()
    };
    let (a, b) = (texts(0), texts(1));
    let n = a.iter().filter(|l| l.starts_with("line ")).count();
    assert!(n > 10 && n < 80, "{n} lines on the first page");
    assert_eq!(b.first().map(String::as_str), Some(format!("line {n}").as_str()), "the next line, not a repeat or a gap");
}

/// A shaded cell carries its colour to the page (a form's grey header
/// cells), and the shading moves with its cell when rows and columns are
/// inserted or deleted.
#[test]
fn a_shaded_cell_carries_its_fill() {
    let fills = |d: &Document| -> Vec<(u32, u32, Option<String>)> {
        lay(d).pages[0]
            .items
            .iter()
            .filter_map(|i| match i { Item::Cell { row, col, fill, .. } => Some((*row, *col, fill.clone())), _ => None })
            .collect()
    };
    let mut d = doc_of(1, "after");
    let table = d.insert_table_at(0, 2, 2);
    d.table_fills.insert(table, vec![CellFill { row: 0, col: 1, color: "D9D9D9".into() }]);
    let grey = Some("D9D9D9".to_string());
    assert_eq!(fills(&d), [(0, 0, None), (0, 1, grey.clone()), (1, 0, None), (1, 1, None)]);
    assert!(d.insert_table_rows(table, 0, 1));
    assert!(d.insert_table_cols(table, 0, 1));
    assert_eq!(d.table_fills[&table], [CellFill { row: 1, col: 2, color: "D9D9D9".into() }]);
    assert!(d.delete_table_cols(table, 2, 1));
    assert!(d.table_fills[&table].is_empty(), "deleting its column deletes its shading");
}

/// A letterhead's logo is drawn at the header's top on every page, its own
/// empty line taken by the picture's row, and a header taller than the
/// room above the margin pushes the body down rather than under it.
#[test]
fn a_header_picture_is_drawn_and_pushes_the_body_down() {
    let mut d = doc_of(60, "x");
    d.header = Some("\nOFSI".into());
    d.header_pictures = vec![Run {
        text: "logo".into(),
        style: crate::model::RunStyle { image: Some("/nonexistent.png".into()), image_extent_emu: Some((914_400, 914_400)), ..Default::default() },
    }];
    let t = lay(&d);
    let opts = LayoutOptions::default();
    for page in &t.pages {
        let logo: Vec<(f64, f64)> = page.items.iter().filter_map(|i| match i { Item::Image { x_pt, y_pt, .. } => Some((*x_pt, *y_pt)), _ => None }).collect();
        assert_eq!(logo, [(72.0, opts.header_distance_pt)], "page {}", page.index);
    }
    let header: Vec<f64> = t.pages[0].items.iter().filter_map(|i| match i { Item::Line { source: Source::Header, top_pt, .. } => Some(*top_pt), _ => None }).collect();
    assert_eq!(header, [opts.header_distance_pt + 72.0], "the text under the logo, its empty first line taken by it");
    let body_top = t.pages[0].lines().find_map(|l| match l { Item::Line { source: Source::Paragraph(_), top_pt, .. } => Some(*top_pt), _ => None }).unwrap();
    assert_eq!(body_top, opts.header_distance_pt + 72.0 + LINE, "the body starts under the header, not at the 72pt margin");
}

/// A header at the right ends at the right margin, a footer centred sits
/// mid-page.
#[test]
fn an_aligned_header_and_footer_sit_where_the_document_puts_them() {
    let mut d = doc_of(1, "x");
    d.header = Some("FINANCE BILL".into());
    d.header_alignment = Alignment::Right;
    d.footer = Some("OFFICIAL".into());
    d.footer_alignment = Alignment::Center;
    let t = lay(&d);
    let page = &t.pages[0];
    let line = |src: Source| page.items.iter().find_map(|i| match i { Item::Line { source, x_pt, text, .. } if *source == src => Some((*x_pt, text.chars().count())), _ => None }).unwrap();
    let right = page.width_pt - 72.0;
    let (hx, hn) = line(Source::Header);
    assert!((hx + hn as f64 * 6.0 - right).abs() < 1e-6, "ends at the right margin: {hx}");
    let (fx, fn_) = line(Source::Footer);
    assert!((fx + fn_ as f64 * 3.0 - page.width_pt / 2.0).abs() < 1e-6, "centred: {fx}");
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

/// A floating image takes no room in its line and is drawn where its
/// anchor puts it: right-aligned in the margins, 36pt above its paragraph.
#[test]
fn a_floating_image_is_placed_by_its_anchor_not_in_the_line() {
    use crate::model::{AnchorAlign, AnchorFrame, ImageAnchor};
    let mut d = doc_of(2, "text");
    d.paragraphs[1].runs.push(Run {
        text: "logo".into(),
        style: crate::model::RunStyle {
            image: Some("/nonexistent.png".into()),
            image_extent_emu: Some((914_400, 914_400)),
            image_anchor: Some(ImageAnchor {
                h_from: AnchorFrame::Margin,
                h_align: Some(AnchorAlign::End),
                v_from: AnchorFrame::Text,
                y_emu: -36 * 12_700,
                ..Default::default()
            }),
            ..Default::default()
        },
    });
    let t = lay(&d);
    let images: Vec<(f64, f64, f64)> = t.pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Image { x_pt, y_pt, width_pt, .. } => Some((*x_pt, *y_pt, *width_pt)),
            _ => None,
        })
        .collect();
    let right = t.pages[0].width_pt - 72.0;
    assert_eq!(images, vec![(right - 72.0, 72.0 + LINE - 36.0, 72.0)], "1in square at the right margin, 36pt above the second line");
    let heights: Vec<f64> = t.pages[0].lines().map(|l| match l { Item::Line { height_pt, .. } => *height_pt, _ => 0.0 }).collect();
    assert_eq!(heights[0], heights[1], "the line does not grow to hold it");
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

/// Footnote lines on page `p`: (note, line index).
fn note_lines_on(t: &RenderTree, p: usize) -> Vec<(usize, usize)> {
    t.pages[p]
        .items
        .iter()
        .filter_map(|i| match i { Item::Line { source: Source::Footnote(n), line, .. } => Some((*n, *line)), _ => None })
        .collect()
}

#[test]
fn a_footnote_too_long_for_its_page_continues_on_the_next() {
    // 40 lines, then a line referencing an 8-line note (its first line
    // 15 pt, holding the number at body size; the rest 12.5 pt): below the
    // reference there is 76.9 pt after the separator, room for 5 of them
    // (65 pt; 6 would take 77.5). 5 stay with the reference and 3 continue
    // on page 2, at the foot and ahead of anything else there.
    let mut d = doc_of(40, "x");
    d.paragraphs.push(with_note("noted", 0));
    d.paragraphs.extend(doc_of(3, "after").paragraphs);
    d.footnotes = vec!["word ".repeat(8 * 90 / 5 - 10).trim().to_string()];
    let t = lay(&d);
    assert_eq!(t.page_of_paragraph(40), Some(0), "the reference stays on page 1");
    let one = note_lines_on(&t, 0);
    let two = note_lines_on(&t, 1);
    assert_eq!(one.len() + two.len(), 8, "every line of the note is drawn once: {one:?} {two:?}");
    assert_eq!(one, (0..5).map(|k| (0, k)).collect::<Vec<_>>());
    assert_eq!(two, [(0, 5), (0, 6), (0, 7)]);
    // Page 1 is full: what follows the reference goes to page 2, where the
    // continued lines take room at the foot.
    assert_eq!(t.page_of_paragraph(41), Some(1));
    let foot = 841.9 - 72.0;
    let last = t.pages[1].items.iter().filter_map(|i| match i { Item::Line { source: Source::Footnote(_), top_pt, height_pt, .. } => Some(top_pt + height_pt), _ => None }).fold(0.0, f64::max);
    assert!((last - foot).abs() < 1e-6);
}

#[test]
fn a_paragraph_kept_with_the_next_moves_with_it() {
    // 45 lines, then a one-line paragraph kept with a two-line one: the
    // first fits on page 1 but its follower does not, so both go to page 2.
    let mut d = doc_of(45, "x");
    d.paragraphs.push(Paragraph { style: ParaStyle { keep_with_next: true, ..Default::default() }, runs: vec![Run::plain("Title")] });
    d.paragraphs.push(Paragraph { style: ParaStyle::default(), runs: vec![Run::plain("word ".repeat(20).trim())] });
    assert_eq!(lay(&d).page_of_paragraph(45), Some(1), "kept with its follower");
    d.paragraphs[45].style.keep_with_next = false;
    assert_eq!(lay(&d).page_of_paragraph(45), Some(0), "not kept: it fits on page 1");
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

/// The page stack's lookups (#1282) against the definitions they replace:
/// a page's top re-summed from the start, the visible pages found by
/// testing every page, and the nearest page by the minimum distance.
#[test]
fn page_stack_lookups_match_testing_every_page() {
    let sizes: Vec<(f64, f64)> = (0..40).map(|i| (816.0, if i % 7 == 3 { 1056.0 } else { 816.0 + (i % 5) as f64 * 13.7 })).collect();
    let gap = 24.0;
    let stack = PageStack::new(sizes.clone(), gap);
    let top = |i: usize| (gap + sizes.iter().take(i).map(|s| s.1 + gap).sum::<f64>()).floor();
    for i in 0..sizes.len() {
        assert_eq!(stack.top(i).floor(), top(i), "page {i}");
    }
    assert_eq!(stack.height(), gap + sizes.iter().map(|s| s.1 + gap).sum::<f64>());
    let mut y = -50.0;
    while y < stack.height() + 50.0 {
        let (band_top, band_bottom) = (y, y + 700.0);
        let expected: Vec<usize> = (0..sizes.len()).filter(|&i| !(top(i) + sizes[i].1 < band_top || top(i) > band_bottom)).collect();
        assert_eq!(stack.visible(band_top, band_bottom).collect::<Vec<_>>(), expected, "band at {y}");
        let d = |i: usize| if y < top(i) { top(i) - y } else if y > top(i) + sizes[i].1 { y - top(i) - sizes[i].1 } else { 0.0 };
        let nearest = (0..sizes.len()).min_by(|&a, &b| d(a).partial_cmp(&d(b)).unwrap());
        assert_eq!(stack.nearest(y), nearest, "nearest to {y}");
        y += 7.3;
    }
    assert_eq!(PageStack::new(Vec::new(), gap).nearest(10.0), None);
    assert!(PageStack::new(Vec::new(), gap).visible(0.0, 100.0).is_empty());
}

/// A merged cell is as wide as the columns it spans, and one down two
/// rows makes the second tall enough for its content; the positions it
/// covers are not drawn.
#[test]
fn a_merged_cell_spans_its_columns_and_rows() {
    let mut d = doc_of(1, "after");
    let table = d.insert_table_at(0, 3, 3);
    for p in d.paragraphs.iter_mut() {
        let Some(c) = p.style.table_cell else { continue };
        p.runs = vec![Run::plain(match (c.row, c.col) {
            (0, 0) => "heading",
            // Five lines in one column, as MonoShaper wraps it.
            (1, 0) => "word word word word word word word word word word word word word word word word word word word word word word word word word",
            (0, _) | (2, 0) => "",
            _ => "x",
        })];
    }
    d.table_spans.insert(table, vec![
        crate::model::CellSpan { row: 0, col: 0, rows: 1, cols: 3 },
        crate::model::CellSpan { row: 1, col: 0, rows: 2, cols: 1 },
    ]);
    let t = lay(&d);
    let cells: Vec<(u32, u32, f64, f64, f64)> = t.pages[0]
        .items
        .iter()
        .filter_map(|i| match i { Item::Cell { row, col, y_pt, width_pt, height_pt, .. } => Some((*row, *col, *y_pt, *width_pt, *height_pt)), _ => None })
        .collect();
    let at = |r: u32, c: u32| cells.iter().find(|x| (x.0, x.1) == (r, c)).copied();
    let table_w = 595.3 - 144.0;
    // The heading spans the table; what it covers is not drawn.
    let heading = at(0, 0).expect("the heading cell");
    assert!((heading.3 - table_w).abs() < 1e-6, "{cells:?}");
    assert!(at(0, 1).is_none() && at(0, 2).is_none() && at(2, 0).is_none(), "{cells:?}");
    // The tall cell runs from row 1 to the foot of row 2, which is tall
    // enough for its five lines; row 1 is one line.
    let tall = at(1, 0).expect("the tall cell");
    let (row1, row2) = (at(1, 1).expect("row 1"), at(2, 1).expect("row 2"));
    assert!((row1.4 - (LINE + CELL_RULE_PT)).abs() < 1e-6, "row 1 is its own content's height: {cells:?}");
    assert!((tall.2 - row1.2).abs() < 1e-6 && (tall.2 + tall.4 - (row2.2 + row2.4)).abs() < 1e-6, "{cells:?}");
    assert!(tall.4 >= 5.0 * LINE + CELL_RULE_PT - 1e-6, "{cells:?}");
    // Its text is all there, in its column.
    let words: usize = t.pages[0].items.iter().filter_map(|i| match i { Item::Line { text, .. } => Some(text.matches("word").count()), _ => None }).sum();
    assert_eq!(words, 25);
}
