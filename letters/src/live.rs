// SPDX-License-Identifier: GPL-3.0-or-later
//
// live.rs — the tab's live GTK-free document, and the source of truth
// (ADR 0010 stage 3c-3, RFC-0001 Phase 0).
//
// A `LiveModel` holds the tab's `letters_core::Document`. Every change is a
// `letters_core::edit` op on it, and its `History` (the ops' inverses) is
// the tab's undo and redo; Save, the page view and copy read the model.
//
// Edits reach it two ways:
// - Model first: the Print Layout view turns typing and deleting into ops
//   (`apply_user_ops`), applies them to the model, and the buffer — now the
//   Draft view of the model — is updated from the model (`project`).
// - Buffer first: the Draft TextView, formatting actions and anything else
//   that edits the GtkTextBuffer. Each change is turned into ops at once:
//   the buffer lines it touched are read back (`bridge::capture_span`) and
//   diffed against the model's paragraphs (`edit::diff`). Typing, Enter,
//   Backspace across a paragraph break, formatting, list markers — all
//   local, a line or two re-read, never the whole buffer.
// Only what a local read cannot follow — a table's pipes, an inline image or
// footnote marker, or a whole-buffer rewrite (open, structured edits) —
// re-reads the whole buffer, and even then the difference becomes ops, so
// it is undoable like any other change.
//
// The guard is an equivalence test: after every kind of edit, including
// thousands of seeded random ones, the model equals a fresh
// `capture_from_buffer` (below).

use gtk4::{self as gtk, gio, glib, prelude::*};
use letters_core::edit::{self, History, Op};
use letters_core::{Document, Paragraph};
use std::cell::RefCell;
use std::rc::Rc;

/// Tags that are not document content (capture ignores them), so applying
/// or removing them leaves the model as it is.
const PRESENTATION_TAGS: [&str; 4] = ["spelling-error", "search-match", "search-current", "tab-stops"];

const KEY: &str = "letters-live-model";

#[derive(Default)]
pub struct LiveModel {
    doc: Document,
    /// Buffer offset where each paragraph's text starts.
    starts: Vec<usize>,
    /// The model must be re-read from the whole buffer before use.
    stale: bool,
    /// The model has never been read (history starts after the first read).
    fresh: bool,
    /// The buffer is being changed from the model: don't follow it.
    projecting: bool,
    /// A pending delete: its range and newline count, stashed before GTK
    /// deletes so the change can be resolved after.
    pending_delete: Option<(usize, usize, usize)>,
    history: History,
    /// Changes followed with a local read, and whole-buffer reads.
    pub local_reads: usize,
    pub full_reads: usize,
}

/// The live model of `buf`, if it has one.
pub fn of(buf: &gtk::TextBuffer) -> Option<Rc<RefCell<LiveModel>>> {
    unsafe { buf.data::<Rc<RefCell<LiveModel>>>(KEY).map(|p| p.as_ref().clone()) }
}

impl LiveModel {
    /// Give `buf` a live model. Its undo replaces the buffer's own.
    pub fn attach(buf: &gtk::TextBuffer) -> Rc<RefCell<LiveModel>> {
        let model = Rc::new(RefCell::new(LiveModel { stale: true, fresh: true, ..Default::default() }));
        unsafe { buf.set_data(KEY, model.clone()) };
        buf.set_enable_undo(false);
        {
            // After GTK inserted, so the text's tags are known.
            let m = Rc::downgrade(&model);
            buf.connect_closure(
                "insert-text",
                true,
                glib::closure_local!(move |b: gtk::TextBuffer, end: gtk::TextIter, text: &str, _len: i32| {
                    let Some(m) = m.upgrade() else { return };
                    let Ok(mut m) = m.try_borrow_mut() else { return };
                    let n = text.chars().count();
                    let from = (end.offset().max(0) as usize).saturating_sub(n);
                    let breaks = text.matches('\n').count();
                    m.changed(&b, from, from, n, breaks as isize, word_typing(text));
                }),
            );
        }
        {
            let m = Rc::downgrade(&model);
            buf.connect_delete_range(move |b, s, e| {
                let Some(m) = m.upgrade() else { return };
                let Ok(mut m) = m.try_borrow_mut() else { return };
                let breaks = b.text(s, e, true).matches('\n').count();
                m.pending_delete = Some((s.offset().max(0) as usize, e.offset().max(0) as usize, breaks));
            });
        }
        {
            let m = Rc::downgrade(&model);
            buf.connect_closure(
                "delete-range",
                true,
                glib::closure_local!(move |b: gtk::TextBuffer, _s: gtk::TextIter, _e: gtk::TextIter| {
                    let Some(m) = m.upgrade() else { return };
                    let Ok(mut m) = m.try_borrow_mut() else { return };
                    if let Some((from, to, breaks)) = m.pending_delete.take() {
                        m.changed(&b, from, to, 0, -(breaks as isize), false);
                    }
                }),
            );
        }
            // Formatting: after GTK applied or removed the tag.
        for signal in ["apply-tag", "remove-tag"] {
            let m = Rc::downgrade(&model);
            buf.connect_closure(
                signal,
                true,
                glib::closure_local!(move |b: gtk::TextBuffer, tag: gtk::TextTag, s: gtk::TextIter, e: gtk::TextIter| {
                    if tag.name().is_some_and(|n| PRESENTATION_TAGS.contains(&n.as_str())) {
                        return;
                    }
                    let Some(m) = m.upgrade() else { return };
                    let Ok(mut m) = m.try_borrow_mut() else { return };
                    let (s, e) = (s.offset().max(0) as usize, e.offset().max(0) as usize);
                    m.changed(&b, s, e, e - s, 0, false);
                }),
            );
        }
        let stale_on = |model: &Rc<RefCell<LiveModel>>| {
            let m = Rc::downgrade(model);
            move || {
                if let Some(m) = m.upgrade() {
                    if let Ok(mut m) = m.try_borrow_mut() {
                        if !m.projecting {
                            m.stale = true;
                        }
                    }
                }
            }
        };
        {
            // An inline image is one object char: follow it like text.
            let m = Rc::downgrade(&model);
            buf.connect_closure(
                "insert-paintable",
                true,
                glib::closure_local!(move |b: gtk::TextBuffer, end: gtk::TextIter, _p: gtk::gdk::Paintable| {
                    let Some(m) = m.upgrade() else { return };
                    let Ok(mut m) = m.try_borrow_mut() else { return };
                    let from = (end.offset().max(0) as usize).saturating_sub(1);
                    m.changed(&b, from, from, 1, 0, false);
                }),
            );
        }
        {
            let f = stale_on(&model);
            buf.connect_insert_child_anchor(move |_, _, _| f());
        }
        {
            let m = Rc::downgrade(&model);
            buf.connect_begin_user_action(move |_| {
                if let Some(Ok(mut m)) = m.upgrade().as_ref().map(|m| m.try_borrow_mut()) {
                    m.history.begin();
                }
            });
        }
        {
            let m = Rc::downgrade(&model);
            buf.connect_end_user_action(move |b| {
                if let Some(Ok(mut m)) = m.upgrade().as_ref().map(|m| m.try_borrow_mut()) {
                    // Resolve a pending whole-buffer read inside the action,
                    // so its ops are this action's undo step.
                    m.resolve(b);
                    m.history.end();
                }
                sync_actions(b);
            });
        }
        {
            // A change outside a user action (a formatting command, the
            // page view's model-first edits) is a step of its own.
            buf.connect_changed(|b| {
                let b = b.clone();
                glib::idle_add_local_once(move || sync_actions(&b));
            });
        }
        // The (usually empty) buffer is the starting document.
        model.borrow_mut().resolve(buf);
        model
    }

    /// The buffer changed at `from..old_to` (pre-edit offsets); the new text
    /// there is `new_len` chars, and paragraph breaks changed by `breaks`.
    fn changed(&mut self, buf: &gtk::TextBuffer, from: usize, old_to: usize, new_len: usize, breaks: isize, typing: bool) {
        if self.projecting || self.stale {
            return;
        }
        if !self.follow_locally(buf, from, old_to, new_len, breaks, typing) {
            self.stale = true;
        }
    }

    /// Follow a change by re-reading only the lines it touched, widened to
    /// whole tables (and any line that could join one). `false` when that
    /// cannot be exact: then the whole buffer is read.
    fn follow_locally(&mut self, buf: &gtk::TextBuffer, from: usize, old_to: usize, new_len: usize, breaks: isize, typing: bool) -> bool {
        if self.starts.len() != self.doc.paragraphs.len() {
            return false;
        }
        let (p0, _) = crate::bridge::paragraph_offset(&self.doc, &self.starts, from);
        let (p1, _) = crate::bridge::paragraph_offset(&self.doc, &self.starts, old_to);
        let (s, e) = widen(&self.doc.paragraphs, p0, p1.max(p0));
        // The region's first line is unchanged by the edit: it starts at
        // or before `from`.
        let first_line = buf.iter_at_offset(self.starts[s].min(from) as i32).line();
        let old_lines = line_count(&self.doc.paragraphs[s..=e]) as isize;
        let last_line = first_line as isize + old_lines - 1 + breaks;
        if last_line < first_line as isize || last_line >= buf.line_count() as isize {
            return false;
        }
        let Some(start) = buf.iter_at_line(first_line) else { return false };
        let Some(mut end) = buf.iter_at_line(last_line as i32) else { return false };
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        let (mut paras, mut starts) = crate::bridge::capture_span(buf, start.offset(), end.offset());
        let first_id = tables_before(&self.doc.paragraphs, s) + 1;
        crate::bridge::capture_tables(&mut paras, &mut starts, first_id);
        let mut next = self.doc.clone();
        let old_tables = table_ids(&self.doc.paragraphs[s..=e]).len() as i64;
        let new_tables = table_ids(&paras).len() as i64;
        next.paragraphs.splice(s..=e, paras.iter().cloned());
        // Tables are numbered in document order: tables after the region
        // shift when it gains or loses one.
        if new_tables != old_tables {
            let after = s + paras.len();
            for p in &mut next.paragraphs[after..] {
                if let Some(c) = &mut p.style.table_cell {
                    c.table = (i64::from(c.table) + new_tables - old_tables) as u32;
                }
            }
        }
        let ops = edit::diff(&self.doc, &next);
        let Ok(inverse) = edit::apply_all(&mut self.doc, &ops) else { return false };
        if self.doc.paragraphs != next.paragraphs {
            return false;
        }
        let delta = new_len as isize - (old_to - from) as isize;
        let tail: Vec<usize> = self.starts[e + 1..].iter().map(|x| (*x as isize + delta) as usize).collect();
        self.starts.truncate(s);
        self.starts.extend(starts);
        self.starts.extend(tail);
        self.history.set_merge(typing);
        self.history.record(inverse);
        self.local_reads += 1;
        true
    }

    /// Bring the model up to date with a whole-buffer read if it is stale.
    fn resolve(&mut self, buf: &gtk::TextBuffer) {
        if !self.stale || self.projecting {
            return;
        }
        let (doc, starts) = crate::bridge::capture_with_starts(buf);
        if self.fresh {
            self.doc = doc;
            self.fresh = false;
        } else {
            let ops = edit::diff(&self.doc, &doc);
            match edit::apply_all(&mut self.doc, &ops) {
                Ok(inverse) => self.history.record(inverse),
                Err(_) => {
                    self.doc = doc;
                    self.history.clear();
                }
            }
            crate::bridge::read_sidecars(buf, &mut self.doc);
        }
        self.starts = starts;
        self.stale = false;
        self.full_reads += 1;
    }

    /// The current document and its paragraphs' buffer starts.
    pub fn snapshot(&mut self, buf: &gtk::TextBuffer) -> (Document, Vec<usize>) {
        self.resolve(buf);
        // Header, footer, page setup and footnotes live beside the text
        // and change without a buffer edit.
        crate::bridge::read_sidecars(buf, &mut self.doc);
        (self.doc.clone(), self.starts.clone())
    }

    /// Forget the model's history and re-read it (a document was opened).
    pub fn reset(&mut self) {
        self.history.clear();
        self.stale = true;
        self.fresh = true;
    }

    /// Update the buffer — the Draft view — to the model, after the model
    /// changed from `old`. Only the changed paragraphs' lines are
    /// re-rendered; a change to a table or an inline object re-renders the
    /// whole buffer. The caret goes to the end of the change.
    fn project(&mut self, buf: &gtk::TextBuffer, old: &Document) {
        let (pa, pb) = (&old.paragraphs, &self.doc.paragraphs);
        let head = pa.iter().zip(pb).take_while(|(x, y)| x == y).count();
        if head == pa.len() && head == pb.len() {
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
            crate::bridge::render_to_buffer(&self.doc, buf);
            self.starts = crate::bridge::capture_with_starts(buf).1;
        } else {
            let first_line = buf.iter_at_offset(self.starts[h] as i32).line();
            let old_lines = line_count(&pa[h..ea]) as i32;
            let (Some(mut s), Some(mut e)) = (buf.iter_at_line(first_line), buf.iter_at_line(first_line + old_lines - 1)) else {
                self.projecting = false;
                return;
            };
            if !e.ends_line() {
                e.forward_to_line_end();
            }
            let old_len = e.offset() - s.offset();
            buf.delete(&mut s, &mut e);
            let lines = crate::bridge::render_lines(&pb[h..eb]);
            let ordinals = letters_core::lists::ordinals(lines.iter().map(|p| &p.style));
            let lines: Vec<&Paragraph> = lines.iter().map(|p| p.as_ref()).collect();
            let mut at = buf.iter_at_line(first_line).unwrap_or_else(|| buf.end_iter());
            let from = at.offset();
            crate::bridge::render_paragraphs(buf, &mut at, &lines, &ordinals);
            let mut end = buf.iter_at_line(first_line + lines.len() as i32 - 1).unwrap_or_else(|| buf.end_iter());
            if !end.ends_line() {
                end.forward_to_line_end();
            }
            let (mut paras, mut starts) = crate::bridge::capture_span(buf, from, end.offset());
            crate::bridge::capture_tables(&mut paras, &mut starts, tables_before(pb, h) + 1);
            let delta = (end.offset() - from) as isize - old_len as isize;
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
        self.resolve(buf);
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
    pub fn sequence_offset(&mut self, buf: &gtk::TextBuffer, off: usize) -> usize {
        self.resolve(buf);
        let (para, offset) = crate::bridge::paragraph_offset(&self.doc, &self.starts, off);
        edit::paragraph_start(&self.doc, para) + offset
    }

    pub fn document(&mut self, buf: &gtk::TextBuffer) -> &Document {
        self.resolve(buf);
        crate::bridge::read_sidecars(buf, &mut self.doc);
        &self.doc
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Undo (or redo) the last step: on the model, then in the buffer.
    pub fn undo(&mut self, buf: &gtk::TextBuffer, redo: bool) -> bool {
        self.resolve(buf);
        let old = self.doc.clone();
        let done = if redo { self.history.redo(&mut self.doc) } else { self.history.undo(&mut self.doc) };
        if done.is_some() {
            self.project(buf, &old);
        }
        done.is_some()
    }
}

/// Whether paragraph `p` is or could become part of a table on screen: a
/// table cell, or prose whose line reads as a pipe row.
fn table_like(p: &Paragraph) -> bool {
    p.style.table_cell.is_some() || letters_core::table_text::parse_row(&p.text()).is_some()
}

/// Paragraphs `p0..=p1` widened to whole tables and to every adjacent line
/// that could join a table. A table is found by reading neighbouring lines
/// (`bridge::capture_tables`), so this is the smallest span whose re-read
/// gives the same paragraphs as a whole-buffer read: at its edges are
/// lines that are no part of any table.
fn widen(paras: &[Paragraph], p0: usize, p1: usize) -> (usize, usize) {
    let (mut s, mut e) = (p0.min(paras.len() - 1), p1.min(paras.len() - 1));
    // Every adjacent table-like line: a whole table, and a row-looking
    // neighbour that typing a delimiter or a pipe can turn into one.
    while s > 0 && table_like(&paras[s - 1]) {
        s -= 1;
    }
    while e + 1 < paras.len() && table_like(&paras[e + 1]) {
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

/// The distinct tables among `paras`, in order.
fn table_ids(paras: &[Paragraph]) -> Vec<u32> {
    let mut ids: Vec<u32> = Vec::new();
    for c in paras.iter().filter_map(|p| p.style.table_cell) {
        if !ids.contains(&c.table) {
            ids.push(c.table);
        }
    }
    ids
}

/// How many tables come before paragraph `para`.
fn tables_before(paras: &[Paragraph], para: usize) -> u32 {
    table_ids(&paras[..para.min(paras.len())]).len() as u32
}

/// Editor lines `paras` (whole tables) take: one per paragraph, and a table
/// one per row plus its delimiter line.
fn line_count(paras: &[Paragraph]) -> usize {
    let prose = paras.iter().filter(|p| p.style.table_cell.is_none()).count();
    let tables: usize = table_ids(paras)
        .into_iter()
        .map(|t| {
            let rows = paras.iter().filter_map(|p| p.style.table_cell).filter(|c| c.table == t).map(|c| c.row).max().unwrap_or(0);
            rows as usize + 2
        })
        .sum();
    prose + tables
}

/// Whether inserted `text` continues a typed word (undo merges those).
fn word_typing(text: &str) -> bool {
    text.chars().count() == 1 && !text.chars().any(char::is_whitespace)
}

/// Whether `buf`'s live model is in the middle of a change (writing into
/// the buffer itself): buffer handlers must not add edits of their own.
pub fn is_busy(buf: &gtk::TextBuffer) -> bool {
    of(buf).is_some_and(|m| m.try_borrow_mut().map_or(true, |m| m.projecting))
}

/// Undo on `buf`'s live model, or the buffer's own undo without one.
pub fn undo(buf: &gtk::TextBuffer, redo: bool) {
    match of(buf) {
        Some(m) => {
            m.borrow_mut().undo(buf, redo);
        }
        None if redo => buf.redo(),
        None => buf.undo(),
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
        None => (buf.can_undo(), buf.can_redo()),
    };
    let Some(app) = gio::Application::default() else { return };
    for (name, on) in [("undo", can_undo), ("redo", can_redo)] {
        if let Some(a) = app.lookup_action(name).and_then(|a| a.downcast::<gio::SimpleAction>().ok()) {
            a.set_enabled(on);
        }
    }
}

/// Run `f`, which rewrites the whole buffer (a structured edit), as one
/// change: it is read once afterwards, as one undo step.
pub fn rewrite(buf: &gtk::TextBuffer, f: impl FnOnce()) {
    let model = of(buf);
    if let Some(m) = &model {
        let mut m = m.borrow_mut();
        m.resolve(buf);
        m.projecting = true;
    }
    f();
    if let Some(m) = &model {
        let mut m = m.borrow_mut();
        m.projecting = false;
        m.stale = true;
        m.resolve(buf);
    }
}

/// A document was loaded into `buf`: start its history there.
pub fn reset(buf: &gtk::TextBuffer) {
    if let Some(m) = of(buf) {
        let mut m = m.borrow_mut();
        m.reset();
        // Read it now, so the first edit is an undo step of its own.
        m.resolve(buf);
    }
}

#[cfg(test)]
mod tests;
