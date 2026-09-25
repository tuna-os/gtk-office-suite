// SPDX-License-Identifier: GPL-3.0-or-later
//
// live.rs — the tab's live GTK-free document (ADR 0010 stage 3c-2,
// RFC-0001 Phase 0).
//
// A `LiveModel` holds a `letters_core::Document` that follows the tab's
// GtkTextBuffer edit by edit: each buffer insertion or deletion inside a
// paragraph's text becomes a `letters_core::edit` op on the model, in the
// same char sequence a CRDT would replicate. The page view lays that model
// out, so typing no longer re-captures the whole buffer.
//
// Edits the mapping cannot follow exactly — a paragraph break typed or
// deleted, anything touching a list marker, a table's pipes, an inline
// image or footnote marker, or a formatting tag applied or removed — mark
// the model stale, and the next read re-captures it from the buffer. So the
// model is always exactly `capture_from_buffer`'s result (tested), and the
// common case, typing and deleting inside a paragraph, is incremental.
//
// The buffer is still the source of truth; the next step (3c-3) inverts
// that: edits go to the model first and the buffer becomes Draft's view.

use gtk4::{self as gtk, glib, prelude::*};
use letters_core::edit::{self, Op};
use letters_core::layout::is_object;
use letters_core::{Document, Paragraph, Run};
use std::cell::RefCell;
use std::rc::Rc;

/// Tags that are not document content (capture ignores them), so applying
/// or removing them leaves the model as it is.
const PRESENTATION_TAGS: [&str; 4] = ["spelling-error", "search-match", "search-current", "tab-stops"];

#[derive(Default)]
pub struct LiveModel {
    doc: Document,
    /// Buffer offset where each paragraph's text starts.
    starts: Vec<usize>,
    stale: bool,
    /// Edits followed incrementally, and re-captures, since creation.
    pub followed: usize,
    pub recaptures: usize,
}

impl LiveModel {
    /// A live model following `buf`.
    pub fn attach(buf: &gtk::TextBuffer) -> Rc<RefCell<LiveModel>> {
        let model = Rc::new(RefCell::new(LiveModel { stale: true, ..Default::default() }));
        {
            // After the default handler, so the inserted text's tags are
            // known: text typed inside a bold range is bold in the buffer,
            // text typed at its end is not, and the model must say the same.
            let m = Rc::downgrade(&model);
            buf.connect_closure(
                "insert-text",
                true,
                glib::closure_local!(move |_: gtk::TextBuffer, end: gtk::TextIter, text: &str, _len: i32| {
                    let Some(m) = m.upgrade() else { return };
                    let n = text.chars().count() as i32;
                    let mut start = end;
                    start.backward_chars(n);
                    let style = crate::bridge::run_style_at(&start);
                    m.borrow_mut().inserted(start.offset().max(0) as usize, text, style);
                }),
            );
        }
        {
            let m = Rc::downgrade(&model);
            buf.connect_delete_range(move |_, s, e| {
                if let Some(m) = m.upgrade() {
                    m.borrow_mut().deleted(s.offset().max(0) as usize, e.offset().max(0) as usize);
                }
            });
        }
        let stale_on = |model: &Rc<RefCell<LiveModel>>| {
            let m = Rc::downgrade(model);
            move || {
                if let Some(m) = m.upgrade() {
                    m.borrow_mut().stale = true;
                }
            }
        };
        {
            let f = stale_on(&model);
            buf.connect_insert_paintable(move |_, _, _| f());
        }
        {
            let f = stale_on(&model);
            buf.connect_insert_child_anchor(move |_, _, _| f());
        }
        for remove in [false, true] {
            let f = stale_on(&model);
            let handler = move |_: &gtk::TextBuffer, tag: &gtk::TextTag, _: &gtk::TextIter, _: &gtk::TextIter| {
                if !tag.name().is_some_and(|n| PRESENTATION_TAGS.contains(&n.as_str())) {
                    f();
                }
            };
            if remove {
                buf.connect_remove_tag(handler);
            } else {
                buf.connect_apply_tag(handler);
            }
        }
        model
    }

    /// The paragraph holding buffer offset `off` when an edit there can be
    /// followed: inside (or at either end of) a text-only, non-table
    /// paragraph's text. Returns the paragraph and the offset within it.
    fn plain_position(&self, off: usize) -> Option<(usize, usize)> {
        if self.stale {
            return None;
        }
        let (para, offset) = crate::bridge::paragraph_offset(&self.doc, &self.starts, off);
        let p = self.doc.paragraphs.get(para)?;
        let start = *self.starts.get(para)?;
        let len = edit::seq_len(&p.runs);
        let simple = p.style.table_cell.is_none() && !p.runs.iter().any(is_object);
        (simple && off >= start && off - start == offset && offset <= len).then_some((para, offset))
    }

    fn inserted(&mut self, off: usize, text: &str, style: letters_core::RunStyle) {
        let Some((para, offset)) = self.plain_position(off).filter(|_| !text.contains('\n')) else {
            self.stale = true;
            return;
        };
        let n = text.chars().count();
        let op = Op::Insert {
            at: edit::paragraph_start(&self.doc, para) + offset,
            content: vec![Paragraph { style: Default::default(), runs: vec![Run { text: text.to_string(), style }] }],
        };
        if edit::apply(&mut self.doc, &op).is_err() {
            self.stale = true;
            return;
        }
        for s in &mut self.starts[para + 1..] {
            *s += n;
        }
        self.followed += 1;
    }

    fn deleted(&mut self, from: usize, to: usize) {
        if to <= from {
            return;
        }
        let (Some((para, a)), Some((para_b, b))) = (self.plain_position(from), self.plain_position(to)) else {
            self.stale = true;
            return;
        };
        if para != para_b {
            self.stale = true;
            return;
        }
        let op = Op::Delete { at: edit::paragraph_start(&self.doc, para) + a, len: b - a };
        if edit::apply(&mut self.doc, &op).is_err() {
            self.stale = true;
            return;
        }
        for s in &mut self.starts[para + 1..] {
            *s -= to - from;
        }
        self.followed += 1;
    }

    /// The current document and its paragraphs' buffer starts, re-captured
    /// from `buf` first if an edit could not be followed.
    pub fn snapshot(&mut self, buf: &gtk::TextBuffer) -> (Document, Vec<usize>) {
        if self.stale {
            let (doc, starts) = crate::bridge::capture_with_starts(buf);
            self.doc = doc;
            self.starts = starts;
            self.stale = false;
            self.recaptures += 1;
        } else {
            // Header, footer, page setup and footnotes live beside the text
            // and change without a buffer edit.
            crate::bridge::read_sidecars(buf, &mut self.doc);
        }
        (self.doc.clone(), self.starts.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use suite_common::gtk_test::run as gtk_test;

    /// After every kind of buffer edit, the live model is exactly what a
    /// full capture of the buffer gives; typing and deleting inside a
    /// paragraph are followed without re-capturing.
    #[test]
    fn the_live_model_is_always_the_captured_document() {
        gtk_test(|| {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            let mut d = Document::from_plain_text("Title\nfirst item\nsecond item\nbody text here");
            d.paragraphs[0].style.heading = Some(1);
            for p in &mut d.paragraphs[1..3] {
                p.style.list = letters_core::ListKind::Numbered;
            }
            crate::bridge::render_to_buffer(&d, &buf);
            let live = LiveModel::attach(&buf);
            let check = |what: &str| {
                let (doc, starts) = live.borrow_mut().snapshot(&buf);
                let (want, want_starts) = crate::bridge::capture_with_starts(&buf);
                assert_eq!(doc, want, "after {what}");
                assert_eq!(starts, want_starts, "starts after {what}");
            };
            check("attach");
            let base = live.borrow().recaptures;

            let at = |needle: &str| {
                let text = buf.text(&buf.start_iter(), &buf.end_iter(), false).to_string();
                text[..text.find(needle).unwrap()].chars().count() as i32
            };
            // Typing and deleting inside paragraphs, including list items.
            let mut it = buf.iter_at_offset(at("item"));
            buf.insert(&mut it, "numbered ");
            check("typing in a list item");
            let mut it = buf.iter_at_offset(at("here"));
            buf.insert(&mut it, "right ");
            check("typing in the last paragraph");
            let (mut s, mut e) = (buf.iter_at_offset(at("body")), buf.iter_at_offset(at("body") + 5));
            buf.delete(&mut s, &mut e);
            check("deleting a word");
            let mut end = buf.end_iter();
            buf.insert(&mut end, "!");
            check("typing at the end");
            assert_eq!(live.borrow().recaptures, base, "all of that was followed incrementally");
            assert_eq!(live.borrow().followed, 4);

            // Edits that are re-captured.
            let mut it = buf.iter_at_offset(at("text"));
            buf.insert(&mut it, "\n");
            check("a paragraph break");
            let (s, e) = (buf.iter_at_offset(at("Title")), buf.iter_at_offset(at("Title") + 3));
            buf.apply_tag_by_name("bold", &s, &e);
            check("bold applied");
            let (mut s, mut e) = (buf.iter_at_offset(at("second") - 1), buf.iter_at_offset(at("second") + 2));
            buf.delete(&mut s, &mut e);
            check("deleting across a list marker and a break");
            buf.set_text("replaced");
            check("replacing everything");
            // Presentation tags change nothing.
            let before = live.borrow().recaptures;
            let (s, e) = (buf.start_iter(), buf.end_iter());
            buf.apply_tag_by_name("search-match", &s, &e);
            check("a search highlight");
            assert_eq!(live.borrow().recaptures, before);
        });
    }

    /// Random buffer edits (seeded): typing, paragraph breaks, deletions
    /// and formatting anywhere, including list markers and table pipes. The
    /// live model must equal a full capture after every one.
    #[test]
    fn random_edits_never_leave_the_live_model_behind() {
        gtk_test(|| {
          for start_seed in [0x2545_f491_4f6c_dd1du64, 0x9e37_79b9_7f4a_7c15, 0xdead_beef_cafe_f00d, 7, 12345] {
            let buf = gtk::TextBuffer::new(None);
            crate::actions::register_formatting_tags(&buf);
            let mut d = Document::from_plain_text("intro text\nitem one\nitem two\nclosing words");
            d.paragraphs[1].style.list = letters_core::ListKind::Bullet;
            d.paragraphs[2].style.list = letters_core::ListKind::Bullet;
            let t = d.insert_table_at(3, 1, 2);
            let _ = t;
            crate::bridge::render_to_buffer(&d, &buf);
            let live = LiveModel::attach(&buf);
            let mut seed = start_seed;
            let mut next = |n: u64| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed % n.max(1)
            };
            for step in 0..300 {
                let len = buf.char_count().max(0) as u64;
                match next(10) {
                    0..=4 => {
                        let text = ["a", "bc", " ", "xyz", "\n", "|"][next(6) as usize];
                        let mut it = buf.iter_at_offset(next(len + 1) as i32);
                        buf.insert(&mut it, text);
                    }
                    5..=7 => {
                        let a = next(len + 1) as i32;
                        let b = (a + next(4) as i32).min(len as i32);
                        let (mut s, mut e) = (buf.iter_at_offset(a), buf.iter_at_offset(b));
                        buf.delete(&mut s, &mut e);
                    }
                    _ => {
                        let a = next(len + 1) as i32;
                        let (s, e) = (buf.iter_at_offset(a), buf.iter_at_offset((a + 3).min(len as i32)));
                        buf.apply_tag_by_name("italic", &s, &e);
                    }
                }
                if next(3) == 0 {
                    let (doc, starts) = live.borrow_mut().snapshot(&buf);
                    let (want, want_starts) = crate::bridge::capture_with_starts(&buf);
                    assert_eq!(doc, want, "seed {start_seed} step {step}");
                    assert_eq!(starts, want_starts, "starts at step {step}");
                }
            }
            let l = live.borrow();
            assert!(l.followed > 30, "most typing was followed incrementally: {} followed, {} recaptured", l.followed, l.recaptures);
          }
        });
    }
}
