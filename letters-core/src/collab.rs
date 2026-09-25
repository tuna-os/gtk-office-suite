// SPDX-License-Identifier: GPL-3.0-or-later
//! The document as a Loro rich text: RFC-0001 Phase 5 for Letters, behind
//! the `collab` feature (off by default). No transport yet: a [`Replica`]
//! records local ops into its Loro document, exchanges updates with another
//! replica in-process, and rebuilds the paragraphs from the merged text.
//!
//! # Encoding (ADR 0010, "Design constraints from the CRDT spike")
//!
//! One Loro `Text`, `body`, holding exactly the edit sequence
//! (`edit::sequence_text`): paragraph text, one `OBJECT` char per inline
//! object, one `\n` between paragraphs. Every `edit::Op` offset is an
//! offset into it.
//!
//! - **Formatting is Peritext marks**, one Loro mark key per `MarkKey`,
//!   configured with the key's own expand rule (`MarkKey::expand`): bold,
//!   italic, size, colour… expand after, so text typed at their end
//!   continues them, concurrently too; a link, inline code and raw HTML
//!   don't. Ops carry their style explicitly (rule 3): an inserted run sets
//!   every key it has and clears every key it hasn't, so a replay never
//!   depends on what the neighbours happened to be.
//! - **An inline object** (image, footnote reference, smart chip) is its
//!   `OBJECT` char with an `obj` mark (expand none) holding the whole run.
//! - **A paragraph's style** is a `p` mark (expand none) on the `\n` that
//!   starts it; the first paragraph's is `first` in the `doc` map. So
//!   deleting a break joins two paragraphs and the joined one keeps the
//!   first one's style, as `Op::Delete` does, and a concurrent restyle of
//!   the second is dropped with its break.
//! - **Tables are not sequence edits** (rule 6): `Op::SetParagraphs`, and
//!   any op carrying a table cell's paragraph, is refused
//!   ([`CollabError::Tables`]). A table needs its own tree or map (the
//!   Tables and Decks encodings), which is later work.
//!
//! # Known limits
//! - Only the paragraphs are replicated: footnote texts, the header and
//!   footer, page setup and heading styles are not yet (a footnote
//!   reference replicates, its text doesn't).
//! - Undo stays local (ADR 0011): the history's inverse ops are recorded
//!   like any other op.

use loro::{ExpandType, ExportMode, LoroDoc, LoroMap, LoroText, LoroValue, StyleConfig, StyleConfigMap, TextDelta, ValueOrContainer};

use crate::edit::{self, MarkKey, Op};
use crate::layout::{is_object, OBJECT};
use crate::model::{Document, ParaStyle, Paragraph, Run, RunStyle, VertAlign};

const BODY: &str = "body";
const DOC: &str = "doc";
const FIRST: &str = "first";
const OBJ: &str = "obj";
const PARA: &str = "p";

/// Why an op could not be replicated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollabError {
    /// Tables change through structure, not the text (see the module docs).
    Tables,
    /// Loro refused the change (an offset out of range: a bug).
    Loro(String),
}

fn loro_err(e: impl std::fmt::Display) -> CollabError {
    CollabError::Loro(e.to_string())
}

/// The Loro mark key of `key`.
fn mark_name(key: MarkKey) -> &'static str {
    match key {
        MarkKey::Bold => "bold",
        MarkKey::Italic => "italic",
        MarkKey::Underline => "underline",
        MarkKey::Strikethrough => "strike",
        MarkKey::Highlight => "highlight",
        MarkKey::Code => "code",
        MarkKey::Link => "link",
        MarkKey::FontFamily => "font",
        MarkKey::FontSize => "size",
        MarkKey::Color => "color",
        MarkKey::VertAlign => "valign",
        MarkKey::Html => "html",
    }
}

/// Every key's expand rule, as the model's (`MarkKey::expand`); `obj` and
/// `p` never expand.
fn style_config() -> StyleConfigMap {
    let mut styles = StyleConfigMap::new();
    for key in MarkKey::ALL {
        let expand = match key.expand() {
            edit::Expand::After => ExpandType::After,
            edit::Expand::None => ExpandType::None,
        };
        styles.insert(mark_name(key).into(), StyleConfig { expand });
    }
    styles.insert(OBJ.into(), StyleConfig { expand: ExpandType::None });
    styles.insert(PARA.into(), StyleConfig { expand: ExpandType::None });
    styles
}

/// `style`'s value for `key`, `None` when it is the default (no mark).
fn mark_value(style: &RunStyle, key: MarkKey) -> Option<LoroValue> {
    let flag = |b: bool| b.then_some(LoroValue::Bool(true));
    let text = |s: &Option<String>| s.as_ref().map(|s| LoroValue::from(s.as_str()));
    match key {
        MarkKey::Bold => flag(style.bold),
        MarkKey::Italic => flag(style.italic),
        MarkKey::Underline => flag(style.underline),
        MarkKey::Strikethrough => flag(style.strikethrough),
        MarkKey::Highlight => flag(style.highlight),
        MarkKey::Code => flag(style.code),
        MarkKey::Link => text(&style.link),
        MarkKey::FontFamily => text(&style.font_family),
        MarkKey::FontSize => style.font_size_hp.map(|hp| LoroValue::I64(i64::from(hp))),
        MarkKey::Color => text(&style.color),
        MarkKey::VertAlign => style.vert_align.map(|v| {
            LoroValue::from(match v {
                VertAlign::Superscript => "super",
                VertAlign::Subscript => "sub",
            })
        }),
        MarkKey::Html => flag(style.html),
    }
}

/// Set `key` on `style` from a mark's `value`.
fn read_mark(style: &mut RunStyle, key: MarkKey, value: &LoroValue) {
    let on = matches!(value, LoroValue::Bool(true));
    let text = match value {
        LoroValue::String(s) => Some(s.to_string()),
        _ => None,
    };
    match key {
        MarkKey::Bold => style.bold = on,
        MarkKey::Italic => style.italic = on,
        MarkKey::Underline => style.underline = on,
        MarkKey::Strikethrough => style.strikethrough = on,
        MarkKey::Highlight => style.highlight = on,
        MarkKey::Code => style.code = on,
        MarkKey::Link => style.link = text,
        MarkKey::FontFamily => style.font_family = text,
        MarkKey::FontSize => {
            style.font_size_hp = match value {
                LoroValue::I64(hp) => u16::try_from(*hp).ok(),
                _ => None,
            }
        }
        MarkKey::Color => style.color = text,
        MarkKey::VertAlign => {
            style.vert_align = match text.as_deref() {
                Some("super") => Some(VertAlign::Superscript),
                Some("sub") => Some(VertAlign::Subscript),
                _ => None,
            }
        }
        MarkKey::Html => style.html = on,
    }
}

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("the model serializes")
}

fn has_table(p: &Paragraph) -> bool {
    p.style.table_cell.is_some()
}

/// One peer's copy of the shared document.
pub struct Replica {
    doc: LoroDoc,
    text: LoroText,
    meta: LoroMap,
}

impl Replica {
    fn with_doc(doc: LoroDoc, peer: u64) -> Result<Self, CollabError> {
        doc.set_peer_id(peer).map_err(loro_err)?;
        doc.config_text_style(style_config());
        let text = doc.get_text(BODY);
        let meta = doc.get_map(DOC);
        Ok(Replica { doc, text, meta })
    }

    /// Start a shared document from `doc`'s paragraphs, as peer `peer`.
    pub fn new(peer: u64, doc: &Document) -> Result<Self, CollabError> {
        if doc.paragraphs.iter().any(has_table) {
            return Err(CollabError::Tables);
        }
        let me = Self::with_doc(LoroDoc::new(), peer)?;
        let first = doc.paragraphs.first().cloned().unwrap_or_default();
        me.meta.insert(FIRST, json(&first.style)).map_err(loro_err)?;
        let content: Vec<Paragraph> = doc.paragraphs.clone();
        // The whole document is one insert at 0 into an empty text: its
        // first paragraph is the text's first paragraph.
        me.insert(0, &content)?;
        me.doc.commit();
        Ok(me)
    }

    /// A second peer on a copy of this document, with the paragraphs it
    /// shows.
    pub fn fork(&self, peer: u64) -> Result<(Replica, Vec<Paragraph>), CollabError> {
        self.doc.commit();
        let other = Self::with_doc(self.doc.fork(), peer)?;
        let paragraphs = other.paragraphs();
        Ok((other, paragraphs))
    }

    /// Take in what `other` has that this replica doesn't; then
    /// [`Replica::paragraphs`] is the merged document.
    pub fn merge_from(&mut self, other: &Replica) -> Result<(), CollabError> {
        other.doc.commit();
        let updates = other.doc.export(ExportMode::updates(&self.doc.oplog_vv())).map_err(loro_err)?;
        self.doc.import(&updates).map_err(loro_err)?;
        Ok(())
    }

    /// Record local `op`, applied to the document that was `before` it
    /// (the replica's own paragraphs, or the editor's model in step).
    pub fn record(&mut self, before: &Document, op: &Op) -> Result<(), CollabError> {
        match op {
            Op::Insert { at, content } => {
                if content.iter().any(has_table) || edit::locate(before, *at).is_some_and(|(p, _)| has_table(&before.paragraphs[p])) {
                    return Err(CollabError::Tables);
                }
                self.insert(*at, content)?;
            }
            Op::Delete { at, len } => {
                let (Some((a, _)), Some((b, _))) = (edit::locate(before, *at), edit::locate(before, at + len)) else {
                    return Err(CollabError::Loro("delete out of range".into()));
                };
                if before.paragraphs[a..=b].iter().any(has_table) {
                    return Err(CollabError::Tables);
                }
                self.text.delete(*at, *len).map_err(loro_err)?;
            }
            Op::Mark { start, end, key, value } => {
                if *end > *start {
                    self.set_mark(*start..*end, *key, value)?;
                }
            }
            Op::SetParaStyle { at, style } => {
                let (p, _) = edit::locate(before, *at).ok_or_else(|| CollabError::Loro("style out of range".into()))?;
                if has_table(&before.paragraphs[p]) || style.table_cell.is_some() {
                    return Err(CollabError::Tables);
                }
                self.set_para_style(p, edit::paragraph_start(before, p), style)?;
            }
            Op::SetParagraphs { .. } => return Err(CollabError::Tables),
        }
        self.doc.commit();
        Ok(())
    }

    /// Set (or clear) mark `key` on `range` to `value`'s field.
    fn set_mark(&self, range: std::ops::Range<usize>, key: MarkKey, value: &RunStyle) -> Result<(), CollabError> {
        match mark_value(value, key) {
            Some(v) => self.text.mark(range, mark_name(key), v).map_err(loro_err),
            None => self.text.unmark(range, mark_name(key)).map_err(loro_err),
        }
    }

    fn set_para_style(&self, index: usize, start: usize, style: &ParaStyle) -> Result<(), CollabError> {
        let value = json(&ParaStyle { table_cell: None, ..style.clone() });
        if index == 0 {
            self.meta.insert(FIRST, value).map_err(loro_err)
        } else {
            // The break before the paragraph carries its style.
            self.text.mark(start - 1..start, PARA, value).map_err(loro_err)
        }
    }

    /// Insert `content` at `at`, as `Op::Insert` does: its first
    /// paragraph's runs join the paragraph at `at`, each further one starts
    /// a paragraph with its own style (on its break).
    fn insert(&self, at: usize, content: &[Paragraph]) -> Result<(), CollabError> {
        let mut pos = at;
        for (i, p) in content.iter().enumerate() {
            if i > 0 {
                self.text.insert(pos, "\n").map_err(loro_err)?;
                self.text.mark(pos..pos + 1, PARA, json(&ParaStyle { table_cell: None, ..p.style.clone() })).map_err(loro_err)?;
                pos += 1;
            }
            for run in &p.runs {
                let n = if is_object(run) { 1 } else { run.text.chars().count() };
                if n == 0 {
                    continue;
                }
                if is_object(run) {
                    self.text.insert(pos, &OBJECT.to_string()).map_err(loro_err)?;
                    self.text.mark(pos..pos + 1, OBJ, json(run)).map_err(loro_err)?;
                } else {
                    self.text.insert(pos, &run.text).map_err(loro_err)?;
                    // The run's style is explicit (rule 3), but only keys
                    // whose inherited value (a neighbour's mark expanding
                    // in) differs are written: typed text takes exactly
                    // what the expand rules give it, so typing records no
                    // mark at all, and a concurrent mark at the edge still
                    // expands over it on merge, as Peritext intends.
                    let inherited = self.marks_at(pos, pos + n)?;
                    for key in MarkKey::ALL {
                        let want = mark_value(&run.style, key);
                        let have = inherited.get(mark_name(key)).filter(|v| !matches!(v, LoroValue::Null)).cloned();
                        if want != have {
                            self.set_mark(pos..pos + n, key, &run.style)?;
                        }
                    }
                }
                pos += n;
            }
        }
        Ok(())
    }

    /// The marks on `start..end`, as the first of its segments has them
    /// (text just inserted is uniformly marked by what expanded into it).
    fn marks_at(&self, start: usize, end: usize) -> Result<std::collections::HashMap<String, LoroValue>, CollabError> {
        let delta = self.text.slice_delta(start, end, loro::cursor::PosType::Unicode).map_err(loro_err)?;
        Ok(delta
            .into_iter()
            .find_map(|d| match d {
                TextDelta::Insert { attributes, .. } => Some(attributes.unwrap_or_default().into_iter().collect()),
                _ => None,
            })
            .unwrap_or_default())
    }

    /// The paragraphs of the (merged) document.
    pub fn paragraphs(&self) -> Vec<Paragraph> {
        let first: ParaStyle = match self.meta.get(FIRST) {
            Some(ValueOrContainer::Value(LoroValue::String(s))) => serde_json::from_str(&s).unwrap_or_default(),
            _ => ParaStyle::default(),
        };
        let mut paragraphs = vec![Paragraph { style: first, runs: Vec::new() }];
        for segment in self.text.to_delta() {
            let TextDelta::Insert { insert, attributes } = segment else { continue };
            let attrs = attributes.unwrap_or_default();
            let attr = |k: &str| attrs.get(k).filter(|v| !matches!(v, LoroValue::Null));
            let mut style = RunStyle::default();
            for key in MarkKey::ALL {
                if let Some(v) = attr(mark_name(key)) {
                    read_mark(&mut style, key, v);
                }
            }
            for ch in insert.chars() {
                match ch {
                    '\n' => {
                        let style: ParaStyle = match attr(PARA) {
                            Some(LoroValue::String(s)) => serde_json::from_str(s).unwrap_or_default(),
                            _ => ParaStyle::default(),
                        };
                        paragraphs.push(Paragraph { style, runs: Vec::new() });
                    }
                    OBJECT => {
                        if let Some(LoroValue::String(s)) = attr(OBJ) {
                            if let Ok(run) = serde_json::from_str::<Run>(s) {
                                paragraphs.last_mut().expect("a paragraph").runs.push(run);
                            }
                        }
                    }
                    c => {
                        let runs = &mut paragraphs.last_mut().expect("a paragraph").runs;
                        match runs.last_mut() {
                            Some(last) if !is_object(last) && last.style == style => last.text.push(c),
                            _ => runs.push(Run { text: c.to_string(), style: style.clone() }),
                        }
                    }
                }
            }
        }
        paragraphs
    }
}

#[cfg(test)]
mod tests;
