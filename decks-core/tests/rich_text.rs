// rich_text.rs — DCK-007: caret and range editing of a text box through
// DecksController, as the native editor drives it, and what both formats
// keep of the result.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// - Random sessions (typing, deleting, range and caret formats, the
//   session's own undo and redo) keep the text/runs/paragraph invariants
//   at every step, commit as exactly one document step, and cancel
//   without a trace.
// - A mixed-style paragraph formatted by range, with a new paragraph,
//   saves and reopens with those spans in PPTX and ODP, across formats,
//   and an imported box's unaffected runs survive an edit elsewhere.

use decks_core::engine::{Bullet, ParaAlign, ParaStyle, Run, RunStyle, SlideObject, TextBody};
use decks_core::format::{FormatEdit, ListKind};
use decks_core::rich_text::RichText;
use decks_core::{Deck, DecksController};
use proptest::prelude::*;

fn bold() -> RunStyle {
    RunStyle { bold: true, ..Default::default() }
}

/// A slide holding one text box: "Alpha Beta Gamma" in mixed runs.
fn deck_with_mixed_box() -> Deck {
    let mut d = Deck::new();
    let runs = vec![
        Run { text: "Alpha ".into(), style: bold() },
        Run { text: "Beta".into(), style: RunStyle::default() },
        Run { text: " Gamma".into(), style: RunStyle { italic: true, ..Default::default() } },
    ];
    d.slides[0].objects = vec![SlideObject::TextBox {
        text: "Alpha Beta Gamma".into(),
        x: 60.0,
        y: 60.0,
        w: 600.0,
        h: 200.0,
        rotation: 0.0,
        runs,
        body: TextBody::default(),
    }];
    d
}

fn controller(d: Deck) -> DecksController {
    DecksController::new(d.slides, d.masters)
}

fn text_of(c: &DecksController) -> RichText {
    RichText::of(&c.slides.borrow()[0].objects[0]).expect("a text box")
}

type Span = (String, bool, bool, bool, bool);

/// The runs as (text, bold, italic, underline, strike), equal neighbours
/// merged: what a reader must give back, whatever sizes, resolved theme
/// colours or language tags the writer and reader add.
fn spans(rt: &RichText) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    for r in &rt.runs {
        let s = &r.style;
        let key = (s.bold, s.italic, s.underline, s.strikethrough);
        match out.last_mut() {
            Some(last) if (last.1, last.2, last.3, last.4) == key => last.0.push_str(&r.text),
            _ => out.push((r.text.clone(), key.0, key.1, key.2, key.3)),
        }
    }
    out
}

/// The colour of the run holding `word`.
fn colour_of(rt: &RichText, word: &str) -> Option<String> {
    rt.runs.iter().find(|r| r.text.contains(word)).and_then(|r| r.style.color.as_ref()).map(|c| c.to_lowercase())
}

/// What the editor journey does: select "Beta" and bold, underline and
/// colour it; strike "Gamma"; insert a break before "Gamma" and make
/// that second paragraph a centred bulleted item.
fn edit_like_the_journey(c: &DecksController) {
    let (s, o) = (c.slide_id_at(0).unwrap(), c.object_id_at(0, 0).unwrap());
    assert!(c.begin_text_session(s, o));
    c.edit_text_session(|t| {
        t.set_range(6, 10).unwrap();
        assert!(t.format(&FormatEdit::Bold(true)));
        assert!(t.format(&FormatEdit::Underline(true)));
        assert!(t.format(&FormatEdit::TextColor(Some("c00000".into()))));
        t.set_range(11, 16).unwrap();
        assert!(t.format(&FormatEdit::Strikethrough(true)));
        t.set_range(10, 11).unwrap();
        t.replace(10, 11, "\n").unwrap();
        assert!(t.format(&FormatEdit::List(ListKind::Bullet)));
        assert!(t.format(&FormatEdit::Align(ParaAlign::Center)));
    })
    .unwrap();
    assert_eq!(c.commit_text_session(), Ok(true));
}

fn expected_spans() -> Vec<Span> {
    vec![
        ("Alpha ".into(), true, false, false, false),
        ("Beta".into(), true, false, true, false),
        ("\n".into(), false, true, false, false),
        ("Gamma".into(), false, true, false, true),
    ]
}

fn check_reopened(rt: &RichText, what: &str) {
    assert_eq!(rt.text, "Alpha Beta\nGamma", "{what}");
    // The break may be written with either neighbour's style; compare the
    // visible text of each span.
    let visible = |v: Vec<Span>| -> Vec<Span> {
        v.into_iter().map(|(t, b, i, u, s)| (t.trim_matches('\n').to_string(), b, i, u, s)).filter(|s| !s.0.is_empty()).collect()
    };
    assert_eq!(visible(spans(rt)), visible(expected_spans()), "{what}: {:?}", rt.runs);
    assert_eq!(colour_of(rt, "Beta").as_deref(), Some("c00000"), "{what}");
    assert_ne!(colour_of(rt, "Gamma").as_deref(), Some("c00000"), "{what}: the colour stayed on its range");
    let p0 = rt.body.para(0);
    let p1 = rt.body.para(1);
    assert_eq!(p0.bullet, Bullet::None, "{what}: the first paragraph is no list item");
    assert_eq!(p0.align, ParaAlign::Left, "{what}");
    assert!(matches!(p1.bullet, Bullet::Char(_)), "{what}: {p1:?}");
    assert_eq!(p1.align, ParaAlign::Center, "{what}");
}

#[test]
fn a_range_formatted_box_saves_and_reopens_in_both_formats_and_across_them() {
    let c = controller(deck_with_mixed_box());
    edit_like_the_journey(&c);
    let edited = text_of(&c);
    assert_eq!(spans(&edited), expected_spans());
    assert_eq!(colour_of(&edited, "Beta").as_deref(), Some("c00000"));
    edited.check().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let deck = c.deck();
    for ext in ["pptx", "odp"] {
        let path = dir.path().join(format!("edited.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &deck).unwrap();
        let back = decks_core::read_deck(path.to_str().unwrap()).unwrap();
        let rt = RichText::of(&back.slides[0].objects[0]).expect("a text box");
        check_reopened(&rt, ext);
        // And across: read in one format, written in the other.
        let other = if ext == "pptx" { "odp" } else { "pptx" };
        let cross = dir.path().join(format!("from-{ext}.{other}"));
        decks_core::write_deck(cross.to_str().unwrap(), &back).unwrap();
        let again = decks_core::read_deck(cross.to_str().unwrap()).unwrap();
        check_reopened(&RichText::of(&again.slides[0].objects[0]).unwrap(), &format!("{ext} -> {other}"));
    }
}

#[test]
fn an_imported_boxs_unaffected_runs_survive_an_edit_elsewhere_and_a_save() {
    let dir = tempfile::tempdir().unwrap();
    for ext in ["pptx", "odp"] {
        // Imported: written by the app, read back as a foreign file is.
        let path = dir.path().join(format!("imported.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &deck_with_mixed_box()).unwrap();
        let opened = decks_core::read_deck(path.to_str().unwrap()).unwrap();
        let before = RichText::of(&opened.slides[0].objects[0]).unwrap();
        let c = controller(opened);
        let (s, o) = (c.slide_id_at(0).unwrap(), c.object_id_at(0, 0).unwrap());
        // Type at the very end of the box: every existing run is untouched.
        assert!(c.replace_text_range(s, o, before.text.len(), before.text.len(), "!"));
        let after = text_of(&c);
        assert_eq!(&after.runs[..before.runs.len() - 1], &before.runs[..before.runs.len() - 1], "{ext}");
        let out = dir.path().join(format!("resaved.{ext}"));
        decks_core::write_deck(out.to_str().unwrap(), &c.deck()).unwrap();
        let back = RichText::of(&decks_core::read_deck(out.to_str().unwrap()).unwrap().slides[0].objects[0]).unwrap();
        assert_eq!(back.text, "Alpha Beta Gamma!", "{ext}");
        let mut want = spans(&before);
        want.last_mut().unwrap().0.push('!');
        assert_eq!(spans(&back), want, "{ext}: {:?}", back.runs);
    }
}

#[test]
fn text_typed_into_a_blank_box_stays_editable_after_a_save() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = Deck::new();
    d.slides[0].objects = vec![SlideObject::TextBox { text: String::new(), x: 0.0, y: 0.0, w: 300.0, h: 100.0, rotation: 0.0, runs: vec![], body: TextBody::default() }];
    let c = controller(d);
    let (s, o) = (c.slide_id_at(0).unwrap(), c.object_id_at(0, 0).unwrap());
    assert!(c.begin_text_session(s, o));
    c.edit_text_session(|t| {
        assert!(t.format(&FormatEdit::Bold(true)), "a caret format sets the typing style");
        t.replace(0, 0, "Typed").unwrap();
        assert!(t.format(&FormatEdit::Bold(false)));
        t.replace(5, 5, " plain").unwrap();
    })
    .unwrap();
    assert_eq!(c.commit_text_session(), Ok(true));
    for ext in ["pptx", "odp"] {
        let path = dir.path().join(format!("blank.{ext}"));
        decks_core::write_deck(path.to_str().unwrap(), &c.deck()).unwrap();
        let back = decks_core::read_deck(path.to_str().unwrap()).unwrap();
        let rt = RichText::of(&back.slides[0].objects[0]).unwrap();
        assert_eq!(spans(&rt), vec![("Typed".into(), true, false, false, false), (" plain".into(), false, false, false, false)], "{ext}");
        // Reopened, it is still a box the editor can open and change.
        let again = controller(back);
        let (s, o) = (again.slide_id_at(0).unwrap(), again.object_id_at(0, 0).unwrap());
        assert!(again.format_text_range(s, o, 0, 5, &FormatEdit::Italic(true)), "{ext}");
    }
}

#[derive(Clone, Debug)]
enum Op {
    Caret(usize, usize),
    Type(String),
    Delete,
    Format(u8, bool),
    Para(u8),
    Undo,
    Redo,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0usize..40, 0usize..40).prop_map(|(a, b)| Op::Caret(a, b)),
        prop::sample::select(vec!["a", "Zz", " ", "\n", "é", "e\u{301}", "😀", "東京", "שלום 12", "\n\n"]).prop_map(|s| Op::Type(s.to_string())),
        Just(Op::Delete),
        (0u8..5, any::<bool>()).prop_map(|(k, on)| Op::Format(k, on)),
        (0u8..4).prop_map(Op::Para),
        Just(Op::Undo),
        Just(Op::Redo),
    ]
}

/// The nearest scalar boundary at or before byte `at` of `text`.
fn boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn random_sessions_keep_the_invariants_commit_one_step_and_cancel_cleanly(ops in prop::collection::vec(op(), 1..40), cancel in any::<bool>()) {
        let mut d = deck_with_mixed_box();
        if let SlideObject::TextBox { text, runs, body, .. } = &mut d.slides[0].objects[0] {
            *text = "Alpha Beta Gamma\nSecond".into();
            runs.last_mut().unwrap().text.push_str("\nSecond");
            body.paras = vec![ParaStyle { align: ParaAlign::Center, ..Default::default() }, ParaStyle::default()];
        }
        let c = controller(d);
        let (s, o) = (c.slide_id_at(0).unwrap(), c.object_id_at(0, 0).unwrap());
        let before = text_of(&c);
        let (revision, dirty) = (c.revision(), c.dirty.get());
        prop_assert!(c.begin_text_session(s, o));
        for op in &ops {
            c.edit_text_session(|t| {
                let len = t.working().text.len();
                match op {
                    Op::Caret(a, b) => {
                        let (a, b) = (boundary(&t.working().text, (*a).min(len)), boundary(&t.working().text, (*b).min(len)));
                        t.set_range(a.min(b), a.max(b)).unwrap();
                    }
                    Op::Type(x) => {
                        let (a, b) = t.range();
                        t.replace(a, b, x).unwrap();
                    }
                    Op::Delete => {
                        let (a, b) = t.range();
                        t.replace(a, b, "").unwrap();
                    }
                    Op::Format(k, on) => {
                        t.format(&match k {
                            0 => FormatEdit::Bold(*on),
                            1 => FormatEdit::Italic(*on),
                            2 => FormatEdit::Underline(*on),
                            3 => FormatEdit::TextColor(on.then(|| "4472c4".to_string())),
                            _ => FormatEdit::FontSize(if *on { 32.0 } else { 18.0 }),
                        });
                    }
                    Op::Para(k) => {
                        t.format(&match k {
                            0 => FormatEdit::Align(ParaAlign::Right),
                            1 => FormatEdit::List(ListKind::Number),
                            2 => FormatEdit::Indent(true),
                            _ => FormatEdit::SpaceBefore(6.0),
                        });
                    }
                    Op::Undo => {
                        t.undo();
                    }
                    Op::Redo => {
                        t.redo();
                    }
                }
                t.working().check().unwrap();
                let (a, b) = t.range();
                assert!(a <= b && b <= t.working().text.len() && t.working().text.is_char_boundary(a) && t.working().text.is_char_boundary(b));
            }).unwrap();
            prop_assert_eq!(text_of(&c), before.clone(), "the session never writes the document");
        }
        let working = c.text_session(|t| t.working().clone()).unwrap();
        if cancel {
            prop_assert!(c.cancel_text_session());
            prop_assert_eq!(text_of(&c), before);
            prop_assert_eq!((c.revision(), c.dirty.get()), (revision, dirty));
        } else {
            let changed = working != before;
            prop_assert_eq!(c.commit_text_session(), Ok(changed));
            prop_assert_eq!(text_of(&c), working.clone());
            if changed {
                prop_assert!(c.undo());
                prop_assert_eq!(text_of(&c), before);
                prop_assert!(!c.undo(), "exactly one step");
                prop_assert!(c.redo());
                prop_assert_eq!(text_of(&c), working);
            }
        }
    }
}
