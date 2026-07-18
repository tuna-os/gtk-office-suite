// DOCX round-trip tests — the beginning of the Word-fidelity ratchet.
// Every assertion here is a feature a Word-like processor must not lose
// on save/reload. Cases are modeled on LibreOffice Writer's ooxmlexport
// test taxonomy (assertions rewritten against our model; no LO data files).

use letters_core::docx;
use letters_core::model::*;

fn round_trip(doc: &Document) -> Document {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.docx");
    let path = path.to_str().unwrap();
    docx::write(doc, path).expect("write docx");
    docx::read(path).expect("read docx")
}

#[test]
fn plain_paragraphs_survive() {
    let d = Document::from_plain_text("first paragraph\nsecond paragraph");
    let rt = round_trip(&d);
    assert_eq!(rt.to_plain_text(), "first paragraph\nsecond paragraph");
}

#[test]
fn bold_run_survives() {
    let mut d = Document::from_plain_text("normal bold normal");
    d.apply_run_style(7, 11, &StylePatch::set_bold(true));
    let rt = round_trip(&d);
    assert_eq!(rt.to_plain_text(), "normal bold normal");
    assert!(rt.style_at(7).bold, "bold lost in round trip");
    assert!(!rt.style_at(0).bold);
    assert!(!rt.style_at(12).bold);
}

#[test]
fn italic_underline_strike_survive() {
    let mut d = Document::from_plain_text("abc def ghi");
    d.apply_run_style(0, 3, &StylePatch::set_italic(true));
    d.apply_run_style(4, 7, &StylePatch::set_underline(true));
    d.apply_run_style(8, 11, &StylePatch::set_strikethrough(true));
    let rt = round_trip(&d);
    assert!(rt.style_at(0).italic);
    assert!(rt.style_at(4).underline);
    assert!(rt.style_at(8).strikethrough);
}

#[test]
fn combined_styles_survive() {
    let mut d = Document::from_plain_text("all");
    d.apply_run_style(0, 3, &StylePatch::set_bold(true));
    d.apply_run_style(0, 3, &StylePatch::set_italic(true));
    d.apply_run_style(0, 3, &StylePatch::set_underline(true));
    let rt = round_trip(&d);
    let s = rt.style_at(1);
    assert!(s.bold && s.italic && s.underline);
}

#[test]
fn heading_levels_survive() {
    let mut d = Document::from_plain_text("Title\nSection\nbody");
    d.set_heading(0, Some(1));
    d.set_heading(1, Some(2));
    let rt = round_trip(&d);
    assert_eq!(rt.paragraphs[0].style.heading, Some(1));
    assert_eq!(rt.paragraphs[1].style.heading, Some(2));
    assert_eq!(rt.paragraphs[2].style.heading, None);
}

#[test]
fn empty_paragraph_between_text_survives() {
    let d = Document::from_plain_text("above\n\nbelow");
    let rt = round_trip(&d);
    assert_eq!(rt.to_plain_text(), "above\n\nbelow");
    assert_eq!(rt.paragraphs.len(), 3);
}

#[test]
fn unicode_text_survives() {
    let d = Document::from_plain_text("héllo wörld — “quotes” 中文");
    let rt = round_trip(&d);
    assert_eq!(rt.to_plain_text(), "héllo wörld — “quotes” 中文");
}

#[test]
fn style_boundaries_are_exact() {
    // Word processors classically off-by-one run boundaries.
    let mut d = Document::from_plain_text("aaabbbccc");
    d.apply_run_style(3, 6, &StylePatch::set_bold(true));
    let rt = round_trip(&d);
    assert!(!rt.style_at(2).bold);
    assert!(rt.style_at(3).bold);
    assert!(rt.style_at(5).bold);
    assert!(!rt.style_at(6).bold);
}
