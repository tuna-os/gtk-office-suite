// Engine tests with MonoShaper: 12pt text is 6pt per char and 15pt per
// line, so an A4 page with 1in margins holds 75 chars × 46 lines.

use super::*;
use crate::model::{Alignment, BorderStyle, CellFill, HfStory, LineHeight, ParaBorder, ParaBorders, ParaStyle, RowHeight, TabAlign, TabLeader, TabStop, TableCell, CellPadding};

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

/// A paragraph too short to split (three lines, two orphans and two
/// widows) moves whole, and the heading before it goes with it rather
/// than being left at the foot of the page, as Writer pages the W1 notes'
/// "Warehouse name".
#[test]
fn a_heading_goes_with_a_paragraph_too_short_to_split() {
    // 42 lines used; the heading (24pt) and two lines would fit, three
    // would not.
    let mut d = doc_of(42, "x");
    d.paragraphs.push(Paragraph { style: ParaStyle { heading: Some(1), ..Default::default() }, runs: vec![Run::plain("Chapter")] });
    d.paragraphs.push(Paragraph { style: ParaStyle::default(), runs: vec![Run::plain("word ".repeat(40))] });
    let t = lay(&d);
    assert_eq!(t.pages[1].items.iter().filter(|i| matches!(i, Item::Line { source: Source::Paragraph(43), .. })).count(), 3);
    assert_eq!(t.page_of_paragraph(42), Some(1), "the heading moved with its body");
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

/// Two paragraphs of one cell stand the larger of the space between them
/// apart, as in the body (Writer: 4.5pt after and 4.5pt before is 4.5pt
/// in a cell too); the cell's first and last keep theirs.
#[test]
fn a_cells_paragraphs_collapse_the_space_between_them() {
    let mut d = doc_of(1, "after");
    let table = d.insert_table_at(0, 1, 1);
    let at = d.paragraphs.iter().position(|p| p.style.table_cell == Some(TableCell { table, row: 0, col: 0 })).unwrap();
    let cell = d.paragraphs[at].style.table_cell;
    let para = |t: &str| Paragraph { style: ParaStyle { table_cell: cell, space_before_pt: 4.5, space_after_pt: 4.5, ..Default::default() }, runs: vec![Run::plain(t)] };
    d.paragraphs.splice(at..=at, [para("one"), para("two")]);
    let t = lay(&d);
    let tops: Vec<f64> = line_items(&t.pages[0]).iter().map(|l| l.3).collect();
    assert_eq!(tops[1] - tops[0], LINE + 4.5, "{tops:?}");
    let height = t.pages[0].items.iter().find_map(|i| match i { Item::Cell { height_pt, .. } => Some(*height_pt), _ => None }).unwrap();
    assert_eq!(height, 4.5 + LINE + 4.5 + LINE + 4.5 + CELL_RULE_PT);
}

/// A list item in a cell has its marker, as in the body: the crime
/// supervisor form's bullets beside its logo, numbered items counted.
#[test]
fn a_list_item_in_a_cell_has_its_marker() {
    let mut d = doc_of(1, "after");
    let table = d.insert_table_at(0, 1, 1);
    let at = d.paragraphs.iter().position(|p| p.style.table_cell == Some(TableCell { table, row: 0, col: 0 })).unwrap();
    let cell = d.paragraphs[at].style.table_cell;
    let item = |t: &str| Paragraph { style: ParaStyle { table_cell: cell, list: crate::model::ListKind::Numbered, ..Default::default() }, runs: vec![Run::plain(t)] };
    d.paragraphs.splice(at..=at, [item("one"), item("two")]);
    let t = lay(&d);
    let markers: Vec<(String, f64)> = t.pages[0].items.iter().filter_map(|i| match i { Item::Marker { text, x_pt, .. } => Some((text.clone(), *x_pt)), _ => None }).collect();
    assert_eq!(markers.iter().map(|m| m.0.as_str()).collect::<Vec<_>>(), ["1.", "2."]);
    let text_x = line_items(&t.pages[0])[0].2;
    assert!(markers[0].1 < text_x, "the marker hangs before the text: {markers:?} {text_x}");
}

/// An unruled table's cells draw no rule, and its rows hold no rule's
/// height: a one-line row is its line.
#[test]
fn an_unruled_tables_rows_are_their_lines() {
    let mut d = doc_of(1, "after");
    let table = d.insert_table_at(0, 1, 1);
    d.table_unruled.insert(table);
    let t = lay(&d);
    let cells: Vec<(bool, f64)> = t.pages[0]
        .items
        .iter()
        .filter_map(|i| match i { Item::Cell { ruled, height_pt, .. } => Some((*ruled, *height_pt)), _ => None })
        .collect();
    assert_eq!(cells, [(false, LINE)]);
}

/// A table whose first cell has a page break before it starts a page.
#[test]
fn a_table_after_a_page_break_starts_a_page() {
    let mut d = doc_of(2, "text");
    let table = d.insert_table_at(1, 1, 1);
    let first = d.paragraphs.iter().position(|p| p.style.table_cell.is_some_and(|c| c.table == table)).unwrap();
    d.paragraphs[first].style.page_break_before = true;
    let t = lay(&d);
    assert_eq!(t.pages.len(), 2);
    assert!(!t.pages[0].items.iter().any(|i| matches!(i, Item::Cell { .. })));
    assert!(t.pages[1].items.iter().any(|i| matches!(i, Item::Cell { .. })));
}

/// A cell's bare sides reach the painter with it; its neighbours keep
/// their own.
#[test]
fn a_cells_bare_sides_are_laid_with_it() {
    use crate::model::{CellEdges, OpenEdges};
    let mut d = doc_of(1, "after");
    let table = d.insert_table_at(0, 1, 2);
    let bare = OpenEdges { right: true, ..Default::default() };
    d.table_open_edges.insert(table, vec![CellEdges { row: 0, col: 0, open: bare }]);
    let open: Vec<(u32, OpenEdges)> = lay(&d).pages[0]
        .items
        .iter()
        .filter_map(|i| match i { Item::Cell { col, open, .. } => Some((*col, *open)), _ => None })
        .collect();
    assert_eq!(open, [(0, bare), (1, OpenEdges::default())]);
}

/// A cell set in the middle (or at the bottom) of a row taller than it
/// puts its line there; a top cell's line stays at the top.
#[test]
fn a_cells_content_sits_where_the_cell_says() {
    use crate::model::{CellVAlign, VAlign};
    let mut d = doc_of(1, "after");
    let table = d.insert_table_at(0, 1, 3);
    d.table_rows.insert(table, vec![Some(RowHeight { pt: 100.0, exact: false })]);
    d.table_valign.insert(table, vec![CellVAlign { row: 0, col: 1, align: VAlign::Center }, CellVAlign { row: 0, col: 2, align: VAlign::Bottom }]);
    let at = |d: &Document, c: u32| d.paragraphs.iter().position(|p| p.style.table_cell == Some(TableCell { table, row: 0, col: c })).unwrap();
    for c in 0..3 {
        let i = at(&d, c);
        d.paragraphs[i].runs = vec![Run::plain("x")];
    }
    let t = lay(&d);
    let y = |c: u32| {
        let i = at(&d, c);
        t.pages[0].items.iter().find_map(|it| match it { Item::Line { source: Source::Paragraph(q), top_pt, .. } if *q == i => Some(*top_pt), _ => None }).unwrap()
    };
    let (top, mid, bottom) = (y(0), y(1), y(2));
    assert!(mid > top + 30.0 && bottom > mid + 30.0, "{top} {mid} {bottom}");
}

/// A list item's marker hangs as far before its text as the file says,
/// else `HANGING_PT`; the text stays where it was.
#[test]
fn a_list_marker_hangs_as_far_as_the_file_says() {
    let mut d = doc_of(2, "item");
    for p in &mut d.paragraphs {
        p.style.list = crate::model::ListKind::Numbered;
        p.style.list_level = 1;
    }
    d.paragraphs[1].style.list_hanging_pt = Some(36.0);
    let t = lay(&d);
    let markers: Vec<f64> = t.pages[0].items.iter().filter_map(|i| match i { Item::Marker { x_pt, .. } => Some(*x_pt), _ => None }).collect();
    // Level 1's text is 36pt in: the default marker 18pt before it, the
    // file's 36pt before it, at the margin.
    assert_eq!(markers.len(), 2);
    assert!((markers[0] - markers[1] - (36.0 - crate::lists::HANGING_PT)).abs() < 1e-6, "{markers:?}");
}

/// A list item may stand left of its level's own indent, as far as the
/// margin; a marker wider than its hanging is followed by a tab to the
/// next default stop, as Word and Writer draw it. The business
/// appointment rules' bullets hang 2 twips at the margin: the text starts
/// at 0.5in, and the lines below it at the margin.
#[test]
fn a_marker_wider_than_its_hanging_tabs_its_text_to_the_next_stop() {
    let mut d = doc_of(1, &"word ".repeat(60));
    let st = &mut d.paragraphs[0].style;
    st.list = crate::model::ListKind::Bullet;
    st.left_indent_pt = -crate::lists::text_indent_pt(0);
    st.list_hanging_pt = Some(0.1);
    let t = lay(&d);
    let marker = t.pages[0].items.iter().find_map(|i| match i { Item::Marker { x_pt, .. } => Some(*x_pt), _ => None }).unwrap();
    let lines: Vec<f64> = t.pages[0].items.iter().filter_map(|i| match i { Item::Line { x_pt, .. } => Some(*x_pt), _ => None }).collect();
    assert!(lines.len() > 1, "{lines:?}");
    let margin = lines[1];
    assert!((marker - margin).abs() < 1e-6, "the marker at the margin: {marker} {margin}");
    assert!((lines[0] - margin - DEFAULT_TAB_PT).abs() < 1e-6, "the first line at the first stop: {lines:?}");
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
/// wider than the text run on into the right margin and are scaled only
/// past the page's edge; a table sized as a share of the text's width is
/// scaled to it; and a table whose column count changed since it was read
/// falls back to equal columns.
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
    d.table_columns.insert(table, vec![240.0, 240.0]);
    assert_eq!(cells_x_w(&d), [(x0, 240.0), (x0 + 240.0, 240.0)], "into the right margin, as the file has it");
    d.table_columns.insert(table, vec![box_w, box_w]);
    let got = cells_x_w(&d);
    let room = box_w + 72.0;
    assert!((got[0].1 - room / 2.0).abs() < 1e-9 && (got[1].0 - (x0 + room / 2.0)).abs() < 1e-9, "scaled to the page's edge: {got:?}");
    d.table_relative.insert(table);
    let got = cells_x_w(&d);
    assert!((got[0].1 - box_w / 2.0).abs() < 1e-9 && (got[1].0 - (x0 + box_w / 2.0)).abs() < 1e-9, "a share of the text fits it: {got:?}");
    d.table_relative.clear();

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

/// A row kept on one page that does not fit what is left of a page
/// starts the next, as Writer moves the social housing dictionary's long
/// rows whole; one not kept so splits there; and one taller than a page
/// splits all the same.
#[test]
fn a_row_kept_whole_starts_the_next_page() {
    let build = |filler: usize, row_lines: usize| -> (Document, u32) {
        let mut d = doc_of(filler, "filler");
        d.paragraphs.push(Paragraph { style: ParaStyle::default(), runs: vec![Run::plain("after")] });
        let table = d.insert_table_at(filler, 1, 1);
        let at = d.paragraphs.iter().position(|p| p.style.table_cell == Some(TableCell { table, row: 0, col: 0 })).unwrap();
        let lines: Vec<Paragraph> = (0..row_lines)
            .map(|n| Paragraph { style: ParaStyle { table_cell: Some(TableCell { table, row: 0, col: 0 }), ..Default::default() }, runs: vec![Run::plain(format!("line {n}"))] })
            .collect();
        d.paragraphs.splice(at..=at, lines);
        (d, table)
    };
    let first_cell_page = |d: &Document| lay(d).pages.iter().position(|p| p.items.iter().any(|i| matches!(i, Item::Cell { .. }))).unwrap();
    // 40 lines of the page's 46 taken, a 10-line row after them.
    let (mut d, table) = build(40, 10);
    assert_eq!(first_cell_page(&d), 0, "not kept whole, the row splits at the foot");
    d.table_whole_rows.entry(table).or_default().insert(0);
    assert_eq!(first_cell_page(&d), 1, "kept whole, it starts the next page");
    let (mut d, table) = build(40, 80);
    d.table_whole_rows.entry(table).or_default().insert(0);
    assert_eq!(first_cell_page(&d), 0, "taller than a page, it splits all the same");
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

/// A header of one empty paragraph is a line tall, as Word and Writer
/// draw it: the business appointment rules' runs from 35.4pt down past
/// their 45pt top margin, and the body starts under it. A document without
/// a header, or whose header has no paragraph, keeps its margin.
#[test]
fn an_empty_header_paragraph_takes_a_lines_room() {
    use crate::model::{HfLine, PageGeometry};
    let body_top = |header: Option<&str>, lines: Vec<HfLine>| {
        let mut d = doc_of(3, "x");
        d.page = Some(PageGeometry { margin_top_pt: 45.0, ..PageGeometry::default() });
        d.header = header.map(str::to_string);
        d.header_lines = lines;
        let t = lay(&d);
        let top = t.pages[0].lines().find_map(|l| match l { Item::Line { source: Source::Paragraph(_), top_pt, .. } => Some(*top_pt), _ => None });
        top.unwrap()
    };
    let opts = LayoutOptions::default();
    assert_eq!(body_top(None, Vec::new()), 45.0, "no header");
    assert_eq!(body_top(Some(""), Vec::new()), 45.0, "a header with no paragraph");
    assert_eq!(body_top(Some(""), vec![HfLine::default()]), opts.header_distance_pt + LINE, "one empty paragraph");
}

/// Each line of a footer as its own: a page number at the right over a
/// date at the left in a smaller face, each line as tall as its text, and
/// drawn in its line's look.
#[test]
fn a_footers_lines_each_keep_their_own_alignment_and_look() {
    use crate::model::{HfLine, RunStyle};
    let mut d = doc_of(1, "x");
    d.footer = Some("{page}\nOctober 2025".into());
    let small = RunStyle { font_size_hp: Some(16), ..Default::default() };
    d.footer_lines = vec![HfLine { alignment: Alignment::Right, ..Default::default() }, HfLine { look: small.clone(), ..Default::default() }];
    let t = lay(&d);
    let page = &t.pages[0];
    let lines: Vec<(f64, f64, String, Option<HfLine>)> = page
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Line { source: Source::Footer, x_pt, height_pt, text, story_line, .. } => Some((*x_pt, *height_pt, text.clone(), story_line.as_deref().cloned())),
            _ => None,
        })
        .collect();
    assert_eq!(lines.iter().map(|l| l.2.as_str()).collect::<Vec<_>>(), ["1", "October 2025"]);
    assert!((lines[0].0 + 6.0 - (page.width_pt - 72.0)).abs() < 1e-6, "the number ends at the right margin: {}", lines[0].0);
    assert_eq!((lines[0].1, lines[1].0, lines[1].1), (LINE, 72.0, 10.0), "the date at the left margin, as tall as its 8pt");
    assert_eq!(lines[1].3.as_ref().map(|l| &l.look), Some(&small), "drawn in its line's look");
}

/// A floating header or footer line takes no room: the header's title is
/// its first line, and the label sits against the page's top (and the
/// footer's against its foot), centred across the page.
#[test]
fn a_floating_line_sits_against_the_page_and_takes_no_room() {
    use crate::model::{HfFloat, HfLine};
    let mut d = doc_of(1, "x");
    let label = |float| HfLine { alignment: Alignment::Center, float: Some(float), ..Default::default() };
    d.header = Some("OFFICIAL\nTitle".into());
    d.header_lines = vec![label(HfFloat::Top(15.0))];
    d.footer = Some("OFFICIAL\n{page}".into());
    d.footer_lines = vec![label(HfFloat::Bottom(15.0))];
    let t = lay(&d);
    let page = &t.pages[0];
    let lines = |src: Source| -> Vec<(String, f64, f64)> {
        page.items.iter().filter_map(|i| match i { Item::Line { source, text, x_pt, top_pt, .. } if *source == src => Some((text.clone(), *x_pt, *top_pt)), _ => None }).collect()
    };
    let (header, footer) = (lines(Source::Header), lines(Source::Footer));
    assert_eq!((header[0].0.as_str(), header[0].2), ("OFFICIAL", 15.0), "against the page's top");
    assert!((header[0].1 + 8.0 * 6.0 / 2.0 - page.width_pt / 2.0).abs() < 1e-6, "centred across the page: {}", header[0].1);
    assert_eq!(header[1].2, opts().header_distance_pt, "the title is the header's first line");
    assert_eq!(footer[0].2, page.height_pt - 15.0 - LINE, "against the page's foot");
    assert_eq!(footer[1].2, page.height_pt - opts().footer_distance_pt - LINE, "the footer is its number alone");
}

/// A page's own header and footer distances (Word's `w:pgMar w:header`,
/// `w:footer`) place its header and footer, and push the body by them,
/// rather than the layout's default.
#[test]
fn a_pages_own_header_distance_places_its_header() {
    use crate::model::PageGeometry;
    let mut d = doc_of(1, "x");
    d.header = Some("Head".into());
    d.footer = Some("Foot".into());
    d.page = Some(PageGeometry { margin_top_pt: 40.0, header_pt: Some(20.0), footer_pt: Some(25.0), ..PageGeometry::default() });
    let t = lay(&d);
    let page = &t.pages[0];
    let top = |src: Source| page.items.iter().find_map(|i| match i { Item::Line { source, top_pt, .. } if *source == src => Some(*top_pt), _ => None });
    assert_eq!(top(Source::Header), Some(20.0));
    assert_eq!(top(Source::Footer), Some(page.height_pt - 25.0 - LINE));
    assert_eq!(top(Source::Paragraph(0)), Some(40.0), "the header's 20pt and a line fit above the 40pt margin");
}

/// A page border is drawn padding and rule outside the text: around the
/// body alone without a header or footer, around the header and footer
/// a page shows too, on every page.
#[test]
fn a_page_border_surrounds_the_text_and_its_header_and_footer() {
    use crate::model::{PageBorder, PageGeometry};
    let border = |t: &RenderTree, page: usize| {
        t.pages[page].items.iter().find_map(|i| match i {
            Item::PageBorder { x_pt, y_pt, width_pt, height_pt, line_pt, color } => Some((*x_pt, *y_pt, *width_pt, *height_pt, *line_pt, *color)),
            _ => None,
        })
    };
    let mut d = doc_of(LINES_PER_PAGE + 1, "x");
    let rule = PageBorder { width_pt: 1.0, color: 0x12_34_56, padding_pt: [2.0, 3.0, 4.0, 5.0] };
    d.page = Some(PageGeometry { border: Some(rule), ..PageGeometry::default() });
    let g = d.page.unwrap();
    let t = lay(&d);
    assert_eq!(t.pages.len(), 2);
    let (x, y, w, h, line, color) = border(&t, 0).expect("the page's border");
    assert_eq!((line, color), (1.0, 0x12_34_56));
    assert_eq!(x, g.margin_left_pt - 5.0 - 1.0);
    assert_eq!(y, g.margin_top_pt - 2.0 - 1.0);
    assert_eq!(x + w, g.width_pt - g.margin_right_pt + 3.0 + 1.0);
    assert_eq!(y + h, g.height_pt - g.margin_bottom_pt + 4.0 + 1.0);
    assert!(border(&t, 1).is_some(), "every page has its border");
    assert_eq!(line_items(&t.pages[0])[0].3, g.margin_top_pt, "the body stays where the margin puts it");

    d.header = Some("Head".into());
    d.footer = Some("Foot".into());
    d.page = Some(PageGeometry { header_pt: Some(20.0), footer_pt: Some(25.0), border: Some(rule), ..g });
    let t = lay(&d);
    let (_, y, _, h, _, _) = border(&t, 0).unwrap();
    assert_eq!(y, 20.0 - 2.0 - 1.0, "the border runs above the header");
    assert_eq!(y + h, g.height_pt - 25.0 + 4.0 + 1.0, "and below the footer");

    d.page = Some(PageGeometry { border: None, ..g });
    assert!(border(&lay(&d), 0).is_none(), "no border unless the page has one");
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

/// A table's own cell padding: the row is as tall as its text and the
/// padding above and below it, the text inset by the left padding, and
/// the table out to the left by it so the text lines up with the margin.
#[test]
fn a_tables_cell_padding_sizes_its_rows_and_insets_its_text() {
    let cells = |d: &Document| -> Vec<(f64, f64, f64)> {
        lay(d).pages[0].items.iter().filter_map(|i| match i { Item::Cell { x_pt, y_pt, height_pt, .. } => Some((*x_pt, *y_pt, *height_pt)), _ => None }).collect()
    };
    let text_x = |d: &Document| lay(d).pages[0].lines().find_map(|l| match l { Item::Line { text, x_pt, top_pt, .. } if text == "cell" => Some((*x_pt, *top_pt)), _ => None }).unwrap();
    let mut d = doc_of(1, "after");
    let table = d.insert_table_at(0, 1, 1);
    d.paragraphs[0].runs = vec![Run::plain("cell")];
    let plain = cells(&d)[0];
    assert_eq!(text_x(&d).0, 72.0);
    d.table_padding.insert(table, CellPadding { top_pt: 6.0, bottom_pt: 4.0, left_pt: 9.0, right_pt: 9.0 });
    let padded = cells(&d)[0];
    assert_eq!(padded.0, 72.0 - 9.0, "the table sits out by its left padding");
    assert!((padded.2 - (plain.2 + 10.0)).abs() < 1e-9, "6pt above and 4pt below: {padded:?} vs {plain:?}");
    let (x, top) = text_x(&d);
    assert_eq!((x, top), (72.0, padded.1 + 6.0), "text inside the padding, at the margin");
}

/// A table's indent moves the whole table right: its text that far from
/// the margin, its edge its left padding further left.
#[test]
fn a_tables_indent_moves_it_right() {
    let mut d = doc_of(1, "after");
    let table = d.insert_table_at(0, 1, 1);
    d.paragraphs[0].runs = vec![Run::plain("cell")];
    d.table_padding.insert(table, CellPadding { left_pt: 9.0, ..CellPadding::default() });
    d.table_indent.insert(table, 20.0);
    let page = &lay(&d).pages[0];
    let cell = page.items.iter().find_map(|i| match i { Item::Cell { x_pt, .. } => Some(*x_pt), _ => None });
    let text = page.lines().find_map(|l| match l { Item::Line { text, x_pt, .. } if text == "cell" => Some(*x_pt), _ => None });
    assert_eq!((cell, text), (Some(72.0 + 20.0 - 9.0), Some(72.0 + 20.0)));
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

/// The first page's own header (`Document::first_header`, a title page's
/// letterhead) is drawn on the first page alone, which makes room for it
/// and its lines' spacing; the pages after have the running header and
/// the margin it leaves them.
#[test]
fn the_first_page_has_its_own_header_and_the_room_it_needs() {
    let mut d = doc_of(60, "x");
    d.header = Some("Report".into());
    let spaced = HfLine { space_after_pt: 12.0, ..Default::default() };
    d.first_header = Some(HfStory { text: "Letterhead\n\n\n\n\n".into(), lines: vec![spaced; 6], ..Default::default() });
    let t = lay(&d);
    let headers = |p: &Page| -> Vec<String> {
        p.items.iter().filter_map(|i| match i { Item::Line { source: Source::Header, text, .. } => Some(text.clone()), _ => None }).collect()
    };
    let first_body = |p: &Page| p.items.iter().find_map(|i| match i { Item::Line { source: Source::Paragraph(_), top_pt, .. } => Some(*top_pt), _ => None }).unwrap();
    assert_eq!(headers(&t.pages[0]).first().map(String::as_str), Some("Letterhead"));
    assert!(!headers(&t.pages[0]).contains(&"Report".to_string()), "{:?}", headers(&t.pages[0]));
    assert_eq!(headers(&t.pages[1]), vec!["Report"]);
    // Six 15pt lines and the space after the first five, below the
    // header's distance from the page's top.
    assert!((first_body(&t.pages[0]) - (36.0 + 6.0 * 15.0 + 5.0 * 12.0)).abs() < 1e-9, "{}", first_body(&t.pages[0]));
    assert_eq!(first_body(&t.pages[1]), 72.0, "the running header fits the margin");
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

/// A text box floats where its anchor puts it, as a picture does, with its
/// own size and text; its paragraph's line does not hold it.
#[test]
fn a_text_box_is_placed_by_its_anchor_not_in_the_line() {
    use crate::model::{AnchorAlign, AnchorFrame, ImageAnchor, TextBox};
    let tb = TextBox { extent_emu: (2 * 914_400, 457_200), text: "answer".into(), ..Default::default() };
    let mut d = doc_of(2, "text");
    d.paragraphs[1].runs.push(Run {
        text: String::new(),
        style: crate::model::RunStyle {
            text_box: Some(Box::new(tb.clone())),
            image_anchor: Some(ImageAnchor { h_from: AnchorFrame::Margin, h_align: Some(AnchorAlign::End), v_from: AnchorFrame::Text, y_emu: 12 * 12_700, ..Default::default() }),
            ..Default::default()
        },
    });
    let t = lay(&d);
    let boxes: Vec<(f64, f64, f64, f64, &TextBox)> = t.pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::TextBox { x_pt, y_pt, width_pt, height_pt, text_box, .. } => Some((*x_pt, *y_pt, *width_pt, *height_pt, text_box.as_ref())),
            _ => None,
        })
        .collect();
    let right = t.pages[0].width_pt - 72.0;
    assert_eq!(boxes, vec![(right - 144.0, 72.0 + LINE + 12.0, 144.0, 36.0, &tb)], "2in by 0.5in at the right margin, 12pt below the second line's top");
    let heights: Vec<f64> = t.pages[0].lines().map(|l| match l { Item::Line { height_pt, .. } => *height_pt, _ => 0.0 }).collect();
    assert_eq!(heights[0], heights[1], "the line does not grow to hold it");
}

/// A document whose first paragraph floats a `w`-by-`h` inch picture at
/// the left margin, wrapped as `wrap` says, with `n` one-line paragraphs
/// after it: each paragraph's first line's (x, top).
fn beside_a_picture(wrap: crate::model::Wrap, w: u64, h: u64, n: usize) -> Vec<(f64, f64)> {
    use crate::model::{AnchorFrame, ImageAnchor};
    let mut d = doc_of(n + 1, "text");
    d.paragraphs[0].runs.push(Run {
        text: String::new(),
        style: crate::model::RunStyle {
            image: Some("/nonexistent.png".into()),
            image_extent_emu: Some((w * 914_400, h * 914_400)),
            image_anchor: Some(ImageAnchor { h_from: AnchorFrame::Margin, v_from: AnchorFrame::Text, wrap, ..Default::default() }),
            ..Default::default()
        },
    });
    let t = lay(&d);
    line_items(&t.pages[0]).iter().filter(|l| l.1 == 0).map(|l| (l.2, l.3)).collect()
}

/// Text flows beside a picture set to wrap parallel, on its wider side
/// and 9pt clear of it, and back across the column below it.
#[test]
fn paragraphs_beside_a_parallel_picture_are_set_beside_it() {
    let lines = beside_a_picture(crate::model::Wrap::Parallel, 2, 1, 8);
    // The picture spans 72pt from the first line's top: five lines.
    let beside = lines.iter().filter(|l| l.1 < 72.0 + 72.0).count();
    assert_eq!(beside, (72.0 / LINE).ceil() as usize);
    for (x, top) in &lines {
        let want = if *top < 72.0 + 72.0 { 72.0 + 144.0 + 9.0 } else { 72.0 };
        assert_eq!(*x, want, "line at {top}");
    }
}

/// Text wrapped top and bottom starts below the picture, and so does text
/// with too little room beside one set to wrap parallel: the anchoring
/// paragraph's own text too, as Writer moves it, while the picture stays
/// where the paragraph's top put it.
#[test]
fn a_paragraph_that_may_not_sit_beside_a_picture_starts_below_it() {
    use crate::model::Wrap;
    for (wrap, w) in [(Wrap::TopBottom, 2), (Wrap::Parallel, 6)] {
        let lines = beside_a_picture(wrap, w, 1, 2);
        assert_eq!(lines, vec![(72.0, 144.0), (72.0, 144.0 + LINE), (72.0, 144.0 + 2.0 * LINE)], "{wrap:?}");
        let mut d = doc_of(1, "text");
        d.paragraphs[0].runs.push(Run {
            text: String::new(),
            style: crate::model::RunStyle {
                image: Some("/nonexistent.png".into()),
                image_extent_emu: Some((w * 914_400, 914_400)),
                image_anchor: Some(crate::model::ImageAnchor { h_from: crate::model::AnchorFrame::Margin, v_from: crate::model::AnchorFrame::Text, wrap, ..Default::default() }),
                ..Default::default()
            },
        });
        let y = lay(&d).pages[0].items.iter().find_map(|i| match i { Item::Image { y_pt, .. } => Some(*y_pt), _ => None });
        assert_eq!(y, Some(72.0), "{wrap:?}: the picture is not moved with its text");
    }
}

/// A picture over the text (Word's "in front of text") moves nothing.
#[test]
fn text_runs_over_a_picture_that_does_not_wrap() {
    let lines = beside_a_picture(crate::model::Wrap::None, 2, 1, 2);
    assert_eq!(lines, vec![(72.0, 72.0), (72.0, 72.0 + LINE), (72.0, 72.0 + 2.0 * LINE)]);
}

/// An inline image keeps its room beside it (Word's `distL`/`distR`):
/// it is drawn that far in from where its line puts it, and what follows
/// it in the line starts past the room on its right.
#[test]
fn an_inline_image_keeps_its_room_beside_it() {
    let pic = |sides| Run {
        text: String::new(),
        style: crate::model::RunStyle { image: Some("x".into()), image_extent_emu: Some((914_400, 914_400)), image_sides_emu: sides, ..Default::default() },
    };
    let mut d = doc_of(1, "");
    d.paragraphs[0].runs = vec![pic(Some((114_300, 114_300))), Run::plain("after")];
    let t = lay(&d);
    let x = t.pages[0].items.iter().find_map(|i| match i { Item::Image { x_pt, .. } => Some(*x_pt), _ => None }).unwrap();
    assert_eq!(x, 72.0 + 9.0, "9pt in from the margin");
    assert_eq!(inline_size_pt(&pic(Some((114_300, 114_300)))), (90.0, 72.0), "the line keeps 9pt each side");
    assert_eq!(inline_size_pt(&pic(None)), (72.0, 72.0));
}

/// A picture wider than the text keeps its size and runs into the margin,
/// as Writer and Word draw it: it is not shrunk to the column, which drew
/// a chart a little wider than the text shorter than Writer does.
#[test]
fn an_image_wider_than_the_text_keeps_its_size() {
    let run = Run {
        text: String::new(),
        style: crate::model::RunStyle {
            image: Some("x".into()),
            image_extent_emu: Some((12700 * 500, 12700 * 200)),
            ..Default::default()
        },
    };
    assert_eq!(image_size_pt(&run), (500.0, 200.0));
    let mut d = doc_of(2, "text");
    d.paragraphs[0].runs = vec![run];
    let t = lay(&d);
    let size = t.pages[0].items.iter().find_map(|i| match i { Item::Image { width_pt, height_pt, .. } => Some((*width_pt, *height_pt)), _ => None });
    assert_eq!(size, Some((500.0, 200.0)), "wider than the {}pt column, drawn at its own size", t.pages[0].width_pt - 144.0);
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

/// In a document whose spacing adds up (`Document::space_adds`, every
/// ODF document in Writer), the space after a paragraph and the space
/// before the next are both kept between them, in the body and in a cell.
#[test]
fn spacing_adds_up_where_the_document_says_so() {
    let mut d = doc_of(3, "x");
    d.space_adds = true;
    for p in &mut d.paragraphs {
        p.style.space_after_pt = 10.0;
    }
    d.paragraphs[1].style.space_before_pt = 24.0;
    d.paragraphs[2].style.space_before_pt = 4.0;
    let tops: Vec<f64> = line_items(&lay(&d).pages[0]).iter().map(|l| l.3).collect();
    assert_eq!(tops, vec![72.0, 72.0 + LINE + 10.0 + 24.0, 72.0 + 2.0 * LINE + 10.0 + 24.0 + 10.0 + 4.0]);
    let mut d = doc_of(1, "after");
    d.space_adds = true;
    let table = d.insert_table_at(0, 1, 1);
    let at = d.paragraphs.iter().position(|p| p.style.table_cell == Some(TableCell { table, row: 0, col: 0 })).unwrap();
    let cell = d.paragraphs[at].style.table_cell;
    let para = |t: &str| Paragraph { style: ParaStyle { table_cell: cell, space_before_pt: 4.5, space_after_pt: 4.5, ..Default::default() }, runs: vec![Run::plain(t)] };
    d.paragraphs.splice(at..=at, [para("one"), para("two")]);
    let tops: Vec<f64> = line_items(&lay(&d).pages[0]).iter().map(|l| l.3).collect();
    assert_eq!(tops[1] - tops[0], LINE + 9.0, "{tops:?}");
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

/// Writer at the foot: a note's last line stands on the bottom margin by
/// its natural foot, its proportional extra hanging below; and a Word
/// note's own leading space is the only one after its number.
#[test]
fn a_spaced_note_stands_on_the_margin_by_its_natural_foot() {
    let mut d = doc_of(2, "body");
    d.paragraphs.insert(1, with_note("noted", 0));
    for p in &mut d.paragraphs {
        p.style.line_spacing = 2.0;
    }
    d.footnotes = vec![" The note.".into()];
    assert_eq!(note_paragraph(&d, 0, &opts()).runs[1].text, " The note.");
    let t = lay(&d);
    let foot = 841.9 - 72.0;
    let note: Vec<(f64, f64)> = t.pages[0]
        .items
        .iter()
        .filter_map(|i| match i { Item::Line { source: Source::Footnote(0), top_pt, height_pt, .. } => Some((*top_pt, *height_pt)), _ => None })
        .collect();
    // A 30pt line (15pt natural at 2.0): its top is 15pt above the foot.
    assert_eq!(note.len(), 1);
    assert!((note[0].1 - 30.0).abs() < 1e-9 && (note[0].0 + 15.0 - foot).abs() < 1e-6, "{note:?}");
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

/// A footnote referenced in a table cell sits at the foot of the page
/// the cell's line is on, and the row leaves it room: annex-a's
/// "Landlord¹" cell had its note dropped, so the rows under it ran into
/// the space Writer keeps for it.
#[test]
fn a_note_referenced_in_a_cell_sits_at_the_foot_of_its_page() {
    let notes = |t: &RenderTree, k: usize| -> Vec<(f64, f64)> {
        t.pages[k].items.iter().filter_map(|i| match i { Item::Line { source: Source::Footnote(0), top_pt, height_pt, .. } => Some((*top_pt, *height_pt)), _ => None }).collect()
    };
    let cell_line = |t: &RenderTree, k: usize, para: usize| -> Option<(f64, f64)> {
        t.pages[k].items.iter().find_map(|i| match i { Item::Line { source: Source::Paragraph(p), top_pt, height_pt, .. } if *p == para => Some((*top_pt, *height_pt)), _ => None })
    };
    let mut d = doc_of(1, "after");
    let table = d.insert_table_at(0, 1, 1);
    d.paragraphs[0] = Paragraph { style: d.paragraphs[0].style.clone(), ..with_note("noted", 0) };
    assert_eq!(d.paragraphs[0].style.table_cell.map(|c| c.table), Some(table));
    d.footnotes = vec!["The note.".into()];
    let t = lay(&d);
    let foot = 841.9 - 72.0;
    let note = notes(&t, 0);
    assert_eq!(note.len(), 1, "the cell's note is on its page");
    assert!((note[0].0 + note[0].1 - foot).abs() < 1e-6, "{note:?}");

    // Body lines fill the page to just above where the cell's line and
    // its note could both fit: the row goes to the next page with its note.
    let mut d = doc_of(45, "body");
    let at = d.paragraphs.len();
    d.insert_table_at(at, 1, 1);
    d.paragraphs[at] = Paragraph { style: d.paragraphs[at].style.clone(), ..with_note("noted", 0) };
    d.footnotes = vec!["The note.".into()];
    let t = lay(&d);
    let (page, line) = (0..t.pages.len()).find_map(|k| cell_line(&t, k, at).map(|l| (k, l))).expect("the cell's line");
    let note = notes(&t, page);
    assert_eq!(note.len(), 1, "the note is on the page of its reference (page {page})");
    assert!(line.0 + line.1 <= note[0].0 - NOTE_RULE_GAP_PT + 1e-6, "the line ends above the note's rule: {line:?} {note:?}");
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

/// A chain of paragraphs each kept with the next moves as one: two
/// headings over a paragraph, the first heading fitting the page's last
/// line, all go to the next page together (questionnaire-2's "Proposal 1"
/// over "Question 2" over its question), not the first left behind.
#[test]
fn a_chain_kept_with_the_next_moves_whole() {
    let mut d = doc_of(44, "x");
    let kept = |t: &str| Paragraph { style: ParaStyle { keep_with_next: true, ..Default::default() }, runs: vec![Run::plain(t)] };
    d.paragraphs.push(kept("Proposal"));
    d.paragraphs.push(kept("Question"));
    d.paragraphs.push(Paragraph { style: ParaStyle::default(), runs: vec![Run::plain("word ".repeat(20).trim())] });
    let t = lay(&d);
    assert_eq!((t.page_of_paragraph(44), t.page_of_paragraph(45), t.page_of_paragraph(46)), (Some(1), Some(1), Some(1)), "the chain moves whole");
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
    // A file's own quote style gives the indent; none is added to it.
    d.paragraphs[2].style.own_look = true;
    assert_eq!(paragraph_box_x(&d.paragraphs[2]), 0.0);
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

#[test]
fn a_paragraph_that_keeps_its_lines_moves_whole() {
    let long = "abcd ".repeat(75); // 5 lines of 75 chars
    // Room for 2 of 5 lines: orphan control puts 2 here and 3 over.
    let mut d = doc_of(44, "x");
    d.paragraphs.push(Paragraph { style: ParaStyle::default(), runs: vec![Run::plain(long.trim_end())] });
    assert_eq!(lines_per_page(&lay(&d)), vec![46, 3]);
    // Kept together, the whole paragraph moves instead.
    let mut d = doc_of(44, "x");
    d.paragraphs.push(Paragraph {
        style: ParaStyle { keep_lines: true, ..Default::default() },
        runs: vec![Run::plain(long.trim_end())],
    });
    assert_eq!(lines_per_page(&lay(&d)), vec![44, 5]);
}

#[test]
fn a_paragraph_without_widow_control_may_strand_single_lines() {
    let long = "abcd ".repeat(75); // 5 lines of 75 chars
    // Room for one line: an orphan, so the whole paragraph moves.
    let mut d = doc_of(45, "x");
    d.paragraphs.push(Paragraph { style: ParaStyle::default(), runs: vec![Run::plain(long.trim_end())] });
    assert_eq!(lines_per_page(&lay(&d)), vec![45, 5]);
    // Uncontrolled, the one line stays and four go over.
    let mut d = doc_of(45, "x");
    d.paragraphs.push(Paragraph {
        style: ParaStyle { widow_control: Some(false), ..Default::default() },
        runs: vec![Run::plain(long.trim_end())],
    });
    assert_eq!(lines_per_page(&lay(&d)), vec![46, 4]);
}

#[test]
fn exact_and_minimum_line_heights_size_lines() {
    let mut d = doc_of(3, "x");
    d.paragraphs[0].style.line_height = Some(LineHeight::Exact(30.0));
    d.paragraphs[1].style.line_height = Some(LineHeight::Minimum(30.0));
    d.paragraphs[2].style.line_height = Some(LineHeight::Minimum(5.0));
    let tops: Vec<f64> = line_items(&lay(&d).pages[0]).iter().map(|l| l.3).collect();
    // A fixed 30pt line, a 30pt minimum, and a 5pt minimum the 15pt
    // natural line outgrows.
    assert_eq!(tops, vec![72.0, 102.0, 132.0]);
}

#[test]
fn fixed_and_grown_lines_place_the_baseline_as_writer_does() {
    let mut d = doc_of(4, "x");
    d.paragraphs[0].style.line_height = Some(LineHeight::Exact(30.0));
    d.paragraphs[1].style.line_height = Some(LineHeight::Minimum(30.0));
    d.paragraphs[2].style.line_height = Some(LineHeight::Minimum(5.0));
    let t = lay(&d);
    let drops: Vec<f64> = t.pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Line { source: Source::Paragraph(_), top_pt, baseline_pt, .. } => Some(baseline_pt - top_pt),
            _ => None,
        })
        .collect();
    // The plain last paragraph shows the natural ascent; its line is 15pt.
    let natural = drops[3];
    // Exact: four fifths of the fixed height, whatever the font.
    assert!((drops[0] - 24.0).abs() < 1e-9, "{drops:?}");
    // atLeast grown from 15pt to 30pt: the 15pt growth goes above the text.
    assert!((drops[1] - (natural + 15.0)).abs() < 1e-9, "{drops:?}");
    // atLeast the natural line outgrows: unchanged.
    assert!((drops[2] - natural).abs() < 1e-9, "{drops:?}");
}

#[test]
fn tab_requests_carry_alignment_and_default_grid() {
    let mut d = doc_of(1, "a\tb");
    d.paragraphs[0].style.tab_stops_pt =
        vec![TabStop { pos_pt: 100.0, align: TabAlign::Right, leader: TabLeader::Dot }];
    let o = opts();
    let req = paragraph_request(&d.paragraphs[0], 400.0, &o);
    assert_eq!(req.tab_stops[0], TabStop { pos_pt: 100.0, align: TabAlign::Right, leader: TabLeader::Dot });
    // Word's default grid resumes past the paragraph's own stops.
    assert_eq!(req.tab_stops[1], TabStop::left(108.0));
    assert!(req.tab_stops.windows(2).all(|w| w[0].pos_pt < w[1].pos_pt), "stops in order: {:?}", req.tab_stops);
    assert!(req.tab_stops.iter().all(|t| t.pos_pt > 0.0 && t.pos_pt < 400.0));
}

/// A paragraph's shading/border boxes: (paragraph, fill, top edge,
/// bottom edge), in page order.
fn para_boxes(t: &RenderTree) -> Vec<(usize, Option<String>, bool, bool)> {
    t.pages
        .iter()
        .flat_map(|p| &p.items)
        .filter_map(|i| match i {
            Item::ParaBox { para, fill, top_edge, bottom_edge, .. } => Some((*para, fill.clone(), *top_edge, *bottom_edge)),
            _ => None,
        })
        .collect()
}

#[test]
fn shaded_paragraphs_draw_one_box_per_page() {
    let mut d = doc_of(46, "x");
    d.paragraphs[0].style.shading = Some("D9D9D9".into());
    assert_eq!(
        para_boxes(&lay(&d)),
        vec![(0, Some("D9D9D9".into()), true, true)],
        "one box under the shaded paragraph's lines"
    );
}

#[test]
fn bordered_paragraphs_open_their_box_where_they_split() {
    let long = "abcd ".repeat(75); // 5 lines: 2 here, 3 over
    let mut d = doc_of(44, "x");
    d.paragraphs.push(Paragraph {
        style: ParaStyle {
            borders: ParaBorders {
                bottom: Some(ParaBorder { style: BorderStyle::Single, color: "000000".into(), width_pt: 0.75, space_pt: 0.0 }),
                ..Default::default()
            },
            ..Default::default()
        },
        runs: vec![Run::plain(long.trim_end())],
    });
    let t = lay(&d);
    assert_eq!(lines_per_page(&t), vec![46, 3]);
    assert_eq!(para_boxes(&t).iter().map(|b| (b.2, b.3)).collect::<Vec<_>>(), vec![(true, false), (false, true)]);
}

/// Writer's border box (measured in LibreOffice at 10 px/pt): a top
/// border stands its width plus gap above the first line, a bottom one
/// its gap below the last line's natural foot, with that line's
/// proportional extra below the border, and side borders stand off the
/// text by their width plus gap.
#[test]
fn borders_stand_off_the_text_by_their_width_and_gap() {
    let edge = |w: f64, gap: f64| Some(ParaBorder { style: BorderStyle::Single, color: "000000".into(), width_pt: w, space_pt: gap });
    let mut d = doc_of(3, "x");
    d.paragraphs[1].style.line_spacing = 2.0;
    d.paragraphs[1].style.borders = ParaBorders { top: edge(1.0, 4.0), bottom: edge(1.0, 4.0), left: edge(0.5, 1.0), right: edge(0.5, 1.0) };
    let t = lay(&d);
    let tops: Vec<f64> = line_items(&t.pages[0]).iter().map(|l| l.3).collect();
    let boxes: Vec<(f64, f64, f64, f64)> = t.pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::ParaBox { x_pt, y_pt, width_pt, height_pt, .. } => Some((*x_pt, *y_pt, *width_pt, *height_pt)),
            _ => None,
        })
        .collect();
    // MonoShaper lines are 15pt; the bordered one is 30pt at 2.0.
    let (x, y, w, h) = boxes[0];
    assert_eq!(y, tops[0] + 15.0, "the box opens where the paragraph does");
    assert_eq!(tops[1], y + 5.0, "text below the top border's width and gap");
    // Natural foot (15pt in), the gap and the width close the box ...
    assert_eq!(h, 5.0 + 15.0 + 5.0);
    // ... and the line's other 15pt follow it.
    assert_eq!(tops[2], y + h + 15.0);
    let text_x = line_items(&t.pages[0])[0].2;
    assert_eq!((x, w), (text_x - 1.5, t.pages[0].width_pt - t.pages[0].geometry.margin_left_pt - t.pages[0].geometry.margin_right_pt + 3.0));
}

/// Paragraphs with the same borders are one box: its gaps and its top
/// and bottom edges at the run's ends only, its sides unbroken across the
/// space between them (Writer and Word group them so).
#[test]
fn paragraphs_with_the_same_borders_are_one_box() {
    let edge = || Some(ParaBorder { style: BorderStyle::Single, color: "000000".into(), width_pt: 1.0, space_pt: 4.0 });
    let mut d = doc_of(4, "x");
    for p in &mut d.paragraphs[1..3] {
        p.style.borders = ParaBorders { top: edge(), bottom: edge(), left: edge(), right: edge() };
        p.style.space_after_pt = 6.0;
    }
    let t = lay(&d);
    let tops: Vec<f64> = line_items(&t.pages[0]).iter().map(|l| l.3).collect();
    let boxes: Vec<(f64, f64, bool, bool)> = t.pages[0]
        .items
        .iter()
        .filter_map(|i| match i {
            Item::ParaBox { y_pt, height_pt, top_edge, bottom_edge, .. } => Some((*y_pt, *height_pt, *top_edge, *bottom_edge)),
            _ => None,
        })
        .collect();
    assert_eq!(boxes.len(), 2);
    // The first opens the group, the second closes it.
    assert!(boxes[0].2 && !boxes[0].3 && !boxes[1].2 && boxes[1].3, "{boxes:?}");
    assert_eq!(tops[1], boxes[0].0 + 5.0, "the top gap above the first only");
    assert_eq!(tops[2], tops[1] + 15.0 + 6.0, "no gaps between them");
    assert_eq!(boxes[0].0 + boxes[0].1, boxes[1].0, "the box runs on over the space between");
    assert_eq!(boxes[1].1, 15.0 + 5.0, "the bottom gap below the last only");
}

/// An invisible edge keeps its gap: Word's `w:val="none"` with a
/// `w:space` pads a paragraph above and below in Writer, and draws
/// nothing (render-lab `real-questionnaire`'s blank line between tables).
#[test]
fn an_invisible_edge_keeps_its_gap() {
    let none = || Some(ParaBorder { style: BorderStyle::None, color: "FFFFFF".into(), width_pt: 12.0, space_pt: 31.0 });
    let mut d = doc_of(3, "x");
    d.paragraphs[1].style.borders = ParaBorders { top: none(), bottom: none(), ..Default::default() };
    let t = lay(&d);
    let tops: Vec<f64> = line_items(&t.pages[0]).iter().map(|l| l.3).collect();
    // 31pt above and below, none for the width it names.
    assert_eq!(tops[1] - tops[0], 15.0 + 31.0);
    assert_eq!(tops[2] - tops[1], 15.0 + 31.0);
}

/// Give `doc` sections starting at `starts`, on `pages` (LET-005S).
fn sectioned(doc: &mut Document, starts: &[usize], pages: &[PageGeometry]) {
    let n = doc.paragraphs.len();
    doc.sections = starts
        .iter()
        .enumerate()
        .map(|(k, &s)| crate::model::DocSection {
            id: k as u32,
            range: crate::model::ParagraphRange::new(s, starts.get(k + 1).copied().unwrap_or(n)),
            geometry: pages[k],
            header: None,
            footer: None,
        })
        .collect();
    doc.page = Some(pages[0]);
}

fn landscape_two_columns() -> PageGeometry {
    PageGeometry { width_pt: 841.9, height_pt: 595.3, margin_left_pt: 36.0, margin_right_pt: 36.0, columns: 2, column_gap_pt: 24.0, ..PageGeometry::default() }
}

/// Each section starts a page of its own size, lays its text out in its
/// own margins and columns, and shows its own header.
#[test]
fn each_section_is_laid_out_on_its_own_pages() {
    let portrait = PageGeometry::default();
    let landscape = landscape_two_columns();
    let mut d = doc_of(61, "text");
    sectioned(&mut d, &[0, 1], &[portrait, landscape]);
    d.header = Some("Doc".into());
    d.sections[1].header = Some("Appendix {page}".into());
    let t = lay(&d);
    assert_eq!(t.pages.len(), 2, "the second section starts a page and fills both its columns");
    assert_eq!((t.pages[0].width_pt, t.pages[0].height_pt), (portrait.width_pt, portrait.height_pt));
    assert_eq!((t.pages[1].width_pt, t.pages[1].height_pt), (841.9, 595.3));
    assert_eq!(lines_per_page(&t), vec![1, 60]);
    // 595.3 - 144 points of text area hold 30 lines a column.
    let xs: Vec<f64> = line_items(&t.pages[1]).iter().map(|l| l.2).collect();
    let column_w = (841.9 - 72.0 - 24.0) / 2.0;
    assert!(xs[..30].iter().all(|x| (x - 36.0).abs() < 1e-6), "{xs:?}");
    assert!(xs[30..].iter().all(|x| (x - (36.0 + column_w + 24.0)).abs() < 1e-6), "{xs:?}");
    let headers = |p: &Page| {
        p.items
            .iter()
            .filter_map(|i| match i {
                Item::Line { source: Source::Header, text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(headers(&t.pages[0]), vec!["Doc"]);
    assert_eq!(headers(&t.pages[1]), vec!["Appendix 2"]);
}

/// A section that starts with a table puts the whole table on its own
/// first page; a heading ending the section before is not kept with it.
#[test]
fn a_section_starting_with_a_table_keeps_the_table_whole() {
    let mut d = doc_of(1, "before");
    d.paragraphs[0].style.heading = Some(1);
    d.insert_table_at(1, 2, 2);
    sectioned(&mut d, &[0, 1], &[PageGeometry::default(), landscape_two_columns()]);
    let t = lay(&d);
    assert_eq!(t.pages.len(), 2);
    let cells_on = |p: &Page| p.items.iter().filter(|i| matches!(i, Item::Cell { .. })).count();
    assert_eq!(cells_on(&t.pages[0]), 0);
    assert!(cells_on(&t.pages[1]) > 0);
    assert_eq!(lines_per_page(&t)[0], 1, "the heading stays in its own section");
}
