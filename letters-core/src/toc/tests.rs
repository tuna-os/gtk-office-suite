use super::*;

fn text(p: &Paragraph) -> String {
    p.runs.iter().map(|r| r.text.as_str()).collect()
}

fn toc(doc: &Document) -> Vec<(u8, String)> {
    blocks(doc).into_iter().flat_map(|b| doc.paragraphs[b].to_vec()).map(|p| (p.style.toc.unwrap(), text(&p))).collect()
}

#[test]
fn a_table_of_contents_lists_the_headings_with_their_pages() {
    let doc = sample_document();
    assert_eq!(
        toc(&doc),
        [(1, "Introduction\t1".to_string()), (1, "Method\t2".into()), (2, "Details\t2".into()), (1, "Results\t3".into())]
    );
    assert_eq!(doc.paragraphs[2].style.left_indent_pt, INDENT_PT, "a level-2 entry is indented");
    assert!(update(&doc, &sample_pages(&doc)).is_empty(), "already current");
}

#[test]
fn updating_follows_the_headings_and_undoes_exactly() {
    let mut doc = sample_document();
    // A new heading, and one renamed.
    // After "Introduction" (paragraph 4, the entries being 0..4).
    let at = edit::paragraph_start(&doc, 5) - 1;
    edit::apply_all(&mut doc, &[edit::Op::Insert { at, content: vec![Paragraph::default(), Paragraph { style: ParaStyle { heading: Some(3), ..Default::default() }, runs: vec![Run::plain("Caveats")] }] }]).unwrap();
    let before = doc.clone();
    let ops = settle(&doc, Vec::new(), sample_pages);
    let undo = edit::apply_all(&mut doc, &ops).unwrap();
    assert_eq!(toc(&doc)[1], (3, "Caveats\t1".to_string()));
    assert_eq!(doc.paragraphs[1].style.left_indent_pt, 2.0 * INDENT_PT);
    edit::apply_all(&mut doc, &undo).unwrap();
    assert_eq!(doc, before);
}

#[test]
fn page_numbers_settle_when_the_contents_push_headings_on() {
    // Ten lines a page: the contents' own entries move the headings.
    let mut doc = Document::from_plain_text(&(0..12).map(|i| if i % 4 == 0 { format!("Heading {i}") } else { format!("line {i}") }).collect::<Vec<_>>().join("\n"));
    for i in (0..12).step_by(4) {
        doc.paragraphs[i].style.heading = Some(1);
    }
    let ten_a_page = |d: &Document| (0..d.paragraphs.len()).map(|i| Some(i / 10)).collect::<Vec<_>>();
    let ops = settle(&doc, insert(&doc, 0), ten_a_page);
    edit::apply_all(&mut doc, &ops).unwrap();
    // Three entries first: "Heading 8" is paragraph 11, on the second page.
    assert_eq!(toc(&doc), [(1, "Heading 0\t1".to_string()), (1, "Heading 4\t1".into()), (1, "Heading 8\t2".into())]);
}

#[test]
fn no_headings_is_said_and_the_contents_are_not_their_own_entries() {
    let mut doc = Document::from_plain_text("just text");
    let ops = insert(&doc, 0);
    edit::apply_all(&mut doc, &ops).unwrap();
    assert_eq!(toc(&doc), [(1, NO_ENTRIES.to_string())]);
    doc.paragraphs[1].style.heading = Some(1);
    let ops = update(&doc, &[]);
    edit::apply_all(&mut doc, &ops).unwrap();
    assert_eq!(toc(&doc), [(1, "just text".to_string())]);
    assert_eq!(headings(&doc).len(), 1, "an entry is not a heading");
}

#[test]
fn a_table_of_contents_is_not_put_inside_a_table() {
    let mut doc = Document::from_plain_text("a\nb\nc");
    doc.paragraphs[1].style.table_cell = Some(crate::model::TableCell { table: 0, row: 0, col: 0 });
    doc.paragraphs[2].style.table_cell = Some(crate::model::TableCell { table: 0, row: 0, col: 1 });
    match &insert(&doc, 2)[0] {
        Op::SetParagraphs { para, .. } => assert_eq!(*para, 1, "before the table"),
        op => panic!("{op:?}"),
    }
}
