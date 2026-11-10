// SPDX-License-Identifier: GPL-3.0-or-later
//! Application-neutral rich text: the Tier 1 contract of ADR 0012.
//!
//! Runs of styled text in paragraphs, as every app of the suite holds them
//! and as the suite clipboard (`application/x-tunaos-suite+json`) carries
//! them. Only formatting every app can show belongs here; what one app adds
//! (Letters' footnotes, chips, tracked changes, page layout) stays in that
//! app's core, which converts at its boundary (Tier 3).
//!
//! The JSON shape is the wire contract. Field names match the ones Letters
//! has always written, so a payload Letters writes reads here with its extra
//! fields ignored, and one written here reads in Letters with its own fields
//! at their defaults. Every field reads with a default, so a payload from a
//! newer or older app that leaves one out still reads.

use serde::{Deserialize, Serialize};

/// Paragraph alignment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

/// Whether a paragraph is a list item, and of which kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ListKind {
    #[default]
    None,
    Bullet,
    Numbered,
}

/// Superscript or subscript.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VertAlign {
    Superscript,
    Subscript,
}

/// Character formatting every app can show. `Default` is unstyled text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RunStyle {
    pub bold: bool,
    /// Drawn regular where the paragraph's look is bold (a heading).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub regular: bool,
    pub italic: bool,
    /// Drawn upright where the paragraph's look is italic.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub upright: bool,
    /// Drawn in capitals whatever case the text is stored in.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub caps: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub highlight: bool,
    /// Inline code (monospace).
    pub code: bool,
    /// Hyperlink target.
    pub link: Option<String>,
    /// Font family name; None is the document's default.
    pub font_family: Option<String>,
    /// Font size in half-points (24 is 12pt); None is the default.
    pub font_size_hp: Option<u16>,
    /// Text color as RRGGBB hex, no '#'; None is the default.
    pub color: Option<String>,
    pub vert_align: Option<VertAlign>,
}

/// Paragraph formatting every app can show.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParaStyle {
    /// Heading level 1..=6, or None for body text.
    pub heading: Option<u8>,
    pub alignment: Alignment,
    pub list: ListKind,
    /// Nesting depth of a list item; zero is the top level.
    pub list_level: u8,
    /// Line height as a multiple of single spacing.
    pub line_spacing: f32,
    pub space_before_pt: f64,
    pub space_after_pt: f64,
    /// A named paragraph style ("Title", "Subtitle").
    pub named_style: Option<String>,
}

impl Default for ParaStyle {
    fn default() -> Self {
        Self {
            heading: None,
            alignment: Alignment::Left,
            list: ListKind::None,
            list_level: 0,
            line_spacing: 1.0,
            space_before_pt: 0.0,
            space_after_pt: 0.0,
            named_style: None,
        }
    }
}

/// Text in one style.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
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

/// A paragraph of runs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Paragraph {
    pub style: ParaStyle,
    pub runs: Vec<Run>,
}

impl Paragraph {
    /// A paragraph of one unstyled run, or none when `text` is empty.
    pub fn plain(text: impl Into<String>) -> Self {
        let text = text.into();
        let runs = if text.is_empty() { Vec::new() } else { vec![Run::plain(text)] };
        Self { style: ParaStyle::default(), runs }
    }

    /// The paragraph's text, without formatting.
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

/// The paragraphs' text, one line each: what a plain-text paste gets.
pub fn plain_text(paragraphs: &[Paragraph]) -> String {
    paragraphs.iter().map(Paragraph::text).collect::<Vec<_>>().join("\n")
}

/// The suite clipboard's MIME type: a [`Fragment`] as JSON.
pub const MIME: &str = "application/x-tunaos-suite+json";

/// A paragraph a [`Fragment`] can carry: the neutral [`Paragraph`], or an
/// app's own paragraph, which says what it is in neutral terms for the
/// plain-text and HTML forms other apps get.
pub trait ToNeutral {
    fn to_neutral(&self) -> Paragraph;
}

impl ToNeutral for Paragraph {
    fn to_neutral(&self) -> Paragraph {
        self.clone()
    }
}

/// A copied piece of content: styled text or a cell grid.
///
/// Its JSON is the suite clipboard's wire contract. An app may carry its own
/// paragraph type `P` (Letters carries its footnotes and chips Letters to
/// Letters); since every app's paragraph reads from the same JSON with what
/// it does not know left at its defaults, a fragment one app writes reads in
/// every other as `Fragment<its own P>`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Fragment<P = Paragraph> {
    /// Styled text.
    Text(Vec<P>),
    /// A cell grid with everything a spreadsheet knows about each cell.
    Grid(Vec<Vec<GridCell>>),
}

/// One copied spreadsheet cell.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GridCell {
    /// Displayed value.
    pub value: String,
    /// Formula without its leading '=' when the cell has one.
    pub formula: Option<String>,
    /// Number-format code (XLSX style), if not the default.
    pub num_format: Option<String>,
}

impl<P: Serialize + serde::de::DeserializeOwned + ToNeutral> Fragment<P> {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("fragment serialization is infallible")
    }

    pub fn from_json(s: &str) -> Option<Self> {
        serde_json::from_str(s).ok()
    }

    /// Plain-text form: paragraphs joined by newlines; grids as TSV.
    pub fn to_plain(&self) -> String {
        match self {
            Fragment::Text(paras) => paras.iter().map(|p| p.to_neutral().text()).collect::<Vec<_>>().join("\n"),
            Fragment::Grid(rows) => rows
                .iter()
                .map(|r| r.iter().map(|c| c.value.as_str()).collect::<Vec<_>>().join("\t"))
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// HTML form for interchange with apps outside the suite.
    pub fn to_html(&self) -> String {
        fn esc(s: &str) -> String {
            s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
        }
        fn run_html(r: &Run) -> String {
            let mut s = esc(&r.text);
            if r.style.code { s = format!("<code>{s}</code>"); }
            if r.style.bold { s = format!("<b>{s}</b>"); }
            if r.style.italic { s = format!("<i>{s}</i>"); }
            if r.style.underline { s = format!("<u>{s}</u>"); }
            if r.style.strikethrough { s = format!("<s>{s}</s>"); }
            if let Some(url) = &r.style.link {
                s = format!("<a href=\"{}\">{s}</a>", esc(url));
            }
            s
        }
        match self {
            Fragment::Text(paras) => paras
                .iter()
                .map(|p| {
                    let p = p.to_neutral();
                    let inner: String = p.runs.iter().map(run_html).collect();
                    match p.style.heading {
                        Some(l) => format!("<h{l}>{inner}</h{l}>"),
                        None => format!("<p>{inner}</p>"),
                    }
                })
                .collect(),
            Fragment::Grid(rows) => {
                let body: String = rows
                    .iter()
                    .map(|r| {
                        let cells: String = r.iter().map(|c| format!("<td>{}</td>", esc(&c.value))).collect();
                        format!("<tr>{cells}</tr>")
                    })
                    .collect();
                format!("<table>{body}</table>")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_unstyled_left_aligned_single_spaced() {
        let p = ParaStyle::default();
        assert_eq!((p.alignment, p.list, p.line_spacing, p.heading), (Alignment::Left, ListKind::None, 1.0, None));
        assert_eq!(RunStyle::default(), serde_json::from_str::<RunStyle>("{}").unwrap());
        assert_eq!(ParaStyle::default(), serde_json::from_str::<ParaStyle>("{}").unwrap());
    }

    #[test]
    fn the_json_shape_is_the_one_letters_has_always_written() {
        let run = Run {
            text: "Hi".into(),
            style: RunStyle { bold: true, link: Some("https://example.org".into()), vert_align: Some(VertAlign::Superscript), ..RunStyle::default() },
        };
        let json = serde_json::to_value(&run).unwrap();
        assert_eq!(json["text"], "Hi");
        assert_eq!(json["style"]["bold"], true);
        assert_eq!(json["style"]["italic"], false, "plain flags are written, as older readers need them");
        assert_eq!(json["style"]["vert_align"], "Superscript");
        assert!(json["style"].get("regular").is_none(), "an unset override is left out");
        assert_eq!(serde_json::from_value::<Run>(json).unwrap(), run);
    }

    #[test]
    fn unknown_fields_from_another_app_are_ignored() {
        let json = r#"{"style":{"alignment":"Center","list":"Bullet","toc":2,"table_cell":null},
                      "runs":[{"text":"x","style":{"italic":true,"chip":{"Date":"2026-11-10"},"comments":[3]}}]}"#;
        let p: Paragraph = serde_json::from_str(json).unwrap();
        assert_eq!(p.style.alignment, Alignment::Center);
        assert_eq!(p.style.list, ListKind::Bullet);
        assert!(p.runs[0].style.italic);
        assert_eq!(p.text(), "x");
    }

    #[test]
    fn a_grid_fragment_round_trips_and_has_tsv_and_html_forms() {
        let frag: Fragment = Fragment::Grid(vec![vec![
            GridCell { value: "3".into(), formula: Some("1+2".into()), num_format: None },
            GridCell { value: "<x>".into(), formula: None, num_format: Some("0.00".into()) },
        ]]);
        let json = frag.to_json();
        assert!(json.starts_with(r#"{"Grid":"#), "the wire shape is unchanged: {json}");
        assert_eq!(Fragment::from_json(&json), Some(frag.clone()));
        assert_eq!(frag.to_plain(), "3\t<x>");
        assert_eq!(frag.to_html(), "<table><tr><td>3</td><td>&lt;x&gt;</td></tr></table>");
    }

    #[test]
    fn a_text_fragment_has_plain_and_html_forms() {
        let mut heading = Paragraph::plain("Title");
        heading.style.heading = Some(1);
        let body = Paragraph {
            style: ParaStyle::default(),
            runs: vec![Run { text: "bold".into(), style: RunStyle { bold: true, ..RunStyle::default() } }, Run::plain(" & plain")],
        };
        let frag: Fragment = Fragment::Text(vec![heading, body]);
        assert_eq!(frag.to_plain(), "Title\nbold & plain");
        assert_eq!(frag.to_html(), "<h1>Title</h1><p><b>bold</b> &amp; plain</p>");
        assert!(Fragment::<Paragraph>::from_json("not json").is_none());
    }

    #[test]
    fn plain_text_is_one_line_per_paragraph() {
        let ps = [Paragraph::plain("one"), Paragraph::plain(""), Paragraph::plain("three")];
        assert_eq!(plain_text(&ps), "one\n\nthree");
        assert!(Paragraph::plain("").runs.is_empty());
    }
}
