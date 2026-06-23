// export.rs — Typst export for Decks.
// SPDX-License-Identifier: GPL-3.0-or-later

pub fn to_typst(slides: &[crate::engine::Slide]) -> String {
    let mut out = String::from("#set page(width: 16cm, height: 9cm)\n");
    for s in slides {
        out.push_str(&format!("#pagebreak()\n= {}\n", s.title));
        for obj in &s.objects {
            use crate::engine::SlideObject::*;
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

/// Export to PDF via typst CLI.
pub fn to_pdf(slides: &[crate::engine::Slide], path: &str) -> Result<(), String> {
    let tmp = format!("{}.typ", path);
    std::fs::write(&tmp, to_typst(slides)).map_err(|e| e.to_string())?;
    let out = std::process::Command::new("typst")
        .args(["compile", &tmp, path])
        .output()
        .map_err(|e| format!("typst not found: {}", e))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into());
    }
    Ok(())
}
