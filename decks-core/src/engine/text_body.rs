//! text_body.rs — how a text box lays out its paragraphs.
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! A `SlideObject::TextBox` holds its text as runs, with paragraphs split on
//! `'\n'`. What each paragraph looks like — alignment, list level, bullet,
//! indents, spacing — and where the text sits in its box (vertical anchor,
//! insets) is a [`TextBody`]. Readers resolve it from the file, including
//! everything a pptx placeholder inherits from its layout and master (see
//! `text_xml.rs`); the canvas draws it; the writers write it back.
//!
//! All lengths are model units: the 960-wide slide, 96 per inch, so one
//! point is 4/3 of a unit.

use letters_core::model::Run;

/// Horizontal alignment of a paragraph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ParaAlign {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

impl ParaAlign {
    /// From a DrawingML `algn` value.
    pub fn from_drawingml(v: &str) -> Option<Self> {
        Some(match v {
            "l" => ParaAlign::Left,
            "ctr" => ParaAlign::Center,
            "r" => ParaAlign::Right,
            "just" | "dist" | "justLow" | "thaiDist" => ParaAlign::Justify,
            _ => return None,
        })
    }

    /// As a DrawingML `algn` value.
    pub fn to_drawingml(self) -> &'static str {
        match self {
            ParaAlign::Left => "l",
            ParaAlign::Center => "ctr",
            ParaAlign::Right => "r",
            ParaAlign::Justify => "just",
        }
    }

    /// As an ODF `fo:text-align` value.
    pub fn to_odf(self) -> &'static str {
        match self {
            ParaAlign::Left => "start",
            ParaAlign::Center => "center",
            ParaAlign::Right => "end",
            ParaAlign::Justify => "justify",
        }
    }

    /// From an ODF `fo:text-align` value.
    pub fn from_odf(v: &str) -> Option<Self> {
        Some(match v {
            "start" | "left" => ParaAlign::Left,
            "center" => ParaAlign::Center,
            "end" | "right" => ParaAlign::Right,
            "justify" => ParaAlign::Justify,
            _ => return None,
        })
    }
}

/// The marker in front of a paragraph.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Bullet {
    #[default]
    None,
    /// A literal character (`a:buChar`), e.g. `•` or `–`.
    Char(String),
    /// An automatic number (`a:buAutoNum`): the DrawingML scheme name
    /// (`arabicPeriod`, `alphaLcParenR`, …) and the first number.
    AutoNum { scheme: String, start: u32 },
}

/// Space above or below a paragraph.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Spacing {
    /// Model units (already scaled onto the model's slide).
    Units(f64),
    /// A fraction of the paragraph's line height (`a:spcPct` / 100000).
    Lines(f64),
}

impl Default for Spacing {
    fn default() -> Self {
        Spacing::Units(0.0)
    }
}

impl Spacing {
    /// In model units, for a paragraph whose first line is `line` units
    /// tall.
    pub fn resolve(self, line: f64) -> f64 {
        match self {
            Spacing::Units(u) => u,
            Spacing::Lines(f) => f * line,
        }
    }
}

/// One paragraph's layout.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ParaStyle {
    pub align: ParaAlign,
    /// List level, 0-based (`a:pPr lvl`).
    pub level: u8,
    pub bullet: Bullet,
    /// Left edge of the text, from the box's inner left edge.
    pub margin_left: f64,
    /// First-line offset from `margin_left`; negative is a hanging indent,
    /// where a bullet sits.
    pub indent: f64,
    pub space_before: Spacing,
    pub space_after: Spacing,
    /// How the marker is drawn, where it differs from the text.
    pub marker: MarkerStyle,
}

/// A bullet's own font, size and colour (`a:buFont`, `a:buSzPct` or
/// `a:buSzPts`, `a:buClr`). Each `None` follows the paragraph's first run,
/// which is also what the `*Tx` elements say. A bullet character is drawn
/// in its own font: the default template's "•" is Arial's, a good deal
/// smaller than the body font's.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MarkerStyle {
    pub font: Option<String>,
    pub size: Option<MarkerSize>,
    /// `RRGGBB`, lower case.
    pub color: Option<String>,
}

/// A marker's size.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MarkerSize {
    /// A fraction of the text's size (`buSzPct` / 100000).
    Relative(f64),
    /// Points on the model's slide (`buSzPts` / 100, scaled).
    Points(f64),
}

impl MarkerSize {
    /// The marker's size in points for text `text_pt` points tall.
    pub fn points(self, text_pt: f64) -> f64 {
        match self {
            MarkerSize::Relative(f) => text_pt * f,
            MarkerSize::Points(p) => p,
        }
    }
}

/// Where the block of text sits vertically in its box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Anchor {
    #[default]
    Top,
    Middle,
    Bottom,
}

impl Anchor {
    pub fn from_drawingml(v: &str) -> Option<Self> {
        Some(match v {
            "t" => Anchor::Top,
            "ctr" => Anchor::Middle,
            "b" => Anchor::Bottom,
            _ => return None,
        })
    }
    pub fn to_drawingml(self) -> &'static str {
        match self {
            Anchor::Top => "t",
            Anchor::Middle => "ctr",
            Anchor::Bottom => "b",
        }
    }
}

/// Inner margins of a text box, in model units.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Insets {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Insets {
    /// DrawingML's defaults: 0.1in left and right, 0.05in top and bottom.
    pub const DRAWINGML: Insets = Insets { left: 9.6, top: 4.8, right: 9.6, bottom: 4.8 };
}

/// Shrink-on-overflow as the file states it (`a:normAutofit`): the
/// application that last laid the box out found the text too big and
/// recorded by how much it drew it smaller. Kept as stated rather than
/// applied to the run sizes, so the sizes the author chose survive a save.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Autofit {
    /// Multiplier on every run's size (`fontScale` / 100000).
    pub font_scale: f64,
    /// Fraction taken off the line spacing (`lnSpcReduction` / 100000).
    pub line_reduction: f64,
}

impl Autofit {
    /// Nothing shrunk: the box autofits, but its text currently fits.
    pub fn is_identity(&self) -> bool {
        (self.font_scale - 1.0).abs() < 1e-9 && self.line_reduction.abs() < 1e-9
    }
}

/// A text box's paragraph layout and placement.
///
/// `paras` runs parallel to the box's paragraphs (its text split on
/// `'\n'`). It may be shorter — an editor that adds a line doesn't have to
/// invent a style for it — and a paragraph without an entry is laid out
/// like the last one that has one, or plainly if there is none. An empty
/// `TextBody` is a plain box: every paragraph left-aligned, no bullets,
/// the canvas's own small inset, text from the top.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextBody {
    pub paras: Vec<ParaStyle>,
    pub anchor: Anchor,
    /// `None`: the canvas's default inset.
    pub insets: Option<Insets>,
    /// `Some` when the box shrinks text on overflow.
    pub autofit: Option<Autofit>,
    /// The layout placeholder the box fills (decks_core::layouts), if any.
    pub placeholder: Option<crate::layouts::Placeholder>,
}

impl TextBody {
    /// Nothing but defaults: drawn and written exactly as a box was before
    /// paragraph styles existed.
    pub fn is_plain(&self) -> bool {
        self.anchor == Anchor::Top
            && self.insets.is_none()
            && self.autofit.is_none()
            && self.paras.iter().all(|p| *p == ParaStyle::default())
    }

    /// The style of paragraph `i`.
    pub fn para(&self, i: usize) -> ParaStyle {
        self.paras.get(i).or(self.paras.last()).cloned().unwrap_or_default()
    }
}

/// Where a paragraph's text and marker go across its box, in the box's
/// inner coordinates (after the insets), in whatever unit `inner_w` is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParaGeometry {
    /// Left edge the text layout is placed at.
    pub text_x: f64,
    /// Wrap width of the text layout.
    pub text_width: f64,
    /// Pango-style first-line indent relative to `text_x`: positive
    /// indents the first line; negative is a hanging indent (the first line
    /// at `text_x`, the rest `-first_indent` further right).
    pub first_indent: f64,
    /// Where the marker's left edge goes, when there is one.
    pub marker_x: Option<f64>,
}

impl ParaStyle {
    /// The paragraph's horizontal geometry in a box `inner_w` wide, with
    /// `scale` converting model units to the caller's. With a marker, the
    /// marker hangs at `margin_left + indent` and every text line starts
    /// at `margin_left` (DrawingML's hanging bullet). Without one, the
    /// first line starts at `margin_left + indent`, the rest at
    /// `margin_left`.
    pub fn geometry(&self, inner_w: f64, scale: f64, has_marker: bool) -> ParaGeometry {
        let mar = self.margin_left * scale;
        let ind = self.indent * scale;
        if has_marker {
            return ParaGeometry {
                text_x: mar,
                text_width: (inner_w - mar).max(1.0),
                first_indent: 0.0,
                marker_x: Some((mar + ind).max(0.0)),
            };
        }
        let first = mar + ind;
        let text_x = mar.min(first).max(0.0);
        ParaGeometry {
            text_x,
            text_width: (inner_w - text_x).max(1.0),
            // Pango's negative indent is exactly the hanging case.
            first_indent: if ind >= 0.0 { ind } else { -(mar - text_x) },
            marker_x: None,
        }
    }
}

impl Anchor {
    /// How far down the box the block of text starts, for content
    /// `content_h` tall in a box `inner_h` tall. Text that overflows a
    /// centred box overflows both ways, as PowerPoint and Impress draw it.
    pub fn offset(self, inner_h: f64, content_h: f64) -> f64 {
        match self {
            Anchor::Top => 0.0,
            Anchor::Middle => (inner_h - content_h) / 2.0,
            Anchor::Bottom => inner_h - content_h,
        }
    }
}

/// `runs` cut into paragraphs at every `'\n'`, the breaks themselves
/// dropped. There is always at least one (possibly empty) paragraph, and
/// `n` newlines make `n + 1` paragraphs, as `text.split('\n')` does.
pub fn paragraphs(runs: &[Run]) -> Vec<Vec<Run>> {
    let mut out: Vec<Vec<Run>> = vec![Vec::new()];
    for run in runs {
        let mut pieces = run.text.split('\n').peekable();
        while let Some(piece) = pieces.next() {
            if !piece.is_empty() {
                if let Some(cur) = out.last_mut() {
                    cur.push(Run { text: piece.to_string(), style: run.style.clone() });
                }
            }
            if pieces.peek().is_some() {
                out.push(Vec::new());
            }
        }
    }
    out
}

/// The marker drawn before each paragraph, if any: the bullet character,
/// or the automatic number counted within a run of numbered paragraphs at
/// the same level. An empty paragraph shows no marker (and a numbered one
/// doesn't count), which is what PowerPoint and Impress do.
pub fn markers(styles: &[ParaStyle], empty: &[bool]) -> Vec<Option<String>> {
    // Next number per level; a paragraph at a shallower level, or one
    // without numbering at the same level, restarts the deeper counts.
    let mut counters: Vec<Option<u32>> = vec![None; 10];
    styles
        .iter()
        .enumerate()
        .map(|(i, st)| {
            let lvl = (st.level as usize).min(9);
            if empty.get(i).copied().unwrap_or(false) {
                return None;
            }
            for c in counters.iter_mut().skip(lvl + 1) {
                *c = None;
            }
            match &st.bullet {
                Bullet::None => {
                    counters[lvl] = None;
                    None
                }
                Bullet::Char(c) => {
                    counters[lvl] = None;
                    Some(c.clone())
                }
                Bullet::AutoNum { scheme, start } => {
                    let n = counters[lvl].unwrap_or(*start);
                    counters[lvl] = Some(n + 1);
                    Some(autonum_label(scheme, n))
                }
            }
        })
        .collect()
}

/// A number in a DrawingML auto-number scheme (ECMA-376 20.1.10.61).
pub fn autonum_label(scheme: &str, n: u32) -> String {
    let body = if scheme.starts_with("alphaLc") {
        alpha(n).to_lowercase()
    } else if scheme.starts_with("alphaUc") {
        alpha(n)
    } else if scheme.starts_with("romanLc") {
        roman(n).to_lowercase()
    } else if scheme.starts_with("romanUc") {
        roman(n)
    } else {
        n.to_string()
    };
    if scheme.ends_with("ParenBoth") {
        format!("({body})")
    } else if scheme.ends_with("ParenR") {
        format!("{body})")
    } else if scheme.ends_with("Plain") {
        body
    } else if scheme.ends_with("Minus") {
        format!("- {body} -")
    } else {
        format!("{body}.")
    }
}

fn alpha(n: u32) -> String {
    // A..Z, then AA..ZZ (repeated letter), as Office numbers lists.
    let n = n.max(1) - 1;
    let letter = (b'A' + (n % 26) as u8) as char;
    letter.to_string().repeat((n / 26 + 1) as usize)
}

fn roman(mut n: u32) -> String {
    const TABLE: [(u32, &str); 13] = [
        (1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"),
        (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I"),
    ];
    let mut s = String::new();
    for (v, r) in TABLE {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use letters_core::model::RunStyle;

    fn run(t: &str, bold: bool) -> Run {
        Run { text: t.into(), style: RunStyle { bold, ..Default::default() } }
    }

    #[test]
    fn runs_split_into_paragraphs_at_newlines() {
        let runs = vec![run("One ", false), run("two\nThree", true), run("\n", false), run("\nFive", false)];
        let paras = paragraphs(&runs);
        let texts: Vec<String> = paras.iter().map(|p| p.iter().map(|r| r.text.as_str()).collect()).collect();
        assert_eq!(texts, ["One two", "Three", "", "Five"]);
        // The styles stay with their pieces.
        assert!(paras[0][1].style.bold && paras[1][0].style.bold && !paras[0][0].style.bold);
    }

    #[test]
    fn no_runs_is_one_empty_paragraph() {
        assert_eq!(paragraphs(&[]).len(), 1);
    }

    #[test]
    fn a_missing_paragraph_style_repeats_the_last() {
        let body = TextBody {
            paras: vec![ParaStyle { align: ParaAlign::Center, ..Default::default() }],
            ..Default::default()
        };
        assert_eq!(body.para(3).align, ParaAlign::Center);
        assert_eq!(TextBody::default().para(0), ParaStyle::default());
        assert!(TextBody::default().is_plain());
        assert!(!body.is_plain());
    }

    #[test]
    fn bullets_and_numbers_are_counted_per_level() {
        let num = |lvl| ParaStyle {
            level: lvl,
            bullet: Bullet::AutoNum { scheme: "arabicPeriod".into(), start: 1 },
            ..Default::default()
        };
        let dash = ParaStyle { level: 1, bullet: Bullet::Char("–".into()), ..Default::default() };
        let styles = [num(0), num(1), num(1), dash, num(0), num(1), ParaStyle::default()];
        let empty = [false; 7];
        let got = markers(&styles, &empty);
        assert_eq!(
            got,
            [Some("1."), Some("1."), Some("2."), Some("–"), Some("2."), Some("1."), None]
                .map(|s| s.map(String::from))
        );
    }

    #[test]
    fn an_empty_paragraph_has_no_marker() {
        let b = ParaStyle { bullet: Bullet::Char("•".into()), ..Default::default() };
        assert_eq!(markers(&[b.clone(), b], &[true, false]), [None, Some("•".to_string())]);
    }

    #[test]
    fn autonumber_schemes() {
        assert_eq!(autonum_label("arabicPeriod", 3), "3.");
        assert_eq!(autonum_label("arabicParenR", 3), "3)");
        assert_eq!(autonum_label("alphaLcParenBoth", 2), "(b)");
        assert_eq!(autonum_label("alphaUcPeriod", 28), "BB.");
        assert_eq!(autonum_label("romanUcPeriod", 14), "XIV.");
        assert_eq!(autonum_label("romanLcPeriod", 4), "iv.");
        assert_eq!(autonum_label("arabicPlain", 7), "7");
    }

    #[test]
    fn a_bullet_hangs_in_the_indent_and_the_text_starts_at_the_margin() {
        let st = ParaStyle { margin_left: 36.0, indent: -36.0, ..Default::default() };
        let g = st.geometry(500.0, 2.0, true);
        assert_eq!(g, ParaGeometry { text_x: 72.0, text_width: 428.0, first_indent: 0.0, marker_x: Some(0.0) });
    }

    #[test]
    fn without_a_marker_a_negative_indent_hangs_the_other_lines() {
        let st = ParaStyle { margin_left: 36.0, indent: -36.0, ..Default::default() };
        let g = st.geometry(500.0, 1.0, false);
        // First line at 0, the rest at 36.
        assert_eq!((g.text_x, g.first_indent, g.marker_x), (0.0, -36.0, None));
        let st = ParaStyle { margin_left: 10.0, indent: 20.0, ..Default::default() };
        let g = st.geometry(500.0, 1.0, false);
        assert_eq!((g.text_x, g.text_width, g.first_indent), (10.0, 490.0, 20.0));
    }

    #[test]
    fn a_marker_size_is_relative_to_the_text_or_absolute() {
        assert_eq!(MarkerSize::Relative(0.75).points(32.0), 24.0);
        assert_eq!(MarkerSize::Points(10.0).points(32.0), 10.0);
    }

    #[test]
    fn the_anchor_places_the_block() {
        assert_eq!(Anchor::Top.offset(100.0, 40.0), 0.0);
        assert_eq!(Anchor::Middle.offset(100.0, 40.0), 30.0);
        assert_eq!(Anchor::Bottom.offset(100.0, 40.0), 60.0);
        assert_eq!(Anchor::Middle.offset(100.0, 140.0), -20.0);
    }

    #[test]
    fn alignment_and_anchor_names_round_trip() {
        for a in [ParaAlign::Left, ParaAlign::Center, ParaAlign::Right, ParaAlign::Justify] {
            assert_eq!(ParaAlign::from_drawingml(a.to_drawingml()), Some(a));
            assert_eq!(ParaAlign::from_odf(a.to_odf()), Some(a));
        }
        for a in [Anchor::Top, Anchor::Middle, Anchor::Bottom] {
            assert_eq!(Anchor::from_drawingml(a.to_drawingml()), Some(a));
        }
        assert_eq!(Spacing::Lines(0.2).resolve(50.0), 10.0);
        assert_eq!(Spacing::Units(7.0).resolve(50.0), 7.0);
    }
}
