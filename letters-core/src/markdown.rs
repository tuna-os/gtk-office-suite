// markdown.rs — Document ⇄ Markdown.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Parse uses pulldown-cmark; serialize is our own emitter. Markdown cannot
// express highlight, underline, alignment, or line spacing — those survive
// model round-trips but are lost on export (documented lossiness, see
// DESIGN.md). Conformance is measured, not assumed: the CommonMark corpus
// harness in tests/markdown_corpus.rs ratchets our round-trip pass rate.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::model::{Alignment, Document, ListKind, Paragraph, ParaStyle, Run, RunStyle};

/// Parse Markdown into a Document.
pub fn parse(md: &str) -> Document {
    let mut paragraphs: Vec<Paragraph> = Vec::new();
    let mut current: Option<Paragraph> = None;
    let mut style = RunStyle::default();
    let mut list_stack: Vec<ListKind> = Vec::new();
    let mut code_block: Option<String> = None;

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
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                code_block = Some(lang);
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(p) = current.take() { paragraphs.push(p); }
                code_block = None;
            }
            Event::Start(Tag::Strong) => style.bold = true,
            Event::End(TagEnd::Strong) => style.bold = false,
            Event::Start(Tag::Emphasis) => style.italic = true,
            Event::End(TagEnd::Emphasis) => style.italic = false,
            Event::Start(Tag::Strikethrough) => style.strikethrough = true,
            Event::End(TagEnd::Strikethrough) => style.strikethrough = false,
            Event::Start(Tag::Link { dest_url, .. }) => style.link = Some(dest_url.to_string()),
            Event::End(TagEnd::Link) => style.link = None,
            Event::Text(t) if code_block.is_some() => {
                // Code block text arrives with embedded newlines; the model is
                // paragraph-per-line, so each line becomes a code paragraph.
                let lang = code_block.clone().unwrap();
                let mut lines = t.split('\n').peekable();
                while let Some(line) = lines.next() {
                    // Trailing newline yields a final empty segment — skip it.
                    if line.is_empty() && lines.peek().is_none() { break; }
                    let para = current.get_or_insert_with(|| Paragraph {
                        style: ParaStyle { code_block: Some(lang.clone()), ..Default::default() },
                        runs: vec![],
                    });
                    if !line.is_empty() {
                        para.runs.push(Run { text: line.to_string(), style: RunStyle::default() });
                    }
                    if lines.peek().is_some() {
                        if let Some(p) = current.take() { paragraphs.push(p); }
                    }
                }
            }
            Event::Code(t) => {
                let para = current.get_or_insert_with(Paragraph::default);
                let mut code_style = style.clone();
                code_style.code = true;
                para.runs.push(Run { text: t.to_string(), style: code_style });
            }
            Event::Text(t) => {
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
        let prev_code = i.checked_sub(1).and_then(|j| doc.paragraphs[j].style.code_block.as_ref());
        let next_code = doc.paragraphs.get(i + 1).and_then(|p| p.style.code_block.as_ref());

        if i > 0 {
            out.push('\n');
            // Blank line between blocks except between list items of the same
            // list and between lines of the same code block.
            let prev = &doc.paragraphs[i - 1];
            let same_list = prev.style.list != ListKind::None && prev.style.list == para.style.list;
            let same_code = para.style.code_block.is_some()
                && prev.style.code_block == para.style.code_block;
            if !same_list && !same_code { out.push('\n'); }
        }

        if let Some(lang) = &para.style.code_block {
            // Opening fence when the previous paragraph isn't part of this block.
            if prev_code != Some(lang) {
                out.push_str("```");
                out.push_str(lang);
                out.push('\n');
            }
            out.push_str(&para.text());
            // Closing fence when the next paragraph isn't part of this block.
            if next_code != Some(lang) {
                out.push_str("\n```");
            }
            continue;
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
    if run.style.code { s = format!("`{}`", s); }
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
