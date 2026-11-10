// engine.rs — Alternative PDF export through Typst, kept beside the
// page-faithful one (ADR 0010).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// This re-typesets the document's Markdown through Typst: an explicitly
// approximate alternative in the Export menu, never WYSIWYG evidence. What
// Print Layout shows — and what Print, Print Preview and Export as PDF
// write — is the tab's laid-out pages (printing.rs, `Typeset::draw_page`).
//
// Document loading/saving uses letters-core through bridge.rs. Keeping a
// second plain-text save pipeline here caused Save As to discard formatting.

use letters_core::Document;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

// ── Export helpers ───────────────────────────────────────────────────

/// The Typst source for `doc`: its Markdown (the live model, LET-003, not
/// the buffer's presentation text with its list markers and table pipes),
/// typeset on the document's own page size and margins.
pub fn typst_source(doc: &Document) -> String {
    let page = match &doc.page {
        Some(g) => format!(
            "#set page(width: {:.2}pt, height: {:.2}pt, margin: (top: {:.2}pt, bottom: {:.2}pt, left: {:.2}pt, right: {:.2}pt))\n",
            g.width_pt, g.height_pt, g.margin_top_pt, g.margin_bottom_pt, g.margin_left_pt, g.margin_right_pt
        ),
        None => "#set page(margin: 2cm)\n".to_string(),
    };
    format!("{page}#set text(size: 11pt)\n\n{}", markdown_to_typst(&letters_core::markdown::serialize(doc)))
}

/// Compile Typst source to a PDF at `output_path`, in process (#1985): the
/// Flatpaks never shipped a `typst` binary, so the old `typst compile`
/// subprocess failed for every installed user. This is slow on a long
/// document; call it off the main thread.
pub fn export_pdf(typst_src: &str, output_path: &str) -> Result<(), String> {
    suite_export::compile_pdf_to_file(typst_src, output_path)
}

/// Text as Typst markup: every ASCII punctuation mark is escaped, since
/// most of them mean something there (`#` code, `$` math, `//` a comment,
/// `=` or `-` at a line start a heading or list, `<` a label, `@` a
/// reference) and a backslash before any of them is a literal.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A Typst string literal.
fn string_literal(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => {}
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Convert Markdown to Typst markup, from the parser's events rather than
/// by rewriting HTML tags, so text that happens to hold `#`, `$` or `*` is
/// typeset as text instead of failing the compile.
fn markdown_to_typst(md: &str) -> String {
    let mut out = String::new();
    // Ordered (`+`) or bulleted (`-`) for each open list.
    let mut lists: Vec<bool> = Vec::new();
    let mut code: Option<String> = None;
    let mut image_depth = 0usize;
    let line_start = |out: &mut String| {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
    };
    for event in Parser::new_ext(md, Options::ENABLE_STRIKETHROUGH) {
        if let Some(buf) = code.as_mut() {
            match event {
                Event::Text(t) => buf.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    let text = code.take().unwrap_or_default();
                    line_start(&mut out);
                    out.push_str(&format!("#raw(block: true, {})\n\n", string_literal(text.trim_end_matches('\n'))));
                }
                _ => {}
            }
            continue;
        }
        if image_depth > 0 {
            match event {
                Event::Start(Tag::Image { .. }) => image_depth += 1,
                Event::End(TagEnd::Image) => image_depth -= 1,
                Event::Text(t) | Event::Code(t) => out.push_str(&escape(&t)),
                _ => {}
            }
            continue;
        }
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                line_start(&mut out);
                let depth = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                out.push_str(&"=".repeat(depth));
                out.push(' ');
            }
            Event::End(TagEnd::Heading(_)) => out.push_str("\n\n"),
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => out.push_str(if lists.is_empty() { "\n\n" } else { "\n" }),
            Event::Start(Tag::Strong) => out.push_str("#strong["),
            Event::Start(Tag::Emphasis) => out.push_str("#emph["),
            Event::Start(Tag::Strikethrough) => out.push_str("#strike["),
            Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough | TagEnd::Link) => out.push(']'),
            Event::Start(Tag::Link { dest_url, .. }) => out.push_str(&format!("#link({})[", string_literal(&dest_url))),
            Event::Start(Tag::Image { .. }) => image_depth = 1,
            Event::Start(Tag::BlockQuote(_)) => {
                line_start(&mut out);
                out.push_str("#quote(block: true)[\n");
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                line_start(&mut out);
                out.push_str("]\n\n");
            }
            Event::Start(Tag::CodeBlock(_)) => code = Some(String::new()),
            Event::Start(Tag::List(first)) => {
                line_start(&mut out);
                lists.push(first.is_some());
            }
            Event::End(TagEnd::List(_)) => {
                lists.pop();
                if lists.is_empty() {
                    out.push('\n');
                }
            }
            Event::Start(Tag::Item) => {
                line_start(&mut out);
                out.push_str(&"  ".repeat(lists.len().saturating_sub(1)));
                out.push_str(if lists.last() == Some(&true) { "+ " } else { "- " });
            }
            Event::End(TagEnd::Item) => line_start(&mut out),
            Event::Text(t) => out.push_str(&escape(&t)),
            Event::Code(t) => out.push_str(&format!("#raw({})", string_literal(&t))),
            Event::SoftBreak => out.push(' '),
            Event::HardBreak => out.push_str(" \\\n"),
            Event::Rule => {
                line_start(&mut out);
                out.push_str("#line(length: 100%)\n\n");
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compiles(src: &str) -> Vec<u8> {
        suite_export::compile_pdf(src).unwrap_or_else(|e| panic!("{e}\n--- source ---\n{src}"))
    }

    #[test]
    fn headings_emphasis_and_lists_become_typst_markup() {
        let typst = markdown_to_typst("# Title\n\n## Sub\n\nWith **bold** and *italic*.\n\n- a\n- b\n\n1. one\n2. two\n");
        assert!(typst.contains("= Title\n"), "{typst}");
        assert!(typst.contains("== Sub\n"), "{typst}");
        assert!(typst.contains("#strong[bold]"), "{typst}");
        assert!(typst.contains("#emph[italic]"), "{typst}");
        assert!(typst.contains("- a\n- b\n"), "{typst}");
        assert!(typst.contains("+ one\n+ two\n"), "{typst}");
        compiles(&typst);
    }

    /// Text that is Typst syntax must come out as text: before, a `#` or a
    /// `$` in a paragraph went through verbatim and failed the compile.
    #[test]
    fn typst_syntax_in_the_text_is_escaped_and_compiles() {
        let punctuation: String = (0x21u8..0x7f).map(char::from).filter(char::is_ascii_punctuation).collect();
        let md = format!("Price $5 #1 // not a comment, <label> @ref\n\n{}\n\n= not a heading", punctuation.chars().flat_map(|c| ['\\', c]).collect::<String>());
        let typst = markdown_to_typst(&md);
        assert!(typst.contains(r"\$5 \#1 \/\/ not a comment"), "{typst}");
        compiles(&typst);
    }

    #[test]
    fn code_links_quotes_and_rules_compile() {
        let md = "Some `inline \"code\"` here.\n\n```\nfn main() { \"x\\y\" }\n```\n\n> quoted\n\n---\n\n[a link](https://example.org/a?b=c) and ![alt](pic.png)\n\n~~gone~~\n";
        let typst = markdown_to_typst(md);
        assert!(typst.contains("#raw(block: true, \"fn main() { \\\"x\\\\y\\\" }\")"), "{typst}");
        assert!(typst.contains("#link(\"https://example.org/a?b=c\")[a link]"), "{typst}");
        assert!(typst.contains("alt"), "the image's alternative text stays: {typst}");
        assert!(typst.contains("#strike[gone]"), "{typst}");
        compiles(&typst);
    }

    #[test]
    fn nested_lists_indent_under_their_item() {
        let typst = markdown_to_typst("- a\n  - b\n- c\n");
        assert!(typst.contains("- a\n  - b\n- c\n"), "{typst}");
        compiles(&typst);
    }

    /// The source comes from the model, so a table's cells are paragraphs
    /// of text, not the buffer's pipe grid, and the page is the document's.
    #[test]
    fn the_source_is_the_model_on_its_own_page() {
        let mut doc = Document::from_plain_text("Intro");
        doc.insert_table_at(1, 1, 2);
        doc.page = Some(letters_core::model::PageGeometry { width_pt: 612.0, height_pt: 792.0, ..Default::default() });
        let src = typst_source(&doc);
        assert!(src.starts_with("#set page(width: 612.00pt, height: 792.00pt"), "{src}");
        assert!(!src.contains('|'), "{src}");
        compiles(&src);
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("letters-typst-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn export_writes_a_pdf_with_no_typst_binary_and_leaves_no_source_behind() {
        let dir = scratch("ok");
        let out = dir.join("letter.pdf");
        export_pdf(&typst_source(&Document::from_plain_text("Hello #1")), out.to_str().unwrap()).unwrap();
        assert!(std::fs::read(&out).unwrap().starts_with(b"%PDF-"));
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left, [std::ffi::OsString::from("letter.pdf")]);
    }

    #[test]
    fn a_compile_error_is_reported_and_writes_nothing() {
        let out = scratch("bad").join("bad.pdf");
        let err = export_pdf("#nonexistent_function()", out.to_str().unwrap()).unwrap_err();
        assert!(err.contains("typst compile failed"), "{err}");
        assert!(!out.exists());
    }
}
