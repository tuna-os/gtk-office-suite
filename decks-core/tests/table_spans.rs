// table_spans.rs — a merged table cell spans its columns and rows.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The overpayment dashboards' strategy table opens on a title row merged
// across both columns (`a:tc gridSpan="2"`, the next cell `hMerge`). The
// reader kept neither, so the title was squeezed into the first column and
// a blank cell drawn beside it.

use decks_core::engine::table::{TableCell, TableData};
use decks_core::engine::{read_pptx, write_pptx, Deck, SlideObject};
use letters_core::model::Run;

/// A 3x3 table: a title across the top row, and a label down two rows.
fn table() -> TableData {
    let cell = |t: &str| TableCell { runs: if t.is_empty() { vec![] } else { vec![Run::plain(t)] }, ..Default::default() };
    let mut rows: Vec<Vec<TableCell>> = (0..3).map(|r| (0..3).map(|c| cell(&format!("r{r}c{c}"))).collect()).collect();
    rows[0][0] = TableCell { col_span: 3, ..cell("Title") };
    rows[0][1] = TableCell { covered: true, ..cell("") };
    rows[0][2] = TableCell { covered: true, ..cell("") };
    rows[1][0] = TableCell { row_span: 2, ..cell("Label") };
    rows[2][0] = TableCell { covered: true, ..cell("") };
    TableData { col_widths: vec![100.0; 3], row_heights: vec![40.0; 3], rows, first_row: true, ..Default::default() }
}

fn deck() -> Deck {
    let mut d = Deck::new();
    d.slides[0].objects = vec![SlideObject::Table { x: 100.0, y: 100.0, w: 300.0, h: 120.0, rotation: 0.0, table: table() }];
    d
}

fn spans(t: &TableData) -> Vec<Vec<(usize, usize, bool)>> {
    t.rows.iter().map(|row| row.iter().map(|c| (c.span().0, c.span().1, c.covered)).collect()).collect()
}

fn table_of(d: &Deck) -> TableData {
    d.slides[0].objects.iter().find_map(|o| match o {
        SlideObject::Table { table, .. } => Some(table.clone()),
        _ => None,
    }).expect("a table")
}

#[test]
fn every_position_knows_the_cell_drawn_there() {
    let owners = table().owners();
    assert_eq!(owners[0], [(0, 0), (0, 0), (0, 0)]);
    assert_eq!(owners[1], [(1, 0), (1, 1), (1, 2)]);
    assert_eq!(owners[2], [(1, 0), (2, 1), (2, 2)]);
}

#[test]
fn merged_cells_survive_a_pptx_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.pptx");
    write_pptx(path.to_str().unwrap(), &deck()).unwrap();
    let back = table_of(&read_pptx(path.to_str().unwrap()).unwrap());
    assert_eq!(spans(&back), spans(&table()));
    assert_eq!(back.rows[0][0].text(), "Title");
}

/// An odp says the same: the merged cell's span, and a covered cell for
/// each position it covers.
#[test]
fn merged_cells_are_written_to_an_odp() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.odp");
    decks_core::odp::write(&deck(), path.to_str().unwrap()).unwrap();
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("content.xml").unwrap(), &mut xml).unwrap();
    assert!(xml.contains("table:number-columns-spanned=\"3\""), "{xml}");
    assert!(xml.contains("table:number-rows-spanned=\"2\""), "{xml}");
    assert_eq!(xml.matches("<table:covered-table-cell/>").count(), 3, "{xml}");
}
