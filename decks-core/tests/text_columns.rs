// text_columns.rs — text set in columns is kept.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The HR and pay monthly status report sets its highlights in a two-column
// text box (`a:bodyPr numCol="2"`). Decks had no columns, so the text ran
// down one column the box's full width and off its bottom.

use decks_core::engine::{read_pptx, write_pptx, Columns, Deck, SlideObject, TextBody};
use suite_common_core::document::Run;

#[test]
fn a_boxs_columns_survive_a_pptx_save() {
    let mut d = Deck::new();
    let body = TextBody { columns: Some(Columns { count: 2, gap: 18.0 }), ..Default::default() };
    d.slides[0].objects = vec![SlideObject::TextBox {
        text: "one\ntwo".into(),
        x: 50.0,
        y: 50.0,
        w: 800.0,
        h: 300.0,
        rotation: 0.0,
        runs: vec![Run::plain("one\ntwo")],
        body,
    }];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.pptx");
    write_pptx(path.to_str().unwrap(), &d).unwrap();
    let back = read_pptx(path.to_str().unwrap()).unwrap();
    let cols = back.slides[0].objects.iter().find_map(|o| match o {
        SlideObject::TextBox { body, .. } => Some(body.columns),
        _ => None,
    });
    let cols = cols.flatten().expect("the box keeps its columns");
    assert_eq!(cols.count, 2);
    assert!((cols.gap - 18.0).abs() < 0.1, "{cols:?}");
}
