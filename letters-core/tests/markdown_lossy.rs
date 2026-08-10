// markdown::lossy_features — export-loss warning coverage.
//
// `lossy_features` is the public API the UI calls to warn the user which
// formatting cannot survive a Markdown export (see DESIGN.md's documented
// lossiness). It previously had zero tests despite being UI-facing. These
// tests pin down both *which* features are reported and the exact reporting
// order the UI depends on.

use letters_core::markdown;
use letters_core::model::*;

fn doc_with(paragraphs: Vec<Paragraph>) -> Document {
    Document { paragraphs, footnotes: vec![], header: None, footer: None, page: None }
}

fn styled_run(text: &str, style: RunStyle) -> Run {
    Run { text: text.to_string(), style }
}

#[test]
fn plain_document_loses_nothing() {
    let d = Document::from_plain_text("hello world");
    assert!(markdown::lossy_features(&d).is_empty());
}

#[test]
fn empty_document_loses_nothing() {
    let d = Document::new();
    assert!(markdown::lossy_features(&d).is_empty());
}

#[test]
fn markdown_safe_features_are_not_reported() {
    // Bold, italic, strikethrough and code all survive a Markdown round-trip.
    let mut d = Document::from_plain_text("hello world");
    d.apply_run_style(0, 5, &StylePatch::set_bold(true));
    d.apply_run_style(6, 11, &StylePatch::set_italic(true));
    d.apply_run_style(0, 11, &StylePatch::set_strikethrough(true));
    d.apply_run_style(0, 11, &StylePatch::set_code(true));
    assert!(markdown::lossy_features(&d).is_empty());
}

#[test]
fn highlight_is_reported_lost() {
    let mut d = Document::from_plain_text("hi");
    d.apply_run_style(0, 2, &StylePatch::set_highlight(true));
    assert_eq!(markdown::lossy_features(&d), vec!["highlight"]);
}

#[test]
fn underline_is_reported_lost() {
    let mut d = Document::from_plain_text("hi");
    d.apply_run_style(0, 2, &StylePatch::set_underline(true));
    assert_eq!(markdown::lossy_features(&d), vec!["underline"]);
}

#[test]
fn alignment_is_reported_lost() {
    let d = doc_with(vec![Paragraph {
        style: ParaStyle { alignment: Alignment::Center, ..Default::default() },
        runs: vec![Run::plain("centered")],
    }]);
    assert_eq!(markdown::lossy_features(&d), vec!["alignment"]);
}

#[test]
fn line_spacing_is_reported_lost() {
    let d = doc_with(vec![Paragraph {
        style: ParaStyle { line_spacing: 1.5, ..Default::default() },
        runs: vec![Run::plain("wide")],
    }]);
    assert_eq!(markdown::lossy_features(&d), vec!["line spacing"]);
}

#[test]
fn font_size_is_reported_lost() {
    let d = doc_with(vec![Paragraph {
        style: ParaStyle::default(),
        runs: vec![styled_run("big", RunStyle { font_size_hp: Some(1200), ..Default::default() })],
    }]);
    assert_eq!(markdown::lossy_features(&d), vec!["font size"]);
}

#[test]
fn text_color_is_reported_lost() {
    let d = doc_with(vec![Paragraph {
        style: ParaStyle::default(),
        runs: vec![styled_run("red", RunStyle { color: Some("#ff0000".to_string()), ..Default::default() })],
    }]);
    assert_eq!(markdown::lossy_features(&d), vec!["text color"]);
}

#[test]
fn vert_align_is_reported_lost() {
    let d = doc_with(vec![Paragraph {
        style: ParaStyle::default(),
        runs: vec![styled_run("x", RunStyle { vert_align: Some(VertAlign::Superscript), ..Default::default() })],
    }]);
    assert_eq!(markdown::lossy_features(&d), vec!["superscript/subscript"]);
}

#[test]
fn combined_features_report_in_stable_order() {
    // The UI renders the warning as a list; the order is part of the contract.
    let mut d = Document::from_plain_text("mixed");
    d.apply_run_style(0, 5, &StylePatch::set_highlight(true));
    d.apply_run_style(0, 5, &StylePatch::set_underline(true));
    d.paragraphs[0].style.alignment = Alignment::Right;
    d.paragraphs[0].style.line_spacing = 1.25;
    d.paragraphs[0].runs[0].style.font_size_hp = Some(1400);
    d.paragraphs[0].runs[0].style.color = Some("#00ff00".to_string());
    d.paragraphs[0].runs[0].style.vert_align = Some(VertAlign::Subscript);
    assert_eq!(
        markdown::lossy_features(&d),
        vec!["highlight", "underline", "alignment", "line spacing", "font size", "text color", "superscript/subscript"]
    );
}

#[test]
fn single_lossy_run_in_long_document_is_reported() {
    // Only one offending run is enough to warn on export.
    let mut d = Document::from_plain_text("line one\nline two\nline three");
    d.apply_run_style(5, 8, &StylePatch::set_highlight(true));
    assert_eq!(markdown::lossy_features(&d), vec!["highlight"]);
}
