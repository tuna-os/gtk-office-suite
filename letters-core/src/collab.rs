// SPDX-License-Identifier: GPL-3.0-or-later
//! The document as a Loro rich text: RFC-0001 Phase 5 for Letters, behind
//! the `collab` feature (off by default). A [`Replica`] records local ops
//! into its Loro document and builds the paragraphs back from it. It has no
//! transport: what leaves it is bytes ([`Replica::export_since`],
//! [`Replica::export_snapshot`]), what enters it is bytes
//! ([`Replica::import_updates`], [`Replica::from_snapshot`]), and the
//! sidecar of RFC-0005 carries them. The API is the one of
//! `tables_core::controller::collab::Replica`.
//!
//! # Encoding (ADR 0010, "Design constraints from the CRDT spike")
//!
//! One Loro `Text`, `body`, holding exactly the edit sequence
//! (`edit::sequence_text`): paragraph text, one `OBJECT` char per inline
//! object, one `\n` between paragraphs. Every `edit::Op` offset is an
//! offset into it, counted in Unicode scalar values as the model counts.
//!
//! - **Formatting is Peritext marks**, one Loro mark key per character
//!   attribute (`bold`, `italic`, ..., `revision`). The `MarkKey`s carry
//!   their own expand rule (`MarkKey::expand`): bold, italic, size, colour
//!   expand after, so text typed at their end continues them; a link,
//!   inline code, raw HTML and a tracked change don't. The attributes that
//!   have no `MarkKey` (`regular`, `caps`, `spacing`, `position`) never
//!   expand. Ops carry their style explicitly (ADR 0011, rule 3): an
//!   inserted run writes a mark for every key whose value differs from what
//!   the neighbours' marks expanded into it, so a replay never depends on
//!   what the neighbours happened to be.
//! - **An inline object** (image, text box, smart chip) is its `OBJECT`
//!   char with an `obj` mark (expand none) holding the whole run, except a
//!   tracked change on it, which is a `revision` mark of its own so that
//!   `Op::Mark` reaches it.
//! - **A paragraph's style** is a `p` mark (expand none) on the `\n` that
//!   starts it; the first paragraph's is `first` in the `doc` map. So
//!   deleting a break joins two paragraphs and the joined one keeps the
//!   first one's style, as `Op::Delete` does, and a concurrent restyle of
//!   the second is dropped with its break. Styles are JSON; `serde_json`'s
//!   `float_roundtrip` is on with this feature, so a spacing of `0.1 + 0.2`
//!   comes back bit for bit.
//! - **The page** (`Op::SetPage`) is `page` in the `doc` map, one
//!   last-writer-wins value.
//!
//! # What is refused, and why
//!
//! A document or op with content this encoding does not replicate is
//! refused with [`CollabError::Unsupported`], never silently dropped:
//! tables (design constraint 6: a table is not a text edit and needs its own
//! tree), footnotes, headers and footers, sections, comments, and
//! character formatting that has no mark. The same refusal applies to what a
//! peer sends: [`Replica::import_updates`] reads the whole document of a
//! scratch copy first, and an update that would put any of that into it is
//! an error before it is anyone's live state.
//!
//! # Undo (ADR 0011)
//!
//! Undo stays local, as the ADR says: the history holds the user's own
//! steps, a remote edit is never one of them, and an undo is an ordinary
//! local op that [`Replica::record`] sends like any other edit. What a
//! remote edit does to the stored inverses is [`ImportReport::rebase`]:
//! their offsets are moved over the peer's inserts and deletes, text the
//! peer typed inside a range the step would remove is left alone, and a step
//! that has nothing left to do is dropped, that step only
//! (`History::rebase`, `Rebaser`).
//!
//! # Known limits
//! - Only the paragraphs and the page are replicated. Document-level
//!   settings no op reaches (the base font, heading styles, spacing mode)
//!   stay as the local file had them.
//! - A remote update is read in full by the dry run, so an import costs time
//!   in proportion to the document.
//! - Offsets in this module are the model's sequence offsets. The mapping
//!   from `GtkTextBuffer` offsets to them is the shell's, and is not
//!   exercised here.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use loro::event::Diff;
use loro::{
    ContainerID, ExpandType, ExportMode, LoroDoc, LoroMap, LoroText, LoroValue, StyleConfig, StyleConfigMap, Subscription, TextDelta,
    ValueOrContainer, VersionVector,
};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::edit::{self, History, MarkKey, Op};
use crate::layout::{is_object, OBJECT};
use crate::model::{Alignment, Document, PageGeometry, ParaStyle, Paragraph, Revision, Run, RunStyle, VertAlign};

mod rebase;
pub use rebase::Rebaser;

mod live;
pub use live::{map_offset, map_selection, same_text, Bias, Remote, Shared};

const BODY: &str = "body";
const DOC: &str = "doc";
const FIRST: &str = "first";
const PAGE: &str = "page";
const OBJ: &str = "obj";
const PARA: &str = "p";

/// What the encoding does not replicate (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsupported {
    /// A table cell's paragraph, or a block-level op (`Op::SetParagraphs`).
    Tables,
    /// A footnote's text or the reference to it.
    Footnotes,
    /// A header or footer: text, pictures, lines, alignment, the first
    /// page's own.
    HeadersFooters,
    /// The document's sections (`crate::sections`).
    Sections,
    /// A comment, or the mark that anchors one.
    Comments,
    /// Character formatting that none of the marks carries.
    RunStyle,
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Unsupported::Tables => "tables are not shared yet",
            Unsupported::Footnotes => "footnotes are not shared yet",
            Unsupported::HeadersFooters => "headers and footers are not shared yet",
            Unsupported::Sections => "sections are not shared yet",
            Unsupported::Comments => "comments are not shared yet",
            Unsupported::RunStyle => "this character formatting is not shared yet",
        })
    }
}

/// Why a [`Replica`] call failed. A failed [`Replica::import_updates`]
/// leaves the replica as it was; so does a refused op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollabError {
    /// Content the encoding does not replicate, in a document, an op or a
    /// peer's update.
    Unsupported(Unsupported),
    /// The bytes are not a version, an update or a snapshot that Loro reads,
    /// or are damaged (the checksum fails).
    Decode(String),
    /// The bytes read, but what they would do is refused (DCP spec 7 rule
    /// 3): a mark of the wrong type, an object without its run, a document
    /// that is not a Letters document.
    Invalid(String),
    /// Loro could not export what was asked.
    Export(String),
    /// Loro refused a local change (an offset out of range: a bug).
    Loro(String),
}

impl std::fmt::Display for CollabError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CollabError::Unsupported(what) => write!(f, "{what}"),
            CollabError::Decode(why) => write!(f, "not readable: {why}"),
            CollabError::Invalid(why) => write!(f, "refused: {why}"),
            CollabError::Export(why) => write!(f, "cannot export: {why}"),
            CollabError::Loro(why) => write!(f, "cannot record: {why}"),
        }
    }
}

impl std::error::Error for CollabError {}

fn loro_err(e: impl std::fmt::Display) -> CollabError {
    CollabError::Loro(e.to_string())
}

fn decode_error(e: loro::LoroError) -> CollabError {
    CollabError::Decode(e.to_string())
}

fn out_of_range() -> CollabError {
    CollabError::Invalid("an offset past the end of the document".into())
}

/// A character attribute the document replicates: one Loro mark key.
#[derive(Clone, Copy, Debug)]
enum Attr {
    Key(MarkKey),
    Regular,
    Upright,
    Caps,
    Spacing,
    Position,
}

/// Every attribute, in a fixed order.
fn attrs() -> impl Iterator<Item = Attr> {
    MarkKey::ALL.into_iter().map(Attr::Key).chain([Attr::Regular, Attr::Upright, Attr::Caps, Attr::Spacing, Attr::Position])
}

fn json<T: Serialize>(value: &T) -> Result<String, CollabError> {
    serde_json::to_string(value).map_err(|e| CollabError::Invalid(format!("cannot be written: {e}")))
}

fn parse<T: DeserializeOwned>(text: &str, what: &str) -> Result<T, CollabError> {
    serde_json::from_str(text).map_err(|e| CollabError::Invalid(format!("{what} does not parse: {e}")))
}

impl Attr {
    /// The Loro mark key.
    fn name(self) -> &'static str {
        match self {
            Attr::Key(MarkKey::Bold) => "bold",
            Attr::Key(MarkKey::Italic) => "italic",
            Attr::Key(MarkKey::Underline) => "underline",
            Attr::Key(MarkKey::Strikethrough) => "strike",
            Attr::Key(MarkKey::Highlight) => "highlight",
            Attr::Key(MarkKey::Code) => "code",
            Attr::Key(MarkKey::Link) => "link",
            Attr::Key(MarkKey::FontFamily) => "font",
            Attr::Key(MarkKey::FontSize) => "size",
            Attr::Key(MarkKey::Color) => "color",
            Attr::Key(MarkKey::VertAlign) => "valign",
            Attr::Key(MarkKey::Html) => "html",
            Attr::Key(MarkKey::Revision) => "revision",
            Attr::Key(MarkKey::Comment(_)) => "comment",
            Attr::Regular => "regular",
            Attr::Upright => "upright",
            Attr::Caps => "caps",
            Attr::Spacing => "spacing",
            Attr::Position => "position",
        }
    }

    /// The mark's expand rule: the model's for a `MarkKey`, none for the
    /// attributes typing never carries on.
    fn expand(self) -> ExpandType {
        match self {
            Attr::Key(key) => match key.expand() {
                edit::Expand::After => ExpandType::After,
                edit::Expand::None => ExpandType::None,
            },
            _ => ExpandType::None,
        }
    }

    /// `style`'s value for this attribute; `None` when it is the default
    /// (no mark).
    fn value(self, style: &RunStyle) -> Result<Option<LoroValue>, CollabError> {
        let flag = |b: bool| b.then_some(LoroValue::Bool(true));
        let text = |s: &Option<String>| s.as_ref().map(|s| LoroValue::from(s.as_str()));
        Ok(match self {
            Attr::Key(MarkKey::Bold) => flag(style.bold),
            Attr::Key(MarkKey::Italic) => flag(style.italic),
            Attr::Key(MarkKey::Underline) => flag(style.underline),
            Attr::Key(MarkKey::Strikethrough) => flag(style.strikethrough),
            Attr::Key(MarkKey::Highlight) => flag(style.highlight),
            Attr::Key(MarkKey::Code) => flag(style.code),
            Attr::Key(MarkKey::Link) => text(&style.link),
            Attr::Key(MarkKey::FontFamily) => text(&style.font_family),
            Attr::Key(MarkKey::FontSize) => style.font_size_hp.map(|hp| LoroValue::I64(i64::from(hp))),
            Attr::Key(MarkKey::Color) => text(&style.color),
            Attr::Key(MarkKey::VertAlign) => style.vert_align.map(|v| {
                LoroValue::from(match v {
                    VertAlign::Superscript => "super",
                    VertAlign::Subscript => "sub",
                })
            }),
            Attr::Key(MarkKey::Html) => flag(style.html),
            Attr::Key(MarkKey::Revision) => match &style.revision {
                Some(revision) => Some(LoroValue::from(json(revision)?)),
                None => None,
            },
            // A comment is a mark of its own per id, which is refused.
            Attr::Key(MarkKey::Comment(_)) => None,
            Attr::Regular => flag(style.regular),
            Attr::Upright => flag(style.upright),
            Attr::Caps => flag(style.caps),
            Attr::Spacing => style.letter_spacing_tw.map(|tw| LoroValue::I64(i64::from(tw))),
            Attr::Position => style.position_hp.map(|hp| LoroValue::I64(i64::from(hp))),
        })
    }

    /// Set this attribute on `style` from a mark's `value`; an error when
    /// the value is not of the attribute's type.
    fn read(self, style: &mut RunStyle, value: &LoroValue) -> Result<(), CollabError> {
        let wrong = |what: &str| CollabError::Invalid(format!("the {} mark is not {what}", self.name()));
        let flag = || match value {
            LoroValue::Bool(b) => Ok(*b),
            _ => Err(wrong("a flag")),
        };
        let text = || match value {
            LoroValue::String(s) => Ok(s.to_string()),
            _ => Err(wrong("text")),
        };
        let int = || match value {
            LoroValue::I64(n) => Ok(*n),
            _ => Err(wrong("a number")),
        };
        match self {
            Attr::Key(MarkKey::Bold) => style.bold = flag()?,
            Attr::Key(MarkKey::Italic) => style.italic = flag()?,
            Attr::Key(MarkKey::Underline) => style.underline = flag()?,
            Attr::Key(MarkKey::Strikethrough) => style.strikethrough = flag()?,
            Attr::Key(MarkKey::Highlight) => style.highlight = flag()?,
            Attr::Key(MarkKey::Code) => style.code = flag()?,
            Attr::Key(MarkKey::Link) => style.link = Some(text()?),
            Attr::Key(MarkKey::FontFamily) => style.font_family = Some(text()?),
            Attr::Key(MarkKey::FontSize) => style.font_size_hp = Some(u16::try_from(int()?).map_err(|_| wrong("a size"))?),
            Attr::Key(MarkKey::Color) => style.color = Some(text()?),
            Attr::Key(MarkKey::VertAlign) => {
                style.vert_align = Some(match text()?.as_str() {
                    "super" => VertAlign::Superscript,
                    "sub" => VertAlign::Subscript,
                    _ => return Err(wrong("super or sub")),
                })
            }
            Attr::Key(MarkKey::Html) => style.html = flag()?,
            Attr::Key(MarkKey::Revision) => style.revision = Some(parse::<Revision>(&text()?, "a tracked change")?),
            Attr::Key(MarkKey::Comment(_)) => return Err(CollabError::Unsupported(Unsupported::Comments)),
            Attr::Regular => style.regular = flag()?,
            Attr::Upright => style.upright = flag()?,
            Attr::Caps => style.caps = flag()?,
            Attr::Spacing => style.letter_spacing_tw = Some(i32::try_from(int()?).map_err(|_| wrong("a spacing"))?),
            Attr::Position => style.position_hp = Some(i32::try_from(int()?).map_err(|_| wrong("a position"))?),
        }
        Ok(())
    }
}

/// Every mark's expand rule, as the model's; `obj` and `p` never expand.
fn style_config() -> StyleConfigMap {
    let mut styles = StyleConfigMap::new();
    for attr in attrs() {
        styles.insert(attr.name().into(), StyleConfig { expand: attr.expand() });
    }
    styles.insert(OBJ.into(), StyleConfig { expand: ExpandType::None });
    styles.insert(PARA.into(), StyleConfig { expand: ExpandType::None });
    styles
}

/// `style` as the marks would give it back: what a text run keeps.
fn through_marks(style: &RunStyle) -> Result<RunStyle, CollabError> {
    let mut back = RunStyle::default();
    for attr in attrs() {
        if let Some(value) = attr.value(style)? {
            attr.read(&mut back, &value)?;
        }
    }
    Ok(back)
}

/// Refuse a run the encoding cannot carry whole.
fn check_run(run: &Run) -> Result<(), CollabError> {
    if !run.style.comments.is_empty() {
        return Err(CollabError::Unsupported(Unsupported::Comments));
    }
    if run.style.footnote.is_some() {
        return Err(CollabError::Unsupported(Unsupported::Footnotes));
    }
    if is_object(run) {
        return Ok(());
    }
    if run.text.contains(OBJECT) {
        return Err(CollabError::Invalid("a text run holds the object character".into()));
    }
    if through_marks(&run.style)? != run.style {
        return Err(CollabError::Unsupported(Unsupported::RunStyle));
    }
    Ok(())
}

fn check_paragraphs(paragraphs: &[Paragraph]) -> Result<(), CollabError> {
    for p in paragraphs {
        if p.style.table_cell.is_some() {
            return Err(CollabError::Unsupported(Unsupported::Tables));
        }
        p.runs.iter().try_for_each(check_run)?;
    }
    Ok(())
}

/// Refuse what a document holds outside its paragraphs that is not
/// replicated.
fn check_document_level(doc: &Document) -> Result<(), CollabError> {
    let unsupported = |what| Err(CollabError::Unsupported(what));
    if !doc.footnotes.is_empty() {
        return unsupported(Unsupported::Footnotes);
    }
    if doc.header.is_some()
        || doc.footer.is_some()
        || !doc.header_pictures.is_empty()
        || !doc.footer_pictures.is_empty()
        || !doc.header_lines.is_empty()
        || !doc.footer_lines.is_empty()
        || doc.header_alignment != Alignment::Left
        || doc.footer_alignment != Alignment::Left
        || doc.first_header.is_some()
        || doc.first_footer.is_some()
    {
        return unsupported(Unsupported::HeadersFooters);
    }
    if !doc.sections.is_empty() {
        return unsupported(Unsupported::Sections);
    }
    if !doc.comments.is_empty() {
        return unsupported(Unsupported::Comments);
    }
    Ok(())
}

/// A paragraph style as it is stored: JSON, without a table identity.
fn para_json(style: &ParaStyle) -> Result<String, CollabError> {
    json(&ParaStyle { table_cell: None, ..style.clone() })
}

/// What a replica's document holds, read strictly.
struct View {
    paragraphs: Vec<Paragraph>,
    page: Option<PageGeometry>,
}

/// Read `text` and `meta` into paragraphs. Anything the encoding does not
/// say is an error, not a default: this is also the dry run's check.
fn read_view(text: &LoroText, meta: &LoroMap) -> Result<View, CollabError> {
    let para_style = |json: &str| -> Result<ParaStyle, CollabError> {
        let style: ParaStyle = parse(json, "a paragraph style")?;
        if style.table_cell.is_some() {
            return Err(CollabError::Unsupported(Unsupported::Tables));
        }
        Ok(style)
    };
    let first = match meta.get(FIRST) {
        Some(ValueOrContainer::Value(LoroValue::String(s))) => para_style(&s)?,
        _ => return Err(CollabError::Invalid("the document has no first paragraph: it is not a Letters document".into())),
    };
    let page = match meta.get(PAGE) {
        Some(ValueOrContainer::Value(LoroValue::String(s))) => parse::<Option<PageGeometry>>(&s, "the page")?,
        None => None,
        Some(_) => return Err(CollabError::Invalid("the page is not text".into())),
    };
    let mut paragraphs = vec![Paragraph { style: first, runs: Vec::new() }];
    for segment in text.to_delta() {
        let TextDelta::Insert { insert, attributes } = segment else { continue };
        let attrs: HashMap<String, LoroValue> =
            attributes.unwrap_or_default().into_iter().filter(|(_, v)| !matches!(v, LoroValue::Null)).collect();
        let known = |key: &str| key == OBJ || key == PARA || attrs_by_name(key).is_some();
        if let Some(key) = attrs.keys().find(|k| !known(k.as_str())) {
            return Err(CollabError::Invalid(format!("the mark {key:?} is not one this version knows")));
        }
        let mut style = RunStyle::default();
        for attr in attrs_in(&attrs) {
            if let Some(value) = attrs.get(attr.name()) {
                attr.read(&mut style, value)?;
            }
        }
        for ch in insert.chars() {
            match ch {
                '\n' => {
                    let style = match attrs.get(PARA) {
                        Some(LoroValue::String(s)) => para_style(s)?,
                        Some(_) => return Err(CollabError::Invalid("the paragraph mark is not text".into())),
                        None => ParaStyle::default(),
                    };
                    paragraphs.push(Paragraph { style, runs: Vec::new() });
                }
                OBJECT => {
                    let Some(LoroValue::String(s)) = attrs.get(OBJ) else {
                        return Err(CollabError::Invalid("an object character without its object".into()));
                    };
                    let mut run: Run = parse(s, "an object")?;
                    if !is_object(&run) {
                        return Err(CollabError::Invalid("the object mark holds a run that is not an object".into()));
                    }
                    run.style.revision = style.revision.clone();
                    check_run(&run)?;
                    paragraphs.last_mut().ok_or_else(out_of_range)?.runs.push(run);
                }
                c => {
                    if attrs.contains_key(OBJ) {
                        return Err(CollabError::Invalid("an object mark on text".into()));
                    }
                    let runs = &mut paragraphs.last_mut().ok_or_else(out_of_range)?.runs;
                    match runs.last_mut() {
                        Some(last) if !is_object(last) && last.style == style => last.text.push(c),
                        _ => runs.push(Run { text: c.to_string(), style: style.clone() }),
                    }
                }
            }
        }
    }
    Ok(View { paragraphs, page })
}

/// The attribute whose mark key is `name`.
fn attrs_by_name(name: &str) -> Option<Attr> {
    attrs().find(|a| a.name() == name)
}

/// The attributes that have a mark among `marks`.
fn attrs_in(marks: &HashMap<String, LoroValue>) -> impl Iterator<Item = Attr> + '_ {
    attrs().filter(|a| marks.contains_key(a.name()))
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

    /// Start a shared document from `doc`, as peer `peer`. A document with
    /// content the encoding does not replicate is refused
    /// ([`CollabError::Unsupported`]), not shared in part.
    pub fn new(peer: u64, doc: &Document) -> Result<Self, CollabError> {
        check_document_level(doc)?;
        check_paragraphs(&doc.paragraphs)?;
        let me = Self::with_doc(LoroDoc::new(), peer)?;
        let first = doc.paragraphs.first().map(|p| p.style.clone()).unwrap_or_default();
        me.meta.insert(FIRST, para_json(&first)?).map_err(loro_err)?;
        me.meta.insert(PAGE, json(&doc.page)?).map_err(loro_err)?;
        // The whole document is one insert at 0 into an empty text: its
        // first paragraph is the text's first paragraph.
        me.insert(0, &doc.paragraphs)?;
        me.doc.commit();
        Ok(me)
    }

    /// A second peer on a copy of this document.
    pub fn fork(&self, peer: u64) -> Result<Replica, CollabError> {
        self.doc.commit();
        Self::with_doc(self.doc.fork(), peer)
    }

    /// A peer that has only the history in `updates` (from
    /// [`Replica::export_all`]): the whole document replayed from its ops,
    /// and `base` with the paragraphs and page it holds.
    pub fn from_updates(peer: u64, updates: &[u8], base: &Document) -> Result<(Replica, Document), CollabError> {
        let doc = LoroDoc::new();
        doc.import(updates).map_err(decode_error)?;
        let me = Self::with_doc(doc, peer)?;
        let document = me.rebuild(base)?;
        Ok((me, document))
    }

    /// A peer that joins through `snapshot` (from [`Replica::export_snapshot`]
    /// of a replica already in the session): its replica and `base` (the
    /// file the session started from, which the joiner opened) with the
    /// paragraphs and page the document holds. `peer` must be one this
    /// document has never seen and is never used for it again. The
    /// document is read before it is anyone's live state, so a snapshot
    /// that is not a Letters document, or holds what is refused, is an
    /// error here.
    pub fn from_snapshot(peer: u64, snapshot: &[u8], base: &Document) -> Result<(Replica, Document), CollabError> {
        let doc = LoroDoc::from_snapshot(snapshot).map_err(decode_error)?;
        let me = Self::with_doc(doc, peer)?;
        let document = me.rebuild(base)?;
        Ok((me, document))
    }

    /// The document's whole history, as updates.
    pub fn export_all(&self) -> Result<Vec<u8>, CollabError> {
        self.doc.commit();
        self.doc.export(ExportMode::all_updates()).map_err(|e| CollabError::Export(e.to_string()))
    }

    /// The document's version, as opaque bytes (a Loro version vector): what
    /// is handed to [`Replica::export_since`] to take the changes after it.
    pub fn version_bytes(&self) -> Vec<u8> {
        self.doc.commit();
        self.doc.oplog_vv().encode()
    }

    /// Whether this replica is exactly at `version` (from
    /// [`Replica::version_bytes`]). The bytes of a version vector are not
    /// canonical (the vector is a hash map), so two equal versions are
    /// compared decoded, never as bytes.
    pub fn is_at(&self, version: &[u8]) -> Result<bool, CollabError> {
        let theirs = VersionVector::decode(version).map_err(|e| CollabError::Decode(format!("not a version: {e}")))?;
        self.doc.commit();
        Ok(self.doc.oplog_vv() == theirs)
    }

    /// What this replica has that a peer at `version` (from its
    /// [`Replica::version_bytes`]) lacks: the delta one update carries.
    pub fn export_since(&self, version: &[u8]) -> Result<Vec<u8>, CollabError> {
        let from = VersionVector::decode(version).map_err(|e| CollabError::Decode(format!("not a version: {e}")))?;
        self.doc.commit();
        self.doc.export(ExportMode::updates_owned(from)).map_err(|e| CollabError::Export(e.to_string()))
    }

    /// The document's state and history in one blob: the genesis a joiner
    /// starts from ([`Replica::from_snapshot`]).
    pub fn export_snapshot(&self) -> Result<Vec<u8>, CollabError> {
        self.doc.commit();
        self.doc.export(ExportMode::Snapshot).map_err(|e| CollabError::Export(e.to_string()))
    }

    /// Take in what `other` has that this replica doesn't (the update goes
    /// through [`Replica::import_updates`]).
    pub fn merge_from(&mut self, other: &Replica) -> Result<ImportReport, CollabError> {
        let updates = other.export_since(&self.version_bytes())?;
        self.import_updates(&updates)
    }

    /// Take in an update from a peer (a delta from [`Replica::export_since`],
    /// or a snapshot), in any order and any number of times: updates that
    /// are already here change nothing, and updates that arrive before the
    /// ones they follow wait ([`ImportReport::pending`]) and apply on their
    /// own once those arrive.
    ///
    /// This is the dry run of DCP spec 7 rule 3. The update is applied to a
    /// fork first and the live document is changed only if the fork still
    /// reads as a document: every mark of the type its key says, every
    /// object with its run, nothing the encoding refuses. On any error the
    /// replica is exactly as it was.
    ///
    /// The paragraphs are not touched: the report says what text moved
    /// ([`ImportReport::edits`]), [`ImportReport::rebase`] moves the undo
    /// history over it, and [`Replica::rebuild`] builds the document the
    /// replica now holds.
    pub fn import_updates(&mut self, updates: &[u8]) -> Result<ImportReport, CollabError> {
        // The header and the checksum first: garbage costs nothing but this.
        LoroDoc::decode_import_blob_meta(updates, true).map_err(decode_error)?;
        self.doc.commit();
        let probe = self.doc.fork();
        let (tape, _subscription) = watch(&probe);
        let status = probe.import(updates).map_err(decode_error)?;
        let mut report = std::mem::take(&mut *tape.lock().unwrap_or_else(PoisonError::into_inner));
        read_view(&probe.get_text(BODY), &probe.get_map(DOC))?;
        self.doc.import(updates).map_err(decode_error)?;
        report.ops = status.success.iter().map(|(_, &(from, to))| usize::try_from(to - from).unwrap_or(0)).sum();
        report.pending = status.pending.is_some_and(|range| !range.is_empty());
        Ok(report)
    }

    /// The paragraphs of the (merged) document.
    pub fn paragraphs(&self) -> Result<Vec<Paragraph>, CollabError> {
        self.doc.commit();
        Ok(read_view(&self.text, &self.meta)?.paragraphs)
    }

    /// `base` with the paragraphs and the page this replica's document
    /// holds: the document the editor shows. `base` is the local file, whose
    /// other parts (the base font, heading styles) the document does not
    /// carry; it must hold nothing that is refused either.
    pub fn rebuild(&self, base: &Document) -> Result<Document, CollabError> {
        check_document_level(base)?;
        self.doc.commit();
        let view = read_view(&self.text, &self.meta)?;
        let mut document = base.clone();
        document.paragraphs = view.paragraphs;
        document.page = view.page;
        Ok(document)
    }

    // ---------------------------------------------------------------- local edits

    /// Whether `doc` is one a session can start from, with nothing built:
    /// the refusal [`Replica::new`] would give ([`CollabError::Unsupported`]
    /// for a table, a footnote, a header or footer, sections, comments or
    /// character formatting no mark carries).
    pub fn check_document(doc: &Document) -> Result<(), CollabError> {
        check_document_level(doc)?;
        check_paragraphs(&doc.paragraphs)
    }

    /// Whether `op`, applied to `before`, is one the encoding replicates,
    /// with nothing written: the refusal [`Replica::record`] would give
    /// ([`CollabError::Unsupported`] for tables, comments, footnotes, headers
    /// and footers, sections and unmarked formatting; [`CollabError::Invalid`]
    /// for an offset past the end).
    pub fn check_op(before: &Document, op: &Op) -> Result<(), CollabError> {
        match op {
            Op::Insert { at, content } => {
                if content.is_empty() {
                    return Err(CollabError::Invalid("nothing to insert".into()));
                }
                check_paragraphs(content)?;
                let (p, _) = edit::locate(before, *at).ok_or_else(out_of_range)?;
                if before.paragraphs[p].style.table_cell.is_some() {
                    return Err(CollabError::Unsupported(Unsupported::Tables));
                }
            }
            Op::Delete { at, len } => {
                if *len == 0 {
                    return Ok(());
                }
                let (Some((a, _)), Some((b, _))) = (edit::locate(before, *at), edit::locate(before, at + len)) else {
                    return Err(out_of_range());
                };
                if before.paragraphs[a..=b].iter().any(|p| p.style.table_cell.is_some()) {
                    return Err(CollabError::Unsupported(Unsupported::Tables));
                }
            }
            Op::Mark { start, end, key, value } => {
                if matches!(key, MarkKey::Comment(_)) {
                    return Err(CollabError::Unsupported(Unsupported::Comments));
                }
                if end <= start {
                    return Ok(());
                }
                edit::locate(before, *end).ok_or_else(out_of_range)?;
                Attr::Key(*key).value(value)?;
            }
            Op::SetParaStyle { at, style } => {
                let (p, _) = edit::locate(before, *at).ok_or_else(out_of_range)?;
                if before.paragraphs[p].style.table_cell.is_some() || style.table_cell.is_some() {
                    return Err(CollabError::Unsupported(Unsupported::Tables));
                }
                para_json(style)?;
            }
            Op::SetPage { page } => {
                json(page)?;
            }
            Op::SetParagraphs { .. } => return Err(CollabError::Unsupported(Unsupported::Tables)),
            Op::SetComment { .. } => return Err(CollabError::Unsupported(Unsupported::Comments)),
            Op::SetFootnotes { .. } => return Err(CollabError::Unsupported(Unsupported::Footnotes)),
            Op::SetHeaderFooter { .. } => return Err(CollabError::Unsupported(Unsupported::HeadersFooters)),
            Op::SetSections { .. } => return Err(CollabError::Unsupported(Unsupported::Sections)),
        }
        Ok(())
    }

    /// Record local `op`, applied to the document that was `before` it (the
    /// editor's model, in step with this replica), as one change. An op the
    /// encoding does not replicate is refused ([`CollabError::Unsupported`])
    /// before anything is written.
    pub fn record(&mut self, before: &Document, op: &Op) -> Result<(), CollabError> {
        Self::check_op(before, op)?;
        match op {
            Op::Insert { at, content } => self.insert(*at, content)?,
            Op::Delete { at, len } => {
                if *len == 0 {
                    return Ok(());
                }
                self.text.delete(*at, *len).map_err(loro_err)?;
            }
            Op::Mark { start, end, key, value } => {
                if end <= start {
                    return Ok(());
                }
                self.set_attr(*start..*end, Attr::Key(*key), value)?;
            }
            Op::SetParaStyle { at, style } => {
                let (p, _) = edit::locate(before, *at).ok_or_else(out_of_range)?;
                self.set_para_style(p, edit::paragraph_start(before, p), style)?;
            }
            Op::SetPage { page } => self.meta.insert(PAGE, json(page)?).map_err(loro_err)?,
            // `check_op` refused these.
            Op::SetParagraphs { .. } | Op::SetComment { .. } | Op::SetFootnotes { .. } | Op::SetHeaderFooter { .. } | Op::SetSections { .. } => {
                return Err(CollabError::Unsupported(Unsupported::Tables));
            }
        }
        self.doc.commit();
        Ok(())
    }

    /// Record the ops of one user action, each applied to the document the
    /// one before it left (`before` for the first), as all or nothing: when
    /// any of them is refused, or does not apply, nothing is written. One
    /// change per op, so a delta exported afterwards carries the whole group.
    pub fn record_all(&mut self, before: &Document, ops: &[Op]) -> Result<(), CollabError> {
        if let [op] = ops {
            return self.record(before, op);
        }
        let mut scratch = before.clone();
        for op in ops {
            Self::check_op(&scratch, op)?;
            edit::apply(&mut scratch, op).map_err(|e| CollabError::Invalid(format!("an op does not apply: {e:?}")))?;
        }
        let mut doc = before.clone();
        for op in ops {
            self.record(&doc, op)?;
            edit::apply(&mut doc, op).map_err(|e| CollabError::Invalid(format!("an op does not apply: {e:?}")))?;
        }
        Ok(())
    }

    /// Set (or clear) `attr` on `range` to `style`'s value.
    fn set_attr(&self, range: std::ops::Range<usize>, attr: Attr, style: &RunStyle) -> Result<(), CollabError> {
        match attr.value(style)? {
            Some(value) => self.text.mark(range, attr.name(), value).map_err(loro_err),
            None => self.text.unmark(range, attr.name()).map_err(loro_err),
        }
    }

    fn set_para_style(&self, index: usize, start: usize, style: &ParaStyle) -> Result<(), CollabError> {
        let value = para_json(style)?;
        if index == 0 {
            self.meta.insert(FIRST, value).map_err(loro_err)
        } else {
            // The break before the paragraph carries its style.
            self.text.mark(start - 1..start, PARA, value).map_err(loro_err)
        }
    }

    /// Insert `content` at `at`, as `Op::Insert` does: its first
    /// paragraph's runs join the paragraph at `at`, each further one starts
    /// a paragraph with its own style (on its break). `content` has been
    /// checked.
    fn insert(&self, at: usize, content: &[Paragraph]) -> Result<(), CollabError> {
        let mut pos = at;
        for (i, p) in content.iter().enumerate() {
            if i > 0 {
                self.text.insert(pos, "\n").map_err(loro_err)?;
                self.text.mark(pos..pos + 1, PARA, para_json(&p.style)?).map_err(loro_err)?;
                pos += 1;
            }
            for run in &p.runs {
                let object = is_object(run);
                let n = if object { 1 } else { run.text.chars().count() };
                if n == 0 {
                    continue;
                }
                if object {
                    self.text.insert(pos, OBJECT.encode_utf8(&mut [0u8; 4])).map_err(loro_err)?;
                    // The tracked change is a mark of its own, outside the
                    // run, so that `Op::Mark` reaches it.
                    let mut stored = run.clone();
                    stored.style.revision = None;
                    self.text.mark(pos..pos + 1, OBJ, json(&stored)?).map_err(loro_err)?;
                } else {
                    self.text.insert(pos, &run.text).map_err(loro_err)?;
                }
                // The run's style is explicit (ADR 0011 rule 3), but only
                // the attributes whose inherited value (a neighbour's mark
                // expanding in) differs are written: typed text takes
                // exactly what the expand rules give it, so typing records
                // no mark at all, and a concurrent mark at the edge still
                // expands over it on merge, as Peritext intends. An object
                // keeps only its tracked change here; its other marks are
                // its own run's.
                let inherited = self.marks_at(pos, pos + n)?;
                for attr in attrs() {
                    if object && !matches!(attr, Attr::Key(MarkKey::Revision)) {
                        continue;
                    }
                    let want = attr.value(&run.style)?;
                    let have = inherited.get(attr.name()).filter(|v| !matches!(v, LoroValue::Null)).cloned();
                    if want != have {
                        self.set_attr(pos..pos + n, attr, &run.style)?;
                    }
                }
                pos += n;
            }
        }
        Ok(())
    }

    /// The marks on `start..end`, as the first of its segments has them
    /// (text just inserted is uniformly marked by what expanded into it).
    fn marks_at(&self, start: usize, end: usize) -> Result<HashMap<String, LoroValue>, CollabError> {
        let delta = self.text.slice_delta(start, end, loro::cursor::PosType::Unicode).map_err(loro_err)?;
        Ok(delta
            .into_iter()
            .find_map(|d| match d {
                TextDelta::Insert { attributes, .. } => Some(attributes.unwrap_or_default().into_iter().collect()),
                _ => None,
            })
            .unwrap_or_default())
    }
}

/// One change to the text, in sequence offsets (chars, as the model counts
/// them). A list of these is sequential: each is in the coordinates the one
/// before it leaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEdit {
    /// `len` chars inserted at `at`.
    Insert { at: usize, len: usize },
    /// `len` chars deleted from `at`.
    Delete { at: usize, len: usize },
}

/// What an imported update changed in the document, enough to decide what
/// it does to the local undo history (ADR 0011).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Ops the update added; 0 when every one of them was already here.
    pub ops: usize,
    /// Ops that arrived before ones they follow: they wait inside the
    /// document and apply, with a report of their own, when those arrive.
    pub pending: bool,
    /// The text inserted and deleted, in order. Offsets in the editor's
    /// model move over these.
    pub edits: Vec<TextEdit>,
    /// Marks changed on text that was already there: a character or
    /// paragraph style, a tracked change.
    pub formatting: bool,
    /// The page was set.
    pub page: bool,
}

impl ImportReport {
    /// Whether the update added anything.
    pub fn changed(&self) -> bool {
        self.ops > 0
    }

    /// A [`Rebaser`] for the steps of one undo or redo chain, top first.
    pub fn rebaser(&self) -> Rebaser {
        Rebaser::new(&self.edits)
    }

    /// Move the stored steps of `history` over what the update did to the
    /// text: ADR 0011's "stored inverses are rebased on remote events". A
    /// step that no longer fits is dropped, that step alone.
    pub fn rebase(&self, history: &mut History) {
        history.rebase(|| self.rebaser(), |rebaser, ops| rebaser.step(ops));
    }

    fn note(&mut self, target: &ContainerID, diff: &Diff) {
        let ContainerID::Root { name, .. } = target else { return };
        match (name.to_string().as_str(), diff) {
            (BODY, Diff::Text(deltas)) => {
                let mut pos = 0;
                for delta in deltas {
                    match delta {
                        TextDelta::Retain { retain, attributes } => {
                            self.formatting |= attributes.is_some();
                            pos += retain;
                        }
                        TextDelta::Insert { insert, .. } => {
                            let len = insert.chars().count();
                            self.edits.push(TextEdit::Insert { at: pos, len });
                            pos += len;
                        }
                        TextDelta::Delete { delete } => self.edits.push(TextEdit::Delete { at: pos, len: *delete }),
                    }
                }
            }
            (DOC, Diff::Map(map)) => {
                self.page |= map.updated.contains_key(PAGE);
                self.formatting |= map.updated.contains_key(FIRST);
            }
            _ => {}
        }
    }
}

/// Read the events of `doc` into an [`ImportReport`] until the subscription
/// drops.
fn watch(doc: &LoroDoc) -> (Arc<Mutex<ImportReport>>, Subscription) {
    let tape: Arc<Mutex<ImportReport>> = Arc::default();
    let sink = Arc::clone(&tape);
    let subscription = doc.subscribe_root(Arc::new(move |event| {
        let mut report = sink.lock().unwrap_or_else(PoisonError::into_inner);
        for diff in &event.events {
            report.note(diff.target, &diff.diff);
        }
    }));
    (tape, subscription)
}

#[cfg(test)]
mod tests;
