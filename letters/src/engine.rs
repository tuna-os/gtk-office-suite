// engine.rs — Legacy PDF export helpers for Letters.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Document loading/saving uses letters-core through bridge.rs. Keeping a
// second plain-text save pipeline here caused Save As to discard formatting.

use std::fs;

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
    fn test_markdown_to_typst_basic() {
        let md = "# Title\n\nParagraph with **bold** and *italic*.";
        let typst = markdown_to_typst(md);
        assert!(typst.contains("= Title"));
        assert!(typst.contains("*bold*"));
        assert!(typst.contains("_italic_"));
    }



    #[test]
    fn test_markdown_to_typst_heading_levels() {
        let md = "# H1\n## H2\n### H3";
        let typst = markdown_to_typst(md);
        assert!(typst.contains("= H1"));
        assert!(typst.contains("== H2"));
        assert!(typst.contains("=== H3"));
    }

    #[test]
    fn test_markdown_to_typst_code() {
        let md = "Some \n\n```\ncode block\n```";
        let typst = markdown_to_typst(md);
        assert!(typst.contains("code block"));
    }

    // ── html_to_typst_simple ───────────────────────────────────────────

    #[test]
    fn test_html_to_typst_simple_handles_ordered_lists() {
        let html = "<ol>\n<li>one</li>\n<li>two</li>\n</ol>\n";
        let out = html_to_typst_simple(html);
        assert!(out.contains("- one"), "ordered item one missing: {out:?}");
        assert!(out.contains("- two"), "ordered item two missing: {out:?}");
    }

    #[test]
    fn test_html_to_typst_simple_handles_line_break_variants() {
        for tag in ["<br>", "<br/>", "<br />"] {
            let out = html_to_typst_simple(&format!("line1{tag}line2"));
            assert!(
                out.contains("line1\nline2"),
                "tag {tag:?} not converted to newline: {out:?}"
            );
        }
    }

    #[test]
    fn test_html_to_typst_simple_strips_all_html_tags() {
        let html =
            "<h1>T</h1><p><strong>bold</strong> <em>em</em> <code>c</code></p><ul><li>a</li></ul>";
        let out = html_to_typst_simple(html);
        assert!(!out.contains('<'), "HTML leaked: {out:?}");
        assert!(!out.contains('>'), "HTML leaked: {out:?}");
    }

    // ── export_pdf ─────────────────────────────────────────────────────

    use std::sync::Mutex;

    /// Serializes tests that mutate the process-wide PATH variable.
    static PATH_LOCK: Mutex<()> = Mutex::new(());

    /// Sets PATH for the lifetime of the guard, restoring it on drop.
    struct WithPath {
        saved: String,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl WithPath {
        fn set(path: &str) -> WithPath {
            let _guard = PATH_LOCK.lock().unwrap();
            let saved = std::env::var("PATH").unwrap_or_default();
            std::env::set_var("PATH", path);
            WithPath { saved, _guard }
        }
    }

    impl Drop for WithPath {
        fn drop(&mut self) {
            std::env::set_var("PATH", &self.saved);
        }
    }

    /// Installs a fake `typst` executable and points PATH at its directory.
    fn install_fake_typst(script: &str) -> WithPath {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("letters-fake-typst-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("typst"), script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.join("typst"), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        WithPath::set(dir.to_str().unwrap())
    }

    #[test]
    fn test_export_pdf_errors_when_typst_missing() {
        // Point PATH at an empty directory so `typst` is not found.
        let dir = std::env::temp_dir().join(format!("letters-empty-bin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _path = WithPath::set(dir.to_str().unwrap());

        let out =
            std::env::temp_dir().join(format!("letters-pdf-missing-{}.pdf", std::process::id()));
        let err = export_pdf("# Title", out.to_str().unwrap()).unwrap_err();
        assert!(err.contains("typst not found"), "got: {err}");
    }

    #[test]
    fn test_export_pdf_reports_compile_failure() {
        let _path = install_fake_typst("#!/bin/sh\nexit 1\n");
        let out = std::env::temp_dir().join(format!("letters-pdf-fail-{}.pdf", std::process::id()));
        let err = export_pdf("# Title", out.to_str().unwrap()).unwrap_err();
        assert!(err.contains("typst compile failed"), "got: {err}");
    }

    #[test]
    fn test_export_pdf_success_writes_pdf_and_cleans_temp_source() {
        let _path = install_fake_typst("#!/bin/sh\nprintf '%%PDF-1.4 fake' > \"$3\"\n");
        let out = std::env::temp_dir().join(format!("letters-pdf-ok-{}.pdf", std::process::id()));
        export_pdf("# Title", out.to_str().unwrap()).expect("export succeeds with fake typst");

        let pdf = std::fs::read_to_string(&out).expect("pdf written by fake typst");
        assert!(
            pdf.starts_with("%PDF-1.4"),
            "unexpected pdf content: {pdf:?}"
        );

        let temp_source = std::fs::read_to_string(format!("{}.typ", out.display()));
        assert!(
            temp_source.is_err(),
            "temp .typ should be cleaned up on success"
        );

        let _ = std::fs::remove_file(&out);
    }
}
