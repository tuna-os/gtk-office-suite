// fragment.rs — Decks side of the cross-app clipboard.
// SPDX-License-Identifier: GPL-3.0-or-later

pub use suite_common_core::document::{GridCell, MIME};
use serde::{Deserialize, Serialize};
use suite_common_core::document::{ParaStyle, Paragraph, Run, RunStyle, ToNeutral};

use crate::engine::SlideObject;

/// The suite clipboard as Decks reads and writes it (ADR 0012): the shared
/// fragment, with each paragraph read as a [`ClipParagraph`].
pub type Fragment = suite_common_core::document::Fragment<ClipParagraph>;

/// A paragraph from the suite clipboard: neutral text, plus markers of what
/// the copying app had that a slide cannot hold, read only so a paste can
/// name what it flattened and leave out pictures and deleted tracked text.
/// Decks never writes the markers, so what it copies is plain neutral JSON.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipParagraph {
    pub style: ParaStyle,
    pub runs: Vec<ClipRun>,
}

/// A run from the suite clipboard: see [`ClipParagraph`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipRun {
    pub text: String,
    pub style: ClipStyle,
}

/// A run's neutral style and what else the copying app marked on it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ClipStyle {
    #[serde(flatten)]
    pub style: RunStyle,
    #[serde(flatten)]
    pub foreign: Foreign,
}

/// Run markers another app writes that a slide has no place for. Read,
/// never written; only whether each is present matters here.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Foreign {
    /// An inline picture (Letters): the run's text is its alt text.
    #[serde(skip_serializing)]
    pub image: Option<serde_json::Value>,
    /// Raw inline HTML (Letters' Markdown): text here, never markup.
    #[serde(skip_serializing)]
    pub html: bool,
    #[serde(skip_serializing)]
    pub chip: Option<serde_json::Value>,
    #[serde(skip_serializing)]
    pub comments: Vec<serde_json::Value>,
    #[serde(skip_serializing)]
    pub footnote: Option<serde_json::Value>,
    #[serde(skip_serializing)]
    pub revision: Option<ClipRevision>,
}

/// A tracked change on a copied run; only its kind matters to a paste.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipRevision {
    pub kind: String,
}

impl From<Run> for ClipRun {
    fn from(r: Run) -> Self {
        ClipRun { text: r.text, style: ClipStyle { style: r.style, foreign: Foreign::default() } }
    }
}

impl From<Paragraph> for ClipParagraph {
    fn from(p: Paragraph) -> Self {
        ClipParagraph { style: p.style, runs: p.runs.into_iter().map(Into::into).collect() }
    }
}

impl ClipParagraph {
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// The paragraph's runs with only their neutral style.
    pub fn neutral_runs(&self) -> Vec<Run> {
        self.runs.iter().map(|r| Run { text: r.text.clone(), style: r.style.style.clone() }).collect()
    }
}

impl ToNeutral for ClipParagraph {
    fn to_neutral(&self) -> Paragraph {
        Paragraph { style: self.style.clone(), runs: self.neutral_runs() }
    }
}

/// One paragraph per line of `text`, like a plain-text paste.
fn plain_paragraphs(text: &str) -> Vec<ClipParagraph> {
    text.split('\n').map(|line| Paragraph::plain(line).into()).collect()
}

/// Copy a text box as a styled-text fragment (runs carried verbatim).
pub fn copy_object(obj: &SlideObject) -> Option<Fragment> {
    match obj {
        SlideObject::TextBox { text, runs, .. } => {
            let paras = if runs.is_empty() {
                plain_paragraphs(text)
            } else {
                vec![Paragraph { style: Default::default(), runs: runs.clone() }.into()]
            };
            Some(Fragment::Text(paras))
        }
        _ => None,
    }
}

/// Paste a fragment as a new text box at the given position.
pub fn paste_as_text_box(frag: &Fragment, x: f64, y: f64) -> SlideObject {
    let (text, runs) = match frag {
        Fragment::Text(paras) => {
            let text = paras.iter().map(|p| p.text()).collect::<Vec<_>>().join("\n");
            let runs = if paras.len() == 1 { paras[0].neutral_runs() } else { vec![] };
            (text, runs)
        }
        Fragment::Grid(_) => (frag.to_plain(), vec![]),
    };
    SlideObject::TextBox { text, x, y, w: 400.0, h: 100.0, rotation: 0.0, runs, body: Default::default() }
}

/// Copy bytes `start..end` of a text box's text (DCK-007) as a styled
/// fragment: one paragraph per paragraph of the range, runs
/// verbatim, each with its paragraph's alignment and list kind and level.
/// The caller has validated the range.
pub fn copy_range(text: &crate::rich_text::RichText, start: usize, end: usize) -> Fragment {
    use crate::engine::{Bullet, ParaAlign};
    use suite_common_core::document::{Alignment, ListKind};
    let mut runs: Vec<Run> = Vec::new();
    if text.runs.is_empty() {
        runs.push(Run::plain(&text.text[start..end]));
    } else {
        let mut pos = 0;
        for r in &text.runs {
            let (s, e) = (pos, pos + r.text.len());
            pos = e;
            if e <= start || s >= end {
                continue;
            }
            let piece = &r.text[start.max(s) - s..end.min(e) - s];
            runs.push(Run { text: piece.to_string(), style: r.style.clone() });
        }
    }
    let first = text.para_of(start);
    let paras = crate::engine::text_body::paragraphs(&runs)
        .into_iter()
        .enumerate()
        .map(|(i, runs)| {
            let p = text.body.para(first + i);
            let style = ParaStyle {
                alignment: match p.align {
                    ParaAlign::Left => Alignment::Left,
                    ParaAlign::Center => Alignment::Center,
                    ParaAlign::Right => Alignment::Right,
                    ParaAlign::Justify => Alignment::Justify,
                },
                list: match p.bullet {
                    Bullet::None => ListKind::None,
                    Bullet::Char(_) => ListKind::Bullet,
                    Bullet::AutoNum { .. } => ListKind::Numbered,
                },
                list_level: p.level,
                ..Default::default()
            };
            Paragraph { style, runs }.into()
        })
        .collect();
    Fragment::Text(paras)
}

/// A fragment made ready to paste into a text box: its text as runs a
/// slide can hold (paragraphs joined by line breaks), the paragraph edits
/// each pasted paragraph applies (`[0]` is merged into the paragraph the
/// caret is in and keeps that one's style), and what had to be flattened
/// to plain text, to tell the user.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pasted {
    pub runs: Vec<Run>,
    pub paras: Vec<Vec<crate::format::FormatEdit>>,
    pub flattened: Vec<&'static str>,
}

/// Sanitize a fragment for pasting into a text box. Only what a slide's
/// runs keep survives (bold, italic, underline, strikethrough, font,
/// size, colour; alignment and lists): links, raw HTML, chips, comments,
/// tracked changes, footnotes, highlights, code and super/subscript are
/// pasted as their plain text and named in `flattened`; inline pictures
/// and deleted tracked text are not pasted. Raw HTML is text here, never
/// markup. A cell grid pastes as tab-separated text.
pub fn paste_runs(frag: &Fragment) -> Pasted {
    use crate::format::{FormatEdit, ListKind as DeckList};
    use suite_common_core::document::{Alignment, ListKind};
    let mut out = Pasted::default();
    fn note(what: &'static str, out: &mut Vec<&'static str>) {
        if !out.contains(&what) {
            out.push(what);
        }
    }
    let paras = match frag {
        Fragment::Text(paras) => paras,
        Fragment::Grid(_) => {
            out.runs.push(Run::plain(frag.to_plain()));
            note("table cells", &mut out.flattened);
            return out;
        }
    };
    for (i, p) in paras.iter().enumerate() {
        if i > 0 {
            let style = out.runs.last().map(|r: &Run| r.style.clone()).unwrap_or_default();
            out.runs.push(Run { text: "\n".into(), style });
        }
        for r in &p.runs {
            let (s, f) = (&r.style.style, &r.style.foreign);
            if f.image.is_some() {
                note("pictures", &mut out.flattened);
                continue;
            }
            if f.revision.as_ref().is_some_and(|rev| rev.kind == "Delete") {
                note("tracked changes", &mut out.flattened);
                continue;
            }
            for (lost, what) in [
                (s.link.is_some(), "links"),
                (f.html, "raw HTML"),
                (f.chip.is_some(), "smart chips"),
                (!f.comments.is_empty(), "comments"),
                (f.revision.is_some(), "tracked changes"),
                (f.footnote.is_some(), "footnotes"),
                (s.highlight, "highlighting"),
                (s.code, "code formatting"),
                (s.vert_align.is_some(), "superscript and subscript"),
            ] {
                if lost {
                    note(what, &mut out.flattened);
                }
            }
            if r.text.is_empty() {
                continue;
            }
            let style = RunStyle {
                bold: s.bold,
                italic: s.italic,
                underline: s.underline,
                strikethrough: s.strikethrough,
                font_family: s.font_family.clone(),
                font_size_hp: s.font_size_hp,
                color: s.color.clone(),
                ..Default::default()
            };
            match out.runs.last_mut() {
                Some(last) if last.style == style => last.text.push_str(&r.text),
                _ => out.runs.push(Run { text: r.text.clone(), style }),
            }
        }
        if p.style.heading.is_some() {
            note("headings", &mut out.flattened);
        }
        let mut edits = Vec::new();
        match p.style.alignment {
            Alignment::Left => {}
            Alignment::Center => edits.push(FormatEdit::Align(crate::engine::ParaAlign::Center)),
            Alignment::Right => edits.push(FormatEdit::Align(crate::engine::ParaAlign::Right)),
            Alignment::Justify => edits.push(FormatEdit::Align(crate::engine::ParaAlign::Justify)),
        }
        match p.style.list {
            ListKind::None => {}
            ListKind::Bullet => edits.push(FormatEdit::List(DeckList::Bullet)),
            ListKind::Numbered => edits.push(FormatEdit::List(DeckList::Number)),
        }
        if p.style.list != ListKind::None {
            edits.extend(std::iter::repeat_n(FormatEdit::Indent(true), p.style.list_level.min(8) as usize));
        }
        out.paras.push(edits);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styled_text_survives_decks_round_trip() {
        let obj = SlideObject::TextBox {
            text: "plain bold".into(),
            x: 0.0, y: 0.0, w: 100.0, h: 50.0,
            rotation: 0.0,
            runs: vec![
                Run { text: "plain ".into(), style: RunStyle::default() },
                Run { text: "bold".into(), style: RunStyle { bold: true, ..Default::default() } },
            ],
            body: Default::default(),
        };
        let frag = copy_object(&obj).expect("fragment");
        let back = paste_as_text_box(&frag, 10.0, 10.0);
        match back {
            SlideObject::TextBox { runs, text, .. } => {
                assert_eq!(text, "plain bold");
                assert!(runs.iter().any(|r| r.style.bold && r.text == "bold"),
                        "bold run lost through clipboard");
            }
            _ => panic!("wrong object"),
        }
    }

    #[test]
    fn grid_pastes_as_tsv_text_box() {
        let frag = Fragment::Grid(vec![vec![
            GridCell { value: "a".into(), ..Default::default() },
            GridCell { value: "b".into(), ..Default::default() },
        ]]);
        match paste_as_text_box(&frag, 0.0, 0.0) {
            SlideObject::TextBox { text, .. } => assert_eq!(text, "a\tb"),
            _ => panic!("wrong object"),
        }
    }

    // ── copy_object edge cases ───────────────────────────────────────────────

    fn textbox(text: &str, runs: Vec<Run>) -> SlideObject {
        SlideObject::TextBox { text: text.into(), x: 0.0, y: 0.0, w: 100.0, h: 50.0, runs, rotation: 0.0, body: Default::default() }
    }

    #[test]
    fn copy_non_textbox_returns_none() {
        assert!(copy_object(&SlideObject::Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0, rotation: 0.0 }).is_none());
        assert!(copy_object(&SlideObject::Circle { x: 0.0, y: 0.0, r: 1.0, rotation: 0.0 }).is_none());
        assert!(copy_object(&SlideObject::Image { path: "p.png".into(), x: 0.0, y: 0.0, w: 1.0, h: 1.0, rotation: 0.0, crop: Default::default() }).is_none());
    }

    #[test]
    fn copy_unstyled_textbox_falls_back_to_plain_text() {
        let frag = copy_object(&textbox("alpha\nbeta", vec![])).expect("fragment");
        match frag {
            Fragment::Text(paras) => {
                assert_eq!(paras.len(), 2);
                assert_eq!(paras[0].text(), "alpha");
                assert_eq!(paras[1].text(), "beta");
            }
            _ => panic!("expected text fragment"),
        }
    }

    #[test]
    fn copy_styled_textbox_keeps_runs_verbatim() {
        let runs = vec![Run { text: "b".into(), style: RunStyle { bold: true, ..Default::default() } }];
        let frag = copy_object(&textbox("b", runs.clone())).expect("fragment");
        match frag {
            Fragment::Text(paras) => {
                assert_eq!(paras.len(), 1);
                assert_eq!(paras[0].neutral_runs(), runs);
            }
            _ => panic!("expected text fragment"),
        }
    }

    // ── paste_as_text_box edge cases ────────────────────────────────────────

    #[test]
    fn paste_multiparagraph_text_drops_runs() {
        let frag = Fragment::Text(plain_paragraphs("l1\nl2"));
        let back = paste_as_text_box(&frag, 1.0, 2.0);
        match back {
            SlideObject::TextBox { text, runs, x, y, .. } => {
                assert_eq!(text, "l1\nl2");
                assert!(runs.is_empty(), "multi-paragraph paste must drop runs");
                assert_eq!((x, y), (1.0, 2.0));
            }
            _ => panic!("wrong object"),
        }
    }

    #[test]
    fn paste_single_paragraph_keeps_runs() {
        let para = Paragraph { style: Default::default(), runs: vec![Run { text: "styled".into(), style: RunStyle { italic: true, ..Default::default() } }] };
        let frag = Fragment::Text(vec![para.into()]);
        let back = paste_as_text_box(&frag, 0.0, 0.0);
        match back {
            SlideObject::TextBox { runs, .. } => {
                assert_eq!(runs.len(), 1);
                assert!(runs[0].style.italic);
            }
            _ => panic!("wrong object"),
        }
    }

    #[test]
    fn grid_pastes_as_tsv_with_default_geometry() {
        let frag = Fragment::Grid(vec![
            vec![GridCell { value: "a".into(), ..Default::default() }],
            vec![GridCell { value: "b".into(), ..Default::default() }],
        ]);
        match paste_as_text_box(&frag, 5.0, 6.0) {
            SlideObject::TextBox { text, x, y, w, h, .. } => {
                assert_eq!(text, "a\nb");
                assert_eq!((x, y, w, h), (5.0, 6.0, 400.0, 100.0));
            }
            _ => panic!("wrong object"),
        }
    }

    // ── range copy and sanitized paste (DCK-007) ───────────────────────────

    #[test]
    fn a_copied_range_keeps_its_paragraphs_styles_and_pastes_back_into_a_session() {
        use crate::engine::{Bullet, ParaAlign, ParaStyle, TextBody};
        use crate::rich_text::{RichText, TextSession};
        let bold = RunStyle { bold: true, ..Default::default() };
        let body = TextBody {
            paras: vec![ParaStyle { align: ParaAlign::Center, ..Default::default() }, ParaStyle { bullet: Bullet::Char("•".into()), level: 1, ..Default::default() }],
            ..Default::default()
        };
        let rt = RichText { text: "Head bold\nitem".into(), runs: vec![Run::plain("Head "), Run { text: "bold\nitem".into(), style: bold.clone() }], body };
        let Fragment::Text(paras) = copy_range(&rt, 5, rt.text.len()) else { panic!("text") };
        assert_eq!(paras.len(), 2, "one clipboard paragraph per paragraph, no line break inside");
        assert_eq!(paras[0].neutral_runs(), vec![Run { text: "bold".into(), style: bold.clone() }]);
        assert_eq!(paras[0].style.alignment, suite_common_core::document::Alignment::Center);
        assert_eq!(paras[1].style.list, suite_common_core::document::ListKind::Bullet);
        assert_eq!(paras[1].style.list_level, 1);
        let pasted = paste_runs(&Fragment::Text(paras));
        assert!(pasted.flattened.is_empty(), "{:?}", pasted.flattened);
        let plain = RichText { text: "x".into(), runs: vec![], body: TextBody::default() };
        let mut s = TextSession::new(1, 2, None, plain);
        assert_eq!(s.insert_runs(pasted.runs, &pasted.paras), Ok(true));
        let w = s.working();
        assert_eq!(w.text, "xbold\nitem");
        assert!(w.runs.iter().any(|r| r.text.starts_with("bold") && r.style.bold));
        assert_eq!(w.body.paras[1].bullet, Bullet::Char("•".into()));
        assert_eq!(w.body.paras[1].level, 1);
        w.check().unwrap();
    }

    #[test]
    fn decks_copies_plain_neutral_json() {
        let bold = RunStyle { bold: true, ..Default::default() };
        let frag = copy_object(&textbox("b", vec![Run { text: "b".into(), style: bold.clone() }])).expect("fragment");
        let json = frag.to_json();
        for marker in ["\"image\"", "\"html\"", "\"chip\"", "\"comments\"", "\"footnote\"", "\"revision\""] {
            assert!(!json.contains(marker), "{marker} written: {json}");
        }
        let neutral = suite_common_core::document::Fragment::<Paragraph>::from_json(&json).expect("neutral read");
        let suite_common_core::document::Fragment::Text(paras) = neutral else { panic!("text") };
        assert_eq!(paras[0].runs, vec![Run { text: "b".into(), style: bold }]);
    }

    #[test]
    fn pasting_flattens_what_a_slide_cannot_hold_and_never_parses_html() {
        // As Letters puts it on the clipboard: its own run fields beside
        // the shared ones.
        let json = r#"{"Text":[{"style":{},"runs":[
            {"text":"<script>alert(1)</script>","style":{"html":true}},
            {"text":"site","style":{"link":"https://example.org","bold":true}},
            {"text":"alt","style":{"image":"/tmp/x.png"}},
            {"text":"gone","style":{"revision":{"kind":"Delete","author":"a","date":"2026-01-01T00:00:00Z","under":null}}}
        ]}]}"#;
        let frag = Fragment::from_json(json).expect("a Letters fragment reads in Decks");
        let p = paste_runs(&frag);
        let text: String = p.runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text, "<script>alert(1)</script>site", "HTML stays literal text; pictures and deletions are not pasted");
        assert!(p.runs.iter().all(|r| r.style.link.is_none()));
        assert!(p.runs.iter().any(|r| r.text == "site" && r.style.bold), "the supported style survives");
        for what in ["raw HTML", "links", "pictures", "tracked changes"] {
            assert!(p.flattened.contains(&what), "{what} not disclosed: {:?}", p.flattened);
        }
    }
}
