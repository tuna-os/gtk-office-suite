// xlsx_styles.rs — the cell styles an xlsx workbook declares in styles.xml.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// A cell names a style by index (`<c s="3">`). Style 3 is the fourth `<xf>` in
// `<cellXfs>`, which points at a number format, a font and a fill by id, and may
// carry its own `<alignment>`. This resolves each `<xf>` into the model's
// NumberFormat and CellStyle.

use super::numfmt::{builtin_code, kind_for_code};
use crate::sheet::{BorderStyle, CellBorder};
use crate::style::{CellStyle, HAlign, Rgb, VAlign};
use suite_common_core::format::NumberFormat;

/// One resolved `<xf>`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct XfStyle {
    pub format: NumberFormat,
    pub style: CellStyle,
    pub border: CellBorder,
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
/// lt1, dk1, lt2, dk2, then the six accents.
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

thread_local! {
    /// The palette of the workbook whose styles are being read
    /// (`parse_cell_styles_with_theme`): its own theme's, else Office's.
    static PALETTE: std::cell::Cell<[Rgb; 10]> = const { std::cell::Cell::new(THEME) };
}

/// A workbook's theme colours (`xl/theme/theme1.xml`'s `a:clrScheme`) by
/// theme index: lt1, dk1, lt2, dk2, then accent1–6. The scheme lists them
/// dk1, lt1, dk2, lt2, …; a cell's `theme="0"` is lt1. Office's where the
/// scheme is missing or a colour can't be read.
pub fn theme_palette(theme_xml: &str) -> [Rgb; 10] {
    let mut out = THEME;
    let Some(scheme) = theme_xml.split("<a:clrScheme").nth(1).and_then(|s| s.split("</a:clrScheme>").next()) else {
        return out;
    };
    let read = |name: &str| -> Option<Rgb> {
        let body = scheme.split(&format!("<a:{name}>")).nth(1)?.split(&format!("</a:{name}>")).next()?;
        let attrs = |tag: &str| body.split(tag).nth(1).map(|t| format!(" {}", t.split('>').next().unwrap_or("")));
        if let Some(a) = attrs("<a:srgbClr") {
            return Rgb::from_hex(xml_attr(&a, "val")?);
        }
        // A system colour carries the value it last had.
        Rgb::from_hex(xml_attr(&attrs("<a:sysClr")?, "lastClr")?)
    };
    let names = ["lt1", "dk1", "lt2", "dk2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6"];
    for (slot, name) in out.iter_mut().zip(names) {
        if let Some(c) = read(name) {
            *slot = c;
        }
    }
    out
}

/// `c` with a SpreadsheetML tint: toward white by `tint` when positive,
/// toward black when negative, on the colour's HSL lightness.
fn tinted(c: Rgb, tint: f64) -> Rgb {
    if tint == 0.0 || !tint.is_finite() {
        return c;
    }
    let (r, g, b) = (c.0 as f64 / 255.0, c.1 as f64 / 255.0, c.2 as f64 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    let (h, s) = if d == 0.0 {
        (0.0, 0.0)
    } else {
        let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        let h = if max == r { (g - b) / d + if g < b { 6.0 } else { 0.0 } } else if max == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
        (h / 6.0, s)
    };
    let l = if tint < 0.0 { l * (1.0 + tint) } else { l * (1.0 - tint) + tint };
    let hue = |p: f64, q: f64, mut t: f64| {
        if t < 0.0 { t += 1.0 }
        if t > 1.0 { t -= 1.0 }
        if t < 1.0 / 6.0 { p + (q - p) * 6.0 * t } else if t < 0.5 { q } else if t < 2.0 / 3.0 { p + (q - p) * (2.0 / 3.0 - t) * 6.0 } else { p }
    };
    let (r, g, b) = if s == 0.0 {
        (l, l, l)
    } else {
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        (hue(p, q, h + 1.0 / 3.0), hue(p, q, h), hue(p, q, h - 1.0 / 3.0))
    };
    let byte = |v: f64| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgb(byte(r), byte(g), byte(b))
}

fn color(attrs: &str) -> Option<Rgb> {
    let attrs = format!(" {attrs}");
    let base = if let Some(rgb) = xml_attr(&attrs, "rgb") {
        Rgb::from_hex(rgb)?
    } else {
        let theme: usize = xml_attr(&attrs, "theme")?.parse().ok()?;
        PALETTE.with(|p| p.get()).get(theme).copied()?
    };
    let tint = xml_attr(&attrs, "tint").and_then(|t| t.parse::<f64>().ok()).unwrap_or(0.0);
    Some(tinted(base, tint))
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

/// An xlsx border edge style (`<left style="thin">`) in the model's terms.
/// Hairlines draw as thin lines; the dash-dot family draws dashed.
fn edge_style(style: Option<&str>) -> BorderStyle {
    match style {
        None | Some("none") => BorderStyle::None,
        Some("medium") => BorderStyle::Medium,
        Some("thick") => BorderStyle::Thick,
        Some("double") => BorderStyle::Double,
        Some("dotted") => BorderStyle::Dotted,
        Some("dashed" | "mediumDashed" | "dashDot" | "mediumDashDot" | "dashDotDot" | "mediumDashDotDot" | "slantDashDot") => {
            BorderStyle::Dashed
        }
        // thin, hair, and anything newer we don't know: a thin line.
        Some(_) => BorderStyle::Solid,
    }
}

/// One `<border>`: its four edges, and the colour of the first edge that
/// names one (the model has one colour per cell border). `start`/`end` are
/// the ISO 29500 names for left/right.
fn parse_border(body: &str) -> CellBorder {
    let mut edge_color = None;
    let mut edge = |names: &[&str]| {
        let Some((attrs, inner)) = names.iter().find_map(|n| elements(body, n).into_iter().next()) else {
            return BorderStyle::None;
        };
        let style = edge_style(xml_attr(&format!(" {attrs}"), "style"));
        if style != BorderStyle::None && edge_color.is_none() {
            edge_color = elements(inner, "color").first().and_then(|(a, _)| color(a));
        }
        style
    };
    let (left, right, top, bottom) = (edge(&["left", "start"]), edge(&["right", "end"]), edge(&["top"]), edge(&["bottom"]));
    let (r, g, b) = edge_color.unwrap_or(Rgb(0, 0, 0)).to_f64();
    CellBorder { top, bottom, left, right, color: (r, g, b) }
}

/// The workbook's default font, font 0, as `(family, points)`.
pub fn default_font(styles_xml: &str) -> Option<(String, f64)> {
    let font = elements(block(styles_xml, "fonts"), "font").into_iter().next().map(|(_, b)| parse_font(b))?;
    Some((font.family?, font.size?))
}

/// Every cell style (`cellXfs` order) in styles.xml.
/// `parse_cell_styles`, with theme colours from the workbook's own theme
/// (`theme_palette`). A fill Excel names as "accent 1, lighter 60%" drew in
/// Office's blue-and-orange theme whatever the workbook's theme was, and
/// untinted: a teal header drew orange (render-real
/// `july-2026-fiscal-risks-and-sustainability-charts`).
pub fn parse_cell_styles_with_theme(styles_xml: &str, theme_xml: &str) -> Vec<XfStyle> {
    PALETTE.with(|p| p.set(theme_palette(theme_xml)));
    let out = parse_cell_styles(styles_xml);
    PALETTE.with(|p| p.set(THEME));
    out
}

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
    let borders: Vec<CellBorder> =
        elements(block(styles_xml, "borders"), "border").into_iter().map(|(_, b)| parse_border(b)).collect();
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
            let border = borders.get(id("borderId")).cloned().unwrap_or_default();
            XfStyle { format: NumberFormat::new(kind_for_code(code)), style, border }
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
      <borders count="3">
        <border><left/><right/><top/><bottom/><diagonal/></border>
        <border><left style="thin"><color indexed="64"/></left><right style="thin"/><top style="thin"/><bottom style="thin"/><diagonal/></border>
        <border><start style="thick"><color rgb="FF0070C0"/></start><end style="medium"/><top style="dashed"/><bottom style="hair"/></border>
      </borders>
      <cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0"/></cellStyleXfs>
      <cellXfs count="6">
        <xf numFmtId="0" fontId="0" fillId="0" borderId="0"/>
        <xf numFmtId="164" fontId="1" fillId="2" applyFont="1"/>
        <xf numFmtId="0" fontId="2" fillId="0" applyAlignment="1"><alignment horizontal="center" vertical="top" wrapText="1" indent="2"/></xf>
        <xf numFmtId="0" fontId="0" fillId="1"><alignment horizontal="right"/></xf>
        <xf numFmtId="0" fontId="0" fillId="0" borderId="1" applyBorder="1"/>
        <xf numFmtId="0" fontId="0" fillId="0" borderId="2" applyBorder="1"/>
      </cellXfs>
    </styleSheet>"#;

    #[test]
    fn the_default_xf_is_the_default_style() {
        let xfs = parse_cell_styles(STYLES);
        assert_eq!(xfs.len(), 6);
        assert!(xfs[0].border.is_none());
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

    #[test]
    fn font_zero_is_the_workbook_default_font() {
        assert_eq!(default_font(STYLES), Some(("Calibri".into(), 11.0)));
        assert_eq!(default_font("<styleSheet/>"), None);
    }

    #[test]
    fn borders_resolve_through_border_id() {
        let xfs = parse_cell_styles(STYLES);
        let thin = &xfs[4].border;
        assert_eq!((&thin.left, &thin.right, &thin.top, &thin.bottom), (&BorderStyle::Solid, &BorderStyle::Solid, &BorderStyle::Solid, &BorderStyle::Solid));
        assert_eq!(thin.color, (0.0, 0.0, 0.0));
        let mixed = &xfs[5].border;
        assert_eq!(mixed.left, BorderStyle::Thick, "<start> is the left edge");
        assert_eq!(mixed.right, BorderStyle::Medium, "<end> is the right edge");
        assert_eq!(mixed.top, BorderStyle::Dashed);
        assert_eq!(mixed.bottom, BorderStyle::Solid, "a hairline draws thin");
        let (r, g, b) = mixed.color;
        assert_eq!(((r * 255.0).round(), (g * 255.0).round(), (b * 255.0).round()), (0.0, 112.0, 192.0));
    }

    /// A workbook's theme names its own colours: theme 5 (accent2) is the
    /// teal its scheme says, not Office's orange; a tint lightens or
    /// darkens it; with no theme, Office's.
    #[test]
    fn theme_colours_come_from_the_workbooks_theme() {
        let theme = r#"<a:theme><a:themeElements><a:clrScheme name="Custom 55">
            <a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
            <a:dk2><a:srgbClr val="CCE3E0"/></a:dk2><a:lt2><a:srgbClr val="FFFFFF"/></a:lt2>
            <a:accent1><a:srgbClr val="CCE3E0"/></a:accent1><a:accent2><a:srgbClr val="99C7C2"/></a:accent2>
            <a:accent3><a:srgbClr val="66AAA3"/></a:accent3><a:accent4><a:srgbClr val="338E85"/></a:accent4>
            <a:accent5><a:srgbClr val="006F62"/></a:accent5><a:accent6><a:srgbClr val="FFFFFF"/></a:accent6>
            </a:clrScheme></a:themeElements></a:theme>"#;
        let palette = theme_palette(theme);
        assert_eq!(palette[0], Rgb(0xFF, 0xFF, 0xFF), "theme 0 is lt1");
        assert_eq!(palette[1], Rgb(0, 0, 0), "theme 1 is dk1");
        assert_eq!(palette[5], Rgb(0x99, 0xC7, 0xC2), "theme 5 is accent2");
        let styles = r#"<styleSheet><fills count="4"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill>
            <fill><patternFill patternType="solid"><fgColor theme="5"/></patternFill></fill>
            <fill><patternFill patternType="solid"><fgColor theme="5" tint="0.5"/></patternFill></fill></fills>
            <cellXfs count="3"><xf fillId="0"/><xf fillId="2" applyFill="1"/><xf fillId="3" applyFill="1"/></cellXfs></styleSheet>"#;
        let themed = parse_cell_styles_with_theme(styles, theme);
        assert_eq!(themed[1].style.fill, Some(Rgb(0x99, 0xC7, 0xC2)));
        let lighter = themed[2].style.fill.expect("a tinted fill");
        assert!(lighter.0 > 0x99 && lighter.1 > 0xC7 && lighter.2 > 0xC2, "half-way to white: {lighter:?}");
        // The palette is the workbook's only while its styles are read.
        assert_eq!(parse_cell_styles(styles)[1].style.fill, Some(Rgb(0xED, 0x7D, 0x31)));
        assert_eq!(tinted(Rgb(0x80, 0x80, 0x80), -1.0), Rgb(0, 0, 0));
        assert_eq!(tinted(Rgb(0x80, 0x80, 0x80), 1.0), Rgb(0xFF, 0xFF, 0xFF));
    }
}
