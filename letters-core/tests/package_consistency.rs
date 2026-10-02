//! The packages Letters writes are consistent (#1274): every part has a
//! content type or a manifest entry, and nothing relates to a part that
//! isn't there. `suite_common_core::carry::problems` is the check.

use letters_core::model::{Document, Paragraph, ParaStyle, Run, RunStyle, TableCell};

fn rich() -> Document {
    let mut d = letters_core::comments::sample_document();
    for p in letters_core::track::sample_document().paragraphs {
        d.paragraphs.push(p);
    }
    d.paragraphs.push(Paragraph { style: ParaStyle { table_cell: Some(TableCell { table: 1, row: 0, col: 0 }), ..Default::default() }, runs: vec![Run::plain("cell")] });
    d.paragraphs.push(Paragraph { style: ParaStyle::default(), runs: vec![Run { text: "a link".into(), style: RunStyle { link: Some("https://example.com/a?b=1&c=2".into()), ..Default::default() } }] });
    d.footnotes.push("a note".into());
    d.paragraphs.push(Paragraph { style: ParaStyle::default(), runs: vec![Run { text: String::new(), style: RunStyle { footnote: Some(0), ..Default::default() } }] });
    d.header = Some("head".into());
    d
}

#[test]
fn docx_and_odt_packages_are_consistent() {
    let dir = tempfile::tempdir().unwrap();
    for ext in ["docx", "odt"] {
        let path = dir.path().join(format!("rich.{ext}"));
        letters_core::save::write(&rich(), &path).unwrap();
        assert_eq!(suite_common_core::carry::problems(&path), Vec::<String>::new(), "{ext}");
    }
}
