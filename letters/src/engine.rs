// engine.rs — Document model and file I/O for Letters.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Architecture:
//   Document — plain-text model (Markdown is the canonical format)
//   read()   — load from file path, detect format
//   write()  — save to file path with format conversion
//   Export PDF via Typst CLI (see export.rs)
//
// DOCX read/write now uses native rdocx crate (no pandoc dependency).
// For formatting-preserving DOCX I/O, use docx_bridge directly on GtkTextBuffer.

use std::fs;
use std::path::Path;
use rdocx::Document as RDocxDoc;

/// A document represented as Markdown text.
pub struct Document {
    pub text: String,
}

impl Document {
    pub fn new() -> Self {
        Self { text: String::new() }
    }

    pub fn from_text(text: &str) -> Self {
        Self { text: text.to_string() }
    }
}

// ── Read ─────────────────────────────────────────────────────────────

/// Read a file into a Document. Supports:
/// - `.md`, `.txt`, `.html` — read as-is (Markdown or raw text)
/// - `.docx` — extracts plain text via rdocx (native, no pandoc)
pub fn read(path: &str) -> Result<Document, String> {
    let p = Path::new(path);
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    match ext.as_str() {
        "md" | "txt" | "html" | "htm" | "" => {
            let text = fs::read_to_string(path)
                .map_err(|e| format!("Cannot read {}: {}", path, e))?;
            Ok(Document::from_text(&text))
        }
        "docx" => {
            let doc = RDocxDoc::open(path)
                .map_err(|e| format!("Cannot open .docx {}: {}", path, e))?;
            let mut text = String::new();
            for p in doc.paragraphs() {
                if !text.is_empty() { text.push('\n'); }
                text.push_str(&p.text());
            }
            Ok(Document::from_text(&text))
        }
        _ => Err(format!("Unsupported file format: .{}", ext)),
    }
}

// ── Write ────────────────────────────────────────────────────────────

/// Write a Document to a file. Format determined by extension:
/// - `.md` — Markdown (canonical)
/// - `.txt` — plain text
/// - `.html` — HTML
/// - `.docx` — creates via rdocx (native, no pandoc)
pub fn write(path: &str, doc: &Document) -> Result<(), String> {
    let p = Path::new(path);
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("md").to_lowercase();
    let text = &doc.text;

    match ext.as_str() {
        "md" | "txt" => {
            fs::write(path, text).map_err(|e| format!("Cannot write {}: {}", path, e))
        }
        "html" | "htm" => {
            use pulldown_cmark::{Parser, html};
            let parser = Parser::new(text);
            let mut html_buf = String::new();
            html::push_html(&mut html_buf, parser);
            let full = format!(
                "<!DOCTYPE html><html><head><meta charset=\"UTF-8\"><title>Document</title></head><body>\n{}\n</body></html>",
                html_buf
            );
            fs::write(path, &full).map_err(|e| format!("Cannot write {}: {}", path, e))
        }
        "docx" => {
            let mut docx = RDocxDoc::new();
            for line in text.lines() {
                docx.add_paragraph(line);
            }
            docx.save(path).map_err(|e| format!("Cannot save .docx {}: {}", path, e))
        }
        "odt" | "rtf" => {
            // Use pandoc CLI (optional fallback)
            let out = std::process::Command::new("pandoc")
                .args(["-f", "markdown", "-t", &ext, "-o", path, "--wrap=none"])
                .stdin(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("pandoc not found (install pandoc for .{} export): {}", ext, e));
            if let Ok(mut child) = out {
                use std::io::Write;
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(text.as_bytes()).map_err(|e| format!("Write error: {}", e))?;
                }
                let status = child.wait().map_err(|e| format!("Wait error: {}", e))?;
                if !status.success() {
                    return Err(format!("pandoc conversion failed (exit: {:?})", status.code()));
                }
                Ok(())
            } else {
                Err(out.unwrap_err())
            }
        }
        _ => Err(format!("Unsupported format: .{}", ext)),
    }
}

// ── Export helpers ───────────────────────────────────────────────────

/// Export document to PDF via Typst CLI.
/// Steps: Markdown → Typst source → `typst compile` → PDF.
pub fn export_pdf(text: &str, output_path: &str) -> Result<(), String> {
    let typst_src = markdown_to_typst(text);
    let tmp_path = format!("{}.typ", output_path);
    fs::write(&tmp_path, &typst_src).map_err(|e| format!("Write typst source: {}", e))?;

    let result = std::process::Command::new("typst")
        .args(["compile", &tmp_path, output_path])
        .output()
        .map_err(|e| format!("typst not found: {}. Install typst CLI for PDF export.", e))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!("typst compile failed: {}", stderr));
    }

    // Cleanup temp file
    let _ = fs::remove_file(&tmp_path);
    Ok(())
}

/// Convert Markdown text to Typst markup.
fn markdown_to_typst(md: &str) -> String {
    use pulldown_cmark::{Parser, html};
    let parser = Parser::new(md);
    let mut html_buf = String::new();
    html::push_html(&mut html_buf, parser);

    let src = format!(
        "#set page(width: auto, height: auto, margin: 2cm)\n\
         #set text(font: \"Sans\", size: 11pt)\n\n{}",
        html_to_typst_simple(&html_buf)
    );
    src
}

/// Simple HTML-to-Typst conversion (inline only, sufficient for our needs).
fn html_to_typst_simple(html: &str) -> String {
    html.replace("<h1>", "= ").replace("</h1>", "\n")
        .replace("<h2>", "== ").replace("</h2>", "\n")
        .replace("<h3>", "=== ").replace("</h3>", "\n")
        .replace("<p>", "").replace("</p>", "\n\n")
        .replace("<strong>", "*").replace("</strong>", "*")
        .replace("<em>", "_").replace("</em>", "_")
        .replace("<ul>", "").replace("</ul>", "")
        .replace("<li>", "- ").replace("</li>", "\n")
        .replace("<ol>", "").replace("</ol>", "")
        .replace("<code>", "`").replace("</code>", "`")
        .replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_roundtrip() {
        let text = "# Hello\n\nThis is **bold** and *italic*.";
        let doc = Document::from_text(text);
        assert_eq!(doc.text, text);
    }

    #[test]
    fn test_markdown_to_typst_basic() {
        let md = "# Title\n\nParagraph with **bold** and *italic*.";
        let typst = markdown_to_typst(md);
        assert!(typst.contains("= Title"));
        assert!(typst.contains("*bold*"));
        assert!(typst.contains("_italic_"));
    }

    #[test]
    fn test_docx_roundtrip() {
        let text = "# Test Title\n\nHello from Antigravity test.";
        let doc = Document::from_text(text);
        
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_doc.docx");
        let path_str = path.to_string_lossy();
        
        // Write docx
        let write_res = write(&path_str, &doc);
        assert!(write_res.is_ok(), "Write docx failed: {:?}", write_res.err());
        
        // Read docx
        let read_res = read(&path_str);
        assert!(read_res.is_ok(), "Read docx failed: {:?}", read_res.err());
        
        let read_doc = read_res.unwrap();
        assert!(read_doc.text.contains("Hello from Antigravity test"));
        
        let _ = std::fs::remove_file(&path);
    }
}
