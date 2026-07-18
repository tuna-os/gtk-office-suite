// fragment.rs — the cross-app clipboard fragment format.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// One serializable type carries content between Letters, Tables, and
// Decks with styling and data intact (DESIGN-UI.md "Cross-app clipboard").
// MIME: application/x-tunaos-suite+json. Every conversion here is a pure
// function: the paste-mapping matrix is unit-tested without a clipboard.

use serde::{Deserialize, Serialize};

use crate::model::{Paragraph, ParaStyle, Run, RunStyle, TableCell};

pub const MIME: &str = "application/x-tunaos-suite+json";

/// A copied piece of content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Fragment {
    /// Styled text: shared Run/RunStyle, so Letters ⇄ Decks is lossless
    /// by construction.
    Text(Vec<Paragraph>),
    /// A cell grid with everything Tables knows about each cell.
    Grid(Vec<Vec<GridCell>>),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GridCell {
    /// Displayed value.
    pub value: String,
    /// Formula without leading '=' when the cell has one.
    pub formula: Option<String>,
    /// Number-format descriptor (xlsx-style code), if non-default.
    pub num_format: Option<String>,
}

impl Fragment {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("fragment serialization is infallible")
    }

    pub fn from_json(s: &str) -> Option<Fragment> {
        serde_json::from_str(s).ok()
    }

    /// Plain-text form: paragraphs joined by newlines; grids as TSV.
    pub fn to_plain(&self) -> String {
        match self {
            Fragment::Text(paras) => paras
                .iter()
                .map(|p| p.text())
                .collect::<Vec<_>>()
                .join("\n"),
            Fragment::Grid(rows) => rows
                .iter()
                .map(|r| r.iter().map(|c| c.value.as_str()).collect::<Vec<_>>().join("\t"))
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// HTML form for interchange with external apps.
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
                        let cells: String =
                            r.iter().map(|c| format!("<td>{}</td>", esc(&c.value))).collect();
                        format!("<tr>{cells}</tr>")
                    })
                    .collect();
                format!("<table>{body}</table>")
            }
        }
    }

    /// Paste a grid into a document: a real cell-tagged table.
    pub fn grid_to_paragraphs(rows: &[Vec<GridCell>], table_id: u32) -> Vec<Paragraph> {
        let mut out = Vec::new();
        for (ri, row) in rows.iter().enumerate() {
            for (ci, cell) in row.iter().enumerate() {
                out.push(Paragraph {
                    style: ParaStyle {
                        table_cell: Some(TableCell {
                            table: table_id,
                            row: ri as u32,
                            col: ci as u32,
                        }),
                        ..Default::default()
                    },
                    runs: if cell.value.is_empty() {
                        vec![]
                    } else {
                        vec![Run { text: cell.value.clone(), style: RunStyle::default() }]
                    },
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::StylePatch;

    #[test]
    fn styled_text_fragment_round_trips_json() {
        let mut d = crate::model::Document::from_plain_text("plain bold");
        d.apply_run_style(6, 10, &StylePatch::set_bold(true));
        let frag = Fragment::Text(d.paragraphs.clone());
        let back = Fragment::from_json(&frag.to_json()).expect("parse");
        assert_eq!(frag, back);
        match back {
            Fragment::Text(paras) => {
                assert!(paras[0].runs.iter().any(|r| r.style.bold && r.text == "bold"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn grid_fragment_keeps_formulas_and_formats() {
        let frag = Fragment::Grid(vec![vec![
            GridCell { value: "5".into(), formula: Some("A1+B1".into()), num_format: None },
            GridCell { value: "50%".into(), formula: None, num_format: Some("0%".into()) },
        ]]);
        let back = Fragment::from_json(&frag.to_json()).expect("parse");
        assert_eq!(frag, back);
        assert_eq!(frag.to_plain(), "5\t50%");
        assert!(frag.to_html().contains("<td>5</td>"));
    }

    #[test]
    fn grid_pastes_into_document_as_table() {
        let rows = vec![
            vec![GridCell { value: "a".into(), ..Default::default() },
                 GridCell { value: "b".into(), ..Default::default() }],
            vec![GridCell { value: "c".into(), ..Default::default() },
                 GridCell { value: "d".into(), ..Default::default() }],
        ];
        let paras = Fragment::grid_to_paragraphs(&rows, 0);
        assert_eq!(paras.len(), 4);
        let tc = paras[3].style.table_cell.expect("cell tag");
        assert_eq!((tc.row, tc.col), (1, 1));
        assert_eq!(paras[3].text(), "d");
    }

    #[test]
    fn text_fragment_html_carries_styles() {
        let mut d = crate::model::Document::from_plain_text("go bold now");
        d.apply_run_style(3, 7, &StylePatch::set_bold(true));
        let html = Fragment::Text(d.paragraphs).to_html();
        assert!(html.contains("<b>bold"), "{html}");
    }
}

/// Extract the document range [start, end) (global char offsets, where
/// each paragraph break counts as one char) as a text fragment —
/// partial paragraphs keep their runs sliced at the boundaries.
pub fn from_selection(doc: &crate::model::Document, start: usize, end: usize) -> Fragment {
    let mut paras: Vec<Paragraph> = Vec::new();
    let mut pos = 0usize;
    for p in &doc.paragraphs {
        let len = p.char_len();
        let p_start = pos;
        let p_end = pos + len;
        pos = p_end + 1; // the paragraph break
        if p_end < start || p_start >= end {
            continue;
        }
        let s = start.saturating_sub(p_start);
        let e = (end - p_start).min(len);
        paras.push(slice_paragraph(p, s, e));
    }
    Fragment::Text(paras)
}

fn slice_paragraph(p: &Paragraph, start: usize, end: usize) -> Paragraph {
    let mut runs: Vec<Run> = Vec::new();
    let mut pos = 0usize;
    for r in &p.runs {
        let len = r.char_len();
        let r_start = pos;
        let r_end = pos + len;
        pos = r_end;
        if r_end <= start || r_start >= end {
            continue;
        }
        let s = start.saturating_sub(r_start);
        let e = (end - r_start).min(len);
        let text: String = r.text.chars().skip(s).take(e - s).collect();
        if !text.is_empty() {
            runs.push(Run { text, style: r.style.clone() });
        }
    }
    Paragraph { style: p.style.clone(), runs }
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    use crate::model::Document;

    #[test]
    fn whole_document_selection() {
        let d = Document::from_plain_text("one\ntwo");
        let Fragment::Text(paras) = from_selection(&d, 0, 7) else { panic!() };
        assert_eq!(paras.len(), 2);
        assert_eq!(paras[0].text(), "one");
        assert_eq!(paras[1].text(), "two");
    }

    #[test]
    fn partial_span_across_paragraphs() {
        let d = Document::from_plain_text("hello\nworld");
        // chars: h e l l o ⏎ w o r l d  → [3, 8) = "lo" + "wo"
        let Fragment::Text(paras) = from_selection(&d, 3, 8) else { panic!() };
        assert_eq!(paras.len(), 2);
        assert_eq!(paras[0].text(), "lo");
        assert_eq!(paras[1].text(), "wo");
    }

    #[test]
    fn run_styles_survive_slicing() {
        let mut d = Document::from_plain_text("plain bold tail");
        d.apply_run_style(6, 10, &crate::model::StylePatch::set_bold(true));
        // Select "n bold t" = [4, 12)
        let Fragment::Text(paras) = from_selection(&d, 4, 12) else { panic!() };
        assert_eq!(paras[0].text(), "n bold t");
        let bold: String = paras[0]
            .runs
            .iter()
            .filter(|r| r.style.bold)
            .map(|r| r.text.as_str())
            .collect();
        assert_eq!(bold, "bold");
    }

    #[test]
    fn empty_selection_is_empty() {
        let d = Document::from_plain_text("abc");
        let Fragment::Text(paras) = from_selection(&d, 2, 2) else { panic!() };
        assert!(paras.is_empty() || paras.iter().all(|p| p.char_len() == 0));
    }
}
