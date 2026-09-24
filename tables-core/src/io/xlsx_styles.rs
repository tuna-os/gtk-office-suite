// xlsx_styles.rs — the cell styles an xlsx workbook declares in styles.xml.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// A cell names a style by index (`<c s="3">`). Style 3 is the fourth `<xf>` in
// `<cellXfs>`, which points at a number format, a font and a fill by id, and may
// carry its own `<alignment>`. This resolves each `<xf>` into the model's
// NumberFormat and CellStyle.

use super::numfmt::{builtin_code, kind_for_code};
use crate::style::{CellStyle, HAlign, Rgb, VAlign};
use suite_common_core::format::NumberFormat;

/// One resolved `<xf>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct XfStyle {
    pub format: NumberFormat,
    pub style: CellStyle,
}

#[derive(Clone, Debug, Default)]
struct Font {
    family: Option<String>,
    size: Option<f64>,
    bold: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
    color: Option<Rgb>,
}

fn xml_attr<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    let needle = format!(" {attr}=\"");
    let start = tag.find(&needle)? + needle.len();
    tag[start..].split('"').next()
}

fn unescape(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// The contents of `<name ...>...</name>` (or the empty-element tag) blocks
/// inside `xml`, in order: `(attributes, body)`.
fn elements<'a>(xml: &'a str, name: &str) -> Vec<(&'a str, &'a str)> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        let after = &rest[i + open.len()..];
        // `<font` must not match `<fonts`.
        if !after.starts_with([' ', '>', '/']) {
            rest = after;
            continue;
        }
        let tag_end = after.find('>').unwrap_or(after.len());
        let attrs = &after[..tag_end];
        if attrs.ends_with('/') {
            out.push((attrs, ""));
            rest = &after[tag_end.min(after.len())..];
            continue;
        }
        let body_start = (tag_end + 1).min(after.len());
        let body_end = after[body_start..].find(&close).map_or(after.len(), |j| body_start + j);
        out.push((attrs, &after[body_start..body_end]));
        rest = &after[body_end..];
    }
    out
}

/// The body of the first `<name>...</name>` block.
fn block<'a>(xml: &'a str, name: &str) -> &'a str {
    elements(xml, name).first().map_or("", |e| e.1)
}

/// A boolean font property such as `<b/>` or `<b val="0"/>`.
fn flag(font: &str, name: &str) -> bool {
    elements(font, name)
        .first()
        .is_some_and(|(attrs, _)| !matches!(xml_attr(&format!(" {attrs}"), "val"), Some("0") | Some("false")))
}

/// Colours of the default Office theme, by theme index (`<color theme="n">`):
/// lt1, dk1, lt2, dk2, then the six accents. Tints are not applied.
const THEME: [Rgb; 10] = [
    Rgb(0xFF, 0xFF, 0xFF),
    Rgb(0x00, 0x00, 0x00),
    Rgb(0xE7, 0xE6, 0xE6),
    Rgb(0x44, 0x54, 0x6A),
    Rgb(0x44, 0x72, 0xC4),
    Rgb(0xED, 0x7D, 0x31),
    Rgb(0xA5, 0xA5, 0xA5),
    Rgb(0xFF, 0xC0, 0x00),
    Rgb(0x5B, 0x9B, 0xD5),
    Rgb(0x70, 0xAD, 0x47),
];

fn color(attrs: &str) -> Option<Rgb> {
    let attrs = format!(" {attrs}");
    if let Some(rgb) = xml_attr(&attrs, "rgb") {
        return Rgb::from_hex(rgb);
    }
    let theme: usize = xml_attr(&attrs, "theme")?.parse().ok()?;
    THEME.get(theme).copied()
}

fn parse_font(body: &str) -> Font {
    let val = |name: &str| elements(body, name).first().and_then(|(a, _)| xml_attr(&format!(" {a}"), "val").map(str::to_string));
    Font {
        family: val("name").map(|v| unescape(&v)),
        size: val("sz").and_then(|v| v.parse().ok()).filter(|s: &f64| s.is_finite() && *s > 0.0 && *s < 1000.0),
        bold: flag(body, "b"),
        italic: flag(body, "i"),
        underline: elements(body, "u")
            .first()
            .is_some_and(|(a, _)| xml_attr(&format!(" {a}"), "val") != Some("none")),
        strikethrough: flag(body, "strike"),
        color: elements(body, "color").first().and_then(|(a, _)| color(a)),
    }
}

/// A solid pattern fill's colour; other patterns and "none" are no fill.
fn parse_fill(body: &str) -> Option<Rgb> {
    let (attrs, inner) = *elements(body, "patternFill").first()?;
    if xml_attr(&format!(" {attrs}"), "patternType") != Some("solid") {
        return None;
    }
    elements(inner, "fgColor").first().and_then(|(a, _)| color(a))
}

/// Every cell style (`cellXfs` order) in styles.xml.
pub fn parse_cell_styles(styles_xml: &str) -> Vec<XfStyle> {
    let custom: std::collections::HashMap<u32, String> = elements(block(styles_xml, "numFmts"), "numFmt")
        .into_iter()
        .filter_map(|(attrs, _)| {
            let attrs = format!(" {attrs}");
            Some((xml_attr(&attrs, "numFmtId")?.parse().ok()?, unescape(xml_attr(&attrs, "formatCode")?)))
        })
        .collect();
    let fonts: Vec<Font> = elements(block(styles_xml, "fonts"), "font").into_iter().map(|(_, b)| parse_font(b)).collect();
    let fills: Vec<Option<Rgb>> = elements(block(styles_xml, "fills"), "fill").into_iter().map(|(_, b)| parse_fill(b)).collect();
    // Font 0 is the workbook's default font: a cell on it has no font of its own.
    let default_font = fonts.first().cloned().unwrap_or_default();

    elements(block(styles_xml, "cellXfs"), "xf")
        .into_iter()
        .map(|(attrs, body)| {
            let attrs = format!(" {attrs}");
            let id = |k: &str| xml_attr(&attrs, k).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
            let code = custom
                .get(&(id("numFmtId") as u32))
                .map(String::as_str)
                .or_else(|| builtin_code(id("numFmtId") as u32))
                .unwrap_or("General");
            let font = fonts.get(id("fontId")).cloned().unwrap_or_default();
            let differs = |a: &Option<String>, b: &Option<String>| if a != b { a.clone() } else { None };
            let mut style = CellStyle {
                font_family: differs(&font.family, &default_font.family),
                font_size: if font.size != default_font.size { font.size } else { None },
                bold: font.bold,
                italic: font.italic,
                underline: font.underline,
                strikethrough: font.strikethrough,
                color: font.color.filter(|c| *c != Rgb(0, 0, 0)),
                fill: fills.get(id("fillId")).copied().flatten(),
                ..CellStyle::default()
            };
            if let Some((a, _)) = elements(body, "alignment").first() {
                let a = format!(" {a}");
                style.h_align = match xml_attr(&a, "horizontal") {
                    Some("left") => HAlign::Left,
                    Some("center") | Some("centerContinuous") => HAlign::Center,
                    Some("right") => HAlign::Right,
                    _ => HAlign::General,
                };
                style.v_align = match xml_attr(&a, "vertical") {
                    Some("top") => VAlign::Top,
                    Some("center") => VAlign::Center,
                    _ => VAlign::Bottom,
                };
                style.wrap = matches!(xml_attr(&a, "wrapText"), Some("1") | Some("true"));
                style.indent = xml_attr(&a, "indent").and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            XfStyle { format: NumberFormat::new(kind_for_code(code)), style }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use suite_common_core::format::NumberFormatKind;

    const STYLES: &str = r#"<styleSheet>
      <numFmts count="1"><numFmt numFmtId="164" formatCode="0.0%"/></numFmts>
      <fonts count="3">
        <font><sz val="11"/><color theme="1"/><name val="Calibri"/></font>
        <font><b/><i val="1"/><u/><sz val="16"/><color rgb="FFC00000"/><name val="Liberation Serif"/></font>
        <font><b val="0"/><sz val="11"/><name val="Calibri"/></font>
      </fonts>
      <fills count="3">
        <fill><patternFill patternType="none"/></fill>
        <fill><patternFill patternType="gray125"/></fill>
        <fill><patternFill patternType="solid"><fgColor rgb="FFFFC7CE"/><bgColor indexed="64"/></patternFill></fill>
      </fills>
      <cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0"/></cellStyleXfs>
      <cellXfs count="4">
        <xf numFmtId="0" fontId="0" fillId="0" borderId="0"/>
        <xf numFmtId="164" fontId="1" fillId="2" applyFont="1"/>
        <xf numFmtId="0" fontId="2" fillId="0" applyAlignment="1"><alignment horizontal="center" vertical="top" wrapText="1" indent="2"/></xf>
        <xf numFmtId="0" fontId="0" fillId="1"><alignment horizontal="right"/></xf>
      </cellXfs>
    </styleSheet>"#;

    #[test]
    fn the_default_xf_is_the_default_style() {
        let xfs = parse_cell_styles(STYLES);
        assert_eq!(xfs.len(), 4);
        assert!(xfs[0].style.is_default(), "{:?}", xfs[0].style);
        assert_eq!(xfs[0].format.kind, NumberFormatKind::General);
    }

    #[test]
    fn fonts_fills_and_formats_resolve_through_their_ids() {
        let x = &parse_cell_styles(STYLES)[1];
        assert_eq!(x.format.kind, NumberFormatKind::Percent(1));
        let s = &x.style;
        assert_eq!(s.font_family.as_deref(), Some("Liberation Serif"));
        assert_eq!(s.font_size, Some(16.0));
        assert!(s.bold && s.italic && s.underline && !s.strikethrough);
        assert_eq!(s.color, Some(Rgb(0xC0, 0, 0)));
        assert_eq!(s.fill, Some(Rgb(0xFF, 0xC7, 0xCE)));
    }

    #[test]
    fn alignment_and_wrap_come_from_the_xf_itself() {
        let xfs = parse_cell_styles(STYLES);
        let s = &xfs[2].style;
        assert!(!s.bold, "<b val=\"0\"/> is not bold");
        assert_eq!((s.h_align, s.v_align, s.wrap, s.indent), (HAlign::Center, VAlign::Top, true, 2));
        // gray125 is a pattern, not a solid fill.
        assert_eq!(xfs[3].style.fill, None);
        assert_eq!(xfs[3].style.h_align, HAlign::Right);
    }
}
