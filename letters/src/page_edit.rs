// SPDX-License-Identifier: GPL-3.0-or-later
//
// page_edit.rs — editing on the Print Layout view (ADR 0010, stage 3).
//
// The tab's GtkTextBuffer is still the document's live state, so the page
// view edits *it*: a click places the buffer's cursor at the character the
// laid-out page shows there, typing goes through a GtkIMContext into the
// buffer, and the view lays the document out again after every change.
// Undo, formatting actions, find and save all keep working unchanged,
// because they act on the same buffer.
//
// Enter continues and ends lists as in Draft (`bridge::enter_in_list`);
// Markdown shortcuts act on the buffer, so they work here too; the suite
// clipboard format is connected by doc_tab; screen readers read the view
// through GtkAccessibleText (page_view.rs). The Draft view stays the
// fallback until the live model (ADR 0010 stage 3c).

use gtk4::{self as gtk, gdk, glib, prelude::*};

use crate::page_view::PageView;

/// Make `view` edit `buf`.
pub fn make_editable(view: &PageView, buf: &gtk::TextBuffer) {
    view.set_buffer(buf);
    view.set_focusable(true);
    view.set_can_focus(true);

    // Caret and selection follow the buffer's marks, wherever they move.
    {
        let v = view.downgrade();
        buf.connect_mark_set(move |_, _, mark| {
            let Some(v) = v.upgrade() else { return };
            if matches!(mark.name().as_deref(), Some("insert") | Some("selection_bound")) {
                // A moved caret is shown at once, then blinks again.
                v.restart_blink();
            }
        });
    }

    // Click: place the cursor (Shift extends the selection). Drag: select.
    let press_anchor = std::rc::Rc::new(std::cell::Cell::new(None::<i32>));
    {
        let click = gtk::GestureClick::new();
        let (v, anchor) = (view.clone(), press_anchor.clone());
        click.connect_pressed(move |gesture, n_press, x, y| {
            v.grab_focus();
            let (Some(buf), Some(off)) = (v.buffer(), v.buffer_offset_at(x, y)) else { return };
            let iter = buf.iter_at_offset(off as i32);
            let extend = gesture.current_event_state().contains(gdk::ModifierType::SHIFT_MASK);
            match n_press {
                2 => select_word(&buf, &iter),
                3 => select_paragraph(&buf, &iter),
                _ if extend => buf.move_mark(&buf.get_insert(), &iter),
                _ => buf.place_cursor(&iter),
            }
            anchor.set(Some(buf.iter_at_mark(&buf.selection_bound()).offset()));
        });
        view.add_controller(click);
    }
    {
        let drag = gtk::GestureDrag::new();
        let v = view.clone();
        drag.connect_drag_update(move |gesture, dx, dy| {
            let Some((sx, sy)) = gesture.start_point() else { return };
            let (Some(buf), Some(off)) = (v.buffer(), v.buffer_offset_at(sx + dx, sy + dy)) else { return };
            buf.move_mark(&buf.get_insert(), &buf.iter_at_offset(off as i32));
            scroll_to_caret(&v);
        });
        view.add_controller(drag);
    }

    // Keys: text through the input method, editing and movement here,
    // everything with Ctrl or Alt left to the application's shortcuts.
    let im = gtk::IMMulticontext::new();
    im.set_client_widget(Some(view));
    {
        let v = view.clone();
        im.connect_commit(move |_, text| {
            if let Some(buf) = v.buffer() {
                insert_text(&buf, text);
                scroll_to_caret(&v);
            }
        });
    }
    let keys = gtk::EventControllerKey::new();
    keys.set_im_context(Some(&im));
    {
        let v = view.clone();
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(buf) = v.buffer() else { return glib::Propagation::Proceed };
            let handled = handle_key(&v, &buf, key, state);
            if handled {
                scroll_to_caret(&v);
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
    }
    view.add_controller(keys);
    {
        let focus = gtk::EventControllerFocus::new();
        let (i1, v1) = (im.clone(), view.clone());
        focus.connect_enter(move |_| {
            i1.focus_in();
            v1.restart_blink();
        });
        let (i2, v2) = (im.clone(), view.clone());
        focus.connect_leave(move |_| {
            i2.focus_out();
            v2.restart_blink();
        });
        view.add_controller(focus);
    }
    // Tell the input method where the caret is (candidate windows).
    {
        let (v, im) = (view.downgrade(), im.clone());
        buf.connect_mark_set(move |b, _, mark| {
            let Some(v) = v.upgrade() else { return };
            if mark.name().as_deref() != Some("insert") {
                return;
            }
            let off = b.iter_at_mark(mark).offset().max(0) as usize;
            if let Some((x, y, h)) = v.caret_rect(off) {
                im.set_cursor_location(&gdk::Rectangle::new(x as i32, y as i32, 1, h.ceil() as i32));
            }
        });
    }
}

/// Replace the selection (if any) with `text`, as typing does.
/// Type `text` at the caret, as a key press on the page view does.
#[cfg(test)]
pub(crate) fn type_text(buf: &gtk::TextBuffer, text: &str) {
    insert_text(buf, text);
}

fn insert_text(buf: &gtk::TextBuffer, text: &str) {
    // Model first (ADR 0010 stage 3c-3): the text is an op on the live
    // model, styled by the marks' expand rules, and the buffer follows.
    if let Some(m) = crate::live::of(buf) {
        let mut m = m.borrow_mut();
        let (s, e) = selection_offsets(buf);
        let (s, e) = (m.sequence_offset(buf, s), m.sequence_offset(buf, e));
        let doc = m.document(buf).clone();
        let mut ops = Vec::new();
        let mut scratch = doc.clone();
        if e > s {
            let del = letters_core::edit::Op::Delete { at: s, len: e - s };
            if letters_core::edit::apply(&mut scratch, &del).is_err() {
                return;
            }
            ops.push(del);
        }
        let Some(typed) = typed_ops(&scratch, s, text) else { return };
        ops.extend(typed);
        let word = text.chars().count() == 1 && !text.chars().any(char::is_whitespace) && e == s;
        m.apply_user_ops(buf, &ops, word);
        drop(m);
        // Markdown shortcuts, as in Draft: "**bold**" and a space.
        if text == " " {
            let at = buf.iter_at_mark(&buf.get_insert());
            let mut before = at;
            before.backward_char();
            crate::actions::markdown_macro_at(buf, &before);
        }
        return;
    }
    buf.begin_user_action();
    buf.delete_selection(true, true);
    buf.insert_interactive_at_cursor(text, true);
    buf.end_user_action();
}

/// The selection (or caret) as buffer offsets, in order.
fn selection_offsets(buf: &gtk::TextBuffer) -> (usize, usize) {
    let a = buf.iter_at_mark(&buf.get_insert()).offset().max(0) as usize;
    let b = buf.iter_at_mark(&buf.selection_bound()).offset().max(0) as usize;
    (a.min(b), a.max(b))
}

/// The ops typing `text` at sequence offset `at` makes. Enter in an empty
/// list item ends the list instead of adding another item.
fn typed_ops(doc: &letters_core::Document, at: usize, text: &str) -> Option<Vec<letters_core::edit::Op>> {
    use letters_core::edit::{self, Op};
    if text == "\n" {
        let (pi, _) = edit::locate(doc, at)?;
        let p = &doc.paragraphs[pi];
        if p.style.list != letters_core::ListKind::None && p.runs.is_empty() {
            let style = letters_core::ParaStyle { list: letters_core::ListKind::None, list_level: 0, ..p.style.clone() };
            return Some(vec![Op::SetParaStyle { at, style }]);
        }
    }
    Some(vec![edit::typing(doc, at, text)?])
}

/// Backspace (`forward` false) or Delete at the caret, or over the
/// selection, as a model op.
fn delete_ops(buf: &gtk::TextBuffer, m: &mut crate::live::LiveModel, forward: bool) -> Option<letters_core::edit::Op> {
    use letters_core::edit::{doc_len, Op};
    let (s, e) = selection_offsets(buf);
    let (s, e) = (m.sequence_offset(buf, s), m.sequence_offset(buf, e));
    if e > s {
        return Some(Op::Delete { at: s, len: e - s });
    }
    let len = doc_len(m.document(buf));
    match forward {
        false if s > 0 => Some(Op::Delete { at: s - 1, len: 1 }),
        true if s < len => Some(Op::Delete { at: s, len: 1 }),
        _ => None,
    }
}

fn select_word(buf: &gtk::TextBuffer, at: &gtk::TextIter) {
    let (mut start, mut end) = (*at, *at);
    if !start.starts_word() {
        start.backward_word_start();
    }
    if !end.ends_word() {
        end.forward_word_end();
    }
    buf.select_range(&end, &start);
}

fn select_paragraph(buf: &gtk::TextBuffer, at: &gtk::TextIter) {
    let mut start = *at;
    start.set_line_offset(0);
    let mut end = *at;
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    buf.select_range(&end, &start);
}

/// Move the cursor to `off`, extending the selection when `extend`.
fn move_to(buf: &gtk::TextBuffer, off: i32, extend: bool) {
    let iter = buf.iter_at_offset(off);
    if extend {
        buf.move_mark(&buf.get_insert(), &iter);
    } else {
        buf.place_cursor(&iter);
    }
}

/// Handle one key press; `true` when it was an editing or movement key.
fn handle_key(view: &PageView, buf: &gtk::TextBuffer, key: gdk::Key, state: gdk::ModifierType) -> bool {
    let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
    let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
    if state.contains(gdk::ModifierType::ALT_MASK) {
        return false;
    }
    let cursor = buf.iter_at_mark(&buf.get_insert());
    let off = cursor.offset();
    let has_selection = buf.has_selection();
    match key {
        gdk::Key::Left | gdk::Key::Right => {
            let forward = key == gdk::Key::Right;
            let mut it = cursor;
            if has_selection && !shift {
                // Collapse to the selection's edge on that side.
                if let Some((s, e)) = buf.selection_bounds() {
                    buf.place_cursor(if forward { &e } else { &s });
                }
                return true;
            }
            match (forward, ctrl) {
                (true, false) => { it.forward_cursor_position(); }
                (false, false) => { it.backward_cursor_position(); }
                (true, true) => { it.forward_word_end(); }
                (false, true) => { it.backward_word_start(); }
            }
            move_to(buf, it.offset(), shift);
            true
        }
        gdk::Key::Up | gdk::Key::Down => {
            let dir = if key == gdk::Key::Up { -1 } else { 1 };
            let target = view.offset_on_adjacent_line(off as usize, dir).map(|o| o as i32).unwrap_or(if dir < 0 {
                0
            } else {
                buf.char_count()
            });
            move_to(buf, target, shift);
            true
        }
        gdk::Key::Home | gdk::Key::End => {
            let target = if ctrl {
                if key == gdk::Key::Home { 0 } else { buf.char_count() }
            } else {
                let Some((start, end)) = view.line_bounds(off as usize) else { return true };
                (if key == gdk::Key::Home { start } else { end }) as i32
            };
            move_to(buf, target, shift);
            true
        }
        gdk::Key::BackSpace | gdk::Key::Delete | gdk::Key::KP_Delete => {
            if let Some(m) = crate::live::of(buf) {
                let mut m = m.borrow_mut();
                // An op the model refuses (joining a table's cells) does
                // nothing, as in a word processor.
                if let Some(op) = delete_ops(buf, &mut m, key != gdk::Key::BackSpace) {
                    m.apply_user_ops(buf, &[op], false);
                }
                return true;
            }
            buf.begin_user_action();
            if !buf.delete_selection(true, true) {
                let mut it = cursor;
                if key == gdk::Key::BackSpace {
                    buf.backspace(&mut it, true, true);
                } else {
                    let mut end = cursor;
                    if end.forward_cursor_position() {
                        buf.delete_interactive(&mut it, &mut end, true);
                    }
                }
            }
            buf.end_user_action();
            true
        }
        gdk::Key::Return | gdk::Key::KP_Enter if !ctrl => {
            // With a live model, Enter is a paragraph split op (a list item
            // continues its list; an empty one ends it). Without, the
            // buffer's list continuation.
            if crate::live::of(buf).is_some() || shift || !crate::bridge::enter_in_list(buf) {
                insert_text(buf, "\n");
            }
            true
        }
        gdk::Key::Tab if !ctrl => {
            insert_text(buf, "\t");
            true
        }
        gdk::Key::a if ctrl => {
            buf.select_range(&buf.end_iter(), &buf.start_iter());
            true
        }
        gdk::Key::c | gdk::Key::x | gdk::Key::v if ctrl => {
            let clipboard = view.clipboard();
            match key {
                gdk::Key::c => buf.copy_clipboard(&clipboard),
                gdk::Key::x => buf.cut_clipboard(&clipboard, true),
                _ => buf.paste_clipboard(&clipboard, None, true),
            }
            true
        }
        _ => false,
    }
}

/// Scroll the view's scrolled window so the caret is visible.
pub(crate) fn scroll_to_caret(view: &PageView) {
    let v = view.clone();
    // After the relayout the edit triggers, so the caret is where it will
    // be drawn.
    glib::idle_add_local_once(move || {
        let Some(buf) = v.buffer() else { return };
        let off = buf.iter_at_mark(&buf.get_insert()).offset().max(0) as usize;
        let Some((_, y, h)) = v.caret_rect(off) else { return };
        let Some(adj) = v.parent().and_then(|p| p.downcast::<gtk::Viewport>().ok()).and_then(|vp| vp.vadjustment()) else {
            return;
        };
        let margin = 24.0;
        if y - margin < adj.value() {
            adj.set_value((y - margin).max(0.0));
        } else if y + h + margin > adj.value() + adj.page_size() {
            adj.set_value(y + h + margin - adj.page_size());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use letters_core::layout::pango::Typeset;
    use suite_common::gtk_test::run as gtk_test;

    /// A laid-out page view over a buffer, as a tab builds it.
    fn editable(text: &str) -> (PageView, gtk::TextBuffer) {
        let buf = gtk::TextBuffer::new(None);
        crate::actions::register_formatting_tags(&buf);
        crate::bridge::render_to_buffer(&letters_core::Document::from_plain_text(text), &buf);
        let view = PageView::new();
        make_editable(&view, &buf);
        let (doc, starts) = crate::bridge::capture_with_starts(&buf);
        view.set_typeset(Typeset::new(doc, Default::default()), starts);
        (view, buf)
    }

    #[test]
    fn a_click_on_the_page_places_the_buffer_cursor_there() {
        gtk_test(|| {
            let (view, buf) = editable("first paragraph\nsecond paragraph");
            // The caret rectangle of offset 20 ("second pa|ragraph"), clicked
            // just right of its left edge, must hit offset 20 again.
            let (x, y, h) = view.caret_rect(20).expect("a caret for offset 20");
            let off = view.buffer_offset_at(x + 0.5, y + h / 2.0).expect("a hit");
            assert_eq!(off, 20);
            buf.place_cursor(&buf.iter_at_offset(off as i32));
            assert_eq!(buf.iter_at_mark(&buf.get_insert()).offset(), 20);
        });
    }

    #[test]
    fn arrow_keys_move_through_lines_and_home_end_to_line_edges() {
        gtk_test(|| {
            let (view, buf) = editable("alpha\nbeta gamma\ndelta");
            buf.place_cursor(&buf.iter_at_offset(2)); // al|pha
            assert!(handle_key(&view, &buf, gdk::Key::Down, gdk::ModifierType::empty()));
            let on_second = buf.iter_at_mark(&buf.get_insert()).offset();
            assert!((6..=10).contains(&on_second), "down lands on the second line: {on_second}");
            assert!(handle_key(&view, &buf, gdk::Key::End, gdk::ModifierType::empty()));
            assert_eq!(buf.iter_at_mark(&buf.get_insert()).offset(), 16, "end of 'beta gamma'");
            assert!(handle_key(&view, &buf, gdk::Key::Home, gdk::ModifierType::SHIFT_MASK));
            let (s, e) = buf.selection_bounds().expect("shift+home selects");
            assert_eq!((s.offset(), e.offset()), (6, 16));
            assert!(handle_key(&view, &buf, gdk::Key::Right, gdk::ModifierType::empty()));
            assert_eq!(buf.iter_at_mark(&buf.get_insert()).offset(), 16, "right collapses a selection to its end");
        });
    }

    /// Screen readers read the page view's text, caret, words and laid-out
    /// lines through GtkAccessibleText.
    #[test]
    fn the_page_view_exposes_its_text_to_assistive_technologies() {
        use gtk::subclass::prelude::*;
        gtk_test(|| {
            let (view, buf) = editable("alpha beta\ngamma");
            let imp = view.imp();
            let bytes = imp.contents(0, 16).expect("contents");
            assert_eq!(std::str::from_utf8(&bytes).unwrap(), "alpha beta\ngamma");
            buf.place_cursor(&buf.iter_at_offset(7));
            assert_eq!(imp.caret_position(), 7);
            let (s, e, word) = imp.contents_at(7, gtk::AccessibleTextGranularity::Word).expect("a word");
            assert_eq!((s, e, std::str::from_utf8(&word).unwrap()), (6, 10, "beta"));
            let (s, e, _) = imp.contents_at(12, gtk::AccessibleTextGranularity::Line).expect("a line");
            assert_eq!((s, e), (11, 16), "the laid-out line holding 'gamma'");
            buf.select_range(&buf.iter_at_offset(0), &buf.iter_at_offset(5));
            let sel = imp.selection();
            assert_eq!((sel[0].start(), sel[0].length()), (0, 5));
        });
    }

    #[test]
    fn the_caret_blinks_per_the_gtk_settings_and_is_solid_without_focus() {
        gtk_test(|| {
            let (view, buf) = editable("hello");
            let win = gtk::Window::new();
            win.set_child(Some(&view));
            win.present();
            let settings = view.settings();
            settings.set_gtk_cursor_blink(true);
            settings.set_gtk_cursor_blink_time(200);
            view.grab_focus();
            let ctx = glib::MainContext::default();
            let until = |ms: u64| {
                let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
                while std::time::Instant::now() < end {
                    ctx.iteration(false);
                }
            };
            buf.place_cursor(&buf.iter_at_offset(2));
            assert!(view.caret_visible(), "a moved caret shows at once");
            assert!(view.is_focus(), "the page view has its window's focus");
            {
                // Over several half-periods the caret is off at least once.
                let mut saw_off = false;
                for _ in 0..8 {
                    until(60);
                    saw_off |= !view.caret_visible();
                }
                assert!(saw_off, "the caret blinks");
            }
            // gtk-cursor-blink off: solid.
            settings.set_gtk_cursor_blink(false);
            view.restart_blink();
            until(300);
            assert!(view.caret_visible(), "no blinking when the setting is off");
            win.destroy();
        });
    }

    #[test]
    fn typing_and_deleting_edit_the_buffer() {
        gtk_test(|| {
            let (view, buf) = editable("hello");
            buf.place_cursor(&buf.end_iter());
            insert_text(&buf, " world");
            assert!(handle_key(&view, &buf, gdk::Key::BackSpace, gdk::ModifierType::empty()));
            assert!(handle_key(&view, &buf, gdk::Key::Return, gdk::ModifierType::empty()));
            let text = buf.text(&buf.start_iter(), &buf.end_iter(), false);
            assert_eq!(text, "hello worl\n");
            // Formatting shortcuts are the application's, not the view's.
            assert!(!handle_key(&view, &buf, gdk::Key::b, gdk::ModifierType::CONTROL_MASK));
        });
    }
}
