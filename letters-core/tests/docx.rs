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
fn alignment_survives() {
    let mut d = Document::from_plain_text("centered\nrighted\njustified\nplain");
    d.paragraphs[0].style.alignment = Alignment::Center;
    d.paragraphs[1].style.alignment = Alignment::Right;
    d.paragraphs[2].style.alignment = Alignment::Justify;
    let rt = round_trip(&d);
    assert_eq!(rt.paragraphs[0].style.alignment, Alignment::Center);
    assert_eq!(rt.paragraphs[1].style.alignment, Alignment::Right);
    assert_eq!(rt.paragraphs[2].style.alignment, Alignment::Justify);
    assert_eq!(rt.paragraphs[3].style.alignment, Alignment::Left);
}

// ── RED: known fidelity gaps, kept visible until fixed ─────────────────
// Run with `cargo test -- --ignored` to see the guiding set.

#[test]
#[ignore = "RED: rdocx has no numbering read getter — list kind is lost on reload (fix upstream in hanthor/rdocx)"]
fn list_kind_survives() {
    let mut d = Document::from_plain_text("first item\nsecond item");
    d.paragraphs[0].style.list = ListKind::Bullet;
    d.paragraphs[1].style.list = ListKind::Numbered;
    let rt = round_trip(&d);
    assert_eq!(rt.paragraphs[0].style.list, ListKind::Bullet);
    assert_eq!(rt.paragraphs[1].style.list, ListKind::Numbered);
}

#[test]
#[ignore = "RED: rdocx has no highlight read getter on RunRef (fix upstream in hanthor/rdocx)"]
fn highlight_survives() {
    let mut d = Document::from_plain_text("glow");
    d.apply_run_style(0, 4, &StylePatch::set_highlight(true));
    let rt = round_trip(&d);
    assert!(rt.style_at(0).highlight);
}

#[test]
#[ignore = "RED: rdocx has no hyperlink write API — links are dropped on save (fix upstream in hanthor/rdocx)"]
fn link_survives() {
    let mut d = Document::from_plain_text("click here");
    d.apply_run_style(6, 10, &StylePatch::set_link(Some("https://gnome.org".into())));
    let rt = round_trip(&d);
    assert_eq!(rt.style_at(6).link.as_deref(), Some("https://gnome.org"));
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
