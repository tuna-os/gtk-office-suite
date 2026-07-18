// markdown.rs — Document ⇄ Markdown.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Parse uses pulldown-cmark; serialize is our own emitter. Markdown cannot
// express highlight, underline, alignment, or line spacing — those survive
// model round-trips but are lost on export (documented lossiness, see
// DESIGN.md). Conformance is measured, not assumed: the CommonMark corpus
// harness in tests/markdown_corpus.rs ratchets our round-trip pass rate.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::model::{Alignment, Document, ListKind, Paragraph, ParaStyle, Run, RunStyle};

/// Parse Markdown into a Document.
pub fn parse(md: &str) -> Document {
    let mut paragraphs: Vec<Paragraph> = Vec::new();
    let mut current: Option<Paragraph> = None;
    let mut style = RunStyle::default();
    let mut list_stack: Vec<ListKind> = Vec::new();

    let parser = Parser::new_ext(md, Options::ENABLE_STRIKETHROUGH);
    for event in parser {
        match event {
            Event::Start(Tag::Paragraph) => {
                let list = list_stack.last().copied().unwrap_or(ListKind::None);
                current.get_or_insert_with(|| Paragraph {
                    style: ParaStyle { list, ..Default::default() },
                    runs: vec![],
                });
            }
            Event::End(TagEnd::Paragraph) => {
                if let Some(p) = current.take() { paragraphs.push(p); }
            }
            Event::Start(Tag::Heading { level, .. }) => {
                current = Some(Paragraph {
                    style: ParaStyle { heading: Some(heading_to_u8(level)), ..Default::default() },
                    runs: vec![],
                });
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(p) = current.take() { paragraphs.push(p); }
            }
            Event::Start(Tag::List(ordinal)) => {
                list_stack.push(if ordinal.is_some() { ListKind::Numbered } else { ListKind::Bullet });
            }
            Event::End(TagEnd::List(_)) => { list_stack.pop(); }
            Event::Start(Tag::Item) => {
                let list = list_stack.last().copied().unwrap_or(ListKind::Bullet);
                current = Some(Paragraph {
                    style: ParaStyle { list, ..Default::default() },
                    runs: vec![],
                });
            }
            Event::End(TagEnd::Item) => {
                if let Some(p) = current.take() { paragraphs.push(p); }
            }
            Event::Start(Tag::Strong) => style.bold = true,
            Event::End(TagEnd::Strong) => style.bold = false,
            Event::Start(Tag::Emphasis) => style.italic = true,
            Event::End(TagEnd::Emphasis) => style.italic = false,
            Event::Start(Tag::Strikethrough) => style.strikethrough = true,
            Event::End(TagEnd::Strikethrough) => style.strikethrough = false,
            Event::Start(Tag::Link { dest_url, .. }) => style.link = Some(dest_url.to_string()),
            Event::End(TagEnd::Link) => style.link = None,
            Event::Text(t) | Event::Code(t) => {
                let para = current.get_or_insert_with(Paragraph::default);
                para.runs.push(Run { text: t.to_string(), style: style.clone() });
            }
            Event::SoftBreak | Event::HardBreak => {
                let para = current.get_or_insert_with(Paragraph::default);
                para.runs.push(Run { text: " ".to_string(), style: style.clone() });
            }
            _ => {}
        }
    }
    if let Some(p) = current.take() { paragraphs.push(p); }

    let mut doc = Document { paragraphs };
    if doc.paragraphs.is_empty() {
        doc = Document::new();
    }
    for p in &mut doc.paragraphs {
        normalize_para(p);
    }
    doc
}

fn normalize_para(p: &mut Paragraph) {
    p.runs.retain(|r| !r.text.is_empty());
    let mut i = 0;
    while i + 1 < p.runs.len() {
        if p.runs[i].style == p.runs[i + 1].style {
            let next = p.runs.remove(i + 1);
            p.runs[i].text.push_str(&next.text);
        } else {
            i += 1;
        }
    }
}

fn heading_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Serialize a Document to Markdown.
pub fn serialize(doc: &Document) -> String {
    let mut out = String::new();
    let mut numbered_counter = 0usize;
    for (i, para) in doc.paragraphs.iter().enumerate() {
        if i > 0 {
            out.push('\n');
            // Blank line between blocks except between list items of the same list.
            let prev = &doc.paragraphs[i - 1];
            let same_list = prev.style.list != ListKind::None && prev.style.list == para.style.list;
            if !same_list { out.push('\n'); }
        }
        match para.style.list {
            ListKind::Numbered => {
                numbered_counter += 1;
                out.push_str(&format!("{}. ", numbered_counter));
            }
            ListKind::Bullet => out.push_str("- "),
            ListKind::None => numbered_counter = 0,
        }
        if let Some(level) = para.style.heading {
            for _ in 0..level { out.push('#'); }
            out.push(' ');
        }
        for run in &para.runs {
            out.push_str(&serialize_run(run));
        }
    }
    out
}

fn serialize_run(run: &Run) -> String {
    let mut s = run.text.clone();
    if run.style.strikethrough { s = format!("~~{}~~", s); }
    if run.style.italic { s = format!("*{}*", s); }
    if run.style.bold { s = format!("**{}**", s); }
    if let Some(url) = &run.style.link { s = format!("[{}]({})", s, url); }
    s
}

/// What a Markdown export cannot represent (used by UI to warn on export).
pub fn lossy_features(doc: &Document) -> Vec<&'static str> {
    let mut lost = Vec::new();
    let any_run = |f: fn(&RunStyle) -> bool| doc.paragraphs.iter().any(|p| p.runs.iter().any(|r| f(&r.style)));
    if any_run(|s| s.highlight) { lost.push("highlight"); }
    if any_run(|s| s.underline) { lost.push("underline"); }
    if doc.paragraphs.iter().any(|p| p.style.alignment != Alignment::Left) { lost.push("alignment"); }
    if doc.paragraphs.iter().any(|p| (p.style.line_spacing - 1.0).abs() > f32::EPSILON) { lost.push("line spacing"); }
    lost
}
