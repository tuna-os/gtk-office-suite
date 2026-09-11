// save.rs — the one place that decides what a file name means.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Letters used to resolve the save format inside the GTK bridge with a
// two-arm match and a catch-all:
//
//     "docx" => docx::write(..), "odt" => odt::write(..),
//     _      => markdown bytes
//
// so every other extension silently received Markdown. Saving `notes.txt`
// wrote `# Heading` and `**bold**` into a file the user asked to be plain
// text, and the Preferences window offers "HTML" and "Plain Text" as the
// *default* save format — pick HTML and Save As pre-fills `Untitled.html`,
// which then received Markdown too. That is the extension/content mismatch
// #436 rules out, and it lived one arm away from a correct save.
//
// Dispatch is total here instead: every format Letters offers has a writer,
// an unknown extension is an error rather than a guess, and a format that
// cannot carry what the document holds says so in a `CompatibilityReport`
// built from the document's actual contents rather than a blanket warning.
// It is GTK-free so the policy is testable without a display.

use crate::model::{Alignment, Document, ListKind, Paragraph, Run};
use std::path::Path;
use suite_common_core::interop::{CompatibilityReport, FeatureDisposition, UnsupportedFeature};

/// Every format Letters can write. `ALL` is the list the Preferences
/// window offers, so a format cannot appear in the UI without a writer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveFormat {
    Odt,
    Docx,
    Markdown,
    Html,
    PlainText,
}

impl SaveFormat {
    pub const ALL: [SaveFormat; 5] = [
        SaveFormat::Odt,
        SaveFormat::Docx,
        SaveFormat::Markdown,
        SaveFormat::Html,
        SaveFormat::PlainText,
    ];

    /// The extension a Save As dialog should pre-fill for this format.
    pub fn extension(self) -> &'static str {
        match self {
            SaveFormat::Odt => "odt",
            SaveFormat::Docx => "docx",
            SaveFormat::Markdown => "md",
            SaveFormat::Html => "html",
            SaveFormat::PlainText => "txt",
        }
    }

    /// Untranslated UI label; callers localize.
    pub fn label(self) -> &'static str {
        match self {
            SaveFormat::Odt => "ODT (OpenDocument)",
            SaveFormat::Docx => "DOCX (Office Open XML)",
            SaveFormat::Markdown => "Markdown",
            SaveFormat::Html => "HTML",
            SaveFormat::PlainText => "Plain Text",
        }
    }

    /// Accepts the spellings a user actually types, case-insensitively.
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "odt" => Some(SaveFormat::Odt),
            "docx" => Some(SaveFormat::Docx),
            "md" | "markdown" => Some(SaveFormat::Markdown),
            "html" | "htm" => Some(SaveFormat::Html),
            "txt" | "text" => Some(SaveFormat::PlainText),
            _ => None,
        }
    }
}

/// Resolve the format from the file name. An unrecognised or missing
/// extension is an error the caller can show, not a silent fallback: the
/// old catch-all is what put Markdown inside `.txt` and `.html` files.
pub fn format_for_path(path: &Path) -> Result<SaveFormat, String> {
    let Some(extension) = path.extension() else {
        return Err(format!(
            "\"{}\" has no file extension, so Letters cannot tell which format to write. \
             Add one of: {}.",
            path.display(),
            supported_list()
        ));
    };
    let extension = extension.to_string_lossy();
    SaveFormat::from_extension(&extension).ok_or_else(|| {
        format!(
            "Letters cannot save \".{extension}\" files. Supported formats: {}.",
            supported_list()
        )
    })
}

fn supported_list() -> String {
    SaveFormat::ALL
        .iter()
        .map(|format| format!(".{}", format.extension()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Write `doc` to `path` in the format its extension names, returning what
/// the format could not carry. An `Ok` report may still hold warnings: the
/// bytes are written either way, and it is the caller's decision — see
/// `CompatibilityReport::validate_save` — whether to ask first.
pub fn write(doc: &Document, path: &Path) -> Result<CompatibilityReport, String> {
    let format = format_for_path(path)?;
    let report = compatibility_report(doc, format);
    match format {
        SaveFormat::Odt => crate::odt::write(doc, path)?,
        SaveFormat::Docx => crate::docx::write(doc, path)?,
        SaveFormat::Markdown => {
            write_text(path, &crate::markdown::serialize(doc))?;
        }
        SaveFormat::Html => write_text(path, &to_html(doc))?,
        SaveFormat::PlainText => {
            // A text file conventionally ends in a newline; `to_plain_text`
            // joins paragraphs without a trailing one.
            let mut text = doc.to_plain_text();
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            write_text(path, &text)?;
        }
    }
    Ok(report)
}

/// As [`write`], preserving package members captured on read. Only the
/// package formats carry opaque parts; for the text formats there is
/// nowhere to put them, which the report already records.
pub fn write_with_opaque(
    doc: &Document,
    path: &Path,
    opaque: &suite_common_core::interop::OpaquePackage,
) -> Result<CompatibilityReport, String> {
    let report = write(doc, path)?;
    match format_for_path(path)? {
        SaveFormat::Odt | SaveFormat::Docx => opaque.append_to(path)?,
        _ => {}
    }
    Ok(report)
}

/// Read a document back, by the same rule that decided how to write it.
///
/// Deliberately *permissive* where [`write`] is strict: refusing to open a
/// file because its extension is unfamiliar would be a regression, so an
/// unknown extension is still parsed as Markdown. What it must not do is
/// *misread* a format it recognises — `.txt` used to go through the
/// Markdown parser, so a plain text file containing `**stars**` opened as
/// bold and a `# ` line opened as a heading.
pub fn read(path: &Path) -> Result<Document, String> {
    match path.extension().map(|e| e.to_string_lossy().to_string()).as_deref() {
        Some(extension) if SaveFormat::from_extension(extension) == Some(SaveFormat::Odt) => {
            crate::odt::read(&path.to_string_lossy())
        }
        Some(extension) if SaveFormat::from_extension(extension) == Some(SaveFormat::Docx) => {
            crate::docx::read(&path.to_string_lossy())
        }
        Some(extension) if SaveFormat::from_extension(extension) == Some(SaveFormat::PlainText) => {
            Ok(Document::from_plain_text(&read_to_string(path)?))
        }
        // Markdown, HTML (whose markup the Markdown parser keeps as raw
        // HTML blocks) and anything unrecognised.
        _ => Ok(crate::markdown::parse(&read_to_string(path)?)),
    }
}

fn read_to_string(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))
}

fn write_text(path: &Path, text: &str) -> Result<(), String> {
    suite_common_core::atomic_save::atomic_write_bytes(path, text.as_bytes())
}

// ── Loss reporting ───────────────────────────────────────────────────

/// What the document actually contains. The report is built from this
/// rather than from the format alone, so a plain paragraph of unstyled
/// text saves to `.txt` with nothing to warn about.
#[derive(Default)]
struct Present {
    run_style: bool,
    link: bool,
    image: bool,
    footnote: bool,
    heading: bool,
    list: bool,
    table: bool,
    block_quote: bool,
    code_block: bool,
    alignment: bool,
    page_layout: bool,
    header_footer: bool,
}

fn styled(run: &Run) -> bool {
    let style = &run.style;
    style.bold
        || style.italic
        || style.underline
        || style.strikethrough
        || style.highlight
        || style.code
        || style.font_family.is_some()
        || style.font_size_hp.is_some()
        || style.color.is_some()
        || style.vert_align.is_some()
}

fn laid_out(paragraph: &Paragraph) -> bool {
    let style = &paragraph.style;
    style.line_spacing != 1.0
        || style.space_before_pt != 0.0
        || style.space_after_pt != 0.0
        || style.left_indent_pt != 0.0
        || style.right_indent_pt != 0.0
        || style.first_line_indent_pt != 0.0
        || !style.tab_stops_pt.is_empty()
        || style.page_break_before
        || style.named_style.is_some()
}

fn survey(doc: &Document) -> Present {
    let mut present = Present {
        footnote: !doc.footnotes.is_empty(),
        header_footer: doc.header.is_some() || doc.footer.is_some(),
        page_layout: doc.page.is_some(),
        ..Present::default()
    };
    for paragraph in &doc.paragraphs {
        present.heading |= paragraph.style.heading.is_some();
        present.list |= paragraph.style.list != ListKind::None;
        present.table |= paragraph.style.table_cell.is_some();
        present.block_quote |= paragraph.style.block_quote;
        present.code_block |= paragraph.style.code_block.is_some();
        present.alignment |= paragraph.style.alignment != Alignment::Left;
        present.page_layout |= laid_out(paragraph);
        for run in &paragraph.runs {
            present.run_style |= styled(run);
            present.link |= run.style.link.is_some();
            present.image |= run.style.image.is_some();
            present.footnote |= run.style.footnote.is_some();
        }
    }
    present
}

fn lost(
    report: &mut CompatibilityReport,
    present: bool,
    id: &str,
    label: &str,
    detail: &str,
) {
    if present {
        report.record(UnsupportedFeature::new(
            id,
            label,
            "document",
            FeatureDisposition::WarnOnLoss,
            detail,
        ));
    }
}

/// What each format drops, keyed off what this document holds. Every entry
/// names something the writer below genuinely does not emit — a warning
/// the output contradicts is worse than none.
pub fn compatibility_report(doc: &Document, format: SaveFormat) -> CompatibilityReport {
    let mut report = CompatibilityReport::new(format.extension());
    let present = survey(doc);
    match format {
        // The package formats are the reference targets: they carry the
        // whole model, and opaque parts on top of it.
        SaveFormat::Odt | SaveFormat::Docx => {}
        SaveFormat::Markdown => {
            lost(&mut report, present.page_layout, "page-layout", "Page size, margins and paragraph spacing", "Markdown has no page geometry or point-precise spacing");
            lost(&mut report, present.header_footer, "header-footer", "Page header and footer", "Markdown has no running header or footer");
            lost(&mut report, present.run_style && doc.paragraphs.iter().flat_map(|p| &p.runs).any(|r| r.style.font_family.is_some() || r.style.font_size_hp.is_some() || r.style.color.is_some()), "character-appearance", "Font family, size and colour", "Markdown carries emphasis but not typeface, size or colour");
        }
        SaveFormat::Html => {
            lost(&mut report, present.page_layout, "page-layout", "Page size, margins and paragraph spacing", "the HTML written here has no page geometry or point-precise spacing");
            lost(&mut report, present.header_footer, "header-footer", "Page header and footer", "HTML has no running header or footer");
        }
        SaveFormat::PlainText => {
            lost(&mut report, present.run_style, "character-formatting", "Bold, italic and other character formatting", "plain text holds characters only");
            lost(&mut report, present.heading, "headings", "Heading levels", "plain text has no heading structure");
            lost(&mut report, present.list, "lists", "Bulleted and numbered lists", "list markers are not written back");
            lost(&mut report, present.table, "tables", "Tables", "cells are written as consecutive lines");
            lost(&mut report, present.block_quote, "block-quotes", "Block quotes", "quote markers are not written back");
            lost(&mut report, present.code_block, "code-blocks", "Code blocks", "fences and language labels are not written back");
            lost(&mut report, present.alignment, "alignment", "Paragraph alignment", "plain text has no alignment");
            lost(&mut report, present.link, "links", "Link targets", "only the link text is kept");
            lost(&mut report, present.image, "images", "Images", "only the alternative text is kept");
            lost(&mut report, present.footnote, "footnotes", "Footnotes", "footnote text is not written back");
            lost(&mut report, present.page_layout, "page-layout", "Page size, margins and paragraph spacing", "plain text has no page geometry");
            lost(&mut report, present.header_footer, "header-footer", "Page header and footer", "plain text has no running header or footer");
        }
    }
    report
}

// ── HTML ─────────────────────────────────────────────────────────────

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(character),
        }
    }
    out
}

fn span_style(run: &Run) -> String {
    let mut declarations = Vec::new();
    if let Some(family) = &run.style.font_family {
        declarations.push(format!("font-family:{}", escape(family)));
    }
    if let Some(half_points) = run.style.font_size_hp {
        declarations.push(format!("font-size:{}pt", f64::from(half_points) / 2.0));
    }
    if let Some(color) = &run.style.color {
        declarations.push(format!("color:#{}", escape(color)));
    }
    declarations.join(";")
}

fn run_html(run: &Run, footnote_numbers: &mut Vec<usize>) -> String {
    // A raw-HTML run is the document's own markup; escaping it here would
    // turn the user's `<br>` into visible text.
    if run.style.html {
        return run.text.clone();
    }
    if let Some(source) = &run.style.image {
        return format!(
            "<img src=\"{}\" alt=\"{}\">",
            escape(source),
            escape(&run.text)
        );
    }
    if let Some(index) = run.style.footnote {
        let number = footnote_numbers.len() + 1;
        footnote_numbers.push(index);
        return format!(
            "<sup id=\"fnref{number}\"><a href=\"#fn{number}\">{number}</a></sup>"
        );
    }
    let mut html = escape(&run.text);
    if run.style.code {
        html = format!("<code>{html}</code>");
    }
    if run.style.bold {
        html = format!("<strong>{html}</strong>");
    }
    if run.style.italic {
        html = format!("<em>{html}</em>");
    }
    if run.style.underline {
        html = format!("<u>{html}</u>");
    }
    if run.style.strikethrough {
        html = format!("<s>{html}</s>");
    }
    if run.style.highlight {
        html = format!("<mark>{html}</mark>");
    }
    match run.style.vert_align {
        Some(crate::model::VertAlign::Superscript) => html = format!("<sup>{html}</sup>"),
        Some(crate::model::VertAlign::Subscript) => html = format!("<sub>{html}</sub>"),
        None => {}
    }
    if let Some(url) = &run.style.link {
        html = format!("<a href=\"{}\">{html}</a>", escape(url));
    }
    let style = span_style(run);
    if !style.is_empty() {
        html = format!("<span style=\"{style}\">{html}</span>");
    }
    html
}

fn inline_html(paragraph: &Paragraph, footnote_numbers: &mut Vec<usize>) -> String {
    paragraph
        .runs
        .iter()
        .map(|run| run_html(run, footnote_numbers))
        .collect()
}

fn alignment_attribute(paragraph: &Paragraph) -> &'static str {
    match paragraph.style.alignment {
        Alignment::Left => "",
        Alignment::Center => " style=\"text-align:center\"",
        Alignment::Right => " style=\"text-align:right\"",
        Alignment::Justify => " style=\"text-align:justify\"",
    }
}

/// Serialize the document as a standalone HTML file.
///
/// Runs of paragraphs that form one structure in the model — a list, a
/// code block, a table — become one element here, the same grouping the
/// ODT and DOCX writers use, rather than a paragraph each.
pub fn to_html(doc: &Document) -> String {
    let mut body = String::new();
    let mut footnote_numbers: Vec<usize> = Vec::new();
    let paragraphs = &doc.paragraphs;
    let mut index = 0;
    while index < paragraphs.len() {
        let paragraph = &paragraphs[index];
        let style = &paragraph.style;

        if let Some(cell) = style.table_cell {
            let start = index;
            while index < paragraphs.len()
                && paragraphs[index].style.table_cell.map(|c| c.table) == Some(cell.table)
            {
                index += 1;
            }
            body.push_str(&table_html(&paragraphs[start..index], &mut footnote_numbers));
            continue;
        }

        if let Some(language) = &style.code_block {
            let start = index;
            while index < paragraphs.len()
                && paragraphs[index].style.code_block.as_ref() == Some(language)
            {
                index += 1;
            }
            let text = paragraphs[start..index]
                .iter()
                .map(|p| escape(&p.text()))
                .collect::<Vec<_>>()
                .join("\n");
            let class = if language.is_empty() {
                String::new()
            } else {
                format!(" class=\"language-{}\"", escape(language))
            };
            body.push_str(&format!("<pre><code{class}>{text}</code></pre>\n"));
            continue;
        }

        if style.html_block {
            let start = index;
            while index < paragraphs.len() && paragraphs[index].style.html_block {
                index += 1;
            }
            for p in &paragraphs[start..index] {
                body.push_str(&p.text());
                body.push('\n');
            }
            continue;
        }

        if style.list != ListKind::None {
            let start = index;
            while index < paragraphs.len() && paragraphs[index].style.list != ListKind::None {
                index += 1;
            }
            body.push_str(&list_html(&paragraphs[start..index], &mut footnote_numbers));
            continue;
        }

        if style.block_quote {
            let start = index;
            while index < paragraphs.len() && paragraphs[index].style.block_quote {
                index += 1;
            }
            body.push_str("<blockquote>\n");
            for p in &paragraphs[start..index] {
                body.push_str(&format!("<p>{}</p>\n", inline_html(p, &mut footnote_numbers)));
            }
            body.push_str("</blockquote>\n");
            continue;
        }

        let inner = inline_html(paragraph, &mut footnote_numbers);
        match style.heading {
            // The model admits 1..=6; anything else would produce an
            // element that is not HTML, so it degrades to a paragraph.
            Some(level) if (1..=6).contains(&level) => {
                body.push_str(&format!(
                    "<h{level}{}>{inner}</h{level}>\n",
                    alignment_attribute(paragraph)
                ));
            }
            _ => body.push_str(&format!(
                "<p{}>{inner}</p>\n",
                alignment_attribute(paragraph)
            )),
        }
        index += 1;
    }

    if !footnote_numbers.is_empty() {
        body.push_str("<hr>\n<ol class=\"footnotes\">\n");
        for (position, source) in footnote_numbers.iter().enumerate() {
            let number = position + 1;
            let text = doc.footnotes.get(*source).map(String::as_str).unwrap_or("");
            body.push_str(&format!(
                "<li id=\"fn{number}\">{} <a href=\"#fnref{number}\">\u{21a9}</a></li>\n",
                escape(text)
            ));
        }
        body.push_str("</ol>\n");
    }

    format!(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n\
         <title>{}</title>\n</head>\n<body>\n{body}</body>\n</html>\n",
        escape(&document_title(doc))
    )
}

/// The first heading, or the first line of text: a `<title>` is required
/// and an empty one is worse than a guess the user can see and change.
fn document_title(doc: &Document) -> String {
    doc.paragraphs
        .iter()
        .find(|p| p.style.heading.is_some() && !p.text().trim().is_empty())
        .or_else(|| doc.paragraphs.iter().find(|p| !p.text().trim().is_empty()))
        .map(|p| p.text().trim().to_string())
        .unwrap_or_default()
}

fn table_html(group: &[Paragraph], footnote_numbers: &mut Vec<usize>) -> String {
    let rows = group
        .iter()
        .filter_map(|p| p.style.table_cell.map(|c| c.row))
        .max()
        .unwrap_or(0);
    let columns = group
        .iter()
        .filter_map(|p| p.style.table_cell.map(|c| c.col))
        .max()
        .unwrap_or(0);
    let mut html = String::from("<table>\n");
    for row in 0..=rows {
        html.push_str("<tr>");
        for column in 0..=columns {
            // Several paragraphs can share one cell; they become several
            // <p> inside it rather than several cells.
            let cell: String = group
                .iter()
                .filter(|p| p.style.table_cell == Some(crate::model::TableCell { table: group[0].style.table_cell.expect("grouped by table_cell").table, row, col: column }))
                .map(|p| inline_html(p, footnote_numbers))
                .collect::<Vec<_>>()
                .join("<br>");
            html.push_str(&format!("<td>{cell}</td>"));
        }
        html.push_str("</tr>\n");
    }
    html.push_str("</table>\n");
    html
}

fn list_html(group: &[Paragraph], footnote_numbers: &mut Vec<usize>) -> String {
    fn emit(
        group: &[Paragraph],
        at: &mut usize,
        level: u8,
        footnote_numbers: &mut Vec<usize>,
    ) -> String {
        let kind = group[*at].style.list;
        let tag = if kind == ListKind::Numbered { "ol" } else { "ul" };
        let start = group[*at].style.list_start.filter(|first| *first != 1);
        let mut html = match start {
            Some(first) => format!("<{tag} start=\"{first}\">\n"),
            None => format!("<{tag}>\n"),
        };
        while *at < group.len() {
            let paragraph = &group[*at];
            if paragraph.style.list_level < level || paragraph.style.list != kind {
                break;
            }
            if paragraph.style.list_level > level {
                // A deeper item nests inside the item just written, which
                // is what an HTML reader expects; the model only records
                // the depth.
                let nested = emit(group, at, paragraph.style.list_level, footnote_numbers);
                html.push_str(&nested);
                continue;
            }
            html.push_str(&format!(
                "<li>{}</li>\n",
                inline_html(paragraph, footnote_numbers)
            ));
            *at += 1;
        }
        html.push_str(&format!("</{tag}>\n"));
        html
    }

    let mut at = 0;
    let mut html = String::new();
    while at < group.len() {
        let level = group[at].style.list_level;
        html.push_str(&emit(group, &mut at, level, footnote_numbers));
    }
    html
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ParaStyle, RunStyle, TableCell, VertAlign};

    fn heading(level: u8, text: &str) -> Paragraph {
        Paragraph {
            style: ParaStyle { heading: Some(level), ..Default::default() },
            runs: vec![Run::plain(text)],
        }
    }

    fn styled_run(text: &str, style: RunStyle) -> Paragraph {
        Paragraph { style: ParaStyle::default(), runs: vec![Run { text: text.into(), style }] }
    }

    /// A heading and a bold run: the smallest document whose Markdown
    /// serialization is visibly not plain text.
    fn formatted() -> Document {
        let mut doc = Document::from_plain_text("");
        doc.paragraphs = vec![
            heading(1, "Quarterly report"),
            Paragraph {
                style: ParaStyle::default(),
                runs: vec![
                    Run { text: "bold".into(), style: RunStyle { bold: true, ..Default::default() } },
                    Run::plain(" and plain"),
                ],
            },
        ];
        doc
    }

    /// The #436 regression. Letters offers `.txt` in its Save As filter and
    /// "Plain Text" as a default format, and the old catch-all gave both
    /// Markdown: `# Quarterly report` and `**bold**` inside a file the user
    /// asked to be plain text.
    #[test]
    fn saving_as_plain_text_writes_text_and_not_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.txt");
        let report = write(&formatted(), &path).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();

        assert_eq!(written, "Quarterly report\nbold and plain\n");
        assert!(!written.contains('#'), "heading markup leaked into .txt: {written:?}");
        assert!(!written.contains("**"), "emphasis markup leaked into .txt: {written:?}");
        // And the loss is reported rather than silent.
        let lost: Vec<&str> = report.features.iter().map(|f| f.id.as_str()).collect();
        assert!(lost.contains(&"headings"), "{lost:?}");
        assert!(lost.contains(&"character-formatting"), "{lost:?}");
        assert!(report.requires_confirmation());
    }

    /// The same defect through the Preferences window: choosing "HTML" as
    /// the default format pre-fills `Untitled.html`, which used to receive
    /// Markdown bytes.
    #[test]
    fn saving_as_html_writes_html_and_not_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("page.html");
        write(&formatted(), &path).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();

        assert!(written.starts_with("<!DOCTYPE html>"), "{written}");
        assert!(written.contains("<h1>Quarterly report</h1>"), "{written}");
        assert!(written.contains("<strong>bold</strong> and plain"), "{written}");
        assert!(!written.contains("**bold**"), "markdown leaked into .html: {written}");
        assert!(written.contains("<title>Quarterly report</title>"), "{written}");
    }

    /// Plain text with no formatting has nothing to warn about — a blanket
    /// warning on every `.txt` save would train the user to dismiss it.
    #[test]
    fn an_unformatted_document_saves_to_text_without_warnings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plain.txt");
        let report = write(&Document::from_plain_text("just words\nand more"), &path).unwrap();
        assert!(report.features.is_empty(), "{:?}", report.features);
        assert!(!report.needs_save_warning());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "just words\nand more\n");
    }

    /// The package formats are the reference targets and lose nothing.
    #[test]
    fn the_package_formats_report_no_loss() {
        for format in [SaveFormat::Odt, SaveFormat::Docx] {
            let report = compatibility_report(&formatted(), format);
            assert!(report.features.is_empty(), "{format:?}: {:?}", report.features);
        }
    }

    /// An extension with no writer is an error, and nothing is written:
    /// the old behavior put Markdown bytes under whatever name was given.
    #[test]
    fn an_unsupported_extension_is_refused_without_writing_anything() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["report.rtf", "report.pdf", "report.wpd", "report"] {
            let path = dir.path().join(name);
            let error = write(&formatted(), &path).unwrap_err();
            assert!(error.contains(".odt"), "{name}: {error}");
            assert!(!path.exists(), "{name} was written anyway");
        }
    }

    /// The Preferences window builds its list from `SaveFormat::ALL`, so
    /// this is what stops a format appearing there without a writer — the
    /// shape of the original defect, where "HTML" and "RTF" were offered
    /// and neither had one.
    #[test]
    fn every_offered_format_resolves_from_its_own_extension() {
        for format in SaveFormat::ALL {
            assert_eq!(
                SaveFormat::from_extension(format.extension()),
                Some(format),
                "{format:?} is offered but its extension does not resolve"
            );
            let path = std::path::PathBuf::from(format!("doc.{}", format.extension()));
            assert_eq!(format_for_path(&path), Ok(format));
        }
    }

    #[test]
    fn extensions_are_matched_case_insensitively_and_by_alias() {
        for (name, expected) in [
            ("A.ODT", SaveFormat::Odt),
            ("a.DocX", SaveFormat::Docx),
            ("a.markdown", SaveFormat::Markdown),
            ("a.MD", SaveFormat::Markdown),
            ("a.htm", SaveFormat::Html),
            ("a.text", SaveFormat::PlainText),
        ] {
            assert_eq!(format_for_path(Path::new(name)), Ok(expected), "{name}");
        }
    }

    /// Every format now reaches the byte writer with a `Path`, so a name a
    /// user can create but Rust cannot call a `&str` saves like any other.
    #[cfg(unix)]
    #[test]
    fn a_non_utf8_file_name_saves_in_every_text_format() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let dir = tempfile::tempdir().unwrap();
        for format in [SaveFormat::Markdown, SaveFormat::Html, SaveFormat::PlainText] {
            let mut name = OsString::from_vec(b"r\xffport".to_vec());
            name.push(format!(".{}", format.extension()));
            let path = dir.path().join(&name);
            write(&formatted(), &path).unwrap_or_else(|e| panic!("{format:?}: {e}"));
            assert!(!std::fs::read(&path).unwrap().is_empty(), "{format:?}");
        }
    }

    /// The read side had the same catch-all: `.txt` went through the
    /// Markdown parser, so a plain text file that happens to contain
    /// `**stars**` or a leading `#` opened as bold and a heading. Text
    /// saved as text must come back as the same text.
    #[test]
    fn a_plain_text_file_is_read_as_text_and_not_as_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("literal.txt");
        std::fs::write(&path, "# not a heading\n**not bold** 2 * 3\n").unwrap();

        let doc = read(&path).unwrap();
        assert_eq!(doc.to_plain_text(), "# not a heading\n**not bold** 2 * 3\n");
        assert!(doc.paragraphs.iter().all(|p| p.style.heading.is_none()));
        assert!(doc
            .paragraphs
            .iter()
            .flat_map(|p| &p.runs)
            .all(|run| !run.style.bold));
    }

    #[test]
    fn a_plain_text_save_round_trips_through_the_reader() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("round.txt");
        write(&formatted(), &path).unwrap();
        // Formatting is gone — that is what the report said — but every
        // character the user could see survives.
        assert_eq!(read(&path).unwrap().to_plain_text(), "Quarterly report\nbold and plain\n");
    }

    #[test]
    fn markdown_and_the_packages_still_read_back_through_the_same_entry_point() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["out.md", "out.odt", "out.docx"] {
            let path = dir.path().join(name);
            write(&formatted(), &path).unwrap();
            let back = read(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(back.to_plain_text().contains("bold and plain"), "{name}");
            assert!(
                back.paragraphs.iter().any(|p| p.style.heading == Some(1)),
                "{name}: the heading did not survive"
            );
        }
    }

    /// An unfamiliar extension must still open — refusing would be a
    /// regression, and this is where read and write deliberately differ.
    #[test]
    fn an_unknown_extension_still_opens_rather_than_being_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.journal");
        std::fs::write(&path, "# a heading\n").unwrap();
        let doc = read(&path).unwrap();
        assert_eq!(doc.paragraphs[0].style.heading, Some(1));
        // ...while writing that same name is refused.
        assert!(write(&doc, &path).is_err());
    }

    #[test]
    fn html_escapes_text_but_not_a_raw_html_run() {
        let mut doc = Document::from_plain_text("");
        doc.paragraphs = vec![
            styled_run("a < b & \"c\"", RunStyle::default()),
            styled_run("<br>", RunStyle { html: true, ..Default::default() }),
        ];
        let html = to_html(&doc);
        assert!(html.contains("a &lt; b &amp; &quot;c&quot;"), "{html}");
        assert!(html.contains("<p><br></p>"), "{html}");
    }

    #[test]
    fn html_carries_links_images_and_vertical_alignment() {
        let mut doc = Document::from_plain_text("");
        doc.paragraphs = vec![Paragraph {
            style: ParaStyle::default(),
            runs: vec![
                Run {
                    text: "a link".into(),
                    style: RunStyle { link: Some("https://example.invalid/x?a=1&b=2".into()), ..Default::default() },
                },
                Run {
                    text: "a chart".into(),
                    style: RunStyle { image: Some("chart.png".into()), ..Default::default() },
                },
                Run {
                    text: "2".into(),
                    style: RunStyle { vert_align: Some(VertAlign::Superscript), ..Default::default() },
                },
            ],
        }];
        let html = to_html(&doc);
        assert!(html.contains("<a href=\"https://example.invalid/x?a=1&amp;b=2\">a link</a>"), "{html}");
        assert!(html.contains("<img src=\"chart.png\" alt=\"a chart\">"), "{html}");
        assert!(html.contains("<sup>2</sup>"), "{html}");
    }

    #[test]
    fn html_groups_a_list_and_nests_a_deeper_item() {
        let mut doc = Document::from_plain_text("");
        let item = |text: &str, level: u8| Paragraph {
            style: ParaStyle { list: ListKind::Bullet, list_level: level, ..Default::default() },
            runs: vec![Run::plain(text)],
        };
        doc.paragraphs = vec![item("one", 0), item("one-a", 1), item("two", 0)];
        let html = to_html(&doc);
        // One outer list, one nested list, three items in total.
        assert_eq!(html.matches("<ul>").count(), 2, "{html}");
        assert_eq!(html.matches("<li>").count(), 3, "{html}");
        assert!(html.contains("<li>one</li>\n<ul>\n<li>one-a</li>"), "{html}");
    }

    #[test]
    fn html_groups_a_numbered_list_and_keeps_its_start() {
        let mut doc = Document::from_plain_text("");
        doc.paragraphs = vec![Paragraph {
            style: ParaStyle { list: ListKind::Numbered, list_start: Some(4), ..Default::default() },
            runs: vec![Run::plain("fourth")],
        }];
        let html = to_html(&doc);
        assert!(html.contains("<ol start=\"4\">"), "{html}");
    }

    #[test]
    fn html_rebuilds_a_table_from_cell_tagged_paragraphs() {
        let mut doc = Document::from_plain_text("");
        doc.paragraphs = (0..2)
            .flat_map(|row| {
                (0..2).map(move |col| Paragraph {
                    style: ParaStyle {
                        table_cell: Some(TableCell { table: 1, row, col }),
                        ..Default::default()
                    },
                    runs: vec![Run::plain(format!("r{row}c{col}"))],
                })
            })
            .collect();
        let html = to_html(&doc);
        assert_eq!(html.matches("<table>").count(), 1, "{html}");
        assert_eq!(html.matches("<tr>").count(), 2, "{html}");
        assert!(html.contains("<td>r0c0</td><td>r0c1</td>"), "{html}");
        assert!(html.contains("<td>r1c0</td><td>r1c1</td>"), "{html}");
    }

    #[test]
    fn html_joins_a_code_block_into_one_pre() {
        let mut doc = Document::from_plain_text("");
        doc.paragraphs = ["fn main() {", "    // <not markup>", "}"]
            .iter()
            .map(|line| Paragraph {
                style: ParaStyle { code_block: Some("rust".into()), ..Default::default() },
                runs: vec![Run::plain(*line)],
            })
            .collect();
        let html = to_html(&doc);
        assert_eq!(html.matches("<pre>").count(), 1, "{html}");
        assert!(html.contains("<code class=\"language-rust\">"), "{html}");
        assert!(html.contains("    // &lt;not markup&gt;"), "{html}");
    }

    #[test]
    fn html_numbers_footnotes_and_links_them_both_ways() {
        let mut doc = Document::from_plain_text("");
        doc.footnotes = vec!["the first note".into(), "the second note".into()];
        doc.paragraphs = vec![Paragraph {
            style: ParaStyle::default(),
            runs: vec![
                Run::plain("text"),
                Run { text: String::new(), style: RunStyle { footnote: Some(1), ..Default::default() } },
                Run { text: String::new(), style: RunStyle { footnote: Some(0), ..Default::default() } },
            ],
        }];
        let html = to_html(&doc);
        // Numbered by order of appearance, not by model index.
        assert!(html.contains("<sup id=\"fnref1\"><a href=\"#fn1\">1</a></sup>"), "{html}");
        assert!(html.contains("<li id=\"fn1\">the second note"), "{html}");
        assert!(html.contains("<li id=\"fn2\">the first note"), "{html}");
    }

    /// A heading level the model admits but HTML does not must not become
    /// an `<h9>` element.
    #[test]
    fn an_out_of_range_heading_level_degrades_to_a_paragraph() {
        let mut doc = Document::from_plain_text("");
        doc.paragraphs = vec![heading(9, "too deep")];
        let html = to_html(&doc);
        assert!(html.contains("<p>too deep</p>"), "{html}");
        assert!(!html.contains("<h9"), "{html}");
    }

    /// Every warning must name something the writer really drops. This is
    /// the cheap version of that check: HTML keeps emphasis and headings,
    /// so neither may be reported lost.
    #[test]
    fn html_does_not_warn_about_formatting_it_keeps() {
        let report = compatibility_report(&formatted(), SaveFormat::Html);
        let ids: Vec<&str> = report.features.iter().map(|f| f.id.as_str()).collect();
        assert!(!ids.contains(&"headings"), "{ids:?}");
        assert!(!ids.contains(&"character-formatting"), "{ids:?}");
    }

    /// Page geometry and a running header have nowhere to go in any of the
    /// text formats, and that is worth saying.
    #[test]
    fn page_layout_and_headers_are_reported_lost_by_the_text_formats() {
        let mut doc = formatted();
        doc.header = Some("Confidential".into());
        doc.page = Some(crate::model::PageGeometry::default());
        for format in [SaveFormat::Markdown, SaveFormat::Html, SaveFormat::PlainText] {
            let ids: Vec<String> = compatibility_report(&doc, format)
                .features
                .iter()
                .map(|f| f.id.clone())
                .collect();
            assert!(ids.iter().any(|id| id == "page-layout"), "{format:?}: {ids:?}");
            assert!(ids.iter().any(|id| id == "header-footer"), "{format:?}: {ids:?}");
        }
    }

    /// Opaque parts only exist in the package formats; asking to preserve
    /// them while saving as text must not fail the save.
    #[test]
    fn saving_as_text_with_opaque_parts_captured_still_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let package = dir.path().join("source.odt");
        crate::odt::write(&formatted(), &package).unwrap();
        let opaque = suite_common_core::interop::OpaquePackage::capture(
            &package,
            &["mimetype", "META-INF/manifest.xml", "content.xml", "styles.xml"],
        )
        .unwrap();
        let text = dir.path().join("out.txt");
        write_with_opaque(&formatted(), &text, &opaque).unwrap();
        assert_eq!(std::fs::read_to_string(&text).unwrap(), "Quarterly report\nbold and plain\n");
    }

    /// Round trip through the package writer to prove `write` really wrote
    /// a package and not text with a package extension.
    #[test]
    fn the_package_formats_are_readable_by_their_own_readers() {
        let dir = tempfile::tempdir().unwrap();
        for (name, read) in [
            ("out.odt", crate::odt::read as fn(&str) -> Result<Document, String>),
            ("out.docx", crate::docx::read as fn(&str) -> Result<Document, String>),
        ] {
            let path = dir.path().join(name);
            write(&formatted(), &path).unwrap();
            let back = read(path.to_str().unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(
                back.to_plain_text().contains("bold and plain"),
                "{name}: {:?}",
                back.to_plain_text()
            );
        }
    }
}
