// model.rs — the document model: paragraphs of styled runs.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Invariants (enforced by normalize(), asserted by tests/model.rs):
//   1. Run.text is never empty.
//   2. Adjacent runs with identical style are merged.
//   3. A document always has at least one paragraph.
//
// Offsets are global character offsets; each paragraph break counts as one
// character, matching GtkTextBuffer's offset model exactly.

use serde::{Deserialize, Serialize};

pub use crate::sections::{DocSection, ParagraphRange};

/// Character-level formatting. `Default` is unstyled text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunStyle {
    pub bold: bool,
    /// Drawn regular where the paragraph's look is bold (a heading, the
    /// Title): ODF's `fo:font-weight="normal"`, OOXML's `<w:b w:val="0"/>`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub regular: bool,
    pub italic: bool,
    /// Drawn in capitals, whatever case the text is stored in: OOXML's
    /// `<w:caps/>`, ODF's `fo:text-transform="uppercase"` (a form's title
    /// style). The text keeps its own case.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub caps: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub highlight: bool,
    /// Inline code (monospace) span.
    pub code: bool,
    pub link: Option<String>,
    /// Inline image: source path/URI. The run's text is the alt text.
    pub image: Option<String>,
    /// Inline image's displayed size as (width, height) in EMU (914400 per
    /// inch, OOXML's unit); None = the image's own size.
    #[serde(default)]
    pub image_extent_emu: Option<(u64, u64)>,
    /// The room kept beside an inline image, left and right, in EMU: an
    /// ODF frame's margins, or Writer's own 3.19mm a side for a .docx
    /// picture that leaves out `distL`/`distR`. None is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_sides_emu: Option<(u64, u64)>,
    /// A floating image's or text box's place on the page; None = inline
    /// in the text. A floating object takes no room in its line: it is
    /// drawn where its anchor puts it, over the text (Word's "in front of
    /// text").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_anchor: Option<ImageAnchor>,
    /// A text box (ODF's `draw:frame` holding a `draw:text-box`, Word's
    /// `wps:txbx`): a framed block of text floating where `image_anchor`
    /// puts it. The run's own text is empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_box: Option<Box<TextBox>>,
    /// Font family name (e.g. "Liberation Serif"); None = document default.
    #[serde(default)]
    pub font_family: Option<String>,
    /// Font size in half-points (24 = 12pt); None = document default.
    pub font_size_hp: Option<u16>,
    /// Text color as RRGGBB hex (no '#'); None = default.
    pub color: Option<String>,
    /// Superscript/subscript position.
    pub vert_align: Option<VertAlign>,
    /// Extra space after each character, in twentieths of a point
    /// (OOXML's `w:rPr/w:spacing`, ODF's `fo:letter-spacing`); negative
    /// condenses. None = normal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub letter_spacing_tw: Option<i32>,
    /// Raised (positive) or lowered (negative) from the baseline by this
    /// many half-points, at the run's own size (OOXML `w:position`); the
    /// line grows to hold it, as LibreOffice draws it. None = on the
    /// baseline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_hp: Option<i32>,
    /// Footnote reference: index into `Document::footnotes`. The run's
    /// text is the visible marker (usually empty; renderers number it).
    #[serde(default)]
    pub footnote: Option<usize>,
    /// Raw inline HTML: the run's text is emitted verbatim on Markdown
    /// export (never escaped) and rendered as plain text in the editor.
    #[serde(default)]
    pub html: bool,
    /// Smart chip (a date, person or link) — an inline object whose text
    /// is its label (`crate::chips`).
    #[serde(default)]
    pub chip: Option<crate::chips::Chip>,
    /// A tracked change: this text was inserted, or deleted, by someone at
    /// some time and waits to be accepted or rejected (`crate::track`).
    /// Deleted text stays in the document, marked, until accepted.
    #[serde(default)]
    pub revision: Option<Revision>,
    /// The comments on this text: ids of `Document::comments`, ascending.
    /// Each comment is a mark of its own (`edit::MarkKey::Comment`), so
    /// comments can overlap (`crate::comments`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<u32>,
}

/// A comment: its text, who wrote it and when. A thread's first comment is
/// anchored to the text marked with its id; a reply names the comment it
/// answers and has no text of its own (`crate::comments`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Comment {
    pub id: u32,
    pub author: String,
    /// ISO 8601 UTC, to the second, as tracked changes are dated.
    pub date: String,
    pub text: String,
    #[serde(default)]
    pub resolved: bool,
    /// The comment this one replies to.
    #[serde(default)]
    pub parent: Option<u32>,
}

/// What a tracked change did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RevisionKind {
    Insert,
    Delete,
}

/// A tracked change on a run: what, by whom, when.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Revision {
    pub kind: RevisionKind,
    pub author: String,
    /// ISO 8601 UTC, to the second ("2026-09-25T20:30:00Z"), as Word and
    /// ODF store it.
    pub date: String,
    /// A deletion of someone else's pending insertion: that insertion.
    /// Rejecting the deletion brings it back (Word's `w:del` inside
    /// `w:ins`).
    #[serde(default)]
    pub under: Option<Box<Revision>>,
}

/// A text box's size, frame and text. Lengths are in EMU (914400 per inch,
/// OOXML's unit), as an image's extent is.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextBox {
    /// Width and height of its outer box.
    pub extent_emu: (u64, u64),
    /// The rule drawn around it; None is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<BoxRule>,
    /// Its background colour (six hex digits); None is transparent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    /// Room between its frame and its text: top, right, bottom, left.
    #[serde(default)]
    pub padding_emu: [u32; 4],
    /// Its text, a line per paragraph.
    #[serde(default)]
    pub text: String,
}

/// A solid rule `width_emu` wide in `color` (six hex digits).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxRule {
    pub width_emu: u32,
    pub color: String,
}

/// Where a floating image sits: on each axis an offset from, or an
/// alignment within, a frame (Word's `wp:anchor`, ODF's `svg:x`/`svg:y`
/// with `style:horizontal-rel`/`vertical-rel`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageAnchor {
    pub h_from: AnchorFrame,
    /// Offset of the image's left edge from the frame's, in EMU.
    pub x_emu: i64,
    /// Aligned within the frame instead of offset.
    pub h_align: Option<AnchorAlign>,
    pub v_from: AnchorFrame,
    /// Offset of the image's top edge from the frame's, in EMU.
    pub y_emu: i64,
    pub v_align: Option<AnchorAlign>,
    /// Drawn behind the text rather than in front of it.
    pub behind: bool,
    /// How the text flows around it.
    #[serde(default)]
    pub wrap: Wrap,
}

/// How text flows around a floating picture or text box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wrap {
    /// Over or under it, as if it were not there (Writer's run-through,
    /// Word's "in front of" or "behind text").
    #[default]
    None,
    /// Beside it, on its wider side (Writer's parallel, Word's square).
    Parallel,
    /// Above and below it, nothing beside it.
    TopBottom,
}

/// The box a floating image is placed in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnchorFrame {
    /// The whole page.
    Page,
    /// The page inside its margins.
    Margin,
    /// The anchoring paragraph: its column horizontally, its top
    /// vertically.
    #[default]
    Text,
}

/// Start, centre or end of a frame (left/top, centre, right/bottom).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnchorAlign {
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VertAlign {
    Superscript,
    Subscript,
}

/// A partial style change: `None` fields are left untouched.
#[derive(Clone, Debug, Default)]
pub struct StylePatch {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    pub highlight: Option<bool>,
    pub code: Option<bool>,
    pub link: Option<Option<String>>,
}

impl StylePatch {
    pub fn set_bold(v: bool) -> Self { Self { bold: Some(v), ..Default::default() } }
    pub fn set_italic(v: bool) -> Self { Self { italic: Some(v), ..Default::default() } }
    pub fn set_underline(v: bool) -> Self { Self { underline: Some(v), ..Default::default() } }
    pub fn set_strikethrough(v: bool) -> Self { Self { strikethrough: Some(v), ..Default::default() } }
    pub fn set_highlight(v: bool) -> Self { Self { highlight: Some(v), ..Default::default() } }
    pub fn set_code(v: bool) -> Self { Self { code: Some(v), ..Default::default() } }
    pub fn set_link(v: Option<String>) -> Self { Self { link: Some(v), ..Default::default() } }

    pub fn apply_to(&self, style: &mut RunStyle) {
        if let Some(v) = self.bold { style.bold = v; }
        if let Some(v) = self.italic { style.italic = v; }
        if let Some(v) = self.underline { style.underline = v; }
        if let Some(v) = self.strikethrough { style.strikethrough = v; }
        if let Some(v) = self.highlight { style.highlight = v; }
        if let Some(v) = self.code { style.code = v; }
        if let Some(v) = &self.link { style.link = v.clone(); }
    }
}

fn is_left(a: &Alignment) -> bool {
    *a == Alignment::Left
}

/// One line (paragraph) of a header's or footer's text: its alignment,
/// the look of its text (font, size, weight, slant, colour; a line is
/// drawn in one look), and its tab stops.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HfLine {
    #[serde(default, skip_serializing_if = "is_left")]
    pub alignment: Alignment,
    #[serde(default)]
    pub look: RunStyle,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tab_stops: Vec<TabStop>,
    /// A line that floats out of the header's or footer's flow, as a text
    /// box anchored to the page does (a classification label such as
    /// "OFFICIAL" at the page's top): it takes no room there, and sits
    /// across the page by its alignment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub float: Option<HfFloat>,
    /// The room above and below the line's paragraph, in points (a
    /// letterhead's blank lines with Normal's space after).
    #[serde(default, skip_serializing_if = "is_no_space")]
    pub space_before_pt: f64,
    #[serde(default, skip_serializing_if = "is_no_space")]
    pub space_after_pt: f64,
    /// How tall the line is, when its paragraph fixes it (exactly or at
    /// least); None is its text's own height.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_height: Option<LineHeight>,
}

fn is_no_space(pt: &f64) -> bool {
    *pt == 0.0
}

/// A header or footer of its own for a page: its text ("{page}"
/// substitutes the page number), the pictures above it, how the text is
/// aligned and each line of it, as `Document::header` and its
/// companions have them. An empty story is a page without one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HfStory {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pictures: Vec<Run>,
    #[serde(default, skip_serializing_if = "is_left")]
    pub alignment: Alignment,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<HfLine>,
}

/// Where a floating header or footer line's text is: its distance from
/// the page's top edge (to the text's top) or bottom edge (to its foot),
/// in points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum HfFloat {
    Top(f64),
    Bottom(f64),
}

/// Where the text after a tab stop sits relative to the stop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TabAlign {
    #[default]
    Left,
    Center,
    Right,
    /// On the decimal separator (OOXML `decimal`, ODF `char`).
    Decimal,
}

/// What fills the gap a tab jump leaves (OOXML `w:leader`, ODF
/// `style:leader-style`/`style:leader-text`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TabLeader {
    #[default]
    None,
    Dot,
    Hyphen,
    Underscore,
    MiddleDot,
}

/// One tab stop: where the text after a tab goes, how it aligns there,
/// and what fills the gap. Positions are points from the paragraph's
/// left edge, as `ParaStyle::tab_stops_pt` keeps them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct TabStop {
    pub pos_pt: f64,
    #[serde(default)]
    pub align: TabAlign,
    #[serde(default)]
    pub leader: TabLeader,
}

impl TabStop {
    /// A plain left-aligned stop with no leader, as the ruler and older
    /// documents (which stored bare positions) mean.
    pub fn left(pos_pt: f64) -> Self {
        Self { pos_pt, align: TabAlign::Left, leader: TabLeader::None }
    }
}

impl<'de> serde::Deserialize<'de> for TabStop {
    /// Older documents stored bare positions (`[72.0, 144.0]`); those
    /// read as plain left-aligned stops.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Compat {
            Pos(f64),
            Full { pos_pt: f64, #[serde(default)] align: TabAlign, #[serde(default)] leader: TabLeader },
        }
        Ok(match Compat::deserialize(deserializer)? {
            Compat::Pos(pos_pt) => TabStop::left(pos_pt),
            Compat::Full { pos_pt, align, leader } => TabStop { pos_pt, align, leader },
        })
    }
}

/// A paragraph's line height beyond the proportional multiple
/// (`ParaStyle::line_spacing`): OOXML's `w:lineRule="exact"` and
/// `"atLeast"`, in points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum LineHeight {
    /// Every line is exactly this tall; taller content overflows into
    /// the next line, as Word and Writer draw it.
    Exact(f64),
    /// Every line is at least this tall, and taller when its content
    /// needs it.
    Minimum(f64),
}

/// One side of a paragraph's border (OOXML `w:pBdr`, ODF `fo:border-*`):
/// its look, its colour as RRGGBB hex, its width in points, and the gap
/// between it and the text (`w:space`, ODF `fo:padding-*`) in points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParaBorder {
    pub style: BorderStyle,
    pub color: String,
    pub width_pt: f64,
    #[serde(default)]
    pub space_pt: f64,
}

impl ParaBorder {
    /// How far this side stands off the text: its gap plus its width.
    pub fn inset_pt(&self) -> f64 {
        // An invisible edge has no width, whatever size it names.
        let width = if self.style == BorderStyle::None { 0.0 } else { self.width_pt.max(0.0) };
        self.space_pt.max(0.0) + width
    }
}

/// A paragraph border's look. Only the looks both formats name the same
/// way are modelled; anything else (wave, triple, inset) is a loss the
/// save dialog reports (`crate::loss`), never a silent restyle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BorderStyle {
    /// No line, but the edge keeps its gap: Word's `w:val="none"` with a
    /// `w:space`, which Writer still pads a paragraph's top and bottom
    /// with (render-lab `real-questionnaire`: 31pt above and below the
    /// blank line between its tables).
    None,
    Single,
    Dashed,
    Dotted,
    Double,
}

/// A paragraph's borders, one optional edge each (OOXML `w:pBdr`,
/// ODF `fo:border-top` and its siblings). Empty is no border.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ParaBorders {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<ParaBorder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<ParaBorder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<ParaBorder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<ParaBorder>,
}

impl ParaBorders {
    pub fn is_empty(&self) -> bool {
        self.top.is_none() && self.left.is_none() && self.bottom.is_none() && self.right.is_none()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListKind {
    #[default]
    None,
    Bullet,
    Numbered,
}

/// How a numbered item shows its number: `prefix`, the number in
/// `format`, then `suffix`. An item numbered "3.1" under a heading that
/// Word or LibreOffice numbers 3 has the prefix "3.": the label is what
/// the file showed, and the model counts only the item's own level.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListLabel {
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub format: NumberFormat,
    #[serde(default)]
    pub suffix: String,
}

/// The digits of a list number. `None` shows no number, only the label's
/// text (Word's level text "2.1" with no placeholder).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NumberFormat {
    #[default]
    Decimal,
    LowerLetter,
    UpperLetter,
    LowerRoman,
    UpperRoman,
    None,
}

/// Paragraph-level formatting.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParaStyle {
    /// Heading level 1..=6, or None for body text.
    pub heading: Option<u8>,
    pub alignment: Alignment,
    pub list: ListKind,
    /// Nesting depth for list items. Zero is the top-level list.
    #[serde(default)]
    pub list_level: u8,
    /// Optional first number for a numbered list item (restart support).
    #[serde(default)]
    pub list_start: Option<u32>,
    /// A numbered item's label when it is not "N.": Word's "3.%2" and
    /// "(%1)", ODF's num-prefix and num-suffix. `None` is "N.".
    #[serde(default)]
    pub list_label: Option<ListLabel>,
    /// How far a list item's marker sits before its text, in points, when
    /// the file says other than `lists::HANGING_PT` (Word's numbered
    /// "1.1" paragraphs hang their number 0.5in).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_hanging_pt: Option<f64>,
    /// Line spacing multiplier (1.0, 1.15, 1.5, 2.0). Applies to every
    /// line unless `line_height` fixes the height.
    pub line_spacing: f32,
    /// A fixed or minimum line height, in points (OOXML `w:lineRule`
    /// exact/atLeast, ODF `fo:line-height` length /
    /// `style:line-height-at-least`). `None` is purely proportional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_height: Option<LineHeight>,
    /// Paragraph layout values in points. Tabs are absolute stops from the
    /// paragraph's left edge, also in points.
    #[serde(default)]
    pub space_before_pt: f64,
    #[serde(default)]
    pub space_after_pt: f64,
    /// A list item's is from its level's own text indent
    /// (`lists::text_indent_pt`), and may stand left of it as far as the
    /// margin.
    #[serde(default)]
    pub left_indent_pt: f64,
    #[serde(default)]
    pub right_indent_pt: f64,
    #[serde(default)]
    pub first_line_indent_pt: f64,
    #[serde(default)]
    pub tab_stops_pt: Vec<TabStop>,
    /// Code-block membership: Some(language) marks this paragraph as a line
    /// of a code block ("" = no language). Consecutive code paragraphs with
    /// the same language form one block; paragraphs never contain newlines,
    /// so a multi-line block is a run of these.
    pub code_block: Option<String>,
    /// Block quote membership.
    pub block_quote: bool,
    /// Raw HTML block line (Markdown source fidelity, like code_block:
    /// consecutive html_block paragraphs form one block, emitted verbatim).
    #[serde(default)]
    pub html_block: bool,
    /// Start this paragraph on a new page.
    pub page_break_before: bool,
    /// Named document style (e.g. "Title", "Subtitle"); orthogonal to
    /// heading levels.
    pub named_style: Option<String>,
    /// The file defines `named_style` itself and its runs carry that
    /// look (an imported Title's 26pt plain accent text), so layout adds
    /// none of the built-in one (`layout::Look`) on top.
    #[serde(default)]
    pub own_look: bool,
    /// Table membership: this paragraph lives in the given cell. The
    /// document stays a flat paragraph list (offset invariants intact);
    /// consecutive paragraphs sharing a `table` id form one table, and
    /// (row, col) recovers the grid.
    pub table_cell: Option<TableCell>,
    /// Keep this paragraph on the page of the one after it (Word's
    /// keepNext, ODF's fo:keep-with-next): a Title, or a style that asks.
    /// Headings are kept with what follows whatever this says.
    #[serde(default)]
    pub keep_with_next: bool,
    /// Keep this paragraph's lines on one page (Word's keepLines, ODF's
    /// fo:keep-together): a paragraph that fits a page never splits. One
    /// taller than a page still splits, as Word splits it.
    #[serde(default)]
    pub keep_lines: bool,
    /// Widow/orphan control for this paragraph alone: `None` is the
    /// document default (controlled, two and two), `Some(false)` lets a
    /// split strand single lines (Word's widowControl off, ODF
    /// fo:widows/fo:orphans of one), `Some(true)` controls explicitly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widow_control: Option<bool>,
    /// Paragraph shading fill as RRGGBB hex (OOXML `w:shd`, ODF
    /// fo:background-color on the paragraph style). `None` is unshaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shading: Option<String>,
    /// Paragraph borders, one edge each (OOXML `w:pBdr`, ODF
    /// `fo:border-*`). Empty is no border.
    #[serde(default, skip_serializing_if = "ParaBorders::is_empty")]
    pub borders: ParaBorders,
    /// A table of contents entry for a heading of this level (1..=9): its
    /// title, a tab and its page number, as `crate::toc` generates it.
    /// Consecutive entries form one table of contents, which is a field in
    /// .docx and an index in .odt, so other applications can update it.
    #[serde(default)]
    pub toc: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableCell {
    pub table: u32,
    pub row: u32,
    pub col: u32,
}

impl Default for ParaStyle {
    fn default() -> Self {
        Self { heading: None, alignment: Alignment::Left, list: ListKind::None, list_level: 0, list_start: None, list_label: None, list_hanging_pt: None, line_spacing: 1.0, line_height: None, space_before_pt: 0.0, space_after_pt: 0.0, left_indent_pt: 0.0, right_indent_pt: 0.0, first_line_indent_pt: 0.0, tab_stops_pt: Vec::new(), code_block: None, block_quote: false, html_block: false, page_break_before: false, named_style: None, own_look: false, table_cell: None, keep_with_next: false, keep_lines: false, widow_control: None, shading: None, borders: ParaBorders::default(), toc: None }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub text: String,
    pub style: RunStyle,
}

impl Run {
    pub fn plain(text: impl Into<String>) -> Self {
        Self { text: text.into(), style: RunStyle::default() }
    }
    pub fn char_len(&self) -> usize {
        self.text.chars().count()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Paragraph {
    pub style: ParaStyle,
    pub runs: Vec<Run>,
}

/// Join each run into the one before it when `mergeable` says so, in one
/// pass. The readers used to do this with `Vec::remove` in a loop, which
/// shifts every later run on each join: a paragraph of n same-styled runs
/// took O(n²), and a 40 KB Markdown file of nested brackets took four
/// seconds to open (#1206).
pub(crate) fn merge_adjacent_runs(runs: &mut Vec<Run>, mergeable: impl Fn(&Run, &Run) -> bool) {
    let mut merged: Vec<Run> = Vec::with_capacity(runs.len());
    for run in runs.drain(..) {
        match merged.last_mut() {
            Some(last) if mergeable(last, &run) => last.text.push_str(&run.text),
            _ => merged.push(run),
        }
    }
    *runs = merged;
}

impl Paragraph {
    pub fn char_len(&self) -> usize {
        self.runs.iter().map(Run::char_len).sum()
    }

    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// Merge adjacent runs with equal style and drop empty runs.
    /// Image and footnote-reference runs are kept even with empty text
    /// and never merged.
    fn normalize(&mut self) {
        self.runs
            .retain(|r| !r.text.is_empty() || crate::layout::is_object(r));
        // Objects are never merged: two chips side by side stay two.
        merge_adjacent_runs(&mut self.runs, |a, b| {
            !crate::layout::is_object(a) && !crate::layout::is_object(b) && a.style == b.style
        });
    }

    /// Split at a character offset within this paragraph; self keeps the
    /// head, returns the tail (same paragraph style).
    fn split_at(&mut self, offset: usize) -> Paragraph {
        let mut head: Vec<Run> = Vec::new();
        let mut tail: Vec<Run> = Vec::new();
        let mut pos = 0;
        for run in self.runs.drain(..) {
            let len = run.char_len();
            if pos + len <= offset {
                head.push(run);
            } else if pos >= offset {
                tail.push(run);
            } else {
                let split = offset - pos;
                let byte = run.text.char_indices().nth(split).map(|(b, _)| b).unwrap_or(run.text.len());
                let (a, b) = run.text.split_at(byte);
                head.push(Run { text: a.to_string(), style: run.style.clone() });
                tail.push(Run { text: b.to_string(), style: run.style });
            }
            pos += len;
        }
        self.runs = head;
        self.normalize();
        let mut tail_para = Paragraph { style: self.style.clone(), runs: tail };
        tail_para.normalize();
        tail_para
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub paragraphs: Vec<Paragraph>,
    /// Footnote texts; runs reference them by index via
    /// `RunStyle::footnote`.
    pub footnotes: Vec<String>,
    /// Page header/footer text; "{page}" substitutes the page number.
    pub header: Option<String>,
    pub footer: Option<String>,
    /// The pictures in the header, above its text (a letterhead's logo):
    /// image runs (`RunStyle::image`, its extent, its alt text as the
    /// run's text, and an anchor for one that floats).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub header_pictures: Vec<Run>,
    /// The pictures in the footer, above its text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub footer_pictures: Vec<Run>,
    /// How the header's text is aligned (its first paragraph with text):
    /// a bill's "FINANCE BILL" at the right, a classification centred.
    #[serde(default, skip_serializing_if = "is_left")]
    pub header_alignment: Alignment,
    /// How the footer's text is aligned.
    #[serde(default, skip_serializing_if = "is_left")]
    pub footer_alignment: Alignment,
    /// Each line (paragraph) of the header's text in turn: how it is
    /// aligned, the look of its text and its tab stops (a contract's 16pt
    /// title centred, a ship's name with its ID at a right tab). Empty, or
    /// short of a line: that line as `header_alignment` has it, in the
    /// body's look.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub header_lines: Vec<HfLine>,
    /// Each line of the footer's text in turn (a page number at the right
    /// over a date at the left).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub footer_lines: Vec<HfLine>,
    /// Whether the space after a paragraph and the space before the next
    /// add up between them, as Writer spaces every ODF document; otherwise
    /// the larger of the two applies, as Word does and Writer does for a
    /// Word file.
    #[serde(default, skip_serializing_if = "is_false")]
    pub space_adds: bool,
    /// The first page's own header, when it has one (Word's "different
    /// first page", ODF's `style:header-first`): a title page's letterhead
    /// over a running header. `None` is the first page with the header the
    /// others have.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_header: Option<HfStory>,
    /// The first page's own footer, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_footer: Option<HfStory>,
    /// Page size and margins; None = application default (A4).
    pub page: Option<PageGeometry>,
    /// The body font the document's own styles name; runs without a font
    /// of their own are drawn in it.
    #[serde(default)]
    pub base_font: BaseFont,
    /// How headings 1–6 look (index 0 is level 1), from the document's
    /// heading styles: font, size, weight, slant, colour. Empty means the
    /// application's own heading look (`layout::heading_scale`, bold).
    #[serde(default)]
    pub heading_styles: Vec<RunStyle>,
    /// The comments, by ascending id (`crate::comments`).
    #[serde(default)]
    pub comments: Vec<Comment>,
    /// Each table's column widths in points, by table id, as the file gave
    /// them. A table without any (one made here), or whose column count
    /// has changed since, is drawn with equal columns.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub table_columns: std::collections::BTreeMap<u32, Vec<f64>>,
    /// Each table's row heights, by table id and then row (`None` for a
    /// row that sizes to its content), as the file gave them.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub table_rows: std::collections::BTreeMap<u32, Vec<Option<RowHeight>>>,
    /// Each table's merged cells, by table id: a cell that spans more than
    /// one row or column, and the grid positions it covers. A covered
    /// position keeps its (empty) paragraph, so the grid stays whole.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub table_spans: std::collections::BTreeMap<u32, Vec<CellSpan>>,
    /// Each table's shaded cells, by table id: a form's grey header cells
    /// (`w:shd`, `fo:background-color`). A cell not listed is unshaded.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub table_fills: std::collections::BTreeMap<u32, Vec<CellFill>>,
    /// Each table's cell padding, by table id (Word's cell margins, ODF's
    /// `fo:padding`). A table not listed has Word's default.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub table_padding: std::collections::BTreeMap<u32, CellPadding>,
    /// How far right of the text's left edge each table's first column
    /// starts its text, by table id; its left padding sits further left.
    /// Word's `w:tblInd` (plus the left padding under Word 2013's rules),
    /// ODF's table `fo:margin-left` plus the left padding. A table not
    /// listed lines its text up with the paragraphs around it.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub table_indent: std::collections::BTreeMap<u32, f64>,
    /// Tables drawn without rules, by table id: a file's table that
    /// declares no visible border (Word's "Normal Table", a form's grey
    /// answer box). A table not listed (any made here) is ruled.
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub table_unruled: std::collections::BTreeSet<u32>,
    /// The colour of a ruled table's rules (six hex digits), by table id:
    /// Word's "Table Grid Light" and Writer's grey borders. A ruled table
    /// not listed (any made here) is ruled in black.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub table_rule_colors: std::collections::BTreeMap<u32, String>,
    /// The width of a ruled table's rules in points, by table id, when not
    /// the 0.5pt a table made here has: a form whose grid is 0.75pt stands
    /// each row that much taller.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub table_rule_widths: std::collections::BTreeMap<u32, f64>,
    /// Each table's rows kept on one page, by table id (Word's
    /// `w:cantSplit`, ODF's row `fo:keep-together="always"`): a row that
    /// does not fit what is left of a page starts the next, unless it is
    /// taller than a page.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub table_whole_rows: std::collections::BTreeMap<u32, std::collections::BTreeSet<u32>>,
    /// The tables the file sized as a share of the text's width (Word's
    /// `pct`, ODF's `rel-width` or `margins` alignment), by table id: their
    /// columns fit the text however wide the file gave them. Any other
    /// table keeps the file's widths, running on into the right margin as
    /// Word and Writer draw it.
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    pub table_relative: std::collections::BTreeSet<u32>,
    /// Each ruled table's cells that leave a side without a rule, by
    /// table id: a form's header row whose two cells read as one band.
    /// A cell not listed is ruled all round.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub table_open_edges: std::collections::BTreeMap<u32, Vec<CellEdges>>,
    /// Each table's cells set lower than the top, by table id: a form's
    /// label centred against a tall answer cell (`w:vAlign`,
    /// `style:vertical-align`). A cell not listed sits at the top.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub table_valign: std::collections::BTreeMap<u32, Vec<CellVAlign>>,
    /// The document's sections when its parts have different pages,
    /// headers or footers (`crate::sections`, LET-005S); empty for a
    /// document of one section, whose page is `page`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<DocSection>,
    /// The id the next new section takes: ids are never given twice, so
    /// one deleted and undone is still its own.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub next_section_id: u32,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// The room between a table cell's edges and its text, in points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CellPadding {
    pub top_pt: f64,
    pub bottom_pt: f64,
    pub left_pt: f64,
    pub right_pt: f64,
}

impl Default for CellPadding {
    /// Word's and LibreOffice's default for a table: 0.08in left and right
    /// (`layout::CELL_PADDING_PT`), none above or below.
    fn default() -> Self {
        Self { top_pt: 0.0, bottom_pt: 0.0, left_pt: 5.4, right_pt: 5.4 }
    }
}

/// A merged table cell: the cell at `row`, `col` spans `rows` rows and
/// `cols` columns (each at least 1, one of them more), as Word's
/// `w:gridSpan` and `w:vMerge` and ODF's `table:number-columns-spanned`
/// and `table:number-rows-spanned` merge them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellSpan {
    pub row: u32,
    pub col: u32,
    pub rows: u32,
    pub cols: u32,
}

impl CellSpan {
    /// Whether the span covers grid position `row`, `col`, its own
    /// included.
    pub fn contains(&self, row: u32, col: u32) -> bool {
        (self.row..self.row + self.rows).contains(&row) && (self.col..self.col + self.cols).contains(&col)
    }
}

/// A table cell's background colour, as six hex digits (`D9D9D9`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellFill {
    pub row: u32,
    pub col: u32,
    pub color: String,
}

/// The sides of a table cell drawn without a rule (`fo:border-right="none"`,
/// `w:tcBorders/w:right w:val="nil"`). A side its neighbour rules still
/// shows: each cell draws its own sides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenEdges {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub top: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bottom: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub left: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub right: bool,
}

impl OpenEdges {
    pub fn any(&self) -> bool {
        self.top || self.bottom || self.left || self.right
    }
}

/// A table cell's open sides (`OpenEdges`), at its grid position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellEdges {
    pub row: u32,
    pub col: u32,
    pub open: OpenEdges,
}

/// Where a table cell's content sits in a row taller than it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VAlign {
    Center,
    Bottom,
}

impl VAlign {
    /// The share of the cell's spare height above its content.
    pub fn share(self) -> f64 {
        match self {
            VAlign::Center => 0.5,
            VAlign::Bottom => 1.0,
        }
    }
}

/// A table cell set lower than the top (`VAlign`), at its grid position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellVAlign {
    pub row: u32,
    pub col: u32,
    pub align: VAlign,
}

/// A table row's height from the file: at least `pt` (Word's `atLeast`,
/// LibreOffice's minimum row height), or exactly `pt` (`exact`, a fixed
/// row height) with taller content clipped.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RowHeight {
    pub pt: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exact: bool,
}

/// A document's body font: a docx's docDefaults and Normal style, an ODT's
/// default paragraph style. `None` fields fall back to the application's
/// default (Liberation Serif 12pt, LibreOffice Writer's).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseFont {
    pub family: Option<String>,
    /// Size in half-points (24 = 12pt).
    pub size_hp: Option<u16>,
}

/// Page size and margins, in points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageGeometry {
    pub width_pt: f64,
    pub height_pt: f64,
    pub margin_top_pt: f64,
    pub margin_bottom_pt: f64,
    pub margin_left_pt: f64,
    pub margin_right_pt: f64,
    #[serde(default = "default_columns")]
    pub columns: u8,
    #[serde(default = "default_column_gap")]
    pub column_gap_pt: f64,
    /// The distance from the page's top edge to its header (`w:pgMar
    /// w:header`), and from its bottom edge to its footer; `None` takes
    /// the layout's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer_pt: Option<f64>,
    /// A rule around the page's text (ODF's page `fo:border`, Word's
    /// `w:pgBorders`), its headers and footers included.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<PageBorder>,
}

/// A page border: a rule `width_pt` wide in `color` (0xRRGGBB), set
/// `padding_pt` (top, right, bottom, left) outside the text it surrounds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PageBorder {
    pub width_pt: f64,
    pub color: u32,
    pub padding_pt: [f64; 4],
}

fn default_columns() -> u8 { 1 }
fn default_column_gap() -> f64 { 18.0 }

impl Default for PageGeometry {
    /// A4 portrait with 1-inch margins.
    fn default() -> Self {
        Self {
            width_pt: 595.3,
            height_pt: 841.9,
            margin_top_pt: 72.0,
            margin_bottom_pt: 72.0,
            margin_left_pt: 72.0,
            margin_right_pt: 72.0,
            columns: 1,
            column_gap_pt: 18.0,
            header_pt: None,
            footer_pt: None,
            border: None,
        }
    }
}

impl PageGeometry {
    /// Equality within half a point — format conversions round to twips
    /// (1/20 pt) or hundredths of a millimetre.
    pub fn approx_eq(&self, other: &PageGeometry) -> bool {
        let close = |a: f64, b: f64| (a - b).abs() < 0.5;
        close(self.width_pt, other.width_pt)
            && close(self.height_pt, other.height_pt)
            && close(self.margin_top_pt, other.margin_top_pt)
            && close(self.margin_bottom_pt, other.margin_bottom_pt)
            && close(self.margin_left_pt, other.margin_left_pt)
            && close(self.margin_right_pt, other.margin_right_pt)
            && self.columns == other.columns
            && close(self.column_gap_pt, other.column_gap_pt)
    }
}

impl Default for Document {
    fn default() -> Self { Self::new() }
}

impl Document {
    pub fn new() -> Self {
        Self {
            paragraphs: vec![Paragraph::default()],
            footnotes: vec![],
            header: None,
            footer: None,
            header_pictures: Vec::new(),
            footer_pictures: Vec::new(),
            header_alignment: Alignment::Left,
            footer_alignment: Alignment::Left,
            header_lines: Vec::new(),
            footer_lines: Vec::new(),
            space_adds: false,
            first_header: None,
            first_footer: None,
            page: None,
            base_font: BaseFont::default(),
            heading_styles: Vec::new(),
            comments: Vec::new(),
            table_columns: Default::default(),
            table_rows: Default::default(),
            table_spans: Default::default(),
            table_fills: Default::default(),
            table_padding: Default::default(),
            table_indent: Default::default(),
            table_unruled: Default::default(),
            table_rule_colors: Default::default(),
            table_rule_widths: Default::default(),
            table_whole_rows: Default::default(),
            table_relative: Default::default(),
            table_open_edges: Default::default(),
            table_valign: Default::default(),
            sections: Vec::new(),
            next_section_id: 0,
        }
    }

    pub fn from_plain_text(text: &str) -> Self {
        let paragraphs = text
            .split('\n')
            .map(|line| Paragraph {
                style: ParaStyle::default(),
                runs: if line.is_empty() { vec![] } else { vec![Run::plain(line)] },
            })
            .collect::<Vec<_>>();
        let mut d = Self { paragraphs, ..Self::new() };
        d.ensure_non_empty();
        d
    }

    pub fn to_plain_text(&self) -> String {
        self.paragraphs
            .iter()
            .map(|p| p.text())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Total character count; paragraph breaks count as one char each.
    pub fn char_len(&self) -> usize {
        let text: usize = self.paragraphs.iter().map(|p| p.char_len()).sum();
        text + self.paragraphs.len().saturating_sub(1)
    }

    pub(crate) fn ensure_non_empty(&mut self) {
        if self.paragraphs.is_empty() {
            self.paragraphs.push(Paragraph::default());
        }
    }

    /// Locate a global offset as (paragraph index, offset within paragraph).
    /// Offsets equal to a paragraph's length address its end (before the
    /// break); offset len+1 is the start of the next paragraph.
    fn locate(&self, offset: usize) -> (usize, usize) {
        let mut remaining = offset;
        for (i, p) in self.paragraphs.iter().enumerate() {
            let len = p.char_len();
            if remaining <= len {
                return (i, remaining);
            }
            remaining -= len + 1; // +1 for the paragraph break
        }
        // An empty document has no paragraph to land in. It is a state
        // the editing commands avoid (`ensure_non_empty`), but locating
        // an offset must not be the thing that panics when one slips
        // through — every offset lookup in the model routes through here.
        let Some(last) = self.paragraphs.len().checked_sub(1) else { return (0, 0) };
        (last, self.paragraphs[last].char_len())
    }

    /// Style of the character at `offset` (or, at paragraph end / document
    /// end, the style a character typed there would inherit).
    pub fn style_at(&self, offset: usize) -> RunStyle {
        let (pi, po) = self.locate(offset);
        let para = &self.paragraphs[pi];
        let mut pos = 0;
        for run in &para.runs {
            let len = run.char_len();
            if po < pos + len {
                return run.style.clone();
            }
            pos += len;
        }
        // Past the last character: inherit from the paragraph's last run so
        // typing at the end continues the current style.
        para.runs.last().map(|r| r.style.clone()).unwrap_or_default()
    }

    pub fn insert_text(&mut self, offset: usize, text: &str) {
        if text.is_empty() { return; }
        let inherited = self.style_at(offset);
        let (pi, po) = self.locate(offset);

        let tail = self.paragraphs[pi].split_at(po);
        let mut lines = text.split('\n');

        // First line goes at the end of the head paragraph.
        if let Some(first) = lines.next() {
            if !first.is_empty() {
                self.paragraphs[pi].runs.push(Run { text: first.to_string(), style: inherited.clone() });
                self.paragraphs[pi].normalize();
            }
        }
        // Each further line starts a new paragraph (same para style).
        let mut insert_at = pi;
        for line in lines {
            insert_at += 1;
            let runs = if line.is_empty() {
                vec![]
            } else {
                vec![Run { text: line.to_string(), style: inherited.clone() }]
            };
            self.paragraphs.insert(insert_at, Paragraph { style: self.paragraphs[pi].style.clone(), runs });
        }
        // Re-attach the tail to the last inserted/updated paragraph.
        let last = &mut self.paragraphs[insert_at];
        last.runs.extend(tail.runs);
        last.normalize();
    }

    pub fn delete_range(&mut self, start: usize, end: usize) {
        if end <= start { return; }
        let (spi, spo) = self.locate(start);
        let (epi, epo) = self.locate(end);

        if spi == epi {
            let tail = self.paragraphs[spi].split_at(epo);
            self.paragraphs[spi].split_at(spo); // discard middle
            self.paragraphs[spi].runs.extend(tail.runs);
            self.paragraphs[spi].normalize();
        } else {
            let tail = self.paragraphs[epi].split_at(epo);
            self.paragraphs[spi].split_at(spo); // discard rest of start para
            // A table cell is a paragraph, and a table is its cells: remove
            // one and the grid has a hole that reflow cannot close and no
            // renderer, writer or screen reader can interpret. A selection
            // dragged across a table therefore empties the cells it covers
            // and leaves the structure standing — the same thing a word
            // processor does, and the only option here that keeps the
            // document readable. Deleting the table itself is what the
            // row and column commands are for (#532's campaign).
            let mut kept: Vec<Paragraph> = Vec::new();
            for paragraph in self.paragraphs.drain(spi + 1..=epi) {
                if paragraph.style.table_cell.is_some() {
                    let mut emptied = paragraph;
                    emptied.runs.clear();
                    kept.push(emptied);
                }
            }
            // The start paragraph keeps whatever survived the split;
            // anything that followed it inside a table comes back
            // emptied, in order.
            let insert_at = spi + 1;
            self.paragraphs.splice(insert_at..insert_at, kept);
            self.paragraphs[spi].runs.extend(tail.runs);
            self.paragraphs[spi].normalize();
        }
        self.ensure_non_empty();
    }

    /// Apply a style patch to every character in [start, end).
    pub fn apply_run_style(&mut self, start: usize, end: usize, patch: &StylePatch) {
        self.restyle_range(start, end, |style| patch.apply_to(style));
    }

    /// Replace the style of every character in [start, end) outright. Used
    /// when typed text must adopt a style rather than merge into it (e.g.
    /// replacing a styled selection).
    pub fn set_run_style(&mut self, start: usize, end: usize, style: &RunStyle) {
        self.restyle_range(start, end, |target| *target = style.clone());
    }

    fn restyle_range(&mut self, start: usize, end: usize, edit: impl Fn(&mut RunStyle)) {
        if end <= start { return; }
        let (spi, spo) = self.locate(start);
        let (epi, epo) = self.locate(end);

        for pi in spi..=epi {
            let para_len = self.paragraphs[pi].char_len();
            let from = if pi == spi { spo } else { 0 };
            let to = if pi == epi { epo } else { para_len };
            if from >= to { continue; }

            let tail = self.paragraphs[pi].split_at(to);
            let mut mid = self.paragraphs[pi].split_at(from);
            for run in &mut mid.runs {
                edit(&mut run.style);
            }
            self.paragraphs[pi].runs.extend(mid.runs);
            self.paragraphs[pi].runs.extend(tail.runs);
            self.paragraphs[pi].normalize();
        }
    }

    /// Set or clear the heading level of one paragraph.
    pub fn set_heading(&mut self, para_idx: usize, level: Option<u8>) {
        if let Some(p) = self.paragraphs.get_mut(para_idx) {
            p.style.heading = level;
        }
    }

    pub fn set_paragraph_layout(&mut self, para_idx: usize, layout: ParagraphLayout) {
        if let Some(style) = self.paragraphs.get_mut(para_idx).map(|p| &mut p.style) {
            style.space_before_pt = layout.space_before_pt.max(0.0);
            style.space_after_pt = layout.space_after_pt.max(0.0);
            style.left_indent_pt = layout.left_indent_pt.max(0.0);
            style.right_indent_pt = layout.right_indent_pt.max(0.0);
            style.first_line_indent_pt = layout.first_line_indent_pt;
            style.tab_stops_pt = layout.tab_stops_pt;
        }
    }

    pub fn set_list_item(&mut self, para_idx: usize, kind: ListKind, level: u8, start: Option<u32>) {
        if let Some(style) = self.paragraphs.get_mut(para_idx).map(|p| &mut p.style) {
            style.list = kind;
            style.list_level = level;
            style.list_start = start.filter(|n| *n > 0);
        }
    }

    /// Index of the paragraph containing `offset`.
    pub fn paragraph_at(&self, offset: usize) -> usize {
        self.locate(offset).0
    }

    /// Global character offset of a paragraph's first character.
    pub fn paragraph_offset(&self, para_idx: usize) -> usize {
        self.paragraphs
            .iter()
            .take(para_idx.min(self.paragraphs.len()))
            .map(|p| p.char_len() + 1) // + the paragraph break
            .sum()
    }

    /// The table cell `offset` sits in, if it is inside a table. This is
    /// how a GUI command ("insert a row below") learns what the cursor is
    /// pointing at, instead of assuming a table id.
    pub fn table_cell_at(&self, offset: usize) -> Option<TableCell> {
        self.paragraphs.get(self.paragraph_at(offset)).and_then(|p| p.style.table_cell)
    }

    /// First and last paragraph indices (inclusive) holding `table`'s
    /// cells, or None when the document has no such table. Cells are kept
    /// contiguous by `reflow_table`, so this is a range, not a scatter.
    pub fn table_paragraph_range(&self, table: u32) -> Option<(usize, usize)> {
        let mut first = None;
        let mut last = None;
        for (index, paragraph) in self.paragraphs.iter().enumerate() {
            if paragraph.style.table_cell.is_some_and(|c| c.table == table) {
                first.get_or_insert(index);
                last = Some(index);
            }
        }
        Some((first?, last?))
    }

    /// The paragraph holding a given cell, if the table has it.
    pub fn cell_paragraph(&self, cell: TableCell) -> Option<usize> {
        self.paragraphs
            .iter()
            .position(|p| p.style.table_cell == Some(cell))
    }

    /// An id no existing table uses.
    pub fn next_table_id(&self) -> u32 {
        self.paragraphs.iter().filter_map(|p| p.style.table_cell.map(|c| c.table)).max().unwrap_or(0) + 1
    }

    /// Insert a `rows` × `cols` table whose cells start at paragraph
    /// `para_idx`, and return its id.
    ///
    /// Inserting where the cursor is — rather than appending at the end of
    /// the document, which is what an earlier version did — is what makes
    /// "Insert Table" put a table where the user is looking. The cells are
    /// laid down contiguously in row-major order because that adjacency
    /// *is* the table: `table_cell` tags a flat paragraph list, and every
    /// reader (render, DOCX writer, navigation) recovers the grid from it.
    pub fn insert_table_at(&mut self, para_idx: usize, rows: u32, cols: u32) -> u32 {
        let table = self.next_table_id();
        // A new table has no file widths, even under the id of a deleted one.
        self.table_columns.remove(&table);
        self.table_rows.remove(&table);
        self.table_spans.remove(&table);
        self.table_fills.remove(&table);
        self.table_padding.remove(&table);
        self.table_indent.remove(&table);
        self.table_unruled.remove(&table);
        self.table_rule_colors.remove(&table);
        self.table_rule_widths.remove(&table);
        self.table_whole_rows.remove(&table);
        self.table_relative.remove(&table);
        self.table_open_edges.remove(&table);
        self.table_valign.remove(&table);
        if rows == 0 || cols == 0 {
            return table;
        }
        let at = para_idx.min(self.paragraphs.len());
        let cells: Vec<Paragraph> = (0..rows)
            .flat_map(|row| (0..cols).map(move |col| (row, col)))
            .map(|(row, col)| Paragraph {
                style: ParaStyle { table_cell: Some(TableCell { table, row, col }), ..Default::default() },
                runs: Vec::new(),
            })
            .collect();
        self.paragraphs.splice(at..at, cells);
        table
    }

    /// Put a table's paragraphs back in contiguous row-major order at the
    /// position of its first cell. The row/column operations below renumber
    /// cells in place and push the new ones onto the end of the document;
    /// without this the table's paragraphs would be interleaved with the
    /// text that follows it, which no reader of the flat list can undo.
    fn reflow_table(&mut self, table: u32) {
        let first = self.paragraphs.iter().position(|p| p.style.table_cell.is_some_and(|c| c.table == table));
        let Some(first) = first else { return };
        let mut cells: Vec<Paragraph> = Vec::new();
        let mut rest: Vec<Paragraph> = Vec::new();
        for p in self.paragraphs.drain(..) {
            if p.style.table_cell.is_some_and(|c| c.table == table) {
                cells.push(p);
            } else {
                rest.push(p);
            }
        }
        cells.sort_by_key(|p| {
            let c = p.style.table_cell.expect("partitioned on table membership");
            (c.row, c.col)
        });
        rest.splice(first..first, cells);
        self.paragraphs = rest;
    }

    /// Move `table`'s merged cells for `count` rows or columns inserted at
    /// `at` (`count` < 0: deleted from `at`). `axis` picks a span's start
    /// and extent along the rows or the columns. A span the change falls
    /// inside grows or shrinks; one that no longer spans anything is gone.
    fn shift_spans(&mut self, table: u32, axis: fn(&mut CellSpan) -> (&mut u32, &mut u32), at: u32, count: i64) {
        let Some(spans) = self.table_spans.get_mut(&table) else { return };
        for span in spans.iter_mut() {
            let (start, len) = axis(span);
            let (s, e) = (i64::from(*start), i64::from(*start) + i64::from(*len));
            let (s, e) = if count >= 0 {
                let at = i64::from(at);
                match () {
                    _ if at <= s => (s + count, e + count),
                    _ if at < e => (s, e + count),
                    _ => (s, e),
                }
            } else {
                // Rows or columns `at..end` go; what of the span is left
                // closes up around them.
                let (from, to) = (i64::from(at), i64::from(at) - count);
                let gone = (e.min(to) - s.max(from)).max(0);
                let new_s = if s >= to { s + count } else { s.min(from) };
                (new_s, new_s + (e - s - gone))
            };
            *start = s.max(0) as u32;
            *len = (e - s).max(0) as u32;
        }
        spans.retain(|s| s.rows >= 1 && s.cols >= 1 && (s.rows > 1 || s.cols > 1));
        if spans.is_empty() {
            self.table_spans.remove(&table);
        }
    }

    /// The merged cell of `table` covering grid position `row`, `col`.
    pub fn span_at(&self, table: u32, row: u32, col: u32) -> Option<CellSpan> {
        self.table_spans.get(&table)?.iter().copied().find(|s| s.contains(row, col))
    }

    pub fn table_dimensions(&self, table: u32) -> Option<(u32, u32)> {
        let cells = self.paragraphs.iter().filter_map(|p| p.style.table_cell)
            .filter(|c| c.table == table).collect::<Vec<_>>();
        if cells.is_empty() { return None; }
        Some((cells.iter().map(|c| c.row).max()? + 1, cells.iter().map(|c| c.col).max()? + 1))
    }

    /// Insert rows in a tagged document table, carrying existing paragraphs
    /// with their cells and creating empty paragraphs for the new cells.
    pub fn insert_table_rows(&mut self, table: u32, at: u32, count: u32) -> bool {
        let Some((rows, cols)) = self.table_dimensions(table) else { return false; };
        if at > rows || count == 0 { return false; }
        for p in &mut self.paragraphs {
            if let Some(mut cell) = p.style.table_cell.filter(|c| c.table == table) {
                if cell.row >= at { cell.row += count; p.style.table_cell = Some(cell); }
            }
        }
        let new_cells = (0..count).flat_map(|row| (0..cols).map(move |col| Paragraph {
            style: ParaStyle { table_cell: Some(TableCell { table, row: at + row, col }), ..Default::default() },
            runs: Vec::new(),
        }));
        self.paragraphs.extend(new_cells);
        if let Some(heights) = self.table_rows.get_mut(&table) {
            let at = (at as usize).min(heights.len());
            heights.splice(at..at, std::iter::repeat_n(None, count as usize));
        }
        self.shift_spans(table, |s| (&mut s.row, &mut s.rows), at, count as i64);
        self.move_fills(table, |row, col| Some((if row >= at { row + count } else { row }, col)));
        self.reflow_table(table);
        true
    }

    pub fn insert_table_cols(&mut self, table: u32, at: u32, count: u32) -> bool {
        let Some((_rows, cols)) = self.table_dimensions(table) else { return false; };
        if at > cols || count == 0 { return false; }
        for p in &mut self.paragraphs {
            if let Some(mut cell) = p.style.table_cell.filter(|c| c.table == table) {
                if cell.col >= at { cell.col += count; p.style.table_cell = Some(cell); }
            }
        }
        let rows = self.table_dimensions(table).map(|(r, _)| r).unwrap_or(0);
        self.paragraphs.extend((0..rows).flat_map(|row| (0..count).map(move |col| Paragraph {
            style: ParaStyle { table_cell: Some(TableCell { table, row, col: at + col }), ..Default::default() },
            runs: Vec::new(),
        })));
        self.shift_spans(table, |s| (&mut s.col, &mut s.cols), at, count as i64);
        self.move_fills(table, |row, col| Some((row, if col >= at { col + count } else { col })));
        self.reflow_table(table);
        true
    }

    pub fn delete_table_rows(&mut self, table: u32, at: u32, count: u32) -> bool {
        let Some((rows, _)) = self.table_dimensions(table) else { return false; };
        if count == 0 || at >= rows || count > rows - at { return false; }
        let end = at + count;
        self.paragraphs.retain_mut(|p| {
            let Some(mut cell) = p.style.table_cell.filter(|c| c.table == table) else { return true; };
            if (at..end).contains(&cell.row) { return false; }
            if cell.row >= end { cell.row -= count; p.style.table_cell = Some(cell); }
            true
        });
        if let Some(heights) = self.table_rows.get_mut(&table) {
            let len = heights.len();
            heights.drain((at as usize).min(len)..(end as usize).min(len));
        }
        self.shift_spans(table, |s| (&mut s.row, &mut s.rows), at, -(count as i64));
        self.move_fills(table, |row, col| match row {
            r if (at..end).contains(&r) => None,
            r if r >= end => Some((r - count, col)),
            r => Some((r, col)),
        });
        // A document that was nothing but this table is now empty, and an
        // empty document has nowhere to put the caret.
        self.ensure_non_empty();
        true
    }

    pub fn delete_table_cols(&mut self, table: u32, at: u32, count: u32) -> bool {
        let Some((_, cols)) = self.table_dimensions(table) else { return false; };
        if count == 0 || at >= cols || count > cols - at { return false; }
        let end = at + count;
        self.paragraphs.retain_mut(|p| {
            let Some(mut cell) = p.style.table_cell.filter(|c| c.table == table) else { return true; };
            if (at..end).contains(&cell.col) { return false; }
            if cell.col >= end { cell.col -= count; p.style.table_cell = Some(cell); }
            true
        });
        self.shift_spans(table, |s| (&mut s.col, &mut s.cols), at, -(count as i64));
        self.move_fills(table, |row, col| match col {
            c if (at..end).contains(&c) => None,
            c if c >= end => Some((row, c - count)),
            c => Some((row, c)),
        });
        self.ensure_non_empty();
        true
    }

    /// Move a table's cell fills with their cells: `to` gives a cell's new
    /// (row, col), or `None` when the cell is deleted.
    fn move_fills(&mut self, table: u32, mut to: impl FnMut(u32, u32) -> Option<(u32, u32)>) {
        if let Some(fills) = self.table_fills.get_mut(&table) {
            fills.retain_mut(|f| match to(f.row, f.col) {
                Some((row, col)) => {
                    (f.row, f.col) = (row, col);
                    true
                }
                None => false,
            });
        }
        // So does where its content sits.
        if let Some(cells) = self.table_valign.get_mut(&table) {
            cells.retain_mut(|c| match to(c.row, c.col) {
                Some((row, col)) => {
                    (c.row, c.col) = (row, col);
                    true
                }
                None => false,
            });
        }
        // A cell's open sides go with it too.
        if let Some(edges) = self.table_open_edges.get_mut(&table) {
            edges.retain_mut(|e| match to(e.row, e.col) {
                Some((row, col)) => {
                    (e.row, e.col) = (row, col);
                    true
                }
                None => false,
            });
        }
    }

    /// Return the next cell in row-major order, useful for Tab/Shift-Tab
    /// navigation without exposing the flat paragraph representation.
    pub fn next_table_cell(&self, table: u32, row: u32, col: u32, backwards: bool) -> Option<TableCell> {
        let (rows, cols) = self.table_dimensions(table)?;
        let index = (row * cols + col) as i64 + if backwards { -1 } else { 1 };
        if index < 0 || index >= (rows * cols) as i64 { return None; }
        Some(TableCell { table, row: index as u32 / cols, col: index as u32 % cols })
    }

    /// Find all occurrences of `query` in this document's plain text.
    ///
    /// Returns a list of `(start, end)` **character** offset pairs (global,
    /// matching GtkTextBuffer's offset model and this model's own
    /// `char_len`/`delete_range`/`insert_text`). The offsets can be fed
    /// directly to `delete_range` / `insert_text` or to GTK's
    /// `iter_at_offset`.
    ///
    /// `suite_common_core::search` reports byte offsets, which differ from
    /// character offsets the moment the document contains non-ASCII text, so
    /// they are converted here rather than at each call site.
    ///
    /// Paragraph breaks are a single character (offset += 1 per break), so a
    /// match that spans a paragraph break is possible with regex — paragraph
    /// breaks are `'\n'`, which literal patterns can target too.
    pub fn find_in_document(
        &self,
        query: &suite_common_core::search::SearchQuery,
    ) -> Vec<(usize, usize)> {
        let text = self.to_plain_text();
        Self::char_ranges(&text, query)
    }

    /// Character-offset ranges of every match of `query` in `text`.
    fn char_ranges(
        text: &str,
        query: &suite_common_core::search::SearchQuery,
    ) -> Vec<(usize, usize)> {
        use suite_common_core::search::byte_to_char_offset;
        suite_common_core::search::search(text, query)
            .into_iter()
            .map(|m| (byte_to_char_offset(text, m.start), byte_to_char_offset(text, m.end)))
            .collect()
    }

    /// Replace the first occurrence of `query` with `replacement`.
    ///
    /// Returns `true` if a replacement was made, `false` if there was no match.
    /// The document is mutated in place; run styles at the replaced span are
    /// lost (the replacement text is plain/unstyled). For a styled replacement,
    /// callers should apply `apply_run_style` after this call.
    pub fn replace_first(
        &mut self,
        query: &suite_common_core::search::SearchQuery,
        replacement: &str,
    ) -> bool {
        let text = self.to_plain_text();
        match Self::char_ranges(&text, query).into_iter().next() {
            Some((start, end)) => {
                self.delete_range(start, end);
                self.insert_text(start, replacement);
                true
            }
            None => false,
        }
    }

    /// Replace all non-overlapping occurrences of `query` with `replacement`.
    ///
    /// Returns the number of replacements made. Replacements are applied
    /// right-to-left to keep earlier offsets valid through the whole pass.
    pub fn replace_all(
        &mut self,
        query: &suite_common_core::search::SearchQuery,
        replacement: &str,
    ) -> usize {
        let text = self.to_plain_text();
        let matches = Self::char_ranges(&text, query);
        let count = matches.len();
        // Iterate right-to-left so each earlier offset stays valid.
        for (start, end) in matches.into_iter().rev() {
            self.delete_range(start, end);
            self.insert_text(start, replacement);
        }
        count
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParagraphLayout {
    pub space_before_pt: f64,
    pub space_after_pt: f64,
    pub left_indent_pt: f64,
    pub right_indent_pt: f64,
    pub first_line_indent_pt: f64,
    pub tab_stops_pt: Vec<TabStop>,
}

// ── Cursor style readout (status bar) ─────────────────────────────────

/// Human-readable readout of the styles active at the cursor, from the
/// text-tag names present there — "Heading 1 · Bold · Italic". Paragraph
/// style first, inline styles after, presentation-only tags ignored.
/// Empty input (or only ignorable tags) reads "Normal".
pub fn style_readout(tag_names: &[&str]) -> String {
    const PARA: &[(&str, &str)] = &[
        ("h-title", "Title"),
        ("h-subtitle", "Subtitle"),
        ("h1", "Heading 1"),
        ("h2", "Heading 2"),
        ("h3", "Heading 3"),
        ("h4", "Heading 4"),
        ("h5", "Heading 5"),
        ("h6", "Heading 6"),
        ("code", "Code"),
        ("blockquote", "Quote"),
    ];
    const INLINE: &[(&str, &str)] = &[
        ("bold", "Bold"),
        ("italic", "Italic"),
        ("underline", "Underline"),
        ("strikethrough", "Strikethrough"),
        ("highlight", "Highlight"),
        ("bullet", "List"),
        ("numbered", "Numbered List"),
    ];
    let mut parts: Vec<&str> = Vec::new();
    for (tag, label) in PARA {
        if tag_names.contains(tag) {
            parts.push(label);
            break;
        }
    }
    for (tag, label) in INLINE {
        if tag_names.contains(tag) {
            parts.push(label);
        }
    }
    if parts.is_empty() {
        "Normal".to_string()
    } else {
        parts.join(" · ")
    }
}

#[cfg(test)]
mod readout_tests {
    use super::style_readout;

    #[test]
    fn empty_is_normal() {
        assert_eq!(style_readout(&[]), "Normal");
    }

    #[test]
    fn ignores_presentation_tags() {
        assert_eq!(style_readout(&["line-spacing-1.5", "align-left"]), "Normal");
    }

    #[test]
    fn inline_styles_join() {
        assert_eq!(style_readout(&["italic", "bold"]), "Bold · Italic");
    }

    #[test]
    fn paragraph_style_leads() {
        assert_eq!(style_readout(&["bold", "h2"]), "Heading 2 · Bold");
    }

    #[test]
    fn single_paragraph_style_only() {
        assert_eq!(style_readout(&["h1", "h2"]), "Heading 1");
    }
}

#[cfg(test)]
mod structured_editing_tests {
    use super::*;

    #[test]
    fn nested_lists_and_restart_are_explicit_model_state() {
        let mut doc = Document::from_plain_text("parent\nchild\nrestart");
        doc.set_list_item(0, ListKind::Numbered, 0, Some(4));
        doc.set_list_item(1, ListKind::Bullet, 1, None);
        doc.set_list_item(2, ListKind::Numbered, 0, Some(1));
        assert_eq!(doc.paragraphs[0].style.list_start, Some(4));
        assert_eq!(doc.paragraphs[1].style.list_level, 1);
        assert_eq!(doc.paragraphs[2].style.list_level, 0);
    }

    #[test]
    fn merged_cells_move_and_resize_with_rows_and_columns() {
        let mut doc = Document::from_plain_text("after");
        let t = doc.insert_table_at(0, 4, 4);
        let span = |row, col, rows, cols| CellSpan { row, col, rows, cols };
        doc.table_spans.insert(t, vec![span(0, 0, 1, 4), span(1, 1, 2, 2)]);
        // A row inside the tall cell makes it taller; one above moves it.
        assert!(doc.insert_table_rows(t, 2, 1));
        assert_eq!(doc.table_spans[&t], [span(0, 0, 1, 4), span(1, 1, 3, 2)]);
        assert!(doc.insert_table_rows(t, 0, 1));
        assert_eq!(doc.table_spans[&t], [span(1, 0, 1, 4), span(2, 1, 3, 2)]);
        // A column inside both widens them.
        assert!(doc.insert_table_cols(t, 2, 1));
        assert_eq!(doc.table_spans[&t], [span(1, 0, 1, 5), span(2, 1, 3, 3)]);
        // Deleting the tall cell's first row leaves the rest of it, from
        // where the row was.
        assert!(doc.delete_table_rows(t, 2, 1));
        assert_eq!(doc.table_spans[&t], [span(1, 0, 1, 5), span(2, 1, 2, 3)]);
        // Columns down to one: the heading spans nothing and is gone.
        assert!(doc.delete_table_cols(t, 1, 4));
        assert!(!doc.table_spans.contains_key(&t), "{:?}", doc.table_spans);
        assert_eq!(doc.span_at(t, 2, 0), None);
    }

    #[test]
    fn table_rows_and_columns_are_editable_without_flattening_cells() {
        let mut doc = Document::from_plain_text("");
        doc.paragraphs[0].style.table_cell = Some(TableCell { table: 7, row: 0, col: 0 });
        doc.paragraphs[0].runs = vec![Run::plain("A")];
        doc.paragraphs.push(Paragraph {
            style: ParaStyle { table_cell: Some(TableCell { table: 7, row: 0, col: 1 }), ..Default::default() },
            runs: vec![Run::plain("B")],
        });
        assert_eq!(doc.table_dimensions(7), Some((1, 2)));
        assert!(doc.insert_table_rows(7, 1, 1));
        assert!(doc.insert_table_cols(7, 1, 1));
        assert_eq!(doc.table_dimensions(7), Some((2, 3)));
        assert!(doc.paragraphs.iter().any(|p| p.style.table_cell == Some(TableCell { table: 7, row: 1, col: 1 })));
        assert_eq!(doc.next_table_cell(7, 0, 0, false), Some(TableCell { table: 7, row: 0, col: 1 }));
        assert!(doc.delete_table_rows(7, 1, 1));
        assert!(doc.delete_table_cols(7, 1, 1));
        assert_eq!(doc.table_dimensions(7), Some((1, 2)));
    }

    #[test]
    fn paragraph_layout_clamps_negative_spacing_and_keeps_tabs() {
        let mut doc = Document::from_plain_text("layout");
        doc.set_paragraph_layout(0, ParagraphLayout {
            space_before_pt: -2.0, space_after_pt: 8.0, left_indent_pt: 24.0,
            right_indent_pt: 12.0, first_line_indent_pt: -18.0, tab_stops_pt: vec![TabStop::left(72.0), TabStop::left(144.0)],
        });
        let style = &doc.paragraphs[0].style;
        assert_eq!(style.space_before_pt, 0.0);
        assert_eq!(style.left_indent_pt, 24.0);
        assert_eq!(style.first_line_indent_pt, -18.0);
        assert_eq!(style.tab_stops_pt, vec![TabStop::left(72.0), TabStop::left(144.0)]);
    }

    #[test]
    fn legacy_bare_tab_positions_read_as_plain_left_stops() {
        let style: ParaStyle = serde_json::from_str(
            r#"{"alignment":"Left","list":"None","line_spacing":1.0,"tab_stops_pt":[72.0,144.0],"block_quote":false,"page_break_before":false}"#,
        )
        .expect("older snapshots stored bare positions");
        assert_eq!(style.tab_stops_pt, vec![TabStop::left(72.0), TabStop::left(144.0)]);
        assert!(!style.keep_lines);
        assert_eq!(style.widow_control, None);
        assert_eq!(style.line_height, None);
        assert_eq!(style.shading, None);
        assert!(style.borders.is_empty());
    }

    #[test]
    fn new_typography_fields_round_trip_through_json() {
        let mut style = ParaStyle::default();
        style.keep_lines = true;
        style.widow_control = Some(false);
        style.line_height = Some(LineHeight::Minimum(18.0));
        style.shading = Some("D9D9D9".to_string());
        style.borders = ParaBorders {
            bottom: Some(ParaBorder { style: BorderStyle::Single, color: "000000".to_string(), width_pt: 0.75, space_pt: 0.0 }),
            ..ParaBorders::default()
        };
        style.tab_stops_pt = vec![TabStop { pos_pt: 72.0, align: TabAlign::Right, leader: TabLeader::Dot }];
        let json = serde_json::to_string(&style).expect("style serializes");
        let back: ParaStyle = serde_json::from_str(&json).expect("style deserializes");
        assert_eq!(back, style);
    }
}

#[cfg(test)]
mod find_replace_tests {
    use super::*;
    use suite_common_core::search::SearchQuery;

    // ── find_in_document ─────────────────────────────────────────────────

    #[test]
    fn find_returns_correct_offsets_in_single_paragraph() {
        let doc = Document::from_plain_text("the cat sat on the cat mat");
        let q = SearchQuery::new("cat");
        let matches = doc.find_in_document(&q);
        assert_eq!(matches.len(), 2, "expected 2 occurrences: {matches:?}");
        assert_eq!(matches[0], (4, 7));
        assert_eq!(matches[1], (19, 22));
    }

    #[test]
    fn find_is_case_insensitive_by_default() {
        let doc = Document::from_plain_text("Hello HELLO hello");
        let q = SearchQuery::new("hello");
        assert_eq!(doc.find_in_document(&q).len(), 3);
    }

    #[test]
    fn find_case_sensitive_skips_wrong_case() {
        let doc = Document::from_plain_text("Hello HELLO hello");
        let q = SearchQuery::new("hello").case_sensitive(true);
        let matches = doc.find_in_document(&q);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, 12); // third occurrence
    }

    #[test]
    fn find_across_paragraphs_uses_global_offsets() {
        // "one\ntwo" — paragraph break is offset 3, 't' at offset 4.
        let doc = Document::from_plain_text("one\ntwo");
        let q = SearchQuery::new("two");
        let matches = doc.find_in_document(&q);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0], (4, 7));
    }

    #[test]
    fn find_empty_query_returns_no_matches() {
        let doc = Document::from_plain_text("some text");
        let q = SearchQuery::new("");
        assert!(doc.find_in_document(&q).is_empty());
    }

    // ── replace_first ────────────────────────────────────────────────────

    #[test]
    fn replace_first_replaces_only_first_occurrence() {
        let mut doc = Document::from_plain_text("cat and cat");
        let q = SearchQuery::new("cat");
        assert!(doc.replace_first(&q, "dog"));
        assert_eq!(doc.to_plain_text(), "dog and cat");
    }

    #[test]
    fn replace_first_returns_false_on_no_match() {
        let mut doc = Document::from_plain_text("hello");
        let q = SearchQuery::new("xyz");
        assert!(!doc.replace_first(&q, "yyy"));
        assert_eq!(doc.to_plain_text(), "hello");
    }

    #[test]
    fn replace_first_with_longer_text_adjusts_document() {
        let mut doc = Document::from_plain_text("hi");
        let q = SearchQuery::new("hi");
        assert!(doc.replace_first(&q, "hello world"));
        assert_eq!(doc.to_plain_text(), "hello world");
    }

    // ── replace_all ──────────────────────────────────────────────────────

    #[test]
    fn replace_all_returns_count_and_replaces_all() {
        let mut doc = Document::from_plain_text("foo foo foo");
        let q = SearchQuery::new("foo");
        let n = doc.replace_all(&q, "bar");
        assert_eq!(n, 3);
        assert_eq!(doc.to_plain_text(), "bar bar bar");
    }

    #[test]
    fn replace_all_returns_zero_on_no_match() {
        let mut doc = Document::from_plain_text("hello world");
        let q = SearchQuery::new("xyz");
        assert_eq!(doc.replace_all(&q, "yyy"), 0);
        assert_eq!(doc.to_plain_text(), "hello world");
    }

    #[test]
    fn replace_all_case_insensitive_replaces_all_variants() {
        let mut doc = Document::from_plain_text("Cat CAT cat");
        let q = SearchQuery::new("cat"); // case-insensitive by default
        let n = doc.replace_all(&q, "dog");
        assert_eq!(n, 3);
        assert_eq!(doc.to_plain_text(), "dog dog dog");
    }

    #[test]
    fn replace_all_across_paragraphs() {
        // "ab\nab" — two paragraphs, each starting with "ab"
        let mut doc = Document::from_plain_text("ab\nab");
        let q = SearchQuery::new("ab");
        let n = doc.replace_all(&q, "XY");
        assert_eq!(n, 2);
        assert_eq!(doc.to_plain_text(), "XY\nXY");
    }

    // ── non-ASCII offsets ────────────────────────────────────────────────
    // `search` reports byte offsets while the document model addresses text in
    // characters; the two diverge as soon as a document contains non-ASCII
    // text. These tests pin the conversion.

    #[test]
    fn find_reports_character_offsets_not_byte_offsets() {
        // "naïve " is 7 bytes but 6 characters.
        let doc = Document::from_plain_text("naïve cat");
        let q = SearchQuery::new("cat");
        assert_eq!(doc.find_in_document(&q), [(6, 9)]);
    }

    #[test]
    fn replace_first_after_non_ascii_text_does_not_corrupt() {
        let mut doc = Document::from_plain_text("naïve cat");
        let q = SearchQuery::new("cat");
        assert!(doc.replace_first(&q, "dog"));
        assert_eq!(doc.to_plain_text(), "naïve dog");
    }

    #[test]
    fn replace_all_with_non_ascii_matches() {
        let mut doc = Document::from_plain_text("café au café");
        let q = SearchQuery::new("café");
        assert_eq!(doc.replace_all(&q, "thé"), 2);
        assert_eq!(doc.to_plain_text(), "thé au thé");
    }

    #[test]
    fn replace_all_across_paragraphs_with_non_ascii() {
        let mut doc = Document::from_plain_text("é ab
é ab");
        let q = SearchQuery::new("ab");
        assert_eq!(doc.replace_all(&q, "XY"), 2);
        assert_eq!(doc.to_plain_text(), "é XY
é XY");
    }

    #[test]
    fn replace_preserves_styles_outside_the_replaced_span() {
        let mut doc = Document::from_plain_text("bold cat tail");
        doc.apply_run_style(0, 4, &StylePatch { bold: Some(true), ..Default::default() });
        let q = SearchQuery::new("cat");
        assert!(doc.replace_first(&q, "dog"));
        assert_eq!(doc.to_plain_text(), "bold dog tail");
        assert!(doc.style_at(0).bold, "leading bold run must survive the replacement");
        assert!(!doc.style_at(5).bold, "replacement text is unstyled");
    }
}
