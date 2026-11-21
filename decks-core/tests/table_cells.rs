// table_cells.rs — a table cell's own text colour, weight and margins.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The HR delegations deck sets its objectives in a one-cell table whose
// style draws the first row as a white bold header; the runs state black
// (`solidFill 000000`) and regular (`b="0"`), and the cell its own margins
// (`a:tcPr marL="638217" marT="467924"`). The reader kept none of the
// three, so the text was white, bold and hard against the cell's edge.

use decks_core::engine::table::{TableCell, TableData};
use decks_core::engine::{read_pptx, write_pptx, Deck, SlideObject};
use suite_common_core::document::{Run, RunStyle};

fn deck() -> Deck {
    let plain = Run { text: "Review the ".into(), style: RunStyle { regular: true, color: Some("000000".into()), ..Default::default() } };
    let bold = Run { text: "laws".into(), style: RunStyle { bold: true, color: Some("d50057".into()), ..Default::default() } };
    let cell = TableCell { runs: vec![plain, bold], margins: Some((67.0, 49.0, 11.0, 6.0)), ..Default::default() };
    let table = TableData { col_widths: vec![300.0], row_heights: vec![100.0], rows: vec![vec![cell]], first_row: true, ..Default::default() };
    let mut d = Deck::new();
    d.slides[0].objects = vec![SlideObject::Table { x: 100.0, y: 100.0, w: 300.0, h: 100.0, rotation: 0.0, table }];
    d
}

fn cell(d: &Deck) -> TableCell {
    d.slides[0]
        .objects
        .iter()
        .find_map(|o| match o {
            SlideObject::Table { table, .. } => Some(table.rows[0][0].clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no table: {:?}", d.slides[0].objects))
}

#[test]
fn a_header_cells_own_colour_weight_and_margins_survive_a_pptx_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.pptx");
    write_pptx(path.to_str().unwrap(), &deck()).unwrap();
    let back = cell(&read_pptx(path.to_str().unwrap()).unwrap());
    let runs: Vec<_> = back.runs.iter().map(|r| (r.text.as_str(), r.style.bold, r.style.regular, r.style.color.as_deref())).collect();
    assert_eq!(runs, [("Review the ", false, true, Some("000000")), ("laws", true, false, Some("d50057"))]);
    let (l, t, r, b) = back.margins.expect("the cell's margins");
    for (got, want) in [(l, 67.0), (t, 49.0), (r, 11.0), (b, 6.0)] {
        assert!((got - want).abs() < 0.01, "{:?}", back.margins);
    }
}

#[test]
fn a_cell_without_margins_takes_the_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("m.pptx");
    let mut d = deck();
    if let SlideObject::Table { table, .. } = &mut d.slides[0].objects[0] {
        table.rows[0][0].margins = None;
    }
    write_pptx(path.to_str().unwrap(), &d).unwrap();
    assert_eq!(cell(&read_pptx(path.to_str().unwrap()).unwrap()).margins, None);
}

/// A cell's paragraphs keep their alignment and bullets, and the cell its
/// anchor: the HR measures deck's toolkit table centres its headers and
/// bullets its fact sheets (1,506 aligned and 1,571 anchored cells
/// across the real decks' tables).
#[test]
fn a_cells_paragraph_styles_and_anchor_survive_a_pptx_save() {
    use decks_core::engine::{Anchor, Bullet, ParaAlign, ParaStyle};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.pptx");
    let mut d = deck();
    if let SlideObject::Table { table, .. } = &mut d.slides[0].objects[0] {
        let cell = &mut table.rows[0][0];
        cell.runs = vec![Run::plain("Heading\nA point")];
        cell.paras = vec![
            ParaStyle { align: ParaAlign::Center, ..Default::default() },
            ParaStyle { bullet: Bullet::Char("•".into()), margin_left: 20.0, indent: -20.0, ..Default::default() },
        ];
        cell.anchor = Anchor::Middle;
    }
    write_pptx(path.to_str().unwrap(), &d).unwrap();
    let back = cell(&read_pptx(path.to_str().unwrap()).unwrap());
    assert_eq!(back.anchor, Anchor::Middle);
    assert_eq!(back.paras.len(), 2, "{:?}", back.paras);
    assert_eq!(back.paras[0].align, ParaAlign::Center);
    assert_eq!(back.paras[1].bullet, Bullet::Char("•".into()));
    assert!((back.paras[1].margin_left - 20.0).abs() < 0.1 && (back.paras[1].indent + 20.0).abs() < 0.1, "{:?}", back.paras[1]);
}
