// export.rs — Typst export for Decks.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Not yet wired to a menu action (unlike Letters, which has an "Export
// PDF" entry) — kept rather than deleted since the Typst rendering is
// already implemented and tested.
#![allow(dead_code)]

pub fn to_typst(slides: &[decks_core::engine::Slide]) -> String {
    let mut out = String::from("#set page(width: 16cm, height: 9cm)\n");
    for s in slides {
        out.push_str(&format!("#pagebreak()\n= {}\n", s.title));
        for obj in &s.objects {
            use decks_core::engine::SlideObject::*;
            match obj {
                TextBox { text, .. } => out.push_str(&format!("{}\n\n", text)),
                Rect { .. } => out.push_str("#rect(width: 100%, height: 100%)\n"),
                Circle { .. } => out.push_str("#circle(radius: 50%)\n"),
                Image { path, .. } => out.push_str(&format!("#image(\"{}\")\n", path)),
            }
        }
    }
    out
}

/// Export to PDF via the in-process Typst engine.
pub fn to_pdf(slides: &[decks_core::engine::Slide], path: &str) -> Result<(), String> {
    suite_export::compile_pdf_to_file(&to_typst(slides), path)
}
