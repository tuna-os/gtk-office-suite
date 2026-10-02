use super::*;
use suite_common::gtk_test::run as gtk_test;

/// A buffer showing `doc`, with a live model attached (as a tab builds it).
fn tab(doc: &Document) -> (gtk::TextBuffer, Rc<RefCell<LiveModel>>) {
    let buf = gtk::TextBuffer::new(None);
    crate::actions::register_formatting_tags(&buf);
    let live = LiveModel::attach(&buf);
    crate::bridge::load_document(doc, &buf);
    (buf, live)
}

/// Table ids renumbered by order of appearance. An id only groups a
/// table's cells: a capture numbers tables as it meets them, while the
/// model keeps the id a table was made with, so after a table is inserted
/// and undone the two can name the same tables differently.
fn tables_in_order(mut doc: Document) -> Document {
    let mut seen: Vec<u32> = Vec::new();
    for p in &mut doc.paragraphs {
        if let Some(cell) = p.style.table_cell.as_mut() {
            let n = seen.iter().position(|&t| t == cell.table).unwrap_or_else(|| { seen.push(cell.table); seen.len() - 1 });
            cell.table = n as u32;
        }
    }
    doc
}

/// The model must be exactly what a whole-buffer capture gives (table ids
/// aside, see `tables_in_order`).
fn check(buf: &gtk::TextBuffer, live: &Rc<RefCell<LiveModel>>, what: &str) {
    let (doc, starts) = live.borrow_mut().snapshot(buf);
    let (want, want_starts) = crate::bridge::capture_with_starts(buf);
    assert_eq!(tables_in_order(doc), tables_in_order(want), "after {what}");
    assert_eq!(starts, want_starts, "starts after {what}");
}

fn text(buf: &gtk::TextBuffer) -> String {
    buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string()
}

fn at(buf: &gtk::TextBuffer, needle: &str) -> i32 {
    let t = text(buf);
    t[..t.find(needle).unwrap()].chars().count() as i32
}

/// A 1x1 image as the editor inserts one (its source rides on the paintable).
fn image() -> gtk::gdk::Texture {
    let tex = gtk::gdk::MemoryTexture::new(1, 1, gtk::gdk::MemoryFormat::R8g8b8a8, &glib::Bytes::from_static(&[255, 0, 0, 255]), 4);
    let dir = std::env::temp_dir().join("letters-live-test");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("dot.png");
    let _ = tex.save_to_png(&path);
    let tex = gtk::gdk::Texture::from_filename(&path).unwrap();
    unsafe {
        tex.set_data("letters-image-src", path.to_string_lossy().into_owned());
        tex.set_data("letters-image-alt", String::from("a dot"));
    }
    tex
}

fn sample() -> Document {
    let mut d = Document::from_plain_text("Title\nfirst item\nsecond item\nbody text here");
    d.paragraphs[0].style.heading = Some(1);
    for p in &mut d.paragraphs[1..3] {
        p.style.list = letters_core::ListKind::Numbered;
    }
    d
}

/// Typing, Enter, Backspace across a break, formatting and list markers
/// are followed by reading back only the lines they touched: the model is
/// the captured document after each, and the whole buffer is never read.
#[test]
fn breaks_and_formatting_are_followed_locally() {
    gtk_test(|| {
        let (buf, live) = tab(&sample());
        check(&buf, &live, "load");
        let full = live.borrow().full_reads;

        let mut it = buf.iter_at_offset(at(&buf, "item"));
        buf.insert(&mut it, "numbered ");
        check(&buf, &live, "typing in a list item");
        let mut it = buf.iter_at_offset(at(&buf, "text here"));
        buf.insert(&mut it, "\n");
        check(&buf, &live, "Enter in a paragraph");
        let mut end = buf.end_iter();
        buf.insert(&mut end, "\n");
        check(&buf, &live, "Enter at the end");
        let (mut s, mut e) = (buf.iter_at_offset(at(&buf, "text here") - 1), buf.iter_at_offset(at(&buf, "text here")));
        buf.delete(&mut s, &mut e);
        check(&buf, &live, "Backspace joining two paragraphs");
        let (s, e) = (buf.iter_at_offset(at(&buf, "Title")), buf.iter_at_offset(at(&buf, "body") + 4));
        buf.apply_tag_by_name("bold", &s, &e);
        check(&buf, &live, "bold across several paragraphs");
        let (s, e) = (buf.iter_at_offset(at(&buf, "Title")), buf.iter_at_offset(at(&buf, "Title") + 3));
        buf.remove_tag_by_name("bold", &s, &e);
        check(&buf, &live, "bold removed");
        let (s, e) = (buf.iter_at_offset(at(&buf, "body")), buf.iter_at_offset(at(&buf, "body") + 4));
        buf.apply_tag_by_name("align-center", &s, &e);
        check(&buf, &live, "a paragraph tag");
        let mut it = buf.iter_at_offset(at(&buf, "body"));
        buf.insert(&mut it, "- ");
        check(&buf, &live, "typing a list marker");
        assert_eq!(live.borrow().full_reads, full, "none of that read the whole buffer");
        assert!(live.borrow().local_reads >= 8);
    });
}

/// Seeded random edits anywhere — typing (with Enter and pipes), deleting
/// across breaks and markers, formatting — in a document with a list and a
/// table. After each, the model equals a whole-buffer capture.
#[test]
fn random_edits_never_leave_the_live_model_behind() {
    gtk_test(|| {
        for seed in [0x2545_f491_4f6c_dd1du64, 0x9e37_79b9_7f4a_7c15, 0xdead_beef_cafe_f00d, 7, 12345] {
            let mut d = Document::from_plain_text("intro text\nitem one\nitem two\nclosing words");
            d.paragraphs[1].style.list = letters_core::ListKind::Bullet;
            d.paragraphs[2].style.list = letters_core::ListKind::Bullet;
            d.insert_table_at(3, 1, 2);
            let (buf, live) = tab(&d);
            let reads = live.borrow().full_reads;
            let mut state = seed;
            let mut next = |n: u64| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state % n.max(1)
            };
            for step in 0..300 {
                let len = buf.char_count().max(0) as u64;
                match next(13) {
                    12 => {
                        // An inline image, as Insert Image puts one.
                        let tex = image();
                        let mut it = buf.iter_at_offset(next(len + 1) as i32);
                        buf.insert_paintable(&mut it, &tex);
                    }
                    0..=4 => {
                        let t = ["a", "bc", " ", "xyz", "\n", "|"][next(6) as usize];
                        let mut it = buf.iter_at_offset(next(len + 1) as i32);
                        buf.insert(&mut it, t);
                    }
                    5..=7 => {
                        let a = next(len + 1) as i32;
                        let b = (a + next(4) as i32).min(len as i32);
                        let (mut s, mut e) = (buf.iter_at_offset(a), buf.iter_at_offset(b));
                        buf.delete(&mut s, &mut e);
                    }
                    8 | 9 => {
                        let a = next(len + 1) as i32;
                        let (s, e) = (buf.iter_at_offset(a), buf.iter_at_offset((a + 5).min(len as i32)));
                        let tag = ["italic", "bold", "h2", "align-right"][next(4) as usize];
                        buf.apply_tag_by_name(tag, &s, &e);
                    }
                    _ => {
                        let a = next(len + 1) as i32;
                        let (s, e) = (buf.iter_at_offset(a), buf.iter_at_offset((a + 4).min(len as i32)));
                        buf.remove_tag_by_name(["italic", "bold"][next(2) as usize], &s, &e);
                    }
                }
                if next(3) == 0 {
                    check(&buf, &live, &format!("seed {seed} step {step}"));
                }
            }
            check(&buf, &live, &format!("seed {seed} end"));
            assert_eq!(live.borrow().full_reads, reads, "seed {seed}: no edit read the whole buffer, tables and images included");
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
        let loaded = crate::bridge::capture_from_buffer(&buf);
        // Edits as user actions, as the TextView makes them.
        let action = |f: &dyn Fn()| {
            buf.begin_user_action();
            f();
            buf.end_user_action();
        };
        let base = at(&buf, "body");
        for (i, c) in "Hello".chars().enumerate() {
            action(&|| {
                let mut it = buf.iter_at_offset(base + i as i32);
                buf.insert(&mut it, &c.to_string());
            });
        }
        action(&|| {
            let mut it = buf.iter_at_offset(at(&buf, "Title") + 5);
            buf.insert(&mut it, "\n");
        });
        action(&|| {
            let (s, e) = (buf.iter_at_offset(at(&buf, "Title")), buf.iter_at_offset(at(&buf, "Title") + 5));
            buf.apply_tag_by_name("italic", &s, &e);
        });
        // A structured edit: a table inserted by re-rendering.
        buf.place_cursor(&buf.iter_at_offset(at(&buf, "Hellobody")));
        crate::bridge::apply_structured_edit(&buf, |ed| {
            ed.insert_table(2, 2);
        });
        check(&buf, &live, "the edits");
        let edited = crate::bridge::capture_from_buffer(&buf);

        let mut steps = 0;
        while live.borrow().can_undo() {
            undo(&buf, false);
            check(&buf, &live, &format!("undo {steps}"));
            steps += 1;
        }
        assert_eq!(steps, 4, "table, italic, Enter, and 'Hello' as one typed word");
        assert!(!live.borrow().can_undo() && live.borrow().can_redo());
        let now = crate::bridge::capture_from_buffer(&buf);
        for (x, y) in now.paragraphs.iter().zip(&loaded.paragraphs) {
            assert_eq!(x, y, "paragraph differs after undo");
        }
        assert_eq!(now, loaded, "undone to the loaded document");
        while live.borrow().can_redo() {
            undo(&buf, true);
            check(&buf, &live, "redo");
        }
        assert_eq!(crate::bridge::capture_from_buffer(&buf), edited, "redone to the edited document");
    });
}

/// Deleting the empty line between two tables leaves two tables (#1299):
/// the editor's reading and the model agree, and neither runs the second
/// table's header into the first one's body.
#[test]
fn deleting_the_line_between_two_tables_keeps_them_two() {
    gtk_test(|| {
        let mut d = Document::from_plain_text("before\n\nafter");
        d.insert_table_at(2, 2, 2);
        d.insert_table_at(1, 2, 2);
        let (buf, live) = tab(&d);
        check(&buf, &live, "loading");
        let tables = |doc: &Document| doc.paragraphs.iter().filter_map(|p| p.style.table_cell.map(|c| c.table)).collect::<std::collections::BTreeSet<_>>().len();
        assert_eq!(tables(&crate::bridge::capture_from_buffer(&buf)), 2, "two tables to begin with");
        // The empty line between them: the line after the first table's
        // last row.
        let t = text(&buf);
        let gap = t.find("|\n\n|").expect("an empty line between the tables") + 2;
        let at = t[..gap].chars().count() as i32;
        let (mut s, mut e) = (buf.iter_at_offset(at), buf.iter_at_offset(at + 1));
        buf.delete(&mut s, &mut e);
        assert!(!text(&buf).contains("|\n\n|"), "the empty line is gone");
        check(&buf, &live, "deleting the empty line");
        let read = crate::bridge::capture_from_buffer(&buf);
        assert_eq!(tables(&read), 2, "still two tables: {:?}", read.paragraphs.iter().map(|p| p.text()).collect::<Vec<_>>());
        assert!(read.paragraphs.iter().all(|p| p.style.table_cell.is_some() || !p.text().contains('|')), "a delimiter came back as prose");
    });
}

/// The page view edits the model first; the buffer shows the result.
#[test]
fn model_first_edits_reach_the_buffer() {
    gtk_test(|| {
        let (buf, live) = tab(&sample());
        let seq = live.borrow_mut().sequence_offset(&buf, at(&buf, "body") as usize);
        let op = {
            let mut m = live.borrow_mut();
            letters_core::edit::typing(m.document(&buf), seq, "new ").unwrap()
        };
        assert!(live.borrow_mut().apply_user_ops(&buf, &[op], false));
        assert!(text(&buf).contains("new body"), "{}", text(&buf));
        check(&buf, &live, "a model-first insert");
        // Enter in a list item continues the list, in the model and on screen.
        let seq = live.borrow_mut().sequence_offset(&buf, (at(&buf, "first item") + 10) as usize);
        let op = {
            let mut m = live.borrow_mut();
            letters_core::edit::typing(m.document(&buf), seq, "\n").unwrap()
        };
        assert!(live.borrow_mut().apply_user_ops(&buf, &[op], false));
        check(&buf, &live, "a model-first Enter in a list");
        assert!(text(&buf).contains("first item\n2.\t\n3.\tsecond"), "{:?}", text(&buf));
        undo(&buf, false);
        undo(&buf, false);
        assert_eq!(crate::bridge::capture_from_buffer(&buf), {
            let (b2, _) = tab(&sample());
            crate::bridge::capture_from_buffer(&b2)
        });
    });
}

#[test]
fn opening_a_document_reads_it_once_not_edit_by_edit() {
    // Following each insert and tag of the load re-read the touched lines
    // thousands of times: a 290-paragraph form took a minute to open.
    gtk_test(|| {
        let text: Vec<String> = (0..300).map(|i| format!("Paragraph {i} of an opened document.")).collect();
        let buf = gtk::TextBuffer::new(None);
        crate::actions::register_formatting_tags(&buf);
        let live = LiveModel::attach(&buf);
        let reads = live.borrow().full_reads;
        crate::bridge::load_document(&Document::from_plain_text(&text.join("\n")), &buf);
        assert_eq!(live.borrow().local_reads, 0, "the load was not followed edit by edit");
        assert_eq!(live.borrow().full_reads, reads + 1, "the loaded document was read once");
        check(&buf, &live, "opening");
        assert!(!crate::live::is_busy(&buf), "the model takes edits again after the load");
    });
}

/// CI performance gate: one keystroke on a 200-paragraph document — the
/// buffer edit, the live model following it, and the Print Layout
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
        let (doc, _) = live.borrow_mut().snapshot(&buf);
        let mut typeset = Typeset::new(doc, LayoutOptions::default());

        let full = {
            let start = Instant::now();
            let (doc, _) = crate::bridge::capture_with_starts(&buf);
            let _ = Typeset::new(doc, LayoutOptions::default());
            start.elapsed()
        };
        let base = at(&buf, "Paragraph 100:") + 4;
        let reads_before = live.borrow().full_reads;
        let mut samples = Vec::new();
        for k in 0..21 {
            let start = Instant::now();
            let mut it = buf.iter_at_offset(base + k);
            buf.insert(&mut it, "x");
            let (doc, _) = live.borrow_mut().snapshot(&buf);
            let shaped = typeset.update(doc, LayoutOptions::default());
            samples.push(start.elapsed());
            assert_eq!(shaped, 1, "only the edited paragraph is shaped again");
        }
        samples.sort_unstable();
        let (median, p95) = (samples[10], samples[19]);
        eprintln!("keystroke relayout on 200 paragraphs: p95 {p95:?}, median {median:?}; full capture + layout {full:?}");
        let full_reads = live.borrow().full_reads;
        assert_eq!(full_reads, reads_before, "typing never read the whole buffer");
        // Generous for an unoptimised build on a shared CI runner.
        const BUDGET: Duration = Duration::from_millis(100);
        assert!(p95 <= BUDGET, "keystroke p95 {p95:?} over {BUDGET:?}");
        assert!(median < full, "a keystroke must cost less than laying out from scratch ({median:?} vs {full:?})");
    });
}

/// Table commands (insert a table, rows and columns, delete them), list
/// commands and page breaks run on the model as ops: no whole-buffer read,
/// the buffer matches, and each is one undo step.
#[test]
fn structured_commands_are_model_ops() {
    use letters_core::ListKind;
    gtk_test(|| {
        let (buf, live) = tab(&sample());
        let reads = live.borrow().full_reads;
        buf.place_cursor(&buf.iter_at_offset(at(&buf, "body")));
        crate::bridge::apply_structured_edit(&buf, |ed| {
            ed.insert_table(2, 2);
        });
        check(&buf, &live, "insert table");
        // The caret is in the new table's first cell: type there.
        buf.insert_at_cursor("A1");
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
        assert_eq!(live.borrow().full_reads, reads, "no command read the whole buffer");
        // Undo all of it, one command at a time.
        let mut steps = 0;
        while live.borrow().can_undo() {
            undo(&buf, false);
            check(&buf, &live, &format!("undo {steps}"));
            steps += 1;
        }
        assert_eq!(steps, 8, "each command and the typed word is one step");

        let (fresh, _) = tab(&sample());
        assert_eq!(crate::bridge::capture_from_buffer(&buf), crate::bridge::capture_from_buffer(&fresh));
    });
}

/// Markdown shortcuts: "**bold**" and a space makes bold text, typed in
/// Draft or in Print Layout, and the model has it. (They never fired
/// before: the pattern's end was taken from an absent selection.)
#[test]
fn markdown_shortcuts_work_in_both_views() {
    gtk_test(|| {
        let ctx = glib::MainContext::default();
        let settle = || while ctx.iteration(false) {};
        // Draft: typed into the buffer.
        let (buf, live) = tab(&Document::from_plain_text("x"));
        crate::actions::connect_markdown_macros(&buf);
        let mut end = buf.end_iter();
        buf.insert(&mut end, " **bold**");
        let mut end = buf.end_iter();
        buf.insert(&mut end, " ");
        settle();
        check(&buf, &live, "a Draft shortcut");
        let doc = live.borrow_mut().snapshot(&buf).0;
        assert!(doc.paragraphs[0].runs.iter().any(|r| r.text == "bold" && r.style.bold), "{:?}", doc.paragraphs[0].runs);
        assert_eq!(doc.paragraphs[0].text(), "x bold ");

        // Print Layout: typed as model ops through the page view.
        let (buf, live) = tab(&Document::from_plain_text("y"));
        crate::actions::connect_markdown_macros(&buf);
        let view = crate::page_view::PageView::new();
        crate::page_edit::make_editable(&view, &buf);
        buf.place_cursor(&buf.end_iter());
        for c in " _it_ ".chars() {
            crate::page_edit::type_text(&buf, &c.to_string());
        }
        settle();
        check(&buf, &live, "a Print Layout shortcut");
        let doc = live.borrow_mut().snapshot(&buf).0;
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
            let loaded = crate::bridge::capture_from_buffer(&buf);
            assert_eq!(loaded.paragraphs, original.paragraphs, "the document does not load as itself");
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
                        let (mut s, mut e) = (buf.iter_at_offset(a), buf.iter_at_offset(b));
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
                        buf.delete(&mut s, &mut e);
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
            let got = crate::bridge::capture_from_buffer(&buf);
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
                let (doc, _) = live.borrow_mut().snapshot(&buf);
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
            let (back, _) = live.borrow_mut().snapshot(&buf);
            assert_eq!(back.paragraphs, original.paragraphs, "seed {seed:#x}: undoing everything did not restore the document after {ops:?}");
            check(&buf, &live, &format!("seed {seed:#x} after undoing everything"));
        }
    });
}
