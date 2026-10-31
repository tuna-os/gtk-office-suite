// The live model in a session, on real GtkTextBuffers and real Replicas
// (RFC-0005 P5): the offsets between a buffer and the shared sequence, under
// a peer's edits. Run with a display (scripts/with-display.sh), like the rest
// of this crate's buffer tests.

use super::collab::{Hooks, Refusal};
use super::tests::{at, check, shown, text, type_at};
use super::*;
use letters_core::chips::{Chip, ChipKind};
use letters_core::collab::Shared;
use letters_core::model::{Run, RunStyle};
use suite_common::gtk_test::run as gtk_test;

/// One person's tab in a session: its buffer and live model, what it has
/// published and what it was told it could not do.
struct Tab {
    buf: gtk::TextBuffer,
    live: Rc<RefCell<LiveModel>>,
    published: Rc<RefCell<Vec<Vec<u8>>>>,
    refused: Rc<RefCell<Vec<Refusal>>>,
}

fn tab(doc: &Document) -> Tab {
    let buf = gtk::TextBuffer::new(None);
    crate::actions::register_formatting_tags(&buf);
    let live = LiveModel::attach(&buf, doc.clone());
    Tab { buf, live, published: Rc::default(), refused: Rc::default() }
}

fn join_session(tab: &Tab, shared: Shared) {
    let (published, refused) = (tab.published.clone(), tab.refused.clone());
    tab.live.borrow_mut().attach_shared(
        shared,
        Hooks {
            read_only: false,
            publish: Box::new(move |delta| published.borrow_mut().push(delta)),
            refused: Box::new(move |why| refused.borrow_mut().push(why.clone())),
        },
    );
}

/// Two people on `doc`: A shares it, B joins through the genesis.
fn session(doc: &Document) -> (Tab, Tab) {
    let a = tab(doc);
    let (shared, genesis) = Shared::start(1, doc).expect("a document this encoding carries");
    join_session(&a, shared);
    let (shared, joined) = Shared::join(2, &genesis, doc).expect("joined");
    let b = tab(&joined);
    join_session(&b, shared);
    (a, b)
}

/// What `from` published since the last delivery reaches `to`, as the
/// session hands it over, and `to` is checked: the buffer is exactly what
/// rendering its model from scratch gives.
fn deliver(from: &Tab, to: &Tab, what: &str) {
    let updates = from.published.take();
    for update in updates {
        let imported = to.live.borrow_mut().import_remote(&to.buf, &update).expect("a peer's update is taken in");
        assert!(imported.unsent.is_none(), "{what}: the receiver had something unsent");
    }
    check(&to.buf, &to.live, what);
}

/// `deliver` for a document with a chip: a chip's tag name carries a counter that
/// every render advances, so the buffer cannot be compared tag for tag with a fresh
/// render. The text, where each paragraph starts and the model are compared instead.
fn deliver_chips(from: &Tab, to: &Tab, what: &str) {
    for update in from.published.take() {
        to.live.borrow_mut().import_remote(&to.buf, &update).expect("a peer's update is taken in");
    }
    let snap = to.live.borrow().snapshot();
    let fresh = gtk::TextBuffer::new(Some(&to.buf.tag_table()));
    let starts = crate::bridge::render_to_buffer(&snap.doc, &fresh);
    assert_eq!(text(&to.buf), text(&fresh), "the buffer's text after {what}");
    assert_eq!(snap.starts, starts, "starts after {what}");
    assert_eq!(to.live.borrow().foreign_edits, 0, "{what}");
}

fn caret(buf: &gtk::TextBuffer) -> i32 {
    buf.iter_at_mark(&buf.get_insert()).offset()
}

fn selected(buf: &gtk::TextBuffer) -> Option<String> {
    buf.selection_bounds().map(|(s, e)| buf.text(&s, &e, false).to_string())
}

fn chip_doc() -> Document {
    let mut d = Document::from_plain_text("Due: and then the rest");
    d.paragraphs[0].runs = vec![
        Run::plain("Due: "),
        Run { text: "26 Oct 2026".into(), style: RunStyle { chip: Some(Chip { kind: ChipKind::Date, value: "2026-10-26".into() }), ..Default::default() } },
        Run::plain(" and then the rest"),
    ];
    d
}

#[test]
fn a_peers_typing_before_my_caret_keeps_it_on_its_character() {
    gtk_test(|| {
        let (a, b) = session(&Document::from_plain_text("The quick brown fox"));
        a.buf.place_cursor(&a.buf.iter_at_offset(9));
        type_at(&b.buf, 0, ">> ");
        deliver(&b, &a, "B typed before A's caret");
        assert_eq!(text(&a.buf), ">> The quick brown fox");
        assert_eq!(caret(&a.buf), 12, "A's caret is still after 'quick'");
        assert!(a.published.borrow().is_empty(), "taking in B's update sent nothing back");
        // B typing after the caret leaves it where it is.
        type_at(&b.buf, 22, "!");
        deliver(&b, &a, "B typed after A's caret");
        assert_eq!(caret(&a.buf), 12);
        // And B deleting the text around it.
        assert!(b.live.borrow_mut().apply_user_ops(&b.buf, &[letters_core::edit::Op::Delete { at: 3, len: 13 }], false));
        deliver(&b, &a, "B deleted around A's caret");
        assert_eq!(text(&a.buf), text(&b.buf));
        assert_eq!(caret(&a.buf), 3, "the caret is where the deleted text was");
    });
}

#[test]
fn both_typing_in_one_paragraph_converge_and_neither_caret_jumps() {
    gtk_test(|| {
        let (a, b) = session(&Document::from_plain_text("Hello"));
        a.buf.place_cursor(&a.buf.iter_at_offset(5));
        b.buf.place_cursor(&b.buf.iter_at_offset(0));
        type_at(&a.buf, 5, " from A");
        type_at(&b.buf, 0, "B says ");
        // Neither has seen the other's text yet.
        assert_eq!((text(&a.buf), text(&b.buf)), ("Hello from A".to_string(), "B says Hello".to_string()));
        deliver(&a, &b, "A to B");
        deliver(&b, &a, "B to A");
        assert_eq!(text(&a.buf), "B says Hello from A");
        assert_eq!(text(&b.buf), text(&a.buf));
        // Each caret is still at the end of what its owner typed: A's at the
        // end, B's after its own "B says ".
        assert_eq!(caret(&a.buf), "B says Hello from A".chars().count() as i32);
        assert_eq!(caret(&b.buf), "B says ".chars().count() as i32);
        // Typing on goes after the person's own text, on both.
        crate::page_edit::type_text(&a.buf, "!");
        crate::page_edit::type_text(&b.buf, "?");
        deliver(&a, &b, "A typed on");
        deliver(&b, &a, "B typed on");
        assert_eq!(text(&a.buf), "B says ?Hello from A!");
        assert_eq!(text(&b.buf), text(&a.buf));
        assert_eq!(a.live.borrow().document().paragraphs, b.live.borrow().document().paragraphs);
    });
}

/// A peer typing at exactly the caret: the text goes before it, the caret
/// after it, so a person typing at a point the peer also types at keeps
/// typing after both (and it is what the undo history does, see
/// `letters_core::collab::Bias`).
#[test]
fn a_peers_insert_at_the_caret_goes_before_it() {
    gtk_test(|| {
        let (a, b) = session(&Document::from_plain_text("abcdef"));
        a.buf.place_cursor(&a.buf.iter_at_offset(3));
        type_at(&b.buf, 3, "XY");
        deliver(&b, &a, "B typed at A's caret");
        assert_eq!(text(&a.buf), "abcXYdef");
        assert_eq!(caret(&a.buf), 5);
    });
}

#[test]
fn a_selection_keeps_its_characters_whatever_a_peer_types_at_its_edges() {
    gtk_test(|| {
        let (a, b) = session(&Document::from_plain_text("The quick brown fox"));
        a.buf.select_range(&a.buf.iter_at_offset(4), &a.buf.iter_at_offset(9));
        assert_eq!(selected(&a.buf).as_deref(), Some("quick"));
        type_at(&b.buf, 4, "[");
        type_at(&b.buf, 10, "]");
        deliver(&b, &a, "B typed at both edges of A's selection");
        assert_eq!(text(&a.buf), "The [quick] brown fox");
        assert_eq!(selected(&a.buf).as_deref(), Some("quick"));
        type_at(&b.buf, 0, ">>");
        deliver(&b, &a, "B typed before A's selection");
        assert_eq!(selected(&a.buf).as_deref(), Some("quick"));
    });
}

#[test]
fn bold_applied_by_a_peer_shows_in_my_buffer_and_moves_nothing() {
    gtk_test(|| {
        let (a, b) = session(&Document::from_plain_text("The quick brown fox"));
        a.buf.place_cursor(&a.buf.iter_at_offset(19));
        b.buf.select_range(&b.buf.iter_at_offset(4), &b.buf.iter_at_offset(9));
        crate::actions::toggle_tag_in(&b.buf, "bold");
        deliver(&b, &a, "B made 'quick' bold");
        let (_, tags) = shown(&a.buf);
        let bold: Vec<bool> = tags.iter().map(|t| t.iter().any(|n| n == "bold")).collect();
        assert_eq!(bold.iter().filter(|b| **b).count(), 5, "five characters are bold: {bold:?}");
        assert!(bold[4..9].iter().all(|b| *b) && !bold[3] && !bold[9]);
        assert_eq!(caret(&a.buf), 19, "a mark moves no caret");
        assert!(a.published.borrow().is_empty());
        // And the other way round: A's italic reaches B.
        a.buf.select_range(&a.buf.iter_at_offset(10), &a.buf.iter_at_offset(15));
        crate::actions::toggle_tag_in(&a.buf, "italic");
        deliver(&a, &b, "A made 'brown' italic");
        assert_eq!(a.live.borrow().document().paragraphs, b.live.borrow().document().paragraphs);
    });
}

#[test]
fn undo_takes_back_only_my_own_steps_after_a_peer_edited() {
    gtk_test(|| {
        let (a, b) = session(&Document::from_plain_text("Hello world"));
        type_at(&a.buf, 11, " mine");
        deliver(&a, &b, "A typed");
        type_at(&b.buf, 0, ">> ");
        deliver(&b, &a, "B typed");
        assert_eq!(text(&a.buf), ">> Hello world mine");
        // A's undo takes back A's text, not B's, and is an update like any edit.
        crate::live::undo(&a.buf, false);
        assert_eq!(text(&a.buf), ">> Hello world");
        check(&a.buf, &a.live, "A undid");
        assert!(!a.published.borrow().is_empty(), "the undo is sent");
        deliver(&a, &b, "A's undo");
        assert_eq!(text(&b.buf), ">> Hello world");
        // B's own undo takes back B's text, and nothing of A's.
        crate::live::undo(&b.buf, false);
        assert_eq!(text(&b.buf), "Hello world");
        deliver(&b, &a, "B's undo");
        assert_eq!(text(&a.buf), "Hello world");
        // Redo is mine too.
        crate::live::undo(&a.buf, true);
        deliver(&a, &b, "A's redo");
        assert_eq!(text(&a.buf), "Hello world mine");
        assert_eq!(text(&b.buf), text(&a.buf));
    });
}

#[test]
fn a_remote_edit_never_lands_on_the_undo_stack() {
    gtk_test(|| {
        let (a, b) = session(&Document::from_plain_text("Hello"));
        assert!(!a.live.borrow().can_undo());
        type_at(&b.buf, 5, " there");
        deliver(&b, &a, "B typed");
        assert!(!a.live.borrow().can_undo(), "B's typing is not A's to undo");
        assert_eq!(a.live.borrow().foreign_edits, 0);
    });
}

#[test]
fn an_edit_a_shared_document_cannot_carry_is_refused_with_nothing_changed() {
    gtk_test(|| {
        let (a, _b) = session(&Document::from_plain_text("Hello"));
        let before = a.live.borrow().document().clone();
        let done = a.live.borrow_mut().edit_with(&a.buf, |_| vec![letters_core::edit::Op::SetFootnotes { notes: vec!["a note".into()] }]);
        assert!(!done);
        assert_eq!(*a.refused.borrow(), vec![Refusal::Unsupported(letters_core::collab::Unsupported::Footnotes)]);
        assert_eq!(a.live.borrow().document(), &before);
        assert!(a.published.borrow().is_empty());
        check(&a.buf, &a.live, "the refused edit");
        // Typing is not affected.
        a.buf.place_cursor(&a.buf.iter_at_offset(5));
        crate::page_edit::type_text(&a.buf, "!");
        assert_eq!(text(&a.buf), "Hello!");
        assert_eq!(a.published.borrow().len(), 1);
    });
}

#[test]
fn a_document_with_a_table_is_not_shared_at_all() {
    gtk_test(|| {
        let mut doc = Document::from_plain_text("a table:\ncell");
        doc.paragraphs[1].style.table_cell = Some(letters_core::model::TableCell { table: 1, row: 0, col: 0 });
        let refused = Shared::start(1, &doc).err().expect("refused");
        assert_eq!(refused, letters_core::collab::CollabError::Unsupported(letters_core::collab::Unsupported::Tables));
    });
}

#[test]
fn a_chip_and_a_list_marker_do_not_confuse_the_caret_mapping() {
    gtk_test(|| {
        // A date chip is one char of the sequence and eleven in the buffer, a
        // list marker is in the buffer only: the offsets differ, and the
        // caret must follow the character.
        let mut doc = chip_doc();
        doc.paragraphs.push(letters_core::Paragraph {
            style: letters_core::ParaStyle { list: letters_core::ListKind::Numbered, ..Default::default() },
            runs: vec![Run::plain("item one")],
        });
        let (a, b) = session(&doc);
        let before = text(&a.buf);
        // A's caret after the chip and after "item".
        let after_chip = at(&a.buf, " and then");
        let in_item = at(&a.buf, "one");
        a.buf.place_cursor(&a.buf.iter_at_offset(after_chip));
        // B types at the very start (sequence offset 0).
        type_at(&b.buf, 0, ">> ");
        deliver_chips(&b, &a, "B typed before the chip");
        assert_eq!(text(&a.buf), format!(">> {before}"));
        assert_eq!(caret(&a.buf), after_chip + 3, "the caret is still right after the chip");
        // The caret in the list item.
        a.buf.place_cursor(&a.buf.iter_at_offset(in_item + 3));
        type_at(&b.buf, 0, "# ");
        deliver_chips(&b, &a, "B typed again");
        assert_eq!(caret(&a.buf), in_item + 5);
        // B types inside the item, before A's caret.
        let in_b = at(&b.buf, "one");
        type_at(&b.buf, in_b, "NEW ");
        deliver_chips(&b, &a, "B typed in the item");
        assert_eq!(caret(&a.buf), in_item + 5 + 4);
        assert_eq!(a.live.borrow().document().paragraphs, b.live.borrow().document().paragraphs);
    });
}

/// How far buffer offsets are from sequence offsets in the documents Letters
/// opens: what using one for the other (as the first code did) gets wrong.
/// `--success-output final` prints it.
#[test]
fn collab_offsets_buffer_offsets_are_not_sequence_offsets() {
    gtk_test(|| {
        let mut doc = chip_doc();
        for item in ["first", "second", "third"] {
            doc.paragraphs.push(letters_core::Paragraph {
                style: letters_core::ParaStyle { list: letters_core::ListKind::Numbered, ..Default::default() },
                runs: vec![Run::plain(item)],
            });
        }
        doc.paragraphs.push(letters_core::Paragraph { style: Default::default(), runs: vec![Run::plain("plain end")] });
        let (a, _b) = session(&doc);
        let total = a.buf.char_count();
        let mut model = a.live.borrow_mut();
        let (mut differs, mut round_trips) = (0, 0);
        for off in 0..=total {
            let seq = model.sequence_offset(off as usize);
            differs += i32::from(seq != off as usize);
            // A position that is a character of the text maps back to itself.
            round_trips += i32::from(model.buffer_offset(seq) == Some(off as usize));
        }
        println!(
            "collab-offsets: in a document with a date chip and a numbered list, {differs} of {} buffer offsets are not their own sequence offsets (the chip and the markers); {round_trips} of them map back to themselves through the sequence",
            total + 1
        );
        assert!(differs > 0, "the sample must exercise the mapping");
    });
}
