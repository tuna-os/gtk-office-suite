// suite-export — Typst source → PDF, in-process.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Replaces shelling out to a `typst` CLI that the Flatpaks never bundled.
// Apps generate Typst source (their own to_typst functions) and call
// compile_pdf; fonts come from typst-as-lib's embedded set.

use typst_as_lib::typst_kit_options::TypstKitFontOptions;
use typst_as_lib::TypstEngine;

/// Compile Typst source to PDF bytes.
pub fn compile_pdf(source: &str) -> Result<Vec<u8>, String> {
    let engine = TypstEngine::builder()
        .main_file(source.to_string())
        .search_fonts_with(TypstKitFontOptions::default())
        .build();
    let doc = engine
        .compile()
        .output
        .map_err(|e| format!("typst compile failed: {:?}", e))?;
    typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default())
        .map_err(|e| format!("pdf export failed: {:?}", e))
}

/// Compile Typst source and write the PDF to a path.
///
/// The write goes through `atomic_write_bytes` for the same reason every
/// document writer does: exporting over an existing PDF used to truncate it
/// with `fs::write` and then fill it in, so a compile that ran out of disk —
/// or a crash mid-write — destroyed the previous export and left a corrupt
/// file in its place. This was the only format writer in the workspace still
/// bypassing it (#437).
pub fn compile_pdf_to_file(source: &str, path: &str) -> Result<(), String> {
    let bytes = compile_pdf(source)?;
    suite_common_core::atomic_save::atomic_write_bytes(std::path::Path::new(path), &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_minimal_document_to_valid_pdf() {
        let pdf = compile_pdf("Hello, *world*.").expect("compile");
        assert!(pdf.starts_with(b"%PDF-"), "output is not a PDF");
        assert!(pdf.len() > 500, "suspiciously small PDF");
    }

    #[test]
    fn compiles_table_syntax() {
        let src = "#table(columns: 2, [a], [b], [c], [d])";
        let pdf = compile_pdf(src).expect("compile table");
        assert!(pdf.starts_with(b"%PDF-"));
    }

    #[test]
    fn compile_error_is_reported_not_panicked() {
        let err = compile_pdf("#nonexistent_function()").unwrap_err();
        assert!(err.contains("typst compile failed"));
    }

    /// Exporting over an existing PDF must never leave the old one truncated.
    /// `fs::write` opens with O_TRUNC, so a failure after that point destroyed
    /// the previous export; `atomic_write_bytes` writes elsewhere and renames.
    #[test]
    fn export_replaces_an_existing_pdf_without_truncating_it_first() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.pdf");
        std::fs::write(&path, b"%PDF-1.7 previous export").unwrap();

        compile_pdf_to_file("Replacement.", path.to_str().unwrap()).expect("export");

        let written = std::fs::read(&path).unwrap();
        assert!(written.starts_with(b"%PDF-"), "result is not a PDF");
        assert!(written.len() > 500, "suspiciously small: {}", written.len());
        assert_ne!(written, b"%PDF-1.7 previous export", "file was not replaced");
    }

    /// A failed compile must leave the previous export exactly as it was —
    /// the destination should not be opened at all until there are bytes.
    #[test]
    fn a_failed_compile_leaves_the_previous_export_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.pdf");
        let original = b"%PDF-1.7 previous export".to_vec();
        std::fs::write(&path, &original).unwrap();

        // `#invalid` is not a Typst function, so compilation fails.
        let result = compile_pdf_to_file("#invalid(", path.to_str().unwrap());
        assert!(result.is_err(), "expected the compile to fail");
        assert_eq!(std::fs::read(&path).unwrap(), original, "previous export was damaged");
    }

    /// A newly exported PDF is created privately (0600), not with the process
    /// umask default (0644).
    ///
    /// This is the assertion that actually distinguishes the atomic writer
    /// from `fs::write`: `fs::write` creates with `0666 & ~umask`, so an
    /// export lands world-readable in a shared directory. The crash-safety
    /// difference only shows on an I/O failure part-way through a write,
    /// which this suite cannot induce — so without this test a revert to
    /// `fs::write` would go unnoticed.
    #[cfg(unix)]
    #[test]
    fn a_new_export_is_created_with_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private.pdf");
        compile_pdf_to_file("Private.", path.to_str().unwrap()).expect("export");

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "new export should be private, got {mode:o}");
    }

    /// Exporting over an existing file keeps that file's permissions rather
    /// than replacing them with the temporary file's.
    #[cfg(unix)]
    #[test]
    fn replacing_an_export_preserves_its_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shared.pdf");
        std::fs::write(&path, b"%PDF-1.7 old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        compile_pdf_to_file("Replacement.", path.to_str().unwrap()).expect("export");

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644, "existing permissions should survive, got {mode:o}");
    }

    /// Exporting to a fresh path still works and leaves no temp file behind.
    #[test]
    fn export_to_a_new_path_leaves_no_temporary_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fresh.pdf");
        compile_pdf_to_file("Fresh.", path.to_str().unwrap()).expect("export");

        let leftovers: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name != "fresh.pdf")
            .collect();
        assert!(leftovers.is_empty(), "temporary files left behind: {leftovers:?}");
    }
}
