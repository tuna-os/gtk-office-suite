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

/// The model must be exactly what a whole-buffer capture gives.
fn check(buf: &gtk::TextBuffer, live: &Rc<RefCell<LiveModel>>, what: &str) {
    let (doc, starts) = live.borrow_mut().snapshot(buf);
    let (want, want_starts) = crate::bridge::capture_with_starts(buf);
    assert_eq!(doc, want, "after {what}");
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
