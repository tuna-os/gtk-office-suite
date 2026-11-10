//! rich_text.rs — caret and range editing of a text box (DCK-007).
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! A `SlideObject::TextBox` holds its text three ways at once: `text`, the
//! styled `runs` that spell it (empty for plain text), and the `body`'s
//! paragraph styles, one per `'\n'`-separated paragraph. Every edit here
//! keeps them in step:
//!
//! - concatenated runs equal the text (or there are none, or one empty
//!   run holding an emptied box's typing style);
//! - the paragraph styles are either absent (a plain box) or no more than
//!   the paragraphs, and a break added or removed adds or removes exactly
//!   its paragraph's style;
//! - a style edit splits runs at the range and merges equal neighbours
//!   only around it, so unaffected imported runs stay as they were.
//!
//! Offsets are UTF-8 byte offsets of `text`, validated as scalar
//! boundaries. GTK speaks character (scalar) offsets: [`char_to_byte`] and
//! [`byte_to_char`] map them explicitly. A style edit never splits a
//! grapheme cluster ([`grapheme_bounds`]): an accent stays with its letter,
//! a family emoji stays one picture.
//!
//! [`TextSession`] is the editor's transaction: a working copy edited by
//! these operations, with its own undo, committed to the document as one
//! history step or cancelled without trace (`DecksController::
//! begin_text_session`). GTK shows it; it holds no text of its own.

use crate::engine::{ParaStyle, Run, RunStyle, SlideObject, TextBody};
use crate::format::{EditScope, FormatEdit, ListKind};
use unicode_segmentation::UnicodeSegmentation;

/// Why a range was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeError {
    /// `start` after `end`.
    Reversed,
    /// Past the end of the text.
    OutOfBounds,
    /// Inside a UTF-8 sequence.
    NotCharBoundary,
}

impl std::fmt::Display for RangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            RangeError::Reversed => "the range ends before it starts",
            RangeError::OutOfBounds => "the range runs past the end of the text",
            RangeError::NotCharBoundary => "the range splits a character",
        })
    }
}

/// The byte offset of character (Unicode scalar) offset `chars` — what
/// a `GtkTextIter` offset counts — or `None` past the end.
pub fn char_to_byte(text: &str, chars: usize) -> Option<usize> {
    if chars == 0 {
        return Some(0);
    }
    match text.char_indices().nth(chars) {
        Some((b, _)) => Some(b),
        None if text.chars().count() == chars => Some(text.len()),
        None => None,
    }
}

/// The character offset of byte offset `byte`, or `None` when it is not a
/// scalar boundary of `text`.
pub fn byte_to_char(text: &str, byte: usize) -> Option<usize> {
    (byte <= text.len() && text.is_char_boundary(byte)).then(|| text[..byte].chars().count())
}

/// `start..end` widened to whole grapheme clusters: the start moves back
/// to the cluster it is in, the end forward to the end of its cluster. A
/// collapsed range stays collapsed, at the start of its cluster.
pub fn grapheme_bounds(text: &str, start: usize, end: usize) -> (usize, usize) {
    let mut s = 0;
    let mut e = text.len();
    for (at, g) in text.grapheme_indices(true) {
        if at <= start {
            s = at;
        }
        if at < end && at + g.len() >= end {
            e = at + g.len();
        }
    }
    if start == end {
        return (s, s);
    }
    (s, e.max(s))
}

/// What a range looks like, for the editor's controls: a character
/// property is on only when the whole range has it (the typing style
/// for a caret); the rest are the first run's and first paragraph's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RangeFormat {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub font_family: Option<String>,
    /// Points; `None` is the default size.
    pub font_size: Option<f64>,
    pub color: Option<String>,
    pub align: crate::engine::ParaAlign,
    pub list: Option<ListKind>,
    pub level: u8,
}

/// A text box's text, runs and paragraph layout, edited by range.
#[derive(Clone, Debug, PartialEq)]
pub struct RichText {
    pub text: String,
    pub runs: Vec<Run>,
    pub body: TextBody,
}

impl RichText {
    /// The text of a text box, or `None` for any other object.
    pub fn of(obj: &SlideObject) -> Option<RichText> {
        match obj {
            SlideObject::TextBox { text, runs, body, .. } => Some(RichText { text: text.clone(), runs: runs.clone(), body: body.clone() }),
            _ => None,
        }
    }

    /// Put this text into `obj`; `false` (and nothing written) when it is
    /// not a text box.
    pub fn write_to(&self, obj: &mut SlideObject) -> bool {
        match obj {
            SlideObject::TextBox { text, runs, body, .. } => {
                *text = self.text.clone();
                *runs = self.runs.clone();
                *body = self.body.clone();
                true
            }
            _ => false,
        }
    }

    pub fn check_range(&self, start: usize, end: usize) -> Result<(), RangeError> {
        if start > end {
            return Err(RangeError::Reversed);
        }
        if end > self.text.len() {
            return Err(RangeError::OutOfBounds);
        }
        if !self.text.is_char_boundary(start) || !self.text.is_char_boundary(end) {
            return Err(RangeError::NotCharBoundary);
        }
        Ok(())
    }

    /// How many paragraphs: one more than the line breaks.
    pub fn para_count(&self) -> usize {
        self.text.matches('\n').count() + 1
    }

    /// The paragraph byte offset `at` is in.
    pub fn para_of(&self, at: usize) -> usize {
        self.text[..at].matches('\n').count()
    }

    /// Every paragraph's style, one per paragraph.
    fn full_paras(&self) -> Vec<ParaStyle> {
        (0..self.para_count()).map(|i| self.body.para(i)).collect()
    }

    /// The paragraphs `start..end` touches: the caret's own when
    /// collapsed, and a range ending just after a break does not reach
    /// into the paragraph after it.
    pub fn para_span(&self, start: usize, end: usize) -> (usize, usize) {
        let first = self.para_of(start);
        let mut last = self.para_of(end);
        if end > start && self.text.as_bytes()[end - 1] == b'\n' && last > first {
            last -= 1;
        }
        (first, last)
    }

    /// The style typing at `start..end` takes: a selection's first
    /// character's; at a caret, the character before it in the same
    /// paragraph, else the one after it, else the box's last run. A plain
    /// box types plain.
    pub fn style_at(&self, start: usize, end: usize) -> RunStyle {
        if self.runs.is_empty() {
            return RunStyle::default();
        }
        let containing = |at: usize| {
            let mut pos = 0;
            self.runs.iter().find(|r| {
                pos += r.text.len();
                at < pos
            })
        };
        let before = self.text[..start].chars().next_back();
        let pick = if start == end && start > 0 && before != Some('\n') {
            containing(start - before.map_or(1, char::len_utf8))
        } else {
            containing(start)
        };
        pick.or(self.runs.last()).map(|r| r.style.clone()).unwrap_or_default()
    }

    /// The styles of the characters in `start..end`.
    fn styles_in(&self, start: usize, end: usize) -> Vec<RunStyle> {
        if self.runs.is_empty() {
            return vec![RunStyle::default()];
        }
        let mut pos = 0;
        let mut out = Vec::new();
        for r in &self.runs {
            let (s, e) = (pos, pos + r.text.len());
            pos = e;
            if s < end && e > start && !r.text[start.saturating_sub(s).min(r.text.len())..(end - s).min(r.text.len())].chars().all(|c| c == '\n') {
                out.push(r.style.clone());
            }
        }
        if out.is_empty() {
            out.push(self.style_at(start, end));
        }
        out
    }

    /// What the range looks like (see [`RangeFormat`]). `typing` is a
    /// caret's pending style, which wins over the text's.
    pub fn format_at(&self, start: usize, end: usize, typing: Option<&RunStyle>) -> RangeFormat {
        let styles = match typing {
            Some(t) if start == end => vec![t.clone()],
            _ if start == end => vec![self.style_at(start, end)],
            _ => self.styles_in(start, end),
        };
        let first = styles[0].clone();
        let p = self.body.para(self.para_of(start));
        RangeFormat {
            bold: styles.iter().all(|s| s.bold),
            italic: styles.iter().all(|s| s.italic),
            underline: styles.iter().all(|s| s.underline),
            strikethrough: styles.iter().all(|s| s.strikethrough),
            font_family: first.font_family,
            font_size: first.font_size_hp.map(|hp| hp as f64 / 2.0),
            color: first.color,
            align: p.align,
            list: Some(crate::format::list_of(&p.bullet)),
            level: p.level,
        }
    }

    /// Replace `start..end` with `replacement` in `style`, or, with
    /// `None`, in the style of the run the range starts in (the last run
    /// at the end), the rule `DecksController::change_text` has always
    /// followed. A break inserted makes a paragraph styled like the one it
    /// splits; a break removed takes the second paragraph's style with
    /// it. Returns whether anything changed.
    pub fn replace(&mut self, start: usize, end: usize, replacement: &str, style: Option<RunStyle>) -> Result<bool, RangeError> {
        let style = style.or_else(|| (!self.runs.is_empty()).then(|| self.inherited(start)));
        let run = Run { text: replacement.to_string(), style: style.unwrap_or_default() };
        self.replace_runs(start, end, vec![run], &[])
    }

    /// The style an inherited insertion takes (see [`RichText::replace`]).
    fn inherited(&self, start: usize) -> RunStyle {
        let mut pos = 0;
        self.runs
            .iter()
            .find(|r| {
                pos += r.text.len();
                start < pos
            })
            .or(self.runs.last())
            .map(|r| r.style.clone())
            .unwrap_or_default()
    }

    /// Replace `start..end` with `inserted` runs. Each paragraph the
    /// insertion adds is styled like the one it splits, then has
    /// `new_paras[i]`'s paragraph edits applied (pasted alignment and
    /// lists), where given.
    pub fn replace_runs(&mut self, start: usize, end: usize, inserted: Vec<Run>, new_paras: &[Vec<FormatEdit>]) -> Result<bool, RangeError> {
        self.check_range(start, end)?;
        let replacement: String = inserted.iter().map(|r| r.text.as_str()).collect();
        let restyles = inserted.iter().any(|r| !r.text.is_empty() && self.runs.is_empty() && r.style != RunStyle::default());
        if self.text[start..end] == replacement && !restyles && (self.runs.is_empty() || self.styles_match(start, &inserted)) {
            return Ok(false);
        }
        // Paragraph styles first, against the text before the edit.
        let first_para = self.para_of(start);
        let removed = self.text[start..end].matches('\n').count();
        let added = replacement.matches('\n').count();
        if (!self.body.paras.is_empty() && (removed != 0 || added != 0)) || (!new_paras.is_empty() && added != 0) {
            let mut paras = self.full_paras();
            let inherited = paras[first_para].clone();
            let fresh = (0..added).map(|i| {
                let mut p = inherited.clone();
                for edit in new_paras.get(i + 1).into_iter().flatten() {
                    crate::format::edit_para(&mut p, edit);
                }
                p
            });
            paras.splice(first_para + 1..first_para + removed + 1, fresh.collect::<Vec<_>>());
            self.body.paras = paras;
        }
        // Then the runs: a plain box stays plain while what is inserted
        // is plain.
        if !self.runs.is_empty() || restyles {
            if self.runs.is_empty() && !self.text.is_empty() {
                self.runs.push(Run { text: self.text.clone(), style: RunStyle::default() });
            }
            let typing = inserted.first().map(|r| r.style.clone()).unwrap_or_else(|| self.inherited(start));
            let mut edited: Vec<Run> = Vec::with_capacity(self.runs.len() + inserted.len() + 1);
            let mut pos = 0;
            let mut placed = false;
            let put = |edited: &mut Vec<Run>, placed: &mut bool| {
                if !*placed {
                    for r in &inserted {
                        append_run(edited, r.clone());
                    }
                    *placed = true;
                }
            };
            for mut run in std::mem::take(&mut self.runs) {
                let len = run.text.len();
                let prefix = start.saturating_sub(pos).min(len);
                let suffix = end.saturating_sub(pos).min(len);
                pos += len;
                // An empty run (an emptied box's typing style) where the
                // edit starts is replaced, not kept before it.
                if prefix == len && (len != 0 || start != pos) {
                    append_run(&mut edited, run);
                    continue;
                }
                if suffix == 0 {
                    put(&mut edited, &mut placed);
                    append_run(&mut edited, run);
                    continue;
                }
                let tail = run.text.split_off(suffix);
                run.text.truncate(prefix);
                let style = run.style.clone();
                append_run(&mut edited, run);
                put(&mut edited, &mut placed);
                append_run(&mut edited, Run { text: tail, style });
            }
            put(&mut edited, &mut placed);
            // An emptied box keeps its typing style.
            if edited.is_empty() {
                edited.push(Run { text: String::new(), style: typing });
            }
            self.runs = edited;
        }
        self.text.replace_range(start..end, &replacement);
        Ok(true)
    }

    /// Whether the runs at `start` already spell `inserted` in its styles.
    fn styles_match(&self, start: usize, inserted: &[Run]) -> bool {
        let mut at = start;
        inserted.iter().all(|r| {
            let ok = r.text.is_empty() || self.styles_in(at, at + r.text.len()).iter().all(|s| *s == r.style);
            at += r.text.len();
            ok
        })
    }

    /// Apply `edit` to `start..end`, as far as its [`EditScope`] reaches:
    /// the characters (widened to whole grapheme clusters; a caret has
    /// none, see [`TextSession::format`]), the paragraphs touched, or the
    /// box. Never anything outside that. Returns whether anything changed.
    pub fn format(&mut self, start: usize, end: usize, edit: &FormatEdit) -> Result<bool, RangeError> {
        self.check_range(start, end)?;
        Ok(match edit.scope() {
            EditScope::Characters => {
                let (start, end) = grapheme_bounds(&self.text, start, end);
                self.format_chars(start, end, edit)
            }
            EditScope::Paragraphs => {
                let (first, last) = self.para_span(start, end);
                let before = self.full_paras();
                let mut paras = before.clone();
                for p in &mut paras[first..=last] {
                    crate::format::edit_para(p, edit);
                }
                if paras == before {
                    false
                } else {
                    self.body.paras = paras;
                    true
                }
            }
            EditScope::TextBox => crate::format::edit_body(&mut self.body, edit),
            EditScope::Object => false,
        })
    }

    fn format_chars(&mut self, start: usize, end: usize, edit: &FormatEdit) -> bool {
        if start == end {
            return false;
        }
        let plain = self.runs.is_empty();
        let mut runs = if plain { vec![Run { text: self.text.clone(), style: RunStyle::default() }] } else { self.runs.clone() };
        split_runs(&mut runs, start);
        split_runs(&mut runs, end);
        let mut pos = 0;
        let (mut first, mut last) = (usize::MAX, 0);
        for (i, r) in runs.iter_mut().enumerate() {
            let (s, e) = (pos, pos + r.text.len());
            pos = e;
            if s >= start && e <= end && s < e {
                crate::format::edit_style(&mut r.style, edit);
                first = first.min(i);
                last = i;
            }
        }
        if first == usize::MAX {
            return false;
        }
        // Merge equal neighbours only around the edit.
        let lo = first.saturating_sub(1);
        let mut i = (last + 1).min(runs.len() - 1);
        while i > lo {
            if runs[i - 1].style == runs[i].style {
                let t = runs.remove(i).text;
                runs[i - 1].text.push_str(&t);
            }
            i -= 1;
        }
        if (plain && runs.iter().all(|r| r.style == RunStyle::default())) || runs == self.runs {
            return false;
        }
        self.runs = runs;
        true
    }

    /// The invariants every edit keeps (module docs). `Err` names the
    /// first one broken.
    pub fn check(&self) -> Result<(), String> {
        if !self.runs.is_empty() {
            let spelled: String = self.runs.iter().map(|r| r.text.as_str()).collect();
            if spelled != self.text {
                return Err(format!("runs spell {spelled:?}, text is {:?}", self.text));
            }
            if self.runs.len() > 1 && self.runs.iter().any(|r| r.text.is_empty()) {
                return Err("an empty run beside others".into());
            }
        }
        if self.body.paras.len() > self.para_count() {
            return Err(format!("{} paragraph styles for {} paragraphs", self.body.paras.len(), self.para_count()));
        }
        Ok(())
    }
}

/// Split the run holding byte `at` in two there.
fn split_runs(runs: &mut Vec<Run>, at: usize) {
    let mut end = 0;
    let Some(i) = runs.iter().position(|r| {
        let start = end;
        end += r.text.len();
        at > start && at < end
    }) else {
        return;
    };
    let start = end - runs[i].text.len();
    let tail = runs[i].text.split_off(at - start);
    let style = runs[i].style.clone();
    runs.insert(i + 1, Run { text: tail, style });
}

/// Append `run`, merged into the last when their styles match; empty runs
/// are dropped.
fn append_run(runs: &mut Vec<Run>, run: Run) {
    if run.text.is_empty() {
        return;
    }
    match runs.last_mut() {
        Some(last) if last.style == run.style => last.text.push_str(&run.text),
        _ => runs.push(run),
    }
}

/// One step of a session's own undo: the text and where the selection was.
#[derive(Clone, Debug)]
struct Step {
    text: RichText,
    range: (usize, usize),
}

/// An in-place text edit (DCK-007): a working copy of one text box, the
/// caret or selection in it, the style a caret types in, and the
/// session's own undo. Nothing here touches the document;
/// `DecksController::commit_text_session` records it as one step.
#[derive(Clone, Debug)]
pub struct TextSession {
    pub slide_id: u64,
    pub object_id: u64,
    /// The master being edited when the session began, if any.
    pub master: Option<u64>,
    original: RichText,
    working: RichText,
    range: (usize, usize),
    typing: Option<RunStyle>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Where the last typed insertion ended, while typing continues there:
    /// consecutive characters of one word undo together.
    typing_at: Option<usize>,
}

impl TextSession {
    /// A session on `text`, the caret at its end.
    pub fn new(slide_id: u64, object_id: u64, master: Option<u64>, text: RichText) -> Self {
        let end = text.text.len();
        TextSession {
            slide_id,
            object_id,
            master,
            original: text.clone(),
            working: text,
            range: (end, end),
            typing: None,
            undo: Vec::new(),
            redo: Vec::new(),
            typing_at: None,
        }
    }

    pub fn original(&self) -> &RichText {
        &self.original
    }

    pub fn working(&self) -> &RichText {
        &self.working
    }

    /// The selection, start before end, in bytes of the working text.
    pub fn range(&self) -> (usize, usize) {
        self.range
    }

    /// Whether committing would change the document.
    pub fn changed(&self) -> bool {
        self.working != self.original
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Move the caret or selection. A different place forgets a caret's
    /// pending typing style.
    pub fn set_range(&mut self, start: usize, end: usize) -> Result<(), RangeError> {
        self.working.check_range(start, end)?;
        if (start, end) != self.range {
            self.typing = None;
            self.typing_at = None;
            self.range = (start, end);
        }
        Ok(())
    }

    /// The style typing replaces the selection with.
    pub fn typing_style(&self) -> RunStyle {
        self.typing.clone().unwrap_or_else(|| self.working.style_at(self.range.0, self.range.1))
    }

    /// What the selection looks like, for the editor's controls.
    pub fn format_here(&self) -> RangeFormat {
        self.working.format_at(self.range.0, self.range.1, self.typing.as_ref())
    }

    fn remember(&mut self) {
        self.undo.push(Step { text: self.working.clone(), range: self.range });
        self.redo.clear();
    }

    /// Replace `start..end` (bytes) with `text` typed in the typing style.
    /// Typing into a selection takes the selection's style; deleting a
    /// selection keeps its style for what is typed next in its place. The
    /// caret ends after the inserted text.
    pub fn replace(&mut self, start: usize, end: usize, text: &str) -> Result<bool, RangeError> {
        self.working.check_range(start, end)?;
        if start == end && text.is_empty() {
            return Ok(false);
        }
        let style = if (start, end) == self.range { self.typing_style() } else { self.working.style_at(start, end) };
        let word = start == end && !text.is_empty() && text.chars().all(char::is_alphanumeric);
        if !word || self.typing_at != Some(start) {
            self.remember();
        }
        let mut edited = self.working.clone();
        edited.replace(start, end, text, Some(style.clone()))?;
        self.working = edited;
        let caret = start + text.len();
        self.range = (caret, caret);
        self.typing_at = word.then_some(caret);
        // What replaced the deleted selection types on in its style.
        self.typing = (text.is_empty() && end > start).then_some(style);
        Ok(true)
    }

    /// Insert styled runs (a paste) over the selection, each new
    /// paragraph given `new_paras`' paragraph edits.
    pub fn insert_runs(&mut self, runs: Vec<Run>, new_paras: &[Vec<FormatEdit>]) -> Result<bool, RangeError> {
        let (start, end) = self.range;
        let len: usize = runs.iter().map(|r| r.text.len()).sum();
        let mut edited = self.working.clone();
        if !edited.replace_runs(start, end, runs, new_paras)? {
            return Ok(false);
        }
        self.remember();
        self.working = edited;
        self.range = (start + len, start + len);
        self.typing = None;
        self.typing_at = None;
        Ok(true)
    }

    /// Format the selection. A character edit at a caret changes nothing
    /// in the text: it sets the style the next typed text takes. Returns
    /// whether the working text (or, at a caret, the typing style) changed.
    pub fn format(&mut self, edit: &FormatEdit) -> bool {
        let (start, end) = self.range;
        if start == end && edit.scope() == EditScope::Characters {
            let mut style = self.typing_style();
            if !crate::format::edit_style(&mut style, edit) {
                return false;
            }
            self.typing = Some(style);
            return true;
        }
        let mut edited = self.working.clone();
        match edited.format(start, end, edit) {
            Ok(true) => {
                self.remember();
                self.working = edited;
                self.typing_at = None;
                true
            }
            _ => false,
        }
    }

    /// Undo the session's last step; the document is never touched.
    pub fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop() else { return false };
        self.redo.push(Step { text: std::mem::replace(&mut self.working, step.text), range: self.range });
        self.range = step.range;
        self.typing = None;
        self.typing_at = None;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else { return false };
        self.undo.push(Step { text: std::mem::replace(&mut self.working, step.text), range: self.range });
        self.range = step.range;
        self.typing = None;
        self.typing_at = None;
        true
    }
}

/// The link under byte `offset` of a text box's runs (#1998): the target
/// of the run that holds that byte, so a click on a linked word finds what
/// it opens. A run covers its first byte up to, not including, the next
/// run's first; the end of the text belongs to no run, and a box with no
/// runs (plain text) has no link to find.
pub fn link_at(runs: &[Run], offset: usize) -> Option<&str> {
    let mut start = 0;
    for run in runs {
        let end = start + run.text.len();
        if offset < end {
            return if offset >= start { run.style.link.as_deref() } else { None };
        }
        start = end;
    }
    None
}

/// Whether a link from a document is one to open on a click: a web or mail
/// address. The readers already keep only addresses with a scheme; this is
/// the second check, at the point of opening, so a crafted file cannot
/// make a click launch a `file:` path or another program's scheme.
pub fn is_followable(uri: &str) -> bool {
    let scheme = uri.split(':').next().unwrap_or("");
    uri.contains(':') && ["http", "https", "mailto"].iter().any(|ok| scheme.eq_ignore_ascii_case(ok))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Anchor, Bullet, ParaAlign};

    fn bold() -> RunStyle {
        RunStyle { bold: true, ..Default::default() }
    }

    fn italic() -> RunStyle {
        RunStyle { italic: true, ..Default::default() }
    }

    fn run(text: &str, style: RunStyle) -> Run {
        Run { text: text.into(), style }
    }

    /// An imported mixed paragraph and a second, differently styled one.
    fn mixed() -> RichText {
        let first = ParaStyle { align: ParaAlign::Center, ..Default::default() };
        let second = ParaStyle { margin_left: 36.0, bullet: Bullet::Char("•".into()), indent: -36.0, ..Default::default() };
        RichText {
            text: "Alpha Beta Gamma\nSecond".into(),
            runs: vec![run("Alpha ", bold()), run("Beta", RunStyle::default()), run(" Gamma\nSecond", italic())],
            body: TextBody { paras: vec![first, second], anchor: Anchor::Middle, ..Default::default() },
        }
    }

    fn spans(t: &RichText) -> Vec<(&str, bool, bool, bool)> {
        t.runs.iter().map(|r| (r.text.as_str(), r.style.bold, r.style.italic, r.style.underline)).collect()
    }

    #[test]
    fn a_click_finds_the_link_of_the_run_it_lands_in() {
        let linked = RunStyle { link: Some("https://example.com/a".into()), ..Default::default() };
        let runs = vec![run("See ", RunStyle::default()), run("the page", linked), run(" now", RunStyle::default())];
        // "See " is bytes 0..4, "the page" 4..12, " now" 12..16.
        assert_eq!(link_at(&runs, 0), None);
        assert_eq!(link_at(&runs, 3), None);
        assert_eq!(link_at(&runs, 4), Some("https://example.com/a"));
        assert_eq!(link_at(&runs, 11), Some("https://example.com/a"));
        assert_eq!(link_at(&runs, 12), None);
        // The end of the text, and a box with no runs, hold no link.
        assert_eq!(link_at(&runs, 16), None);
        assert_eq!(link_at(&[], 0), None);
    }

    #[test]
    fn a_link_in_a_multibyte_run_is_found_by_byte() {
        let linked = RunStyle { link: Some("mailto:a@b.example".into()), ..Default::default() };
        // "é" is two bytes: the link run starts at byte 3.
        let runs = vec![run("é ", RunStyle::default()), run("café", linked)];
        assert_eq!(link_at(&runs, 2), None);
        assert_eq!(link_at(&runs, 3), Some("mailto:a@b.example"));
        assert_eq!(link_at(&runs, 7), Some("mailto:a@b.example"));
    }

    #[test]
    fn only_web_and_mail_links_are_followed() {
        for ok in ["https://example.com", "HTTP://example.com/x", "mailto:a@b.example"] {
            assert!(is_followable(ok), "{ok}");
        }
        for no in ["file:///etc/passwd", "javascript:alert(1)", "smb://host/share", "example.com", "", "https", "x-https:y"] {
            assert!(!is_followable(no), "{no}");
        }
    }

    #[test]
    fn range_replace_keeps_unaffected_runs_and_paras() {
        let mut t = mixed();
        let before = t.clone();
        // "Beta" -> "Delta": in Beta's style; neighbours untouched.
        assert_eq!(t.replace(6, 10, "Delta", None), Ok(true));
        assert_eq!(spans(&t), vec![("Alpha ", true, false, false), ("Delta", false, false, false), (" Gamma\nSecond", false, true, false)]);
        assert_eq!(t.body, before.body, "no break added or removed: paragraphs as they were");
        t.check().unwrap();
        // A break inside the first paragraph: its style is copied to the
        // new paragraph; the second paragraph keeps its own.
        t.replace(5, 5, "\n", None).unwrap();
        assert_eq!(t.text, "Alpha\n Delta Gamma\nSecond");
        assert_eq!(t.body.paras, vec![before.body.paras[0].clone(), before.body.paras[0].clone(), before.body.paras[1].clone()]);
        assert_eq!(t.body.anchor, Anchor::Middle);
        t.check().unwrap();
        // Joining the last two paragraphs drops the second's style only.
        let join = t.text.rfind('\n').unwrap();
        t.replace(join, join + 1, " ", None).unwrap();
        assert_eq!(t.body.paras.len(), 2);
        assert_eq!(t.body.paras[0], before.body.paras[0]);
        t.check().unwrap();
        // A no-op replacement changes nothing.
        let same = t.clone();
        assert_eq!(t.replace(0, 5, "Alpha", Some(bold())), Ok(false));
        assert_eq!(t, same);
    }

    #[test]
    fn range_format_splits_only_the_selection_and_never_the_whole_box() {
        let mut t = mixed();
        // Underline "Gam" inside the italic run.
        assert_eq!(t.format(11, 14, &FormatEdit::Underline(true)), Ok(true));
        assert_eq!(
            spans(&t),
            vec![("Alpha ", true, false, false), ("Beta", false, false, false), (" ", false, true, false), ("Gam", false, true, true), ("ma\nSecond", false, true, false)]
        );
        t.check().unwrap();
        // Undoing it by formatting back merges around the edit only.
        assert_eq!(t.format(11, 14, &FormatEdit::Underline(false)), Ok(true));
        assert_eq!(t.runs, mixed().runs);
        // A paragraph edit from inside the second paragraph reaches it alone.
        let at = t.text.find("Sec").unwrap();
        assert_eq!(t.format(at, at, &FormatEdit::Align(ParaAlign::Right)), Ok(true));
        assert_eq!(t.body.paras[0].align, ParaAlign::Center);
        assert_eq!(t.body.paras[1].align, ParaAlign::Right);
        // A selection ending just after a break does not reach the next
        // paragraph.
        let nl = t.text.find('\n').unwrap();
        t.format(0, nl + 1, &FormatEdit::Align(ParaAlign::Justify)).unwrap();
        assert_eq!((t.body.paras[0].align, t.body.paras[1].align), (ParaAlign::Justify, ParaAlign::Right));
        // An object edit is not a text edit.
        assert_eq!(t.format(0, 3, &FormatEdit::X(4.0)), Ok(false));
        // Bad ranges are refused, not clamped.
        assert_eq!(t.format(4, 2, &FormatEdit::Bold(true)), Err(RangeError::Reversed));
        assert_eq!(t.format(0, 999, &FormatEdit::Bold(true)), Err(RangeError::OutOfBounds));
    }

    #[test]
    fn a_plain_box_stays_plain_until_styled_and_an_emptied_box_keeps_its_style() {
        let mut t = RichText { text: "plain".into(), runs: vec![], body: TextBody::default() };
        t.replace(5, 5, " text", None).unwrap();
        assert!(t.runs.is_empty(), "plain typing keeps a plain box plain");
        assert_eq!(t.format(0, 5, &FormatEdit::Bold(false)), Ok(false), "no visible change, no runs");
        assert_eq!(t.format(0, 5, &FormatEdit::Bold(true)), Ok(true));
        assert_eq!(spans(&t), vec![("plain", true, false, false), (" text", false, false, false)]);
        t.replace(0, t.text.len(), "", None).unwrap();
        assert_eq!(t.runs, vec![run("", bold())], "the emptied box types bold");
        t.check().unwrap();
        t.replace(0, 0, "again", None).unwrap();
        assert_eq!(t.runs, vec![run("again", bold())]);
    }

    #[test]
    fn grapheme_selection_maps_gtk_offsets() {
        // e + combining acute, a family emoji (ZWJ sequence), Hebrew with
        // digits, CJK.
        let text = "e\u{301}👨\u{200d}👩\u{200d}👧 שלום 123 東京";
        // GTK offsets count scalars: e, U+0301, then the emoji's five.
        assert_eq!(char_to_byte(text, 0), Some(0));
        assert_eq!(char_to_byte(text, 1), Some(1));
        assert_eq!(char_to_byte(text, 2), Some(3));
        let n = text.chars().count();
        assert_eq!(char_to_byte(text, n), Some(text.len()));
        assert_eq!(char_to_byte(text, n + 1), None);
        for (i, (b, _)) in text.char_indices().enumerate() {
            assert_eq!(byte_to_char(text, b), Some(i));
        }
        assert_eq!(byte_to_char(text, 2), None, "inside the combining acute");
        // A selection of the bare "e" widens to its accent; one of half
        // the family widens to all of it.
        assert_eq!(grapheme_bounds(text, 0, 1), (0, 3));
        let fam = 3;
        let fam_end = fam + "👨\u{200d}👩\u{200d}👧".len();
        assert_eq!(grapheme_bounds(text, fam + 4, fam + 8), (fam, fam_end));
        assert_eq!(grapheme_bounds(text, 1, 1), (0, 0), "a caret inside a cluster sits at its start");
        let mut t = RichText { text: text.into(), runs: vec![], body: TextBody::default() };
        t.format(0, 1, &FormatEdit::Bold(true)).unwrap();
        assert_eq!(t.runs[0].text, "e\u{301}", "the accent stays with its letter");
        // Hebrew and its digits keep their logical order through a
        // range edit.
        let heb = text.find('ש').unwrap();
        t.format(heb, heb + "שלום".len(), &FormatEdit::Italic(true)).unwrap();
        assert_eq!(t.text, text);
        assert!(t.runs.iter().any(|r| r.text == "שלום" && r.style.italic));
        t.check().unwrap();
    }

    #[test]
    fn a_session_types_formats_and_undoes_locally() {
        let mut s = TextSession::new(1, 2, None, mixed());
        assert!(!s.changed());
        // Select "Beta" and bold it.
        s.set_range(6, 10).unwrap();
        assert!(s.format(&FormatEdit::Bold(true)));
        assert!(s.format_here().bold);
        // A caret after it: Ctrl+I sets the typing style only.
        s.set_range(10, 10).unwrap();
        assert!(s.format(&FormatEdit::Italic(true)));
        assert_eq!(s.working().text, mixed().text, "a caret format changes no text");
        s.replace(10, 10, "X").unwrap();
        assert!(s.working().runs.iter().any(|r| r.text.contains('X') && r.style.bold && r.style.italic));
        // Typing a word is one undo step; the format before it another.
        s.replace(11, 11, "y").unwrap();
        assert!(s.undo());
        assert!(!s.working().text.contains('X'));
        assert!(s.undo());
        assert_eq!(s.working(), &mixed());
        assert!(!s.changed());
        assert!(s.redo() && s.redo());
        assert!(s.working().text.contains("BetaXy"));
        // Deleting a selection keeps its style for what replaces it.
        s.set_range(0, 5).unwrap();
        s.replace(0, 5, "").unwrap();
        s.replace(0, 0, "Omega").unwrap();
        let first = &s.working().runs[0];
        assert!(first.text.starts_with("Omega ") && first.style.bold, "{first:?}");
        s.working().check().unwrap();
        assert!(s.changed());
    }

    #[test]
    fn pasted_paragraphs_take_the_split_paragraphs_style_with_their_own_edits() {
        let mut s = TextSession::new(1, 2, None, mixed());
        s.set_range(5, 5).unwrap();
        let runs = vec![run(" one\ntwo", RunStyle::default())];
        assert_eq!(s.insert_runs(runs, &[vec![], vec![FormatEdit::List(ListKind::Number)]]), Ok(true));
        let w = s.working();
        assert_eq!(w.text, "Alpha one\ntwo Beta Gamma\nSecond");
        assert_eq!(w.body.paras.len(), 3);
        assert_eq!(w.body.paras[1].align, ParaAlign::Center, "inherits the split paragraph");
        assert!(matches!(w.body.paras[1].bullet, Bullet::AutoNum { .. }), "with its own list");
        w.check().unwrap();
        assert_eq!(s.range(), (13, 13));
    }
}
