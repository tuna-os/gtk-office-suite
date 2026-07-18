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
    // Same-type emphasis nests (*foo **bar *baz* bim** bop*): booleans
    // would be cleared by the inner End while the outer is still open.
    let (mut bold_depth, mut italic_depth, mut strike_depth) = (0u32, 0u32, 0u32);
    let mut list_stack: Vec<ListKind> = Vec::new();
    let mut code_block: Option<String> = None;
    let mut in_html_block = false;
    let mut quote_depth = 0usize;

    let parser = Parser::new_ext(md, Options::ENABLE_STRIKETHROUGH);
    for event in parser {
        match event {
            Event::Start(Tag::Paragraph) => {
                let list = list_stack.last().copied().unwrap_or(ListKind::None);
                // Flags cannot express container nesting order, so quotes
                // inside list items stay plain (stable) rather than wrong.
                let block_quote = quote_depth > 0 && list_stack.is_empty();
                current.get_or_insert_with(|| Paragraph {
                    style: ParaStyle { list, block_quote, ..Default::default() },
                    runs: vec![],
                });
            }
            Event::Start(Tag::BlockQuote(_)) => quote_depth += 1,
            Event::End(TagEnd::BlockQuote(_)) => quote_depth = quote_depth.saturating_sub(1),
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
            Event::Start(Tag::HtmlBlock) => {
                // Raw HTML block: preserved verbatim, line-per-paragraph
                // like code blocks (source fidelity, no interpretation).
                in_html_block = true;
            }
            Event::End(TagEnd::HtmlBlock) => {
                if let Some(p) = current.take() { paragraphs.push(p); }
                in_html_block = false;
            }
            Event::Html(t) | Event::InlineHtml(t) if in_html_block => {
                let mut lines = t.split('\n').peekable();
                while let Some(line) = lines.next() {
                    if line.is_empty() && lines.peek().is_none() { break; }
                    let para = current.get_or_insert_with(|| Paragraph {
                        style: ParaStyle { html_block: true, ..Default::default() },
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
            Event::InlineHtml(t) => {
                let para = current.get_or_insert_with(Paragraph::default);
                para.runs.push(Run {
                    text: t.to_string(),
                    style: RunStyle { html: true, ..style.clone() },
                });
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
            Event::Start(Tag::Strong) => {
                bold_depth += 1;
                style.bold = true;
            }
            Event::End(TagEnd::Strong) => {
                bold_depth = bold_depth.saturating_sub(1);
                style.bold = bold_depth > 0;
            }
            Event::Start(Tag::Emphasis) => {
                italic_depth += 1;
                style.italic = true;
            }
            Event::End(TagEnd::Emphasis) => {
                italic_depth = italic_depth.saturating_sub(1);
                style.italic = italic_depth > 0;
            }
            Event::Start(Tag::Strikethrough) => {
                strike_depth += 1;
                style.strikethrough = true;
            }
            Event::End(TagEnd::Strikethrough) => {
                strike_depth = strike_depth.saturating_sub(1);
                style.strikethrough = strike_depth > 0;
            }
            Event::Start(Tag::Link { dest_url, .. }) => style.link = Some(dest_url.to_string()),
            Event::End(TagEnd::Link) => style.link = None,
            Event::Start(Tag::Image { dest_url, .. }) => {
                // Alt text arrives as Text events inside and lands in runs
                // carrying this image source in their style.
                style.image = Some(dest_url.to_string());
            }
            Event::End(TagEnd::Image) => {
                // Alt-less images produced no Text event; emit the run here.
                let para = current.get_or_insert_with(Paragraph::default);
                let has = para.runs.last().map(|r| r.style.image == style.image).unwrap_or(false);
                if !has {
                    para.runs.push(Run {
                        text: String::new(),
                        style: RunStyle {
                            image: style.image.clone(),
                            link: style.link.clone(),
                            ..Default::default()
                        },
                    });
                }
                style.image = None;
            }
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
                if style.image.is_some() {
                    // Alt text is plain by definition; keep only image + link
                    // so alt-internal styling cannot destabilize round-trips.
                    para.runs.push(Run {
                        text: t.to_string(),
                        style: RunStyle {
                            image: style.image.clone(),
                            link: style.link.clone(),
                            ..Default::default()
                        },
                    });
                } else {
                    para.runs.push(Run { text: t.to_string(), style: style.clone() });
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                let para = current.get_or_insert_with(Paragraph::default);
                para.runs.push(Run { text: " ".to_string(), style: style.clone() });
            }
            _ => {}
        }
    }
    if let Some(p) = current.take() { paragraphs.push(p); }

    let mut doc = Document { paragraphs, footnotes: vec![], header: None, footer: None, page: None };
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
    let mut code_fence = String::from("```");
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
            let same_html = para.style.html_block && prev.style.html_block;
            let same_quote = para.style.block_quote && prev.style.block_quote;
            if !same_list && !same_code && !same_html {
                // Paragraph separation inside a quote needs a '>'-prefixed
                // blank line, or the quote splits on reparse.
                if same_quote { out.push_str(">\n"); } else { out.push('\n'); }
            }
        }

        if para.style.html_block {
            out.push_str(&para.text());
            continue;
        }
        if let Some(lang) = &para.style.code_block {
            // Opening fence when the previous paragraph isn't part of this block.
            if prev_code != Some(lang) {
                // The fence must not appear in the content: use one char
                // more than the longest interior run. Info strings holding
                // backticks are illegal on backtick fences — use tildes.
                let mut content = String::new();
                for p2 in &doc.paragraphs[i..] {
                    if p2.style.code_block.as_ref() != Some(lang) { break; }
                    content.push_str(&p2.text());
                    content.push('\n');
                }
                let fence_char = if lang.contains('`') { '~' } else { '`' };
                let longest = content
                    .split(|c| c != fence_char)
                    .map(str::len)
                    .max()
                    .unwrap_or(0);
                let fence: String = std::iter::repeat(fence_char)
                    .take((longest + 1).max(3))
                    .collect();
                out.push_str(&fence);
                out.push_str(lang);
                out.push('\n');
                code_fence = fence;
            }
            out.push_str(&para.text());
            // Closing fence when the next paragraph isn't part of this block.
            if next_code != Some(lang) {
                out.push('\n');
                out.push_str(&code_fence);
            }
            continue;
        }
        if para.style.block_quote {
            out.push_str("> ");
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
        let body = serialize_runs(&para.runs);
        // Leading spaces/tabs would be stripped (or become indented code)
        // on reparse — encode them as character references.
        let kept = body.trim_start_matches([' ', '\t']);
        for c in body[..body.len() - kept.len()].chars() {
            out.push_str(if c == ' ' { "&#32;" } else { "&#9;" });
        }
        out.push_str(kept);
    }
    out
}

/// Escape Markdown metacharacters so literal text survives a re-parse.
fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '~' | '!' | '&' => {
                out.push('\\');
                out.push(c);
            }
            // Newlines/tabs inside run text only come from character
            // references; raw, they would reparse as breaks/indented code.
            '\n' => out.push_str("&#10;"),
            '\t' => out.push_str("&#9;"),
            _ => out.push(c),
        }
    }
    out
}

/// A link/image destination: wrapped in angle brackets when it contains
/// whitespace or parentheses, which would otherwise terminate the
/// `(...)` early on re-parse.
fn format_dest(dest: &str) -> String {
    if dest.contains(char::is_whitespace) || dest.contains('(') || dest.contains(')') {
        format!("<{}>", dest)
    } else {
        dest.to_string()
    }
}

fn serialize_image(run: &Run) -> String {
    let src = run.style.image.as_deref().unwrap_or_default();
    let alt = escape_text(&run.text);
    let img = format!("![{}]({})", alt, format_dest(src));
    match &run.style.link {
        Some(url) => format!("[{}]({})", img, format_dest(url)),
        None => img,
    }
}

/// Inline features that serialize as paired markers. Links join the
/// stack so emphasis spanning a link stays outside the brackets.
#[derive(Clone, PartialEq)]
enum Feat {
    Bold,
    Italic,
    Strike,
    Link(String),
}

fn run_has(run: &Run, f: &Feat) -> bool {
    match f {
        Feat::Bold => run.style.bold,
        Feat::Italic => run.style.italic,
        Feat::Strike => run.style.strikethrough,
        Feat::Link(url) => run.style.link.as_deref() == Some(url.as_str()),
    }
}

fn open_marker(f: &Feat) -> String {
    match f {
        Feat::Bold => "**".into(),
        Feat::Italic => "*".into(),
        Feat::Strike => "~~".into(),
        Feat::Link(_) => "[".into(),
    }
}

fn close_marker(f: &Feat) -> String {
    match f {
        Feat::Bold => "**".into(),
        Feat::Italic => "*".into(),
        Feat::Strike => "~~".into(),
        Feat::Link(url) => format!("]({})", format_dest(url)),
    }
}

/// Serialize a paragraph's runs with a marker stack: markers shared by
/// consecutive runs stay open across the boundary, so `**foo *bar***`
/// never degrades into `**foo *****bar***` (the old per-run closing).
fn serialize_runs(runs: &[Run]) -> String {
    let runs: Vec<&Run> = runs
        .iter()
        .filter(|r| !r.text.is_empty() || r.style.image.is_some())
        .collect();
    let mut out = String::new();
    let mut stack: Vec<Feat> = Vec::new();

    // Length of the maximal consecutive interval of `f` starting at `i`.
    let interval_len = |i: usize, f: &Feat| -> usize {
        runs[i..].iter().take_while(|r| run_has(r, f)).count()
    };

    for (i, &run) in runs.iter().enumerate() {
        if run.style.image.is_some() {
            out.push_str(&serialize_image(run));
            continue;
        }

        // Autolink: a plain link whose visible text is its destination
        // round-trips as <url> (escaping the [text](url) form would
        // mangle backslashes in the URL).
        if let Some(url) = &run.style.link {
            let plain = !run.style.bold
                && !run.style.italic
                && !run.style.strikethrough
                && !run.style.code
                && run.text == *url
                && !url.contains([' ', '<', '>'])
                && interval_len(i, &Feat::Link(url.clone())) == 1
                && !stack.iter().any(|f| matches!(f, Feat::Link(_)));
            if plain {
                // Close any inherited markers first (stack discipline).
                while let Some(f) = stack.pop() {
                    out.push_str(&close_marker(&f));
                }
                out.push('<');
                out.push_str(url);
                out.push('>');
                continue;
            }
        }

        // Close the deepest marker this run doesn't carry — and, stack
        // discipline, everything above it (re-opened below if needed).
        let keep = stack.iter().position(|f| !run_has(run, f)).unwrap_or(stack.len());
        while stack.len() > keep {
            let f = stack.pop().expect("len > keep");
            out.push_str(&close_marker(&f));
        }

        // Open missing markers, longest-lasting first so longer spans
        // nest outermost.
        let mut missing: Vec<Feat> = Vec::new();
        if run.style.bold && !stack.contains(&Feat::Bold) {
            missing.push(Feat::Bold);
        }
        if run.style.italic && !stack.contains(&Feat::Italic) {
            missing.push(Feat::Italic);
        }
        if run.style.strikethrough && !stack.contains(&Feat::Strike) {
            missing.push(Feat::Strike);
        }
        if let Some(url) = &run.style.link {
            let f = Feat::Link(url.clone());
            if !stack.contains(&f) {
                missing.push(f);
            }
        }
        missing.sort_by_key(|f| std::cmp::Reverse(interval_len(i, f)));
        for f in missing {
            out.push_str(&open_marker(&f));
            stack.push(f);
        }

        // Code spans are per-run and innermost; their text is verbatim.
        // Backticks inside the span need a longer fence, and content that
        // starts/ends with a backtick (or is all spaces) needs padding.
        if run.style.html {
            out.push_str(&run.text);
            continue;
        }
        if run.style.code {
            let text = &run.text;
            let longest_tick_run = text
                .split(|c| c != '`')
                .map(str::len)
                .max()
                .unwrap_or(0);
            let fence = "`".repeat(longest_tick_run + 1);
            let all_spaces = !text.is_empty() && text.chars().all(|c| c == ' ');
            let pad = text.starts_with('`')
                || text.ends_with('`')
                || (text.starts_with(' ') && text.ends_with(' ') && !all_spaces);
            out.push_str(&fence);
            if pad {
                out.push(' ');
            }
            out.push_str(text);
            if pad {
                out.push(' ');
            }
            out.push_str(&fence);
        } else {
            out.push_str(&escape_text(&run.text));
        }
    }
    while let Some(f) = stack.pop() {
        out.push_str(&close_marker(&f));
    }
    out
}

/// What a Markdown export cannot represent (used by UI to warn on export).
pub fn lossy_features(doc: &Document) -> Vec<&'static str> {
    let mut lost = Vec::new();
    let any_run = |f: fn(&RunStyle) -> bool| doc.paragraphs.iter().any(|p| p.runs.iter().any(|r| f(&r.style)));
    if any_run(|s| s.highlight) { lost.push("highlight"); }
    if any_run(|s| s.underline) { lost.push("underline"); }
    if doc.paragraphs.iter().any(|p| p.style.alignment != Alignment::Left) { lost.push("alignment"); }
    if doc.paragraphs.iter().any(|p| (p.style.line_spacing - 1.0).abs() > f32::EPSILON) { lost.push("line spacing"); }
    if any_run(|s| s.font_size_hp.is_some()) { lost.push("font size"); }
    if any_run(|s| s.color.is_some()) { lost.push("text color"); }
    if any_run(|s| s.vert_align.is_some()) { lost.push("superscript/subscript"); }
    lost
}
