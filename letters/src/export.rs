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

#[cfg(test)]
mod tests {
    use super::{markdown_to_html, markdown_to_typst, save_typst, typst_to_pdf};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TEMP_COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Unique scratch directory for filesystem tests, cleaned up on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let id = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!(
                "letters-export-test-{name}-{}-{id}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).expect("create temp dir");
            TempDir(dir)
        }

        fn path(&self, file: &str) -> PathBuf {
            self.0.join(file)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    // ── markdown_to_html ──────────────────────────────────────────────────────

    #[test]
    fn markdown_to_html_empty_input_produces_empty_output() {
        assert_eq!(markdown_to_html(""), "");
    }

    #[test]
    fn markdown_to_html_renders_paragraph_with_inline_markup() {
        assert_eq!(
            markdown_to_html("Some *emph* and **bold** text."),
            "<p>Some <em>emph</em> and <strong>bold</strong> text.</p>\n"
        );
    }

    #[test]
    fn markdown_to_html_renders_heading_levels() {
        assert_eq!(
            markdown_to_html("# One\n\n## Two"),
            "<h1>One</h1>\n<h2>Two</h2>\n"
        );
    }

    #[test]
    fn markdown_to_html_renders_bullet_list() {
        assert_eq!(
            markdown_to_html("- alpha\n- beta"),
            "<ul>\n<li>alpha</li>\n<li>beta</li>\n</ul>\n"
        );
    }

    #[test]
    fn markdown_to_html_renders_code_span() {
        assert_eq!(
            markdown_to_html("Use `cargo test` today."),
            "<p>Use <code>cargo test</code> today.</p>\n"
        );
    }

    // ── markdown_to_typst ─────────────────────────────────────────────────────

    #[test]
    fn markdown_to_typst_maps_heading_to_equals_prefix() {
        assert_eq!(markdown_to_typst("# Title"), "= Title\n\n");
    }

    #[test]
    fn markdown_to_typst_maps_heading_levels() {
        assert_eq!(
            markdown_to_typst("# One\n\n## Two\n\n### Three"),
            "= One\n\n== Two\n\n=== Three\n\n"
        );
    }

    #[test]
    fn markdown_to_typst_converts_inline_markup() {
        assert_eq!(
            markdown_to_typst("Some *emph* and **bold** text."),
            "Some _emph_ and *bold* text.\n\n\n"
        );
    }

    #[test]
    fn markdown_to_typst_turns_list_items_into_dashes() {
        assert_eq!(
            markdown_to_typst("- alpha\n- beta"),
            "\n- alpha\n\n- beta\n\n\n"
        );
    }

    #[test]
    fn markdown_to_typst_keeps_inline_code_backticks() {
        assert_eq!(
            markdown_to_typst("Use `cargo test` today."),
            "Use `cargo test` today.\n\n\n"
        );
    }

    #[test]
    fn markdown_to_typst_leaves_no_html_tags_behind() {
        let out = markdown_to_typst(
            "# Title\n\nSome *emph* and `code` and **bold** items:\n\n- one\n- two",
        );
        assert!(
            !out.contains('<'),
            "HTML tag leaked into Typst output: {out:?}"
        );
        assert!(
            !out.contains('>'),
            "HTML tag leaked into Typst output: {out:?}"
        );
    }

    // ── save_typst ────────────────────────────────────────────────────────────

    #[test]
    fn save_typst_writes_header_then_converted_body() {
        let tmp = TempDir::new("save");
        let path = tmp.path("out.typ");
        save_typst("# Title", path.to_str().unwrap()).expect("save succeeds");
        let written = fs::read_to_string(&path).expect("file exists after save");
        assert_eq!(
            written,
            "#set page(width: auto, height: auto, margin: 2cm)\n\
             #set text(font: \"Sans\", size: 11pt)\n\n\
             = Title\n\n"
        );
    }

    #[test]
    fn save_typst_returns_err_on_unwritable_path() {
        let err =
            save_typst("# Title", "/nonexistent-dir-xyz/out.typ").expect_err("bad path must fail");
        assert!(!err.is_empty(), "error message should not be empty");
    }

    // ── typst_to_pdf ──────────────────────────────────────────────────────────

    #[test]
    fn typst_to_pdf_returns_err_for_missing_input() {
        let tmp = TempDir::new("pdf");
        let missing = tmp.path("does-not-exist.typ");
        let err = typst_to_pdf(missing.to_str().unwrap(), "unused.pdf")
            .expect_err("missing input must fail");
        assert!(!err.is_empty(), "error message should not be empty");
    }
}
