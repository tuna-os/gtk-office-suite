// edit.rs — the live document's edit operations (ADR 0010 stage 3c,
// RFC-0001 Phase 0).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Every change to a live `Document` is an `Op`, and `apply` returns the ops
// that undo it. The ops are the ones a rich-text sequence CRDT replicates
// (see docs/rfc/0001-spike-results.md, "Letters marks"), so the model can be
// replicated later without a rewrite:
//
// - The document is one sequence of chars: each paragraph's text, with an
//   inline object (image, footnote reference) as one `layout::OBJECT` char,
//   and one break char between paragraphs. `Insert` and `Delete` address
//   that sequence by offset.
// - Character formatting is *marks*: `Mark` sets one `RunStyle` field on a
//   range. Each mark key has an expand rule (`MarkKey::expand`): bold
//   extends to text typed at its end, a link does not. `Insert` carries the
//   inserted text's style explicitly, so replaying an op never depends on
//   the rule; `typing_style` is where the rule is applied, once, when the
//   user types.
// - Paragraph formatting is an attribute of the paragraph (in a CRDT, of its
//   break char): `SetParaStyle`.
//
// No CRDT library is used here; that is the owner's decision (RFC-0001).

use serde::{Deserialize, Serialize};

use crate::layout::{is_object, OBJECT};
use crate::model::{Document, ParaStyle, Paragraph, Run, RunStyle, VertAlign};

/// A character-formatting key: one field of `RunStyle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MarkKey {
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Highlight,
    Code,
    Link,
    FontFamily,
    FontSize,
    Color,
    VertAlign,
    Html,
}

/// What happens to a mark when text is inserted at its edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expand {
    /// Text typed right after the marked range takes the mark (Loro's
    /// `ExpandType::After`, Automerge's `ExpandMark::After`).
    After,
    /// Text typed at either edge does not take the mark.
    None,
}

impl MarkKey {
    pub const ALL: [MarkKey; 12] = [
        MarkKey::Bold,
        MarkKey::Italic,
        MarkKey::Underline,
        MarkKey::Strikethrough,
        MarkKey::Highlight,
        MarkKey::Code,
        MarkKey::Link,
        MarkKey::FontFamily,
        MarkKey::FontSize,
        MarkKey::Color,
        MarkKey::VertAlign,
        MarkKey::Html,
    ];

    /// The expand rule, as Word and LibreOffice behave: typing after bold
    /// text continues it; typing after a link, inline code or raw HTML
    /// starts plain text.
    pub fn expand(self) -> Expand {
        match self {
            MarkKey::Link | MarkKey::Code | MarkKey::Html => Expand::None,
            _ => Expand::After,
        }
    }

    /// Copy this key's field from `from` into `to`.
    pub fn copy(self, from: &RunStyle, to: &mut RunStyle) {
        match self {
            MarkKey::Bold => to.bold = from.bold,
            MarkKey::Italic => to.italic = from.italic,
            MarkKey::Underline => to.underline = from.underline,
            MarkKey::Strikethrough => to.strikethrough = from.strikethrough,
            MarkKey::Highlight => to.highlight = from.highlight,
            MarkKey::Code => to.code = from.code,
            MarkKey::Link => to.link.clone_from(&from.link),
            MarkKey::FontFamily => to.font_family.clone_from(&from.font_family),
            MarkKey::FontSize => to.font_size_hp = from.font_size_hp,
            MarkKey::Color => to.color.clone_from(&from.color),
            MarkKey::VertAlign => to.vert_align = from.vert_align,
            MarkKey::Html => to.html = from.html,
        }
    }

    /// Whether `a` and `b` agree on this key.
    pub fn same(self, a: &RunStyle, b: &RunStyle) -> bool {
        let mut x = a.clone();
        self.copy(b, &mut x);
        &x == a
    }
}

/// A `RunStyle` holding only the marks, not an object's fields.
fn marks_only(style: &RunStyle) -> RunStyle {
    let mut out = RunStyle::default();
    for key in MarkKey::ALL {
        key.copy(style, &mut out);
    }
    out
}

/// One change to a live document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Op {
    /// Insert `content` at sequence offset `at`. The first paragraph's runs
    /// join the paragraph at `at` (its style is not used); each further
    /// paragraph starts a new paragraph with its own style, and the text
    /// that followed `at` ends up in the last one. Typing "x" is one
    /// paragraph with one run; Enter is two empty paragraphs.
    Insert { at: usize, content: Vec<Paragraph> },
    /// Delete `len` sequence chars from `at`. Deleting a break joins two
    /// paragraphs; the joined paragraph keeps the first one's style.
    Delete { at: usize, len: usize },
    /// Set mark `key` on the text in `start..end` to `value`'s field.
    /// Objects keep their own style.
    Mark { start: usize, end: usize, key: MarkKey, value: RunStyle },
    /// Replace the style of the paragraph holding sequence offset `at`
    /// (its table-cell identity is kept).
    SetParaStyle { at: usize, style: ParaStyle },
    /// Replace `remove` whole paragraphs from paragraph index `para` with
    /// `insert`. A block-level op, for what is not a text edit: a table's
    /// rows and columns (design constraint 6). In a CRDT it is an edit of
    /// the list of blocks, not of the text sequence. `insert` may carry
    /// table cells; the document keeps at least one paragraph.
    SetParagraphs { para: usize, remove: usize, insert: Vec<Paragraph> },
}

/// Why an op could not be applied. The document is unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    /// An offset past the end of the document.
    OutOfRange,
    /// A paragraph break next to a table cell: joining a cell with its
    /// neighbour, or splitting one, would break the grid.
    TableStructure,
    /// Nothing to insert.
    Empty,
}

/// Length of runs in sequence chars (an object is one).
pub fn seq_len(runs: &[Run]) -> usize {
    runs.iter().map(run_len).sum()
}

fn run_len(r: &Run) -> usize {
    if is_object(r) { 1 } else { r.text.chars().count() }
}

/// The document's sequence text: paragraphs' layout text joined by '\n'.
pub fn sequence_text(doc: &Document) -> String {
    doc.paragraphs.iter().map(|p| crate::layout::layout_text(&p.runs)).collect::<Vec<_>>().join("\n")
}

/// Total sequence length.
pub fn doc_len(doc: &Document) -> usize {
    doc.paragraphs.iter().map(|p| seq_len(&p.runs)).sum::<usize>() + doc.paragraphs.len().saturating_sub(1)
}

/// Paragraph and offset within it of sequence offset `at` (an offset at a
/// break is the end of the paragraph before it).
pub fn locate(doc: &Document, at: usize) -> Option<(usize, usize)> {
    let mut pos = 0;
    for (i, p) in doc.paragraphs.iter().enumerate() {
        let len = seq_len(&p.runs);
        if at <= pos + len {
            return Some((i, at - pos));
        }
        pos += len + 1;
    }
    None
}

/// Sequence offset of the start of paragraph `para`.
pub fn paragraph_start(doc: &Document, para: usize) -> usize {
    doc.paragraphs[..para.min(doc.paragraphs.len())].iter().map(|p| seq_len(&p.runs) + 1).sum()
}

/// Split runs at sequence offset `at` (an object is atomic).
fn split_runs(runs: &[Run], at: usize) -> (Vec<Run>, Vec<Run>) {
    let (mut head, mut tail) = (Vec::new(), Vec::new());
    let mut pos = 0;
    for r in runs {
        let n = run_len(r);
        if pos + n <= at {
            head.push(r.clone());
        } else if pos >= at || is_object(r) {
            tail.push(r.clone());
        } else {
            let cut = r.text.char_indices().nth(at - pos).map_or(r.text.len(), |(b, _)| b);
            head.push(Run { text: r.text[..cut].to_string(), style: r.style.clone() });
            tail.push(Run { text: r.text[cut..].to_string(), style: r.style.clone() });
        }
        pos += n;
    }
    (head, tail)
}

/// Merge equal neighbours and drop empty text runs (the model's
/// invariants; objects are never merged).
fn normalize(runs: &mut Vec<Run>) {
    runs.retain(|r| is_object(r) || !r.text.is_empty());
    let mut out: Vec<Run> = Vec::with_capacity(runs.len());
    for r in runs.drain(..) {
        match out.last_mut() {
            Some(last) if !is_object(last) && !is_object(&r) && last.style == r.style => last.text.push_str(&r.text),
            _ => out.push(r),
        }
    }
    *runs = out;
}

/// The style text typed at sequence offset `at` takes: the marks of the
/// char before it in the same paragraph whose expand rule is `After`. At a
/// paragraph's start it is the marks of the paragraph's first text, so
/// typing at the start of a bold heading stays bold (as in Word).
pub fn typing_style(doc: &Document, at: usize) -> RunStyle {
    let Some((pi, po)) = locate(doc, at) else { return RunStyle::default() };
    let runs = &doc.paragraphs[pi].runs;
    let (head, tail) = split_runs(runs, po);
    let source = match head.iter().rev().find(|r| !is_object(r)) {
        Some(r) => r.style.clone(),
        None => match tail.iter().find(|r| !is_object(r)) {
            Some(r) => r.style.clone(),
            None => return RunStyle::default(),
        },
    };
    let mut out = RunStyle::default();
    for key in MarkKey::ALL {
        if key.expand() == Expand::After {
            key.copy(&source, &mut out);
        }
    }
    out
}

/// The document's content in `start..end` (sequence offsets) as paragraphs:
/// the first and last take their paragraphs' styles.
pub fn slice(doc: &Document, start: usize, end: usize) -> Option<Vec<Paragraph>> {
    let (sp, so) = locate(doc, start)?;
    let (ep, eo) = locate(doc, end)?;
    let mut out = Vec::new();
    for i in sp..=ep {
        let p = &doc.paragraphs[i];
        let from = if i == sp { so } else { 0 };
        let to = if i == ep { eo } else { seq_len(&p.runs) };
        let (_, rest) = split_runs(&p.runs, from);
        let (mid, _) = split_runs(&rest, to - from);
        out.push(Paragraph { style: p.style.clone(), runs: mid });
    }
    Some(out)
}

/// Apply `op` to `doc`. Returns the ops that undo it, in order.
pub fn apply(doc: &mut Document, op: &Op) -> Result<Vec<Op>, EditError> {
    match op {
        Op::Insert { at, content } => insert(doc, *at, content),
        Op::Delete { at, len } => delete(doc, *at, *len),
        Op::Mark { start, end, key, value } => mark(doc, *start, *end, *key, value),
        Op::SetParagraphs { para, remove, insert } => {
            let (para, remove) = (*para, *remove);
            if para > doc.paragraphs.len() || para + remove > doc.paragraphs.len() {
                return Err(EditError::OutOfRange);
            }
            if doc.paragraphs.len() - remove + insert.len() == 0 {
                return Err(EditError::Empty);
            }
            let removed: Vec<Paragraph> = doc.paragraphs.splice(para..para + remove, insert.iter().cloned()).collect();
            Ok(vec![Op::SetParagraphs { para, remove: insert.len(), insert: removed }])
        }
        Op::SetParaStyle { at, style } => {
            let (pi, _) = locate(doc, *at).ok_or(EditError::OutOfRange)?;
            let para = &mut doc.paragraphs[pi];
            let old = para.style.clone();
            let mut new = style.clone();
            new.table_cell = old.table_cell;
            para.style = new;
            Ok(vec![Op::SetParaStyle { at: *at, style: old }])
        }
    }
}

fn insert(doc: &mut Document, at: usize, content: &[Paragraph]) -> Result<Vec<Op>, EditError> {
    let (first, rest) = content.split_first().ok_or(EditError::Empty)?;
    let (pi, po) = locate(doc, at).ok_or(EditError::OutOfRange)?;
    if !rest.is_empty() && doc.paragraphs[pi].style.table_cell.is_some() {
        return Err(EditError::TableStructure);
    }
    let len: usize = content.iter().map(|p| seq_len(&p.runs)).sum::<usize>() + rest.len();
    let para = &mut doc.paragraphs[pi];
    let (mut head, tail) = split_runs(&para.runs, po);
    head.extend(first.runs.iter().cloned());
    if rest.is_empty() {
        head.extend(tail);
        normalize(&mut head);
        para.runs = head;
    } else {
        normalize(&mut head);
        para.runs = head;
        let mut new: Vec<Paragraph> = rest.to_vec();
        for p in &mut new {
            p.style.table_cell = None;
        }
        let last = new.last_mut().expect("rest is not empty");
        last.runs.extend(tail);
        for p in &mut new {
            normalize(&mut p.runs);
        }
        doc.paragraphs.splice(pi + 1..pi + 1, new);
    }
    Ok(vec![Op::Delete { at, len }])
}

fn delete(doc: &mut Document, at: usize, len: usize) -> Result<Vec<Op>, EditError> {
    if len == 0 {
        return Ok(Vec::new());
    }
    let end = at + len;
    let (sp, so) = locate(doc, at).ok_or(EditError::OutOfRange)?;
    let (ep, eo) = locate(doc, end).ok_or(EditError::OutOfRange)?;
    if sp != ep && doc.paragraphs[sp..=ep].iter().any(|p| p.style.table_cell.is_some()) {
        return Err(EditError::TableStructure);
    }
    let removed = slice(doc, at, end).ok_or(EditError::OutOfRange)?;
    let (mut head, _) = split_runs(&doc.paragraphs[sp].runs, so);
    let (_, tail) = split_runs(&doc.paragraphs[ep].runs, eo);
    head.extend(tail);
    normalize(&mut head);
    doc.paragraphs[sp].runs = head;
    doc.paragraphs.drain(sp + 1..=ep);
    // Undo: insert what was removed. Its first paragraph's style is the
    // joined paragraph's own (unchanged), so only later ones matter.
    Ok(vec![Op::Insert { at, content: removed }])
}

fn mark(doc: &mut Document, start: usize, end: usize, key: MarkKey, value: &RunStyle) -> Result<Vec<Op>, EditError> {
    if end <= start {
        return Ok(Vec::new());
    }
    let before = slice(doc, start, end).ok_or(EditError::OutOfRange)?;
    let (sp, so) = locate(doc, start).ok_or(EditError::OutOfRange)?;
    let (ep, eo) = locate(doc, end).ok_or(EditError::OutOfRange)?;
    for i in sp..=ep {
        let p = &mut doc.paragraphs[i];
        let from = if i == sp { so } else { 0 };
        let to = if i == ep { eo } else { seq_len(&p.runs) };
        let (head, rest) = split_runs(&p.runs, from);
        let (mut mid, tail) = split_runs(&rest, to - from);
        for r in mid.iter_mut().filter(|r| !is_object(r)) {
            key.copy(value, &mut r.style);
        }
        let mut runs = head;
        runs.extend(mid);
        runs.extend(tail);
        normalize(&mut runs);
        p.runs = runs;
    }
    // Undo: restore each stretch that had one value for this key.
    let mut undo = Vec::new();
    let mut pos = start;
    for (k, p) in before.iter().enumerate() {
        if k > 0 {
            pos += 1;
        }
        for r in &p.runs {
            let n = run_len(r);
            if !is_object(r) && !key.same(&r.style, value) {
                undo.push(Op::Mark { start: pos, end: pos + n, key, value: marks_only(&r.style) });
            }
            pos += n;
        }
    }
    Ok(undo)
}

/// Apply `ops` in order; on the first failure, undo what was applied and
/// return the error. Returns the undo ops of the whole group, in order.
pub fn apply_all(doc: &mut Document, ops: &[Op]) -> Result<Vec<Op>, EditError> {
    suite_common_core::ops::apply_all(doc, ops)
}

/// The ops typing `text` at `at` makes: one insert in the typing style,
/// with each '\n' starting a paragraph styled like the one it splits.
pub fn typing(doc: &Document, at: usize, text: &str) -> Option<Op> {
    let (pi, _) = locate(doc, at)?;
    let style = typing_style(doc, at);
    let para_style = ParaStyle { table_cell: None, ..doc.paragraphs[pi].style.clone() };
    let content = text
        .split('\n')
        .enumerate()
        .map(|(i, line)| Paragraph {
            style: if i == 0 { ParaStyle::default() } else { para_style.clone() },
            runs: if line.is_empty() { vec![] } else { vec![Run { text: line.replace(OBJECT, ""), style: style.clone() }] },
        })
        .collect();
    Some(Op::Insert { at, content })
}

/// Ops that turn `a` into `b`, as fine-grained as the change allows: an
/// edit inside one paragraph becomes a `Delete` and an `Insert` of just the
/// changed chars (with their styles) and, if needed, a `SetParaStyle`; a
/// change across paragraphs becomes one `Delete` + `Insert` over the
/// changed paragraphs; a change to a table's structure becomes
/// `SetParagraphs`. Applying the result to `a` gives `b` (property-tested).
///
/// This is how an edit made anywhere — a formatting command, a structured
/// edit that rebuilds the document — reaches the live model as ops, so it
/// has an undo and could be replicated.
pub fn diff(a: &Document, b: &Document) -> Vec<Op> {
    let (pa, pb) = (&a.paragraphs, &b.paragraphs);
    let head = pa.iter().zip(pb).take_while(|(x, y)| x == y).count();
    if head == pa.len() && head == pb.len() {
        return Vec::new();
    }
    let max_tail = pa.len().min(pb.len()) - head;
    let tail = pa.iter().rev().zip(pb.iter().rev()).take(max_tail).take_while(|(x, y)| x == y).count();
    let (mut ra, mut rb) = (head..pa.len() - tail, head..pb.len() - tail);
    let tables = |ra: &std::ops::Range<usize>, rb: &std::ops::Range<usize>| {
        pa[ra.clone()].iter().chain(&pb[rb.clone()]).any(|p| p.style.table_cell.is_some())
    };

    // One paragraph changed on both sides (text inside a table cell too).
    if ra.len() == 1 && rb.len() == 1 && pa[head].style.table_cell == pb[head].style.table_cell {
        let at = paragraph_start(a, head);
        let (x, y) = (&pa[head], &pb[head]);
        let mut ops = Vec::new();
        if x.runs != y.runs {
            let cx = styled_chars(&x.runs);
            let cy = styled_chars(&y.runs);
            let pre = cx.iter().zip(&cy).take_while(|(m, n)| m == n).count();
            let suf = cx[pre..].iter().rev().zip(cy[pre..].iter().rev()).take_while(|(m, n)| m == n).count();
            let (dx, dy) = (cx.len() - pre - suf, cy.len() - pre - suf);
            if dx > 0 {
                ops.push(Op::Delete { at: at + pre, len: dx });
            }
            if dy > 0 {
                let (_, rest) = split_runs(&y.runs, pre);
                let (mid, _) = split_runs(&rest, dy);
                ops.push(Op::Insert { at: at + pre, content: vec![Paragraph { style: ParaStyle::default(), runs: mid }] });
            }
        }
        if x.style != y.style {
            ops.push(Op::SetParaStyle { at, style: y.style.clone() });
        }
        return ops;
    }
    // Paragraphs only added or only removed: widen both ranges by an
    // unchanged neighbour, so the change is "rewrite these paragraphs".
    if ra.is_empty() || rb.is_empty() {
        if ra.start > 0 {
            ra.start -= 1;
            rb.start -= 1;
        } else {
            ra.end += 1;
            rb.end += 1;
        }
    }
    if tables(&ra, &rb) {
        return vec![Op::SetParagraphs { para: ra.start, remove: ra.len(), insert: pb[rb].to_vec() }];
    }
    // Rewrite: delete the old paragraphs' text (leaving the first one
    // empty), insert the new paragraphs, and give the first its style.
    let at = paragraph_start(a, ra.start);
    let len_a: usize = pa[ra.clone()].iter().map(|p| seq_len(&p.runs)).sum::<usize>() + ra.len() - 1;
    let mut ops = Vec::new();
    if len_a > 0 {
        ops.push(Op::Delete { at, len: len_a });
    }
    let content: Vec<Paragraph> = pb[rb.clone()].to_vec();
    if content.len() > 1 || !content[0].runs.is_empty() {
        ops.push(Op::Insert { at, content });
    }
    if pa[ra.start].style != pb[rb.start].style {
        ops.push(Op::SetParaStyle { at, style: pb[rb.start].style.clone() });
    }
    ops
}

/// (char, style) pairs of runs in sequence order (an object is one pair).
fn styled_chars(runs: &[Run]) -> Vec<(char, &RunStyle)> {
    runs.iter()
        .flat_map(|r| -> Vec<(char, &RunStyle)> {
            if is_object(r) {
                vec![(OBJECT, &r.style)]
            } else {
                r.text.chars().map(|c| (c, &r.style)).collect()
            }
        })
        .collect()
}

/// Undo and redo over the live document: the suite's one op history
/// (`suite_common_core::ops`, ADR 0011) over Letters' ops. Typed word
/// characters coalesce because consecutive one-char inserts have
/// contiguous one-`Delete` inverses (see `coalesce` below).
pub type History = suite_common_core::ops::History<Op>;

impl suite_common_core::ops::Op for Op {
    type Doc = Document;
    type Error = EditError;

    fn apply(&self, doc: &mut Document) -> Result<Vec<Op>, EditError> {
        apply(doc, self)
    }

    /// The undo of typing "a" then "b" is one delete of both.
    fn coalesce(&mut self, next: &Op) -> bool {
        match (self, next) {
            (Op::Delete { at, len }, Op::Delete { at: next_at, len: next_len }) if *at + *len == *next_at => {
                *len += next_len;
                true
            }
            _ => false,
        }
    }
}

/// A superscript/subscript value for `Op::Mark`.
pub fn vert_align(v: Option<VertAlign>) -> RunStyle {
    RunStyle { vert_align: v, ..Default::default() }
}

#[cfg(test)]
mod tests;
