// SPDX-License-Identifier: GPL-3.0-or-later
//! Picture and text-box frames: package picture entries, extents and anchors.

#![allow(unused_imports)]
use super::*;

/// An image run's picture as the package stores it (#1292): its member
/// name under `Pictures/`, named by a hash of its bytes so a picture used
/// twice is stored once, its bytes and its media type. None when the
/// source can't be read or is not a picture (`picture_source`); the run is
/// then written as its alt text, as the docx writer does.
pub(super) fn picture(src: &str) -> Option<(String, Vec<u8>, &'static str)> {
    use std::hash::{Hash, Hasher};
    let bytes = crate::picture_source::read(src)?;
    let ext = std::path::Path::new(src)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_else(|| "png".into());
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "tif" | "tiff" => "image/tiff",
        _ => "application/octet-stream",
    };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    Some((format!("Pictures/{:016x}.{ext}", h.finish()), bytes, mime))
}

/// A picture run as a frame (picture number `n`): anchored as a character,
/// so it stays where it is in the line, or, floating, anchored to its
/// paragraph and placed by a graphic style of its own (`{prefix}{n}`, added
/// to `auto`). Its bytes are in Pictures/ (#1292). None when its source
/// can't be read.
pub(super) fn picture_frame(r: &Run, n: usize, prefix: &str, auto: &mut String) -> Option<String> {
    let (name, bytes, _) = picture(r.style.image.as_deref()?)?;
    let (w, h) = picture_extent(r, &bytes);
    let title = if r.text.trim().is_empty() { String::new() } else { format!("<svg:title>{}</svg:title>", esc(&r.text)) };
    let anchoring = match r.style.image_anchor {
        Some(a) => {
            let style = format!("{prefix}{n}");
            auto.push_str(&floating_style(&style, &a));
            format!(
                "draw:style-name=\"{style}\" text:anchor-type=\"paragraph\" svg:x=\"{:.6}in\" svg:y=\"{:.6}in\"",
                a.x_emu as f64 / EMU_PER_INCH,
                a.y_emu as f64 / EMU_PER_INCH,
            )
        }
        // The room kept beside it, as the margins of a graphic style.
        None => match r.style.image_sides_emu {
            Some((left, right)) => {
                let style = format!("{prefix}{n}");
                auto.push_str(&format!(
                    "<style:style style:name=\"{style}\" style:family=\"graphic\"><style:graphic-properties fo:margin-left=\"{:.2}pt\" fo:margin-right=\"{:.2}pt\"/></style:style>",
                    left as f64 / 12700.0,
                    right as f64 / 12700.0,
                ));
                format!("draw:style-name=\"{style}\" text:anchor-type=\"as-char\"")
            }
            None => "text:anchor-type=\"as-char\"".to_string(),
        },
    };
    Some(format!(
        "<draw:frame draw:name=\"Picture {n}\" {anchoring} svg:width=\"{:.6}in\" svg:height=\"{:.6}in\">\
         <draw:image xlink:href=\"{name}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/>\
         {title}</draw:frame>",
        w as f64 / EMU_PER_INCH,
        h as f64 / EMU_PER_INCH,
    ))
}

/// A text box run as a frame (text box number `n`) holding a
/// `draw:text-box`, anchored to its paragraph and placed, framed, filled
/// and padded by a graphic style of its own (`tb{n}`, added to `auto`).
/// Its text is a paragraph per line.
pub(super) fn text_box_frame(r: &Run, n: usize, auto: &mut String) -> Option<String> {
    let tb = r.style.text_box.as_deref()?;
    let anchor = r.style.image_anchor.unwrap_or_default();
    let style = format!("tb{n}");
    let emu_pt = |v: u32| v as f64 / 12700.0;
    let mut look = String::new();
    match &tb.border {
        Some(b) => look.push_str(&format!(" fo:border=\"{:.2}pt solid #{}\"", emu_pt(b.width_emu), b.color)),
        None => look.push_str(" fo:border=\"none\""),
    }
    match &tb.fill {
        Some(c) => look.push_str(&format!(" draw:fill=\"solid\" draw:fill-color=\"#{c}\"")),
        None => look.push_str(" draw:fill=\"none\""),
    }
    let [top, right, bottom, left] = tb.padding_emu;
    look.push_str(&format!(
        " fo:padding-top=\"{:.2}pt\" fo:padding-right=\"{:.2}pt\" fo:padding-bottom=\"{:.2}pt\" fo:padding-left=\"{:.2}pt\"",
        emu_pt(top), emu_pt(right), emu_pt(bottom), emu_pt(left)
    ));
    // Under Writer's "Frame" style it is a text frame to Writer, which
    // keeps its rule; without a parent Writer reads it as a drawing shape
    // and drops the rule.
    let frame_style = floating_style(&style, &anchor)
        .replacen("style:family=\"graphic\"", "style:family=\"graphic\" style:parent-style-name=\"Frame\"", 1)
        .replacen("/></style:style>", &format!("{look}/></style:style>"), 1);
    auto.push_str(&frame_style);
    let text: String = tb.text.split('\n').map(|line| format!("<text:p>{}</text:p>", esc(line))).collect();
    Some(format!(
        "<draw:frame draw:style-name=\"{style}\" draw:name=\"Text Box {n}\" text:anchor-type=\"paragraph\" \
         svg:x=\"{:.6}in\" svg:y=\"{:.6}in\" svg:width=\"{:.6}in\" svg:height=\"{:.6}in\"><draw:text-box>{text}</draw:text-box></draw:frame>",
        anchor.x_emu as f64 / EMU_PER_INCH,
        anchor.y_emu as f64 / EMU_PER_INCH,
        tb.extent_emu.0 as f64 / EMU_PER_INCH,
        tb.extent_emu.1 as f64 / EMU_PER_INCH,
    ))
}

/// EMU per inch (OOXML's unit, which the model keeps image sizes in).
pub(super) const EMU_PER_INCH: f64 = 914_400.0;

/// The size an image run is drawn at, in EMU: the size it was shown at,
/// else a PNG's own pixels at 96 dpi, else 4in x 3in (the docx writer's
/// fallback).
pub(super) fn picture_extent(run: &Run, bytes: &[u8]) -> (u64, u64) {
    if let Some(e) = run.style.image_extent_emu {
        return e;
    }
    if bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let px = |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as u64;
        let (w, h) = (px(16), px(20));
        if w > 0 && h > 0 {
            return (w * 9525, h * 9525);
        }
    }
    (4 * 914_400, 3 * 914_400)
}

/// The automatic graphic style `name` placing a floating picture: each
/// axis aligned in, or offset from (`svg:x`/`svg:y` on the frame), the
/// page, the page inside its margins, or the paragraph; over the text or
/// behind it, never wrapping it.
pub(super) fn floating_style(name: &str, a: &crate::model::ImageAnchor) -> String {
    use crate::model::{AnchorAlign, AnchorFrame};
    let rel = |f: AnchorFrame| match f {
        AnchorFrame::Page => "page",
        AnchorFrame::Margin => "page-content",
        AnchorFrame::Text => "paragraph",
    };
    let pos = |align: Option<AnchorAlign>, names: [&'static str; 4]| match align {
        None => names[0],
        Some(AnchorAlign::Start) => names[1],
        Some(AnchorAlign::Center) => names[2],
        Some(AnchorAlign::End) => names[3],
    };
    format!(
        "<style:style style:name=\"{name}\" style:family=\"graphic\"><style:graphic-properties style:wrap=\"{}\" \
         style:run-through=\"{}\" style:horizontal-pos=\"{}\" style:horizontal-rel=\"{}\" style:vertical-pos=\"{}\" \
         style:vertical-rel=\"{}\"/></style:style>",
        match a.wrap {
            crate::model::Wrap::None => "run-through",
            crate::model::Wrap::Parallel => "parallel",
            crate::model::Wrap::TopBottom => "none",
        },
        if a.behind { "background" } else { "foreground" },
        pos(a.h_align, ["from-left", "left", "center", "right"]),
        rel(a.h_from),
        pos(a.v_align, ["from-top", "top", "middle", "bottom"]),
        rel(a.v_from),
    )
}

/// A graphic style's placement of a floating frame (its offsets are the
/// frame's own `svg:x`/`svg:y`).
pub(super) fn graphic_placement(e: &quick_xml::events::BytesStart<'_>) -> crate::model::ImageAnchor {
    use crate::model::{AnchorAlign, AnchorFrame};
    let frame = |a: &str| match attr_val(e, a).as_deref() {
        Some("page") => AnchorFrame::Page,
        Some("page-content") => AnchorFrame::Margin,
        _ => AnchorFrame::Text,
    };
    let align = |a: &str| match attr_val(e, a).as_deref() {
        Some("left" | "top" | "inside") => Some(AnchorAlign::Start),
        Some("center" | "middle") => Some(AnchorAlign::Center),
        Some("right" | "bottom" | "outside") => Some(AnchorAlign::End),
        _ => None,
    };
    crate::model::ImageAnchor {
        h_from: frame("style:horizontal-rel"),
        h_align: align("style:horizontal-pos"),
        v_from: frame("style:vertical-rel"),
        v_align: align("style:vertical-pos"),
        behind: attr_val(e, "style:run-through").as_deref() == Some("background"),
        // Text beside it on one side or both is read as beside it on its
        // wider side, which is where Writer puts a short paragraph.
        wrap: match attr_val(e, "style:wrap").as_deref() {
            Some("parallel" | "dynamic" | "biggest" | "left" | "right") => crate::model::Wrap::Parallel,
            Some("none") => crate::model::Wrap::TopBottom,
            _ => crate::model::Wrap::None,
        },
        ..Default::default()
    }
}

/// A signed ODF length (a frame's `svg:x`, which may be negative) in EMU.
pub(super) fn offset_emu(s: &str) -> Option<i64> {
    let s = s.trim();
    let (sign, magnitude) = s.strip_prefix('-').map_or((1, s), |m| (-1, m));
    let split = magnitude.find(|c: char| c.is_ascii_alphabetic())?;
    if magnitude[..split].trim().parse::<f64>().ok()? == 0.0 {
        return Some(0);
    }
    Some(sign * length_emu(magnitude)? as i64)
}

/// An ODF length ("2.5cm", "1in", "72pt", "10mm", "6pc", "96px") in EMU.
pub(super) fn length_emu(s: &str) -> Option<u64> {
    let s = s.trim();
    let split = s.find(|c: char| c.is_ascii_alphabetic())?;
    let (n, unit) = s.split_at(split);
    let n: f64 = n.trim().parse().ok()?;
    let inches = match unit {
        "in" => n,
        "cm" => n / 2.54,
        "mm" => n / 25.4,
        "pt" => n / 72.0,
        "pc" => n / 6.0,
        "px" => n / 96.0,
        _ => return None,
    };
    (inches > 0.0).then(|| (inches * EMU_PER_INCH).round() as u64)
}
