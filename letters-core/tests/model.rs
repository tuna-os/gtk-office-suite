// Model invariant and operation tests — written before the implementation
// (TDD). These define the letters-core contract; see DESIGN.md.

use letters_core::model::*;

fn doc(text: &str) -> Document {
    Document::from_plain_text(text)
}

fn assert_invariants(d: &Document) {
    assert!(!d.paragraphs.is_empty(), "document must have >= 1 paragraph");
    for p in &d.paragraphs {
        for r in &p.runs {
            assert!(!r.text.is_empty(), "runs must never be empty");
        }
        for w in p.runs.windows(2) {
            assert_ne!(w[0].style, w[1].style, "adjacent equal-style runs must be merged");
        }
    }
}

// ── Construction ────────────────────────────────────────────────────────

#[test]
fn new_document_has_one_empty_paragraph() {
    let d = Document::new();
    assert_eq!(d.paragraphs.len(), 1);
    assert!(d.paragraphs[0].runs.is_empty());
    assert_eq!(d.to_plain_text(), "");
    assert_eq!(d.char_len(), 0);
    assert_invariants(&d);
}

#[test]
fn plain_text_round_trip() {
    let text = "hello world\nsecond line\n\nfourth line";
    let d = doc(text);
    assert_eq!(d.paragraphs.len(), 4);
    assert_eq!(d.to_plain_text(), text);
    assert_invariants(&d);
}

#[test]
fn char_len_counts_newlines_as_one() {
    let d = doc("ab\ncd");
    assert_eq!(d.char_len(), 5);
}

#[test]
fn char_len_is_unicode_scalars_not_bytes() {
    let d = doc("héllo"); // é is 2 bytes, 1 char
    assert_eq!(d.char_len(), 5);
}

// ── insert_text ─────────────────────────────────────────────────────────

#[test]
fn insert_into_empty_document() {
    let mut d = Document::new();
    d.insert_text(0, "hi");
    assert_eq!(d.to_plain_text(), "hi");
    assert_invariants(&d);
}

#[test]
fn insert_in_middle_of_run() {
    let mut d = doc("hello");
    d.insert_text(3, "XY");
    assert_eq!(d.to_plain_text(), "helXYlo");
    assert_invariants(&d);
}

#[test]
fn insert_newline_splits_paragraph() {
    let mut d = doc("hello world");
    d.insert_text(5, "\n");
    assert_eq!(d.paragraphs.len(), 2);
    assert_eq!(d.to_plain_text(), "hello\n world");
    assert_invariants(&d);
}

#[test]
fn insert_multiline_text() {
    let mut d = doc("ab");
    d.insert_text(1, "1\n2\n3");
    assert_eq!(d.to_plain_text(), "a1\n2\n3b");
    assert_eq!(d.paragraphs.len(), 3);
    assert_invariants(&d);
}

#[test]
fn inserted_text_inherits_style_at_point() {
    let mut d = doc("bold");
    d.apply_run_style(0, 4, &StylePatch::set_bold(true));
    d.insert_text(2, "XX");
    // whole thing should still be one bold run
    assert_eq!(d.paragraphs[0].runs.len(), 1);
    assert!(d.paragraphs[0].runs[0].style.bold);
    assert_eq!(d.to_plain_text(), "boXXld");
    assert_invariants(&d);
}

// ── delete_range ────────────────────────────────────────────────────────

#[test]
fn delete_within_run() {
    let mut d = doc("hello world");
    d.delete_range(5, 11);
    assert_eq!(d.to_plain_text(), "hello");
    assert_invariants(&d);
}

#[test]
fn delete_across_paragraph_boundary_merges() {
    let mut d = doc("hello\nworld");
    d.delete_range(5, 6); // the newline
    assert_eq!(d.paragraphs.len(), 1);
    assert_eq!(d.to_plain_text(), "helloworld");
    assert_invariants(&d);
}

#[test]
fn delete_spanning_multiple_paragraphs() {
    let mut d = doc("one\ntwo\nthree");
    d.delete_range(2, 10); // "e\ntwo\nth"
    assert_eq!(d.to_plain_text(), "onree");
    assert_eq!(d.paragraphs.len(), 1);
    assert_invariants(&d);
}

#[test]
fn delete_everything_leaves_one_empty_paragraph() {
    let mut d = doc("a\nb\nc");
    d.delete_range(0, d.char_len());
    assert_eq!(d.paragraphs.len(), 1);
    assert_eq!(d.to_plain_text(), "");
    assert_invariants(&d);
}

#[test]
fn delete_merges_compatible_runs() {
    let mut d = doc("aXb");
    d.apply_run_style(1, 2, &StylePatch::set_bold(true));
    d.delete_range(1, 2); // remove the bold X; "a" and "b" should merge
    assert_eq!(d.paragraphs[0].runs.len(), 1);
    assert_invariants(&d);
}

// ── run styles ──────────────────────────────────────────────────────────

#[test]
fn apply_bold_to_middle_splits_runs() {
    let mut d = doc("hello world");
    d.apply_run_style(6, 11, &StylePatch::set_bold(true));
    let runs = &d.paragraphs[0].runs;
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].text, "hello ");
    assert!(!runs[0].style.bold);
    assert_eq!(runs[1].text, "world");
    assert!(runs[1].style.bold);
    assert_invariants(&d);
}

#[test]
fn styles_stack_independently() {
    let mut d = doc("abcdef");
    d.apply_run_style(0, 4, &StylePatch::set_bold(true));
    d.apply_run_style(2, 6, &StylePatch::set_italic(true));
    assert!(d.style_at(1).bold && !d.style_at(1).italic);
    assert!(d.style_at(3).bold && d.style_at(3).italic);
    assert!(!d.style_at(5).bold && d.style_at(5).italic);
    assert_invariants(&d);
}

#[test]
fn unset_bold_over_styled_range() {
    let mut d = doc("abc");
    d.apply_run_style(0, 3, &StylePatch::set_bold(true));
    d.apply_run_style(1, 2, &StylePatch::set_bold(false));
    assert!(d.style_at(0).bold);
    assert!(!d.style_at(1).bold);
    assert!(d.style_at(2).bold);
    assert_invariants(&d);
}

#[test]
fn style_range_spanning_paragraphs() {
    let mut d = doc("one\ntwo");
    d.apply_run_style(2, 5, &StylePatch::set_italic(true)); // "e" + "t"
    assert!(d.style_at(2).italic);
    assert!(d.style_at(4).italic);
    assert!(!d.style_at(0).italic);
    assert!(!d.style_at(6).italic);
    assert_invariants(&d);
}

#[test]
fn link_style_carries_url() {
    let mut d = doc("click here");
    d.apply_run_style(6, 10, &StylePatch::set_link(Some("https://gnome.org".into())));
    assert_eq!(d.style_at(6).link.as_deref(), Some("https://gnome.org"));
    assert_eq!(d.style_at(0).link, None);
    assert_invariants(&d);
}

// ── paragraph styles ────────────────────────────────────────────────────

#[test]
fn set_heading_on_paragraph() {
    let mut d = doc("Title\nbody");
    d.set_heading(0, Some(1));
    assert_eq!(d.paragraphs[0].style.heading, Some(1));
    assert_eq!(d.paragraphs[1].style.heading, None);
}

#[test]
fn set_list_and_alignment() {
    let mut d = doc("item");
    d.paragraphs[0].style.list = ListKind::Bullet;
    d.paragraphs[0].style.alignment = Alignment::Center;
    assert_eq!(d.paragraphs[0].style.list, ListKind::Bullet);
    assert_eq!(d.paragraphs[0].style.alignment, Alignment::Center);
}

#[test]
fn paragraph_split_preserves_para_style() {
    let mut d = doc("heading text");
    d.set_heading(0, Some(2));
    d.insert_text(7, "\n");
    assert_eq!(d.paragraphs.len(), 2);
    assert_eq!(d.paragraphs[0].style.heading, Some(2));
    assert_eq!(d.paragraphs[1].style.heading, Some(2));
}
