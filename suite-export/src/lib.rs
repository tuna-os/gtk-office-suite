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
pub fn compile_pdf_to_file(source: &str, path: &str) -> Result<(), String> {
    let bytes = compile_pdf(source)?;
    std::fs::write(path, bytes).map_err(|e| format!("cannot write {path}: {e}"))
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
}
