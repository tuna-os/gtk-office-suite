use super::*;
use suite_common::gtk_test::run as gtk_test;

/// A buffer showing `doc`, with a live model attached (as a tab builds it).
fn tab(doc: &Document) -> (gtk::TextBuffer, Rc<RefCell<LiveModel>>) {
    let buf = gtk::TextBuffer::new(None);
    crate::actions::register_formatting_tags(&buf);
    let live = LiveModel::attach(&buf, doc.clone());
    (buf, live)
}

/// The buffer's text and, for each char, the names of its tags.
fn shown(buf: &gtk::TextBuffer) -> (String, Vec<Vec<String>>) {
    let mut tags = Vec::new();
    let mut it = buf.start_iter();
    while !it.is_end() {
        let mut names: Vec<String> = it.tags().iter().filter_map(|t| t.name().map(|n| n.to_string())).collect();
        names.retain(|n| !PRESENTATION_TAGS.contains(&n.as_str()));
        names.sort();
        tags.push(names);
        it.forward_char();
    }
    (text(buf), tags)
}

/// The projection is exact: the buffer, and where each paragraph starts in
/// it, are what rendering the model's document from scratch gives, and
/// nothing but the model has edited the buffer.
fn check(buf: &gtk::TextBuffer, live: &Rc<RefCell<LiveModel>>, what: &str) {
    let (doc, starts) = live.borrow().snapshot();
    let fresh = gtk::TextBuffer::new(Some(&buf.tag_table()));
    let want_starts = crate::bridge::render_to_buffer(&doc, &fresh);
    let (got, want) = (shown(buf), shown(&fresh));
    assert_eq!(got.0, want.0, "the buffer's text after {what}");
    if let Some(i) = got.1.iter().zip(&want.1).position(|(a, b)| a != b) {
        let around: String = got.0.chars().skip(i.saturating_sub(10)).take(20).collect();
        panic!("tags at char {i} ({around:?}) after {what}: {:?}, rendering gives {:?}", got.1[i], want.1[i]);
    }
    assert_eq!(starts, want_starts, "starts after {what}");
    assert_eq!(live.borrow().foreign_edits, 0, "the buffer was edited behind the model's back by {what}");
}

fn text(buf: &gtk::TextBuffer) -> String {
    buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string()
}

fn at(buf: &gtk::TextBuffer, needle: &str) -> i32 {
    let t = text(buf);
    t[..t.find(needle).unwrap()].chars().count() as i32
}

/// Type `text` at buffer offset `off`, as the page view does.
fn type_at(buf: &gtk::TextBuffer, off: i32, text: &str) {
    buf.place_cursor(&buf.iter_at_offset(off));
    crate::page_edit::type_text(buf, text);
}

/// Select buffer offsets `a..b`.
fn select(buf: &gtk::TextBuffer, a: i32, b: i32) {
    buf.select_range(&buf.iter_at_offset(a), &buf.iter_at_offset(b));
}

fn sample() -> Document {
    let mut d = Document::from_plain_text("Title\nfirst item\nsecond item\nbody text here");
    d.paragraphs[0].style.heading = Some(1);
    for p in &mut d.paragraphs[1..3] {
        p.style.list = letters_core::ListKind::Numbered;
    }
    d
}

/// Nothing but the model edits the buffer: an edit made to it directly is
/// counted, and the document does not have it. (The model used to follow
/// such edits by reading the buffer back.)
#[test]
fn an_edit_behind_the_models_back_is_counted_not_followed() {
    gtk_test(|| {
        let (buf, live) = tab(&sample());
        check(&buf, &live, "loading");
        let mut end = buf.end_iter();
        buf.insert(&mut end, "stray");
        assert_eq!(live.borrow().foreign_edits, 1);
        assert_eq!(live.borrow().document(), &sample(), "the model did not follow the buffer");
    });
}

/// What the buffer could never hold, a document opened in a tab still has:
/// a table cell's own spacing and alignment, a line break inside a
/// paragraph, a cell of two paragraphs, and text that reads like a list
/// marker. Opening used to read the document back out of the buffer, so
/// 21 of 61 real and fixture documents changed on open, and seven of them
/// gained or lost paragraphs (a table cell among them).
#[test]
fn a_document_opens_as_it_is() {
    use letters_core::{Alignment, Paragraph, Run};
    gtk_test(|| {
        let mut d = Document::from_plain_text("1. not a list item\nfirst line\nafter the table");
        d.paragraphs[1].runs = vec![Run::plain("first line\nsecond line")];
        let table = d.insert_table_at(2, 2, 2);
        let cell = |d: &Document, row: u32, col: u32| d.paragraphs.iter().position(|p| p.style.table_cell == Some(letters_core::TableCell { table, row, col })).unwrap();
        let c = cell(&d, 0, 0);
        d.paragraphs[c].runs = vec![Run::plain("centred")];
        d.paragraphs[c].style.alignment = Alignment::Center;
        d.paragraphs[c].style.space_after_pt = 10.0;
        d.paragraphs[c].style.line_spacing = 1.15;
        let c = cell(&d, 1, 0);
        d.paragraphs[c].runs = vec![Run::plain("one")];
        let mut second = d.paragraphs[c].clone();
        second.runs = vec![Run::plain("two")];
        d.paragraphs.insert(c + 1, second);
        let c = cell(&d, 1, 1);
        d.paragraphs[c].runs = vec![Run::plain("last cell")];

        let buf = gtk::TextBuffer::new(None);
        crate::actions::register_formatting_tags(&buf);
        crate::bridge::load_document(&d, &buf);
        let live = crate::live::of(&buf).unwrap();
        assert_eq!(live.borrow().document(), &d, "the opened document changed");
        check(&buf, &live, "opening");

        // Typing in the cell's second paragraph lands there.
        type_at(&buf, at(&buf, "two") + 3, "!");
        let doc = live.borrow().document().clone();
        let texts: Vec<String> = doc.paragraphs.iter().map(Paragraph::text).collect();
        assert!(texts.contains(&"two!".to_string()), "{texts:?}");
        assert!(texts.contains(&"last cell".to_string()), "{texts:?}");
        check(&buf, &live, "typing in a cell's second paragraph");
        // And after the soft line break.
        type_at(&buf, at(&buf, "second line"), ">");
        assert_eq!(live.borrow().document().paragraphs[1].text(), "first line\n>second line");
        check(&buf, &live, "typing after a line break");
    });
}

/// Seeded random edits anywhere, made the way the page view makes them —
/// typing (with Enter), deleting a selection, bold, italic and alignment —
/// in a document with a list and a table. After each, the buffer is
/// exactly a fresh rendering of the model.
#[test]
fn random_edits_keep_the_buffer_an_exact_projection() {
    gtk_test(|| {
        for seed in [0x2545_f491_4f6c_dd1du64, 0x9e37_79b9_7f4a_7c15, 0xdead_beef_cafe_f00d, 7, 12345] {
            let mut d = Document::from_plain_text("intro text\nitem one\nitem two\nclosing words");
            d.paragraphs[1].style.list = letters_core::ListKind::Bullet;
            d.paragraphs[2].style.list = letters_core::ListKind::Bullet;
            d.insert_table_at(3, 1, 2);
            let (buf, live) = tab(&d);
            let mut state = seed;
            let mut next = |n: u64| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state % n.max(1)
            };
            for step in 0..300 {
                let len = buf.char_count().max(0) as u64;
                let a = next(len + 1) as i32;
                match next(10) {
                    0..=4 => type_at(&buf, a, ["a", "bc", " ", "xyz", "\n", "|"][next(6) as usize]),
                    5..=6 => {
                        select(&buf, a, (a + next(4) as i32).min(len as i32));
                        crate::insert::delete_selection(&buf);
                    }
                    7 | 8 => {
                        select(&buf, a, (a + 5).min(len as i32));
                        crate::actions::toggle_tag_in(&buf, ["italic", "bold"][next(2) as usize]);
                    }
                    _ => {
                        buf.place_cursor(&buf.iter_at_offset(a));
                        crate::actions::align_in(&buf, ["align-right", "align-center", "align-left"][next(3) as usize]);
                    }
                }
                if next(3) == 0 {
                    check(&buf, &live, &format!("seed {seed} step {step}"));
                }
            }
            check(&buf, &live, &format!("seed {seed} end"));
        }
    });
}

/// Undo and redo come from the model's inverse ops: undoing every step
/// restores the loaded document, in the model and in the buffer; redoing
/// every step brings the edited one back.
#[test]
fn undo_and_redo_run_on_the_model_and_the_buffer_follows() {
    gtk_test(|| {
        let original = sample();
        let (buf, live) = tab(&original);
        let base = at(&buf, "body");
        for (i, c) in "Hello".chars().enumerate() {
            type_at(&buf, base + i as i32, &c.to_string());
        }
        type_at(&buf, at(&buf, "Title") + 5, "\n");
        select(&buf, at(&buf, "Title"), at(&buf, "Title") + 5);
        crate::actions::toggle_tag_in(&buf, "italic");
        // A structured edit: a table.
        buf.place_cursor(&buf.iter_at_offset(at(&buf, "Hellobody")));
        crate::bridge::apply_structured_edit(&buf, |ed| {
            ed.insert_table(2, 2);
        });
        check(&buf, &live, "the edits");
        let edited = live.borrow().document().clone();

        let mut steps = 0;
        while live.borrow().can_undo() {
            undo(&buf, false);
            check(&buf, &live, &format!("undo {steps}"));
            steps += 1;
        }
        assert_eq!(steps, 4, "table, italic, Enter, and 'Hello' as one typed word");
        assert!(!live.borrow().can_undo() && live.borrow().can_redo());
        assert_eq!(live.borrow().document(), &original, "undone to the loaded document");
        while live.borrow().can_redo() {
            undo(&buf, true);
            check(&buf, &live, "redo");
        }
        assert_eq!(live.borrow().document(), &edited, "redone to the edited document");
    });
}

/// Deleting the empty line between two tables leaves two tables (#1299),
/// and neither runs the second table's header into the first one's body.
#[test]
fn deleting_the_line_between_two_tables_keeps_them_two() {
    gtk_test(|| {
        let mut d = Document::from_plain_text("before\n\nafter");
        d.insert_table_at(2, 2, 2);
        d.insert_table_at(1, 2, 2);
        let (buf, live) = tab(&d);
        check(&buf, &live, "loading");
        let tables = |doc: &Document| doc.paragraphs.iter().filter_map(|p| p.style.table_cell.map(|c| c.table)).collect::<std::collections::BTreeSet<_>>().len();
        assert_eq!(tables(live.borrow().document()), 2, "two tables to begin with");
        // The empty line between them: the line after the first table's
        // last row.
        let t = text(&buf);
        let gap = t.find("|\n\n|").expect("an empty line between the tables") + 2;
        let at = t[..gap].chars().count() as i32;
        select(&buf, at, at + 1);
        crate::insert::delete_selection(&buf);
        check(&buf, &live, "deleting the empty line");
        let doc = live.borrow().document().clone();
        assert_eq!(tables(&doc), 2, "still two tables: {:?}", doc.paragraphs.iter().map(|p| p.text()).collect::<Vec<_>>());
    });
}

/// The page view edits the model first; the buffer shows the result.
#[test]
fn model_first_edits_reach_the_buffer() {
    gtk_test(|| {
        let (buf, live) = tab(&sample());
        let seq = live.borrow_mut().sequence_offset(at(&buf, "body") as usize);
        let op = {
            let m = live.borrow();
            letters_core::edit::typing(m.document(), seq, "new ").unwrap()
        };
        assert!(live.borrow_mut().apply_user_ops(&buf, &[op], false));
        assert!(text(&buf).contains("new body"), "{}", text(&buf));
        check(&buf, &live, "a model-first insert");
        // Enter in a list item continues the list, in the model and on screen.
        let seq = live.borrow_mut().sequence_offset((at(&buf, "first item") + 10) as usize);
        let op = {
            let m = live.borrow();
            letters_core::edit::typing(m.document(), seq, "\n").unwrap()
        };
        assert!(live.borrow_mut().apply_user_ops(&buf, &[op], false));
        check(&buf, &live, "a model-first Enter in a list");
        assert!(text(&buf).contains("first item\n2.\t\n3.\tsecond"), "{:?}", text(&buf));
        undo(&buf, false);
        undo(&buf, false);
        assert_eq!(live.borrow().document(), &sample());
        check(&buf, &live, "undoing both");
    });
}

/// CI performance gate: one keystroke on a 200-paragraph document — the
/// model edit, its projection into the buffer, and the Print Layout
/// relayout — within a fixed budget, re-shaping one paragraph only.
#[test]
fn a_keystroke_on_200_paragraphs_relays_out_within_budget() {
    use letters_core::layout::{pango::Typeset, LayoutOptions};
    use std::time::{Duration, Instant};
    gtk_test(|| {
        let text: Vec<String> = (0..200)
            .map(|i| format!("Paragraph {i}: the quick brown fox jumps over the lazy dog, twice over at least."))
            .collect();
        let (buf, live) = tab(&Document::from_plain_text(&text.join("\n")));
        let (doc, _) = live.borrow().snapshot();
        let mut typeset = Typeset::new(doc.clone(), LayoutOptions::default());

        let full = {
            let start = Instant::now();
            let _ = Typeset::new(doc, LayoutOptions::default());
            start.elapsed()
        };
        let base = at(&buf, "Paragraph 100:") + 4;
        let mut samples = Vec::new();
        for k in 0..21 {
            let start = Instant::now();
            type_at(&buf, base + k, "x");
            let (doc, _) = live.borrow().snapshot();
            let shaped = typeset.update(doc, LayoutOptions::default());
            samples.push(start.elapsed());
            assert_eq!(shaped, 1, "only the edited paragraph is shaped again");
        }
        samples.sort_unstable();
        let (median, p95) = (samples[10], samples[19]);
        eprintln!("keystroke relayout on 200 paragraphs: p95 {p95:?}, median {median:?}; full layout {full:?}");
        // Generous for an unoptimised build on a shared CI runner.
        const BUDGET: Duration = Duration::from_millis(100);
        assert!(p95 <= BUDGET, "keystroke p95 {p95:?} over {BUDGET:?}");
        assert!(median < full, "a keystroke must cost less than laying out from scratch ({median:?} vs {full:?})");
        check(&buf, &live, "typing");
    });
}

/// Table commands (insert a table, rows and columns, delete them), list
/// commands and page breaks run on the model as ops: the buffer matches,
/// and each is one undo step.
#[test]
fn structured_commands_are_model_ops() {
    use letters_core::ListKind;
    gtk_test(|| {
        let (buf, live) = tab(&sample());
        buf.place_cursor(&buf.iter_at_offset(at(&buf, "body")));
        crate::bridge::apply_structured_edit(&buf, |ed| {
            ed.insert_table(2, 2);
        });
        check(&buf, &live, "insert table");
        // The caret is in the new table's first cell: type there.
        crate::page_edit::type_text(&buf, "A1");
        check(&buf, &live, "typing in the new cell");
        for (what, cmd) in [
            ("row below", 0),
            ("column after", 1),
            ("delete row", 2),
            ("delete column", 3),
        ] {
            crate::bridge::apply_structured_edit(&buf, |ed| {
                let _ = match cmd {
                    0 => ed.insert_row_at_cursor(true),
                    1 => ed.insert_col_at_cursor(true),
                    2 => ed.delete_row_at_cursor(),
                    _ => ed.delete_col_at_cursor(),
                };
            });
            check(&buf, &live, what);
        }
        buf.place_cursor(&buf.iter_at_offset(at(&buf, "Title")));
        crate::bridge::apply_structured_edit(&buf, |ed| {
            ed.toggle_list_at_cursor(ListKind::Bullet);
        });
        check(&buf, &live, "a list toggled");
        crate::bridge::apply_structured_edit(&buf, |ed| {
            ed.toggle_page_break_at_cursor();
        });
        check(&buf, &live, "a page break");
        // Undo all of it, one command at a time.
        let mut steps = 0;
        while live.borrow().can_undo() {
            undo(&buf, false);
            check(&buf, &live, &format!("undo {steps}"));
            steps += 1;
        }
        assert_eq!(steps, 8, "each command and the typed word is one step");
        assert_eq!(live.borrow().document(), &sample());
    });
}

/// Markdown shortcuts: "_it_" and a space typed in Print Layout makes
/// italic text, and the model has it.
#[test]
fn markdown_shortcuts_make_formatting() {
    gtk_test(|| {
        let ctx = glib::MainContext::default();
        let settle = || while ctx.iteration(false) {};
        let (buf, live) = tab(&Document::from_plain_text("y"));
        let view = crate::page_view::PageView::new();
        crate::page_edit::make_editable(&view, &buf);
        buf.place_cursor(&buf.end_iter());
        for c in " _it_ ".chars() {
            crate::page_edit::type_text(&buf, &c.to_string());
        }
        settle();
        check(&buf, &live, "a Print Layout shortcut");
        let doc = live.borrow().snapshot().0;
        assert!(doc.paragraphs[0].runs.iter().any(|r| r.text == "it" && r.style.italic), "{:?}", doc.paragraphs[0].runs);
    });
}

/// A document with every field the model has set, for
/// `unrelated_edits_and_commands_leave_every_other_field_alone`. Its last
/// paragraph is the one the edits work in; every other paragraph, and
/// every document-level field, has to come back exactly as it went in.
fn every_field() -> Document {
    use letters_core::model::{BaseFont, PageGeometry, VertAlign};
    use letters_core::{Alignment, Comment, ListKind, ParaStyle, Paragraph, Run, RunStyle};
    let run = |text: &str, style: RunStyle| Run { text: text.into(), style };
    let para = |style: ParaStyle, runs: Vec<Run>| Paragraph { style, runs };
    let mut d = Document::from_plain_text("");
    d.paragraphs = vec![
        para(ParaStyle { heading: Some(1), ..Default::default() }, vec![run("A heading", RunStyle::default())]),
        para(ParaStyle { alignment: Alignment::Justify, line_spacing: 1.5, space_before_pt: 6.0, space_after_pt: 12.0,
            left_indent_pt: 18.0, right_indent_pt: 9.0, first_line_indent_pt: 12.0, tab_stops_pt: vec![72.0, 144.0],
            keep_with_next: true, ..Default::default() }, vec![
            run("bold ", RunStyle { bold: true, ..Default::default() }),
            run("italic ", RunStyle { italic: true, underline: true, ..Default::default() }),
            run("struck ", RunStyle { strikethrough: true, highlight: true, ..Default::default() }),
            run("linked", RunStyle { link: Some("https://gnome.org/".into()), ..Default::default() }),
            run(" red serif ", RunStyle { color: Some("c00000".into()), font_family: Some("Liberation Serif".into()), font_size_hp: Some(28), ..Default::default() }),
            run("x", RunStyle { vert_align: Some(VertAlign::Superscript), ..Default::default() }),
            run(" code", RunStyle { code: true, ..Default::default() }),
            run(" commented", RunStyle { comments: vec![1], ..Default::default() }),
            run("", RunStyle { footnote: Some(0), ..Default::default() }),
        ]),
        para(ParaStyle { list: ListKind::Bullet, ..Default::default() }, vec![run("a bullet", RunStyle::default())]),
        para(ParaStyle { list: ListKind::Bullet, list_level: 1, ..Default::default() }, vec![run("a nested bullet", RunStyle::default())]),
        para(ParaStyle { list: ListKind::Numbered, list_start: Some(3), ..Default::default() }, vec![run("third", RunStyle::default())]),
        para(ParaStyle { list: ListKind::Numbered, ..Default::default() }, vec![run("fourth", RunStyle::default())]),
        para(ParaStyle { code_block: Some("rust".into()), ..Default::default() }, vec![run("let x = 1;", RunStyle::default())]),
        para(ParaStyle { block_quote: true, ..Default::default() }, vec![run("a quotation", RunStyle::default())]),
        para(ParaStyle { named_style: Some("Subtitle".into()), ..Default::default() }, vec![run("a subtitle", RunStyle::default())]),
        para(ParaStyle { page_break_before: true, ..Default::default() }, vec![run("after a page break", RunStyle::default())]),
        para(ParaStyle::default(), vec![run("before the table", RunStyle::default())]),
        para(ParaStyle::default(), vec![run("EDIT ZONE", RunStyle::default())]),
    ];
    d.insert_table_at(11, 2, 2);
    d.footnotes = vec!["The footnote.".into()];
    d.header = Some("The header".into());
    d.footer = Some("Page {page}".into());
    d.page = Some(PageGeometry { width_pt: 612.0, height_pt: 792.0, margin_top_pt: 36.0, margin_bottom_pt: 54.0,
        margin_left_pt: 90.0, margin_right_pt: 45.0, columns: 2, column_gap_pt: 18.0 });
    d.base_font = BaseFont { family: Some("Liberation Sans".into()), size_hp: Some(22) };
    d.heading_styles = vec![RunStyle { color: Some("1a5fb4".into()), bold: true, ..Default::default() }];
    d.comments = vec![
        Comment { id: 1, author: "Ann".into(), date: "2026-10-01T09:00:00Z".into(), text: "Check this".into(), resolved: false, parent: None },
        Comment { id: 2, author: "Bo".into(), date: "2026-10-01T10:00:00Z".into(), text: "Done".into(), resolved: false, parent: Some(1) },
    ];
    d
}

/// Fields survive unrelated edits and structured commands (#1278).
///
/// Random sequences of typing, Enter, deleting, the structured commands
/// (list on and off, indent, outdent, restart numbering, page break,
/// insert table) and undo and redo, all in the document's last paragraph.
/// After each sequence the buffer is captured whole: every other
/// paragraph is exactly as it was loaded, every document-level field
/// (footnotes, header, footer, page, base font, heading look, comments) is
/// unchanged, and no paragraph's text holds a rendered list marker, a
/// footnote's "[n]" or a table's pipes, which are presentation and must
/// never come back as the user's text.
#[test]
fn unrelated_edits_and_commands_leave_every_other_field_alone() {
    gtk_test(|| {
        let original = every_field();
        let zone = original.paragraphs.iter().position(|p| p.text() == "EDIT ZONE").unwrap();
        let seeds = [0x2545_f491_4f6c_dd1du64, 0x9e37_79b9_7f4a_7c15, 0xdead_beef_cafe_f00d, 7, 12345, 99]
            .into_iter()
            .chain((1..=54).map(|i| 0x51_7cc1_b727_220a_u64.wrapping_mul(i)));
        for seed in seeds {
            let (buf, live) = tab(&original);
            assert_eq!(live.borrow().document(), &original, "the document does not load as itself");
            let zone_start = at(&buf, "EDIT ZONE");
            let mut state = seed;
            let mut next = |n: u64| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state % n.max(1)
            };
            let mut ops: Vec<String> = Vec::new();
            for _ in 0..60 {
                let end = buf.char_count();
                let in_zone = |r: u64| zone_start + r as i32;
                let span = (end - zone_start).max(0) as u64;
                match next(10) {
                    0..=2 => {
                        let t = ["a", "bc", " ", "é✨", "\n", "中文"][next(6) as usize];
                        buf.place_cursor(&buf.iter_at_offset(in_zone(next(span + 1))));
                        crate::page_edit::type_text(&buf, t);
                        ops.push(format!("type {t:?}"));
                    }
                    3 => {
                        // Text, not a table's own text: deleting a table
                        // line's pipes, or the break between two of its
                        // rows, takes the table apart, which leaves its
                        // pieces as the user's own text. Deleting next to a
                        // table is fair game, the empty line between two
                        // tables included (#1299: that used to run them
                        // together).
                        let a = in_zone(next(span + 1));
                        let b = (a + 1 + next(4) as i32).min(end);
                        let (s, e) = (buf.iter_at_offset(a), buf.iter_at_offset(b));
                        let is_table_line = |l: i32| {
                            let Some(l0) = buf.iter_at_line(l) else { return false };
                            let mut l1 = l0;
                            if !l1.ends_line() {
                                l1.forward_to_line_end();
                            }
                            buf.text(&l0, &l1, false).contains('|')
                        };
                        // Lines whose own characters the range deletes; a
                        // range starting at a line's end takes only its
                        // break, and one ending at a line's start none of it.
                        let first = s.line() + i32::from(s.ends_line());
                        let last = e.line() - i32::from(e.starts_line());
                        // A deleted break joins what is left of the first
                        // line to what is left of the last: a table line
                        // joined to anything (a row, prose) is no longer a
                        // row. Joined to nothing, as when the empty line
                        // between two tables goes, it stays intact.
                        let mut line_start = s;
                        line_start.set_line_offset(0);
                        let mut line_end = e;
                        if !line_end.ends_line() {
                            line_end.forward_to_line_end();
                        }
                        let joins = s.line() != e.line()
                            && !buf.text(&line_start, &s, false).is_empty()
                            && !buf.text(&e, &line_end, false).is_empty()
                            && (is_table_line(s.line()) || is_table_line(e.line()));
                        if joins || (first..=last).any(is_table_line) {
                            continue;
                        }
                        buf.select_range(&s, &e);
                        crate::insert::delete_selection(&buf);
                        ops.push(format!("delete {a}..{b}"));
                    }
                    4..=6 => {
                        buf.place_cursor(&buf.iter_at_offset(in_zone(next(span + 1))));
                        let which = next(7);
                        crate::bridge::apply_structured_edit(&buf, |e| {
                            match which {
                                0 => { e.toggle_list_at_cursor(letters_core::ListKind::Bullet); }
                                1 => { e.toggle_list_at_cursor(letters_core::ListKind::Numbered); }
                                2 => { e.indent_list_at_cursor(); }
                                3 => { e.outdent_list_at_cursor(); }
                                4 => { e.restart_numbering_at_cursor(); }
                                5 => { e.toggle_page_break_at_cursor(); }
                                _ => { e.insert_table(2, 2); }
                            }
                        });
                        ops.push(format!("command {which}"));
                    }
                    7 | 8 => {
                        crate::live::undo(&buf, false);
                        ops.push("undo".into());
                    }
                    _ => {
                        crate::live::undo(&buf, true);
                        ops.push("redo".into());
                    }
                }
            }
            let got = live.borrow().document().clone();
            let what = format!("seed {seed:#x} after {ops:?}");
            assert_eq!(got.paragraphs[..zone], original.paragraphs[..zone], "a paragraph outside the edits changed: {what}");
            assert_eq!(got.footnotes, original.footnotes, "footnotes: {what}");
            assert_eq!((&got.header, &got.footer), (&original.header, &original.footer), "header/footer: {what}");
            assert_eq!(got.page, original.page, "page: {what}");
            assert_eq!(got.base_font, original.base_font, "base font: {what}");
            assert_eq!(got.heading_styles, original.heading_styles, "heading look: {what}");
            assert_eq!(got.comments, original.comments, "comments: {what}");
            for p in &got.paragraphs {
                let text = p.text();
                let marker = text.starts_with("•\t") || text.starts_with("◦\t")
                    || text.split_once(".\t").is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
                assert!(!marker, "a list marker came back as text: {text:?}, {what}");
                assert!(!text.contains("[1]"), "a footnote marker came back as text: {text:?}, {what}");
                assert!(p.style.table_cell.is_some() || !text.trim_start().starts_with('|'), "table pipes came back as text: {text:?}, {what}");
            }
            check(&buf, &live, &what);
        }
    });
}

/// One live session for typing and structured commands, and review
/// anchors that stay on their text across Unicode edits (#1279).
///
/// Random interleavings of typing (combining marks, a non-BMP emoji with
/// a skin-tone modifier, CJK), Enter, the structured commands, and undo
/// and redo, everywhere except inside the two anchored words. After every
/// step the comment still covers exactly its word, the tracked insertion
/// exactly its own, and the model is what the buffer reads as. Undoing
/// everything then restores the loaded document: typing and commands
/// share one history, so one undo stack reaches both.
#[test]
fn review_anchors_hold_across_unicode_edits_and_one_history_undoes_them() {
    gtk_test(|| {
        use letters_core::{Comment, Paragraph, ParaStyle, Revision, RevisionKind, Run, RunStyle};
        let mut original = Document::from_plain_text("");
        original.paragraphs = vec![
            Paragraph { style: ParaStyle::default(), runs: vec![
                Run::plain("Before the "),
                Run { text: "COMMENTED".into(), style: RunStyle { comments: vec![1], ..Default::default() } },
                Run::plain(" word and the "),
                Run { text: "INSERTED".into(), style: RunStyle {
                    revision: Some(Revision { kind: RevisionKind::Insert, author: "Ann".into(), date: "2026-10-01T09:00:00Z".into(), under: None }),
                    ..Default::default() } },
                Run::plain(" word."),
            ] },
            Paragraph { style: ParaStyle::default(), runs: vec![Run::plain("A second paragraph to work in.")] },
        ];
        original.comments = vec![Comment { id: 1, author: "Ann".into(), date: "2026-10-01T09:00:00Z".into(), text: "Look".into(), resolved: false, parent: None }];
        let covered = |doc: &Document, f: &dyn Fn(&RunStyle) -> bool| -> String {
            doc.paragraphs.iter().flat_map(|p| &p.runs).filter(|r| f(&r.style)).map(|r| r.text.as_str()).collect()
        };
        let seeds = [0x2545_f491_4f6c_dd1du64, 0x9e37_79b9_7f4a_7c15, 0xdead_beef_cafe_f00d, 7, 12345, 99, 4242, 0xabcdef]
            .into_iter()
            .chain((1..=32).map(|i| 0x6c07_8965_u64.wrapping_mul(i * 2654435761)));
        for seed in seeds {
            let (buf, live) = tab(&original);
            let mut state = seed;
            let mut next = |n: u64| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state % n.max(1)
            };
            let mut ops: Vec<String> = Vec::new();
            for step in 0..60 {
                // Anywhere but inside (or at the edges of) the anchored words.
                let t = text(&buf);
                let words: Vec<(i32, i32)> = ["COMMENTED", "INSERTED"].iter().filter_map(|w| {
                    let b = t.find(w)?;
                    let s = t[..b].chars().count() as i32;
                    Some((s, s + w.chars().count() as i32))
                }).collect();
                let len = buf.char_count();
                let place = loop {
                    let o = next(len as u64 + 1) as i32;
                    if words.iter().all(|&(s, e)| o < s || o > e) {
                        break o;
                    }
                };
                match next(10) {
                    0..=3 => {
                        let typed = ["e\u{301}", "👍🏽", "中文", "a", " ", "ñ", "\n"][next(7) as usize];
                        buf.place_cursor(&buf.iter_at_offset(place));
                        crate::page_edit::type_text(&buf, typed);
                        ops.push(format!("type {typed:?}"));
                    }
                    4 | 5 => {
                        buf.place_cursor(&buf.iter_at_offset(place));
                        let which = next(4);
                        crate::bridge::apply_structured_edit(&buf, |e| match which {
                            0 => { e.toggle_list_at_cursor(letters_core::ListKind::Bullet); }
                            1 => { e.toggle_list_at_cursor(letters_core::ListKind::Numbered); }
                            2 => { e.toggle_page_break_at_cursor(); }
                            _ => { e.indent_list_at_cursor(); }
                        });
                        ops.push(format!("command {which}"));
                    }
                    6..=8 => { undo(&buf, false); ops.push("undo".into()); }
                    _ => { undo(&buf, true); ops.push("redo".into()); }
                }
                let (doc, _) = live.borrow_mut().snapshot();
                let what = format!("seed {seed:#x} step {step} after {ops:?}");
                assert_eq!(covered(&doc, &|s| s.comments.contains(&1)), "COMMENTED", "the comment moved: {what}");
                assert_eq!(covered(&doc, &|s| s.revision.is_some()), "INSERTED", "the tracked insertion moved: {what}");
                check(&buf, &live, &what);
            }
            let mut undos = 0;
            while live.borrow().can_undo() && undos < 500 {
                undo(&buf, false);
                undos += 1;
            }
            let (back, _) = live.borrow_mut().snapshot();
            assert_eq!(back.paragraphs, original.paragraphs, "seed {seed:#x}: undoing everything did not restore the document after {ops:?}");
            check(&buf, &live, &format!("seed {seed:#x} after undoing everything"));
        }
    });
}
