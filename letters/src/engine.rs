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

pub use letters_core::typst_export::typst_source;

/// Compile Typst source to a PDF at `output_path`, in process (#1985): the
/// Flatpaks never shipped a `typst` binary, so the old `typst compile`
/// subprocess failed for every installed user. This is slow on a long
/// document; call it off the main thread.
pub fn export_pdf(typst_src: &str, output_path: &str) -> Result<(), String> {
    suite_export::compile_pdf_to_file(typst_src, output_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use letters_core::Document;

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
