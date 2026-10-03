// SPDX-License-Identifier: GPL-3.0-or-later
//
// live.rs — the tab's live GTK-free document, and the source of truth
// (ADR 0010 stage 3c-3, RFC-0001 Phase 0).
//
// A `LiveModel` holds the tab's `letters_core::Document`. Every change is a
// `letters_core::edit` op on it, and its `History` (the ops' inverses) is
// the tab's undo and redo; Save, the page view and copy read the model.
//
// The GtkTextBuffer beside it is a projection: the model writes it
// (`project`), and never reads it (#1202 stage 4). An opened document goes
// into the model as it was read from the file, and the buffer offset where
// each paragraph's text starts comes from rendering it
// (`bridge::render_document_paragraphs`). Reading the document back out of
// the buffer — which it used to do on every open — changed what was
// opened: the buffer cannot hold a table cell's spacing or alignment, a
// line break inside a paragraph, or a cell of two paragraphs, and text that
// read as a list marker became a list.
//
// Nothing else edits the buffer. An edit that does is counted
// (`foreign_edits`), and the tests hold that count at zero.

use gtk4::{self as gtk, gio, glib, prelude::*};
use letters_core::edit::{self, History, Op};
use letters_core::{Document, Paragraph};
use std::cell::RefCell;
use std::rc::Rc;

/// Tags that are not document content: find and spelling highlights,
/// which only change how text looks.
const PRESENTATION_TAGS: [&str; 4] = ["spelling-error", "search-match", "search-current", "tab-stops"];

const KEY: &str = "letters-live-model";

#[derive(Default)]
pub struct LiveModel {
    doc: Document,
    /// Buffer offset where each paragraph's text starts.
    starts: Vec<usize>,
    /// The buffer is being changed from the model.
    projecting: bool,
    history: History,
    /// Edits made to the buffer by anything but the model. Each is a bug:
    /// the document does not have it.
    pub foreign_edits: usize,
    /// Track changes: the author edits are recorded as, or None when
    /// edits apply directly (`letters_core::track`).
    pub tracking: Option<String>,
}

/// The live model of `buf`, if it has one.
pub fn of(buf: &gtk::TextBuffer) -> Option<Rc<RefCell<LiveModel>>> {
    unsafe { buf.data::<Rc<RefCell<LiveModel>>>(KEY).map(|p| p.as_ref().clone()) }
}

impl LiveModel {
    /// Give `buf` a live model holding `doc`, and show it. Its undo
    /// replaces the buffer's own.
    pub fn attach(buf: &gtk::TextBuffer, doc: Document) -> Rc<RefCell<LiveModel>> {
        let model = Rc::new(RefCell::new(LiveModel::default()));
        unsafe { buf.set_data(KEY, model.clone()) };
        buf.set_enable_undo(false);
        let foreign = |model: &Rc<RefCell<LiveModel>>| {
            let m = Rc::downgrade(model);
            move || {
                if let Some(Ok(mut m)) = m.upgrade().as_ref().map(|m| m.try_borrow_mut()) {
                    if !m.projecting {
                        m.foreign_edits += 1;
                    }
                }
            }
        };
        {
            let f = foreign(&model);
            buf.connect_insert_text(move |_, _, _| f());
        }
        {
            let f = foreign(&model);
            buf.connect_delete_range(move |_, _, _| f());
        }
        for signal in ["apply-tag", "remove-tag"] {
            let f = foreign(&model);
            buf.connect_closure(
                signal,
                false,
                glib::closure_local!(move |_b: gtk::TextBuffer, tag: gtk::TextTag, _s: gtk::TextIter, _e: gtk::TextIter| {
                    if !tag.name().is_some_and(|n| PRESENTATION_TAGS.contains(&n.as_str())) {
                        f();
                    }
                }),
            );
        }
        {
            let f = foreign(&model);
            buf.connect_insert_paintable(move |_, _, _| f());
        }
        {
            let f = foreign(&model);
            buf.connect_insert_child_anchor(move |_, _, _| f());
        }
        {
            // A change (the model's own) enables Undo and Redo to match.
            buf.connect_changed(|b| {
                let b = b.clone();
                glib::idle_add_local_once(move || sync_actions(&b));
            });
        }
        model.borrow_mut().show(buf, doc);
        model
    }

    /// Make `doc` the model's document, with no history, and render it
    /// into the buffer.
    fn show(&mut self, buf: &gtk::TextBuffer, doc: Document) {
        self.history.clear();
        self.projecting = true;
        self.starts = crate::bridge::render_to_buffer(&doc, buf);
        self.projecting = false;
        self.doc = doc;
    }

    /// The current document and its paragraphs' buffer starts.
    pub fn snapshot(&self) -> (Document, Vec<usize>) {
        (self.doc.clone(), self.starts.clone())
    }

    /// Update the buffer to the model, after the model
    /// changed from `old`. Only the changed paragraphs' lines are
    /// re-rendered; a change to a table or an inline object re-renders the
    /// whole buffer. The caret goes to the end of the change.
    fn project(&mut self, buf: &gtk::TextBuffer, old: &Document) {
        crate::bridge::set_trailing_style(buf, &self.doc);
        // Comments live beside the text: keep the buffer's copy current.
        let comments_changed = old.comments != self.doc.comments;
        if comments_changed {
            crate::bridge::set_comments(buf, &self.doc.comments);
        }
        // So do the header, footer, footnotes and page setup.
        let notes_changed = (&old.header, &old.footer, &old.footnotes, &old.page) != (&self.doc.header, &self.doc.footer, &self.doc.footnotes, &self.doc.page);
        if notes_changed {
            crate::bridge::set_page_furniture(buf, &self.doc);
        }
        let (pa, pb) = (&old.paragraphs, &self.doc.paragraphs);
        let head = pa.iter().zip(pb).take_while(|(x, y)| x == y).count();
        if head == pa.len() && head == pb.len() {
            if comments_changed || notes_changed {
                // No text changed, but the document did: the views follow
                // the buffer's "changed".
                buf.set_modified(true);
                buf.emit_by_name::<()>("changed", &[]);
            }
            return;
        }
        let max_tail = pa.len().min(pb.len()) - head;
        let tail = pa.iter().rev().zip(pb.iter().rev()).take(max_tail).take_while(|(x, y)| x == y).count();
        // The changed paragraphs, widened on both sides to whole tables and
        // whole lists (a changed item renumbers the rest), until the
        // widenings agree. `h` is the first; `ta` paragraphs at the end are
        // unchanged.
        let (mut h, mut t) = (head, tail);
        loop {
            let (sa, ea) = widen_for_render(pa, h, (pa.len() - t).max(h + 1) - 1);
            let (sb, eb) = widen_for_render(pb, h, (pb.len() - t).max(h + 1) - 1);
            let (nh, nt) = (sa.min(sb), (pa.len() - 1 - ea).min(pb.len() - 1 - eb).min(t));
            if (nh, nt) == (h, t) {
                break;
            }
            (h, t) = (nh, nt);
        }
        let (ea, eb) = (pa.len() - t, pb.len() - t);
        // A line range must not be empty on either side.
        if h == ea || h == eb {
            if h > 0 {
                h -= 1;
            } else {
                t = t.saturating_sub(1);
            }
        }
        let (ea, eb) = (pa.len() - t, pb.len() - t);
        let caret_para = head.min(pb.len() - 1);
        self.projecting = true;
        if self.starts.len() != pa.len() || ea <= h || eb <= h {
            self.starts = crate::bridge::render_to_buffer(&self.doc, buf);
        } else {
            // The old paragraphs' text: from the start of the line the
            // first one starts on (before any list marker) to the newline
            // before the line the first unchanged one starts on. Offsets,
            // not line counts: a paragraph's text may hold a line break.
            let line_start = |off: usize| {
                let mut it = buf.iter_at_offset(off as i32);
                it.set_line_offset(0);
                it
            };
            let mut s = line_start(self.starts[h]);
            let mut e = match self.starts.get(ea) {
                Some(&next) => {
                    let mut it = line_start(next);
                    it.backward_char();
                    it
                }
                None => buf.end_iter(),
            };
            let before = buf.char_count();
            let from = s.offset();
            buf.delete(&mut s, &mut e);
            let mut at = buf.iter_at_offset(from);
            let starts = crate::bridge::render_document_paragraphs(buf, &mut at, &pb[h..eb]);
            let delta = buf.char_count() as isize - before as isize;
            let rest: Vec<usize> = self.starts[ea..].iter().map(|x| (*x as isize + delta) as usize).collect();
            self.starts.truncate(h);
            self.starts.extend(starts);
            self.starts.extend(rest);
        }
        buf.set_modified(true);
        // The caret: after the change in the first changed paragraph.
        let (x, y) = (pa.get(caret_para), &pb[caret_para]);
        let chars = |p: &Paragraph| letters_core::layout::layout_text(&p.runs).chars().collect::<Vec<_>>();
        let cy = chars(y);
        let at = match x {
            Some(x) => {
                let cx = chars(x);
                let pre = cx.iter().zip(&cy).take_while(|(m, n)| m == n).count();
                let suf = cx[pre..].iter().rev().zip(cy[pre..].iter().rev()).take_while(|(m, n)| m == n).count();
                cy.len() - suf
            }
            None => cy.len(),
        };
        let off = crate::bridge::buffer_offset(y, self.starts[caret_para], at);
        buf.place_cursor(&buf.iter_at_offset(off as i32));
        self.projecting = false;
    }

    /// Apply `ops` made by the user (the page view) to the model as one undo
    /// step, then show them in the buffer. `false` if they don't apply.
    pub fn apply_user_ops(&mut self, buf: &gtk::TextBuffer, ops: &[Op], typing: bool) -> bool {
        // While tracking, a text edit is recorded as a tracked change (an
        // insertion marked, a deletion marked rather than removed); an edit
        // that is not a text edit (a table's structure) applies as it is.
        let tracked = self.tracking.as_deref().and_then(|author| letters_core::track::tracked(&self.doc, ops, author, &letters_core::track::now()));
        self.apply_ops(buf, tracked.as_deref().unwrap_or(ops), typing)
    }

    /// Accept (`accept`) or reject the tracked changes touching buffer
    /// offsets `from..to` (whole changes), as one undo step.
    pub fn resolve_changes(&mut self, buf: &gtk::TextBuffer, from: usize, to: usize, accept: bool) -> bool {
        let (a, b) = (self.sequence_offset(from), self.sequence_offset(to));
        let ops = letters_core::track::resolve(&self.doc, a, b, accept);
        !ops.is_empty() && self.apply_ops(buf, &ops, false)
    }

    /// Accept or reject every tracked change, as one undo step.
    pub fn resolve_all_changes(&mut self, buf: &gtk::TextBuffer, accept: bool) -> bool {
        let ops = letters_core::track::resolve_all(&self.doc, accept);
        !ops.is_empty() && self.apply_ops(buf, &ops, false)
    }

    /// Apply the ops `f` makes from the current document (comment ops from
    /// `letters_core::comments`, a header or footnotes), as one undo step.
    /// `false` if there were none.
    pub fn edit_with(&mut self, buf: &gtk::TextBuffer, f: impl FnOnce(&Document) -> Vec<Op>) -> bool {
        let ops = f(&self.doc);
        !ops.is_empty() && self.apply_ops(buf, &ops, false)
    }

    /// The comment threads (`letters_core::comments::threads`), each with
    /// its text's buffer range (None when the text was deleted).
    pub fn comment_threads(&mut self) -> Vec<(letters_core::comments::Thread, Option<(usize, usize)>)> {
        let at = |doc: &Document, starts: &[usize], seq: usize| {
            let (p, off) = edit::locate(doc, seq)?;
            Some(crate::bridge::buffer_offset(&doc.paragraphs[p], *starts.get(p)?, off))
        };
        letters_core::comments::threads(&self.doc)
            .into_iter()
            .map(|t| {
                let range = t.anchor.as_ref().and_then(|a| Some((at(&self.doc, &self.starts, a.start)?, at(&self.doc, &self.starts, a.end)?)));
                (t, range)
            })
            .collect()
    }

    /// The tracked changes, each with the buffer offset where it starts.
    pub fn changes(&mut self) -> Vec<(letters_core::track::Change, usize)> {
        letters_core::track::changes(&self.doc)
            .into_iter()
            .filter_map(|c| {
                let (p, off) = edit::locate(&self.doc, c.start)?;
                let at = crate::bridge::buffer_offset(&self.doc.paragraphs[p], *self.starts.get(p)?, off);
                Some((c, at))
            })
            .collect()
    }

    fn apply_ops(&mut self, buf: &gtk::TextBuffer, ops: &[Op], typing: bool) -> bool {
        let old = self.doc.clone();
        match edit::apply_all(&mut self.doc, ops) {
            Ok(inverse) => {
                self.history.set_merge(typing);
                self.history.record(inverse);
                self.project(buf, &old);
                true
            }
            Err(_) => false,
        }
    }

    /// Sequence offset of buffer offset `off`.
    pub fn sequence_offset(&mut self, off: usize) -> usize {
        let (para, offset) = crate::bridge::paragraph_offset(&self.doc, &self.starts, off);
        edit::paragraph_start(&self.doc, para) + offset
    }

    /// Buffer offset of sequence offset `at` (the inverse of
    /// `sequence_offset`).
    pub fn buffer_offset(&mut self, at: usize) -> Option<usize> {
        let (para, offset) = edit::locate(&self.doc, at)?;
        Some(crate::bridge::buffer_offset(&self.doc.paragraphs[para], *self.starts.get(para)?, offset))
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// The style of the paragraph at buffer offset `off`.
    pub fn paragraph_style_at(&mut self, off: usize) -> Option<letters_core::ParaStyle> {
        let (para, _) = crate::bridge::paragraph_offset(&self.doc, &self.starts, off);
        self.doc.paragraphs.get(para).map(|p| p.style.clone())
    }

    /// Restyle the paragraphs from buffer offset `from` to `to` with `f`,
    /// as one undo step of `SetParaStyle` ops.
    pub fn restyle(&mut self, buf: &gtk::TextBuffer, from: usize, to: usize, f: impl Fn(&letters_core::ParaStyle) -> letters_core::ParaStyle) -> bool {
        let first = crate::bridge::paragraph_offset(&self.doc, &self.starts, from).0;
        let last = crate::bridge::paragraph_offset(&self.doc, &self.starts, to).0;
        let ops: Vec<Op> = (first..=last.min(self.doc.paragraphs.len().saturating_sub(1)))
            .filter_map(|pi| {
                let style = f(&self.doc.paragraphs[pi].style);
                (style != self.doc.paragraphs[pi].style)
                    .then(|| Op::SetParaStyle { at: edit::paragraph_start(&self.doc, pi), style })
            })
            .collect();
        ops.is_empty() || self.apply_user_ops(buf, &ops, false)
    }

    /// The document's headings (and Title/Subtitle) in order: level, text
    /// and the buffer offset where each starts.
    pub fn outline(&mut self) -> Vec<(u8, String, usize)> {
        letters_core::review::table_of_contents(&self.doc)
            .into_iter()
            .filter_map(|e| Some((e.level, e.title, *self.starts.get(e.paragraph)?)))
            .collect()
    }

    /// Undo (or redo) the last step: on the model, then in the buffer.
    pub fn undo(&mut self, buf: &gtk::TextBuffer, redo: bool) -> bool {
        let old = self.doc.clone();
        let done = if redo { self.history.redo(&mut self.doc) } else { self.history.undo(&mut self.doc) };
        if done.is_some() {
            self.project(buf, &old);
        }
        done.is_some()
    }
}

/// Paragraphs `p0..=p1` widened to whole tables: a table is rendered as
/// a grid of rows, so it is re-rendered whole.
fn widen(paras: &[Paragraph], p0: usize, p1: usize) -> (usize, usize) {
    let (mut s, mut e) = (p0.min(paras.len() - 1), p1.min(paras.len() - 1));
    let table = |i: usize| paras[i].style.table_cell.map(|c| c.table);
    while s > 0 && table(s).is_some() && table(s - 1) == table(s) {
        s -= 1;
    }
    while e + 1 < paras.len() && table(e).is_some() && table(e + 1) == table(e) {
        e += 1;
    }
    (s, e)
}

/// `widen`, plus whole list runs (numbers are rendered per run).
fn widen_for_render(paras: &[Paragraph], p0: usize, p1: usize) -> (usize, usize) {
    let (mut s, mut e) = widen(paras, p0, p1);
    let listed = |p: &Paragraph| p.style.list != letters_core::ListKind::None;
    while s > 0 && listed(&paras[s - 1]) && listed(&paras[s]) {
        s -= 1;
    }
    while e + 1 < paras.len() && listed(&paras[e + 1]) {
        e += 1;
    }
    (s, e)
}

/// Undo (or redo) on `buf`'s live model.
pub fn undo(buf: &gtk::TextBuffer, redo: bool) {
    if let Some(m) = of(buf) {
        m.borrow_mut().undo(buf, redo);
    }
    sync_actions(buf);
}

/// Enable the application's Undo and Redo actions as `buf`'s history
/// allows: called after every change to it, and when a tab is selected.
pub fn sync_actions(buf: &gtk::TextBuffer) {
    let (can_undo, can_redo) = match of(buf) {
        Some(m) => match m.try_borrow() {
            Ok(m) => (m.can_undo(), m.can_redo()),
            Err(_) => return,
        },
        None => (false, false),
    };
    let Some(app) = gio::Application::default() else { return };
    for (name, on) in [("undo", can_undo), ("redo", can_redo)] {
        if let Some(a) = app.lookup_action(name).and_then(|a| a.downcast::<gio::SimpleAction>().ok()) {
            a.set_enabled(on);
        }
    }
}

/// Show `doc`, a newly opened (or recovered) document, in `buf`'s tab:
/// it becomes the model's document as it is, and its history starts
/// there, so undo does not un-open it.
pub fn load(buf: &gtk::TextBuffer, doc: &Document) {
    if let Some(m) = of(buf) {
        m.borrow_mut().show(buf, doc.clone());
    }
    sync_actions(buf);
}

#[cfg(test)]
mod tests;
