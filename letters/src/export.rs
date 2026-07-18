// export.rs — Document export: Markdown → Typst, Markdown → PDF (via typst CLI).
// SPDX-License-Identifier: GPL-3.0-or-later

use pulldown_cmark::{Parser, html};

pub fn markdown_to_html(md: &str) -> String {
    let parser = Parser::new(md);
    let mut buf = String::new();
    html::push_html(&mut buf, parser);
    buf
}

pub fn markdown_to_typst(md: &str) -> String {
    let html = markdown_to_html(md);
    html.replace("<h1>", "= ").replace("</h1>", "\n")
        .replace("<h2>", "== ").replace("</h2>", "\n")
        .replace("<h3>", "=== ").replace("</h3>", "\n")
        .replace("<p>", "").replace("</p>", "\n\n")
        .replace("<strong>", "*").replace("</strong>", "*")
        .replace("<em>", "_").replace("</em>", "_")
        .replace("<ul>", "").replace("</ul>", "")
        .replace("<li>", "- ").replace("</li>", "\n")
        .replace("<code>", "`").replace("</code>", "`")
}

pub fn save_typst(text: &str, path: &str) -> Result<(), String> {
    let src = format!("#set page(width: auto, height: auto, margin: 2cm)\n#set text(font: \"Sans\", size: 11pt)\n\n{}", markdown_to_typst(text));
    std::fs::write(path, &src).map_err(|e| format!("{}", e))
}

/// Compile a Typst source file to PDF via the in-process engine.
pub fn typst_to_pdf(input: &str, output: &str) -> Result<(), String> {
    let src = std::fs::read_to_string(input).map_err(|e| format!("{}", e))?;
    suite_export::compile_pdf_to_file(&src, output)
}
