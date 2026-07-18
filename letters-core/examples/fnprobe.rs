fn main() {
    use letters_core::model::*;
    let mut doc = Document::new();
    doc.paragraphs[0].runs = vec![
        Run { text: "Body".into(), style: RunStyle::default() },
        Run { text: String::new(), style: RunStyle { footnote: Some(0), ..Default::default() } },
    ];
    doc.footnotes = vec!["Note".into()];
    letters_core::docx::write(&doc, "/tmp/fnprobe.docx").unwrap();
}
