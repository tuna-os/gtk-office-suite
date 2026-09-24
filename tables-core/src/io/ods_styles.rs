// ods_styles.rs — the cell styles an ods spreadsheet declares in content.xml.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// A cell names an automatic style (`<table:table-cell table:style-name="ce3">`),
// and `ce3` is a `<style:style style:family="table-cell">` whose
// table-cell, paragraph and text properties carry the fill, borders, wrap,
// alignment and font. This resolves each into the model's CellStyle and
// CellBorder, the ODF twin of xlsx_styles.rs.
//
// Not read (yet): number formats (ODF data styles), named parent styles in
// styles.xml, and the document's default font. A cell whose style names no
// font keeps the workbook default.

use crate::sheet::{BorderStyle, CellBorder};
use crate::style::{CellStyle, HAlign, Rgb, VAlign};
use std::collections::HashMap;

/// Attribute `name` of a tag's attribute text. The name must follow
/// whitespace, so `fo:border` doesn't match inside `fo:border-top` and
/// `style:name` doesn't match `style:parent-style-name`.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("{name}=\"");
    let mut from = 0;
    while let Some(i) = tag[from..].find(&needle) {
        let at = from + i;
        if at > 0 && tag.as_bytes()[at - 1].is_ascii_whitespace() {
            return tag[at + needle.len()..].split('"').next();
        }
        from = at + needle.len();
    }
    None
}

/// The opening tag `<name ...>` (attributes only) of the first `name`
/// element in `xml`, if any.
fn first_tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("<{name}");
    let mut rest = xml;
    while let Some(i) = rest.find(&needle) {
        let after = &rest[i + needle.len()..];
        if after.starts_with([' ', '\n', '\t', '\r', '/', '>']) {
            // A leading space so `attr` finds the first attribute too.
            let end = after.find('>').unwrap_or(after.len());
            return Some(&after[..end]);
        }
        rest = after;
    }
    None
}

fn color(v: &str) -> Option<Rgb> {
    Rgb::from_hex(v.trim().strip_prefix('#')?)
}

/// Font size in points from an ODF length (`11pt`, `0.5cm`, ...).
fn points(v: &str) -> Option<f64> {
    const UNITS: [(&str, f64); 5] = [("pt", 1.0), ("in", 72.0), ("cm", 72.0 / 2.54), ("mm", 72.0 / 25.4), ("px", 0.75)];
    let v = v.trim();
    let (n, per_pt) = UNITS.iter().find_map(|&(unit, k)| v.strip_suffix(unit).map(|n| (n, k)))?;
    let p = n.trim().parse::<f64>().ok()? * per_pt;
    (p.is_finite() && p > 0.0 && p < 1000.0).then_some(p)
}

/// One `fo:border*` value, `"0.74pt solid #000000"`, as an edge style and
/// colour. Calc writes a thin line as 0.74pt or less, medium around 1.76pt
/// and thick 2.49pt, so the weight is binned at 1.25pt and 2.25pt.
fn edge(v: &str) -> (BorderStyle, Option<Rgb>) {
    let mut width = None;
    let mut kind = None;
    let mut rgb = None;
    for part in v.split_whitespace() {
        if part == "none" || part == "hidden" {
            return (BorderStyle::None, None);
        } else if part.starts_with('#') {
            rgb = color(part);
        } else if let Some(p) = points(part) {
            width = Some(p);
        } else {
            kind = Some(part);
        }
    }
    let style = match kind {
        Some("double") => BorderStyle::Double,
        Some("dotted") => BorderStyle::Dotted,
        Some("dashed" | "dash-dot" | "dash-dot-dot" | "fine-dashed") => BorderStyle::Dashed,
        _ => match width.unwrap_or(0.75) {
            w if w < 1.25 => BorderStyle::Solid,
            w if w < 2.25 => BorderStyle::Medium,
            _ => BorderStyle::Thick,
        },
    };
    (style, rgb)
}

/// Style name → (style, border) for every `table-cell` automatic style.
/// `font-face-decls` resolves a `style:font-name` to its family.
pub fn parse_ods_cell_styles(content_xml: &str) -> HashMap<String, (CellStyle, CellBorder)> {
    let faces: HashMap<String, String> = content_xml
        .split("<style:font-face")
        .skip(1)
        .filter_map(|f| {
            let tag = format!(" {}", f.split('>').next()?);
            let name = attr(&tag, "style:name")?;
            // Quoted when it has a space: svg:font-family="&apos;Liberation Sans&apos;".
            let family = attr(&tag, "svg:font-family").unwrap_or(name).replace("&apos;", "").replace('\'', "");
            Some((name.to_string(), family.trim().to_string()))
        })
        .collect();

    let mut out = HashMap::new();
    for block in content_xml.split("<style:style").skip(1) {
        let head = format!(" {}", block.split('>').next().unwrap_or(""));
        if attr(&head, "style:family") != Some("table-cell") {
            continue;
        }
        let Some(name) = attr(&head, "style:name") else { continue };
        // This style's own body only (a pretty-printed file puts a newline,
        // not a space, before the next style's attributes).
        let body = block.split("</style:style>").next().unwrap_or("");
        let cell = first_tag(body, "style:table-cell-properties").unwrap_or("");
        let para = first_tag(body, "style:paragraph-properties").unwrap_or("");
        let text = first_tag(body, "style:text-properties").unwrap_or("");

        let mut style = CellStyle {
            font_family: attr(text, "style:font-name")
                .map(|n| faces.get(n).cloned().unwrap_or_else(|| n.to_string()))
                .or_else(|| attr(text, "fo:font-family").map(|f| f.replace("&apos;", "").replace('\'', ""))),
            font_size: attr(text, "fo:font-size").and_then(points),
            bold: matches!(attr(text, "fo:font-weight"), Some("bold" | "600" | "700" | "800" | "900")),
            italic: matches!(attr(text, "fo:font-style"), Some("italic" | "oblique")),
            underline: attr(text, "style:text-underline-style").is_some_and(|v| v != "none"),
            strikethrough: attr(text, "style:text-line-through-style").is_some_and(|v| v != "none"),
            color: attr(text, "fo:color").and_then(color).filter(|c| *c != Rgb(0, 0, 0)),
            fill: attr(cell, "fo:background-color").and_then(color),
            wrap: attr(cell, "fo:wrap-option") == Some("wrap"),
            ..CellStyle::default()
        };
        style.h_align = match attr(para, "fo:text-align") {
            Some("start" | "left") => HAlign::Left,
            Some("center") => HAlign::Center,
            Some("end" | "right") => HAlign::Right,
            _ => HAlign::General,
        };
        style.v_align = match attr(cell, "style:vertical-align") {
            Some("top") => VAlign::Top,
            Some("middle") => VAlign::Center,
            _ => VAlign::Bottom,
        };

        let all = attr(cell, "fo:border").map(edge);
        let side = |s: &str| attr(cell, &format!("fo:border-{s}")).map(edge).or_else(|| all.clone());
        let (top, bottom, left, right) = (side("top"), side("bottom"), side("left"), side("right"));
        let rgb = [&top, &bottom, &left, &right]
            .iter()
            .find_map(|e| e.as_ref().filter(|(s, _)| *s != BorderStyle::None).and_then(|(_, c)| *c))
            .unwrap_or(Rgb(0, 0, 0));
        let pick = |e: Option<(BorderStyle, Option<Rgb>)>| e.map_or(BorderStyle::None, |(s, _)| s);
        let border = CellBorder {
            top: pick(top),
            bottom: pick(bottom),
            left: pick(left),
            right: pick(right),
            color: rgb.to_f64(),
        };
        out.insert(name.to_string(), (style, border));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed from what Calc writes for tables/cell-fonts, tables/fills and
    // tables/borders converted to ods.
    const CONTENT: &str = r##"<office:document-content>
      <office:font-face-decls>
        <style:font-face style:name="Calibri" svg:font-family="Calibri" style:font-family-generic="swiss"/>
        <style:font-face style:name="Liberation Serif1" svg:font-family="&apos;Liberation Serif&apos;"/>
      </office:font-face-decls>
      <office:automatic-styles>
        <style:style style:name="co1" style:family="table-column"><style:table-column-properties style:column-width="0.889in"/></style:style>
        <style:style style:name="ce1" style:family="table-cell" style:parent-style-name="Default">
          <style:text-properties style:font-name="Calibri" fo:font-size="11pt" fo:font-weight="bold"/>
        </style:style>
        <style:style style:name="ce2" style:family="table-cell" style:parent-style-name="Default">
          <style:table-cell-properties fo:background-color="#ffc7ce" fo:wrap-option="wrap" style:vertical-align="middle"/>
          <style:paragraph-properties fo:text-align="center"/>
          <style:text-properties style:font-name="Liberation Serif1" fo:font-size="20pt" fo:font-style="italic" fo:color="#c00000" style:text-underline-style="solid"/>
        </style:style>
        <style:style style:name="ce3" style:family="table-cell">
          <style:table-cell-properties fo:border="0.74pt solid #000000"/>
        </style:style>
        <style:style style:name="ce4" style:family="table-cell">
          <style:table-cell-properties fo:border-bottom="2.49pt solid #0070c0" fo:border-left="0.74pt dashed #0070c0" fo:border-right="none" fo:border-top="1.76pt solid #0070c0"/>
          <style:paragraph-properties fo:text-align="end"/>
        </style:style>
      </office:automatic-styles>
    </office:document-content>"##;

    #[test]
    fn fonts_resolve_through_their_font_face() {
        let styles = parse_ods_cell_styles(CONTENT);
        assert!(!styles.contains_key("co1"), "column styles are not cell styles");
        let (s, b) = &styles["ce1"];
        assert_eq!(s.font_family.as_deref(), Some("Calibri"));
        assert_eq!(s.font_size, Some(11.0));
        assert!(s.bold && !s.italic);
        assert!(b.is_none());
        let (s, _) = &styles["ce2"];
        assert_eq!(s.font_family.as_deref(), Some("Liberation Serif"));
        assert_eq!(s.font_size, Some(20.0));
        assert!(s.italic && s.underline && !s.bold);
        assert_eq!(s.color, Some(Rgb(0xC0, 0, 0)));
    }

    #[test]
    fn fill_wrap_and_alignment_come_from_the_cell_and_paragraph_properties() {
        let styles = parse_ods_cell_styles(CONTENT);
        let (s, _) = &styles["ce2"];
        assert_eq!(s.fill, Some(Rgb(0xFF, 0xC7, 0xCE)));
        assert!(s.wrap);
        assert_eq!((s.h_align, s.v_align), (HAlign::Center, VAlign::Center));
        assert_eq!(styles["ce4"].0.h_align, HAlign::Right, "end is right in a left-to-right sheet");
    }

    #[test]
    fn borders_read_weight_kind_and_colour_per_side() {
        let styles = parse_ods_cell_styles(CONTENT);
        assert_eq!(styles["ce3"].1, CellBorder::outline(BorderStyle::Solid, (0.0, 0.0, 0.0)));
        let b = &styles["ce4"].1;
        assert_eq!(b.bottom, BorderStyle::Thick);
        assert_eq!(b.top, BorderStyle::Medium);
        assert_eq!(b.left, BorderStyle::Dashed);
        assert_eq!(b.right, BorderStyle::None);
        let (r, g, bl) = b.color;
        assert_eq!(((r * 255.0).round(), (g * 255.0).round(), (bl * 255.0).round()), (0.0, 112.0, 192.0));
    }
}
