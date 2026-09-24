// shape_xml.rs — how the shapes in a pptx part are painted, per `p:sp`.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// A shape's fill and outline come from its own `p:spPr` (`a:solidFill`,
// `a:gradFill`, `a:noFill`, `a:ln`) or, where it states none, from its
// `p:style` references into the theme's format scheme: `a:fillRef idx="3"`
// means the third fill style in `a:fillStyleLst`, painted in the reference's
// colour wherever that style says `phClr`. Colours are literal (`srgbClr`),
// system (`sysClr`) or theme slots (`schemeClr`), each followed by an ordered
// list of modifiers (`tint`, `shade`, `satMod`, `lumMod`, `lumOff`).
//
// The parts involved are small, so they're read into a tiny element tree and
// resolved with plain recursion rather than a streaming state machine.

use super::shape::{Color, GradientStop, LinearGradient, ShapeStyle, Stroke};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::HashMap;

// ── A minimal element tree ────────────────────────────────────────────────

#[derive(Debug, Default)]
struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
}

impl Node {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
    /// Every descendant named `name`, in document order, not descending
    /// into matches.
    fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a Node>) {
        for c in &self.children {
            if c.name == name {
                out.push(c);
            } else {
                c.find_all(name, out);
            }
        }
    }
    fn find(&self, name: &str) -> Option<&Node> {
        let mut v = Vec::new();
        self.find_all(name, &mut v);
        v.into_iter().next()
    }
}

fn element(e: &BytesStart) -> Node {
    Node {
        name: e.name().as_ref().to_string(),
        attrs: e
            .attributes()
            .flatten()
            .filter_map(|a| {
                let v = a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?.into_owned();
                Some((a.key.as_ref().to_string(), v))
            })
            .collect(),
        children: Vec::new(),
    }
}

/// The element tree of `xml` (text content is not kept: nothing here needs it).
fn parse_tree(xml: &str) -> Node {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut stack = vec![Node::default()];
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => stack.push(element(e)),
            Ok(Event::Empty(ref e)) => {
                let node = element(e);
                if let Some(top) = stack.last_mut() {
                    top.children.push(node);
                }
            }
            Ok(Event::End(_)) => {
                if stack.len() > 1 {
                    let node = stack.pop().unwrap_or_default();
                    if let Some(top) = stack.last_mut() {
                        top.children.push(node);
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    // An unclosed document still yields what was read.
    while stack.len() > 1 {
        let node = stack.pop().unwrap_or_default();
        if let Some(top) = stack.last_mut() {
            top.children.push(node);
        }
    }
    stack.pop().unwrap_or_default()
}

// ── The theme ─────────────────────────────────────────────────────────────

/// A deck's theme: its colour slots (`a:clrScheme`) and format scheme
/// (`a:fmtScheme`), which shapes reference through `p:style`.
#[derive(Debug)]
pub(crate) struct Theme {
    colors: HashMap<String, Color>,
    fmt: Node,
}

impl Default for Theme {
    /// The Office 2013+ default theme, for a package without a theme part.
    fn default() -> Self {
        let slots = [
            ("dk1", "000000"),
            ("lt1", "FFFFFF"),
            ("dk2", "44546A"),
            ("lt2", "E7E6E6"),
            ("accent1", "4472C4"),
            ("accent2", "ED7D31"),
            ("accent3", "A5A5A5"),
            ("accent4", "FFC000"),
            ("accent5", "5B9BD5"),
            ("accent6", "70AD47"),
            ("hlink", "0563C1"),
            ("folHlink", "954F72"),
        ];
        Theme {
            colors: slots.iter().filter_map(|(k, v)| Some((k.to_string(), Color::from_hex(v)?))).collect(),
            // Office 2013's format scheme, flat: a fill style is phClr, and
            // the three line widths are 6350, 12700 and 19050 EMU.
            fmt: parse_tree(
                r#"<a:fmtScheme><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
                <a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst>
                <a:lnStyleLst><a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>
                <a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>
                <a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst></a:fmtScheme>"#,
            )
            .children
            .into_iter()
            .next()
            .unwrap_or_default(),
        }
    }
}

impl Theme {
    /// A `schemeClr` value, with the presentation's standard mapping of
    /// text/background names onto the dark/light slots.
    fn slot(&self, name: &str) -> Option<Color> {
        let slot = match name {
            "tx1" => "dk1",
            "bg1" => "lt1",
            "tx2" => "dk2",
            "bg2" => "lt2",
            other => other,
        };
        self.colors.get(slot).copied()
    }

    /// The `n`th (1-based) child of a format-scheme list such as
    /// `a:fillStyleLst`.
    fn style(&self, list: &str, n: usize) -> Option<&Node> {
        self.fmt.child(list)?.children.get(n.checked_sub(1)?)
    }
}

/// The theme in a theme part (`ppt/theme/themeN.xml`). Slots or lists the
/// part doesn't state keep the default theme's.
pub(crate) fn theme(theme_xml: &str) -> Theme {
    let mut theme = Theme::default();
    let root = parse_tree(theme_xml);
    if let Some(scheme) = root.find("a:clrScheme") {
        for slot in &scheme.children {
            let value = slot.children.first().and_then(|c| match c.name.as_str() {
                "a:srgbClr" => c.attr("val").and_then(Color::from_hex),
                "a:sysClr" => c.attr("lastClr").and_then(Color::from_hex),
                _ => None,
            });
            if let Some(c) = value {
                theme.colors.insert(slot.name.trim_start_matches("a:").to_string(), c);
            }
        }
    }
    if let Some(fmt) = root.find("a:fmtScheme") {
        // Keep the part's own lists; fall back to the default's per list.
        let mut merged = Node { name: fmt.name.clone(), ..Node::default() };
        for list in ["a:fillStyleLst", "a:lnStyleLst", "a:bgFillStyleLst"] {
            if let Some(own) = fmt.child(list) {
                merged.children.push(clone_node(own));
            } else if let Some(def) = theme.fmt.child(list) {
                merged.children.push(clone_node(def));
            }
        }
        theme.fmt = merged;
    }
    theme
}

fn clone_node(n: &Node) -> Node {
    Node { name: n.name.clone(), attrs: n.attrs.clone(), children: n.children.iter().map(clone_node).collect() }
}

// ── Colours, fills, lines ─────────────────────────────────────────────────

const COLOR_ELEMENTS: [&str; 4] = ["a:srgbClr", "a:schemeClr", "a:sysClr", "a:prstClr"];

/// A colour element with its modifiers applied, `phClr` standing for
/// `placeholder` (the `p:style` reference's own colour).
fn color_of(node: &Node, theme: &Theme, placeholder: Option<Color>) -> Option<Color> {
    let base = match node.name.as_str() {
        "a:srgbClr" => node.attr("val").and_then(Color::from_hex),
        "a:sysClr" => node.attr("lastClr").and_then(Color::from_hex),
        "a:schemeClr" => match node.attr("val") {
            Some("phClr") => placeholder,
            Some(name) => theme.slot(name),
            None => None,
        },
        "a:prstClr" => match node.attr("val") {
            Some("black") => Some(Color(0, 0, 0)),
            Some("white") => Some(Color(255, 255, 255)),
            _ => None,
        },
        _ => None,
    }?;
    let (mut lum_mod, mut lum_off) = (None, None);
    let mut c = base;
    for m in &node.children {
        let v: Option<i32> = m.attr("val").and_then(|v| v.parse().ok());
        let Some(v) = v else { continue };
        match m.name.as_str() {
            "a:tint" => c = c.tint(v),
            "a:shade" => c = c.shade(v),
            "a:satMod" => c = c.sat_mod(v),
            "a:lumMod" => lum_mod = Some(v),
            "a:lumOff" => lum_off = Some(v),
            _ => {}
        }
    }
    Some(c.lum(lum_mod, lum_off))
}

/// The first colour element among `node`'s children.
fn first_color(node: &Node, theme: &Theme, placeholder: Option<Color>) -> Option<Color> {
    node.children
        .iter()
        .find(|c| COLOR_ELEMENTS.contains(&c.name.as_str()))
        .and_then(|c| color_of(c, theme, placeholder))
}

/// A resolved fill.
enum Fill {
    None,
    Solid(Color),
    Gradient(LinearGradient),
}

/// A fill element (`a:solidFill`, `a:gradFill`, `a:noFill`), if `node` is one.
fn fill_of(node: &Node, theme: &Theme, placeholder: Option<Color>) -> Option<Fill> {
    match node.name.as_str() {
        "a:noFill" => Some(Fill::None),
        "a:solidFill" => first_color(node, theme, placeholder).map(Fill::Solid),
        "a:gradFill" => {
            let stops: Vec<GradientStop> = node
                .child("a:gsLst")?
                .children_named("a:gs")
                .filter_map(|gs| {
                    let pos = gs.attr("pos").and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) / 100_000.0;
                    Some(GradientStop { pos: pos.clamp(0.0, 1.0), color: first_color(gs, theme, placeholder)? })
                })
                .collect();
            if stops.is_empty() {
                return None;
            }
            // Only linear gradients are modelled; a path (radial) gradient
            // is drawn as a vertical one rather than dropped.
            let angle = node
                .child("a:lin")
                .and_then(|l| l.attr("ang"))
                .and_then(|v| v.parse::<f64>().ok())
                .map_or(90.0, |a| a / 60_000.0);
            Some(Fill::Gradient(LinearGradient { stops, angle }))
        }
        _ => None,
    }
}

/// The fill among `node`'s children (a `p:spPr`, or a style list entry).
fn fill_among(node: &Node, theme: &Theme, placeholder: Option<Color>) -> Option<Fill> {
    node.children.iter().find_map(|c| fill_of(c, theme, placeholder))
}

/// A line (`a:ln`): its colour (None = no line) and width in EMU.
fn line_of(ln: &Node, theme: &Theme, placeholder: Option<Color>) -> (Option<Option<Color>>, Option<f64>) {
    let width = ln.attr("w").and_then(|v| v.parse::<f64>().ok());
    let color = match fill_among(ln, theme, placeholder) {
        Some(Fill::None) => Some(None),
        Some(Fill::Solid(c)) => Some(Some(c)),
        Some(Fill::Gradient(g)) => Some(g.mean()),
        None => None,
    };
    (color, width)
}

// ── Per shape ─────────────────────────────────────────────────────────────

/// What one `p:sp` says about its painting.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SpStyle {
    pub style: ShapeStyle,
    /// `roundRect`'s corner adjustment (`adj`), as a fraction of the
    /// shorter side, when the shape states one.
    pub round_adj: Option<f64>,
}

fn resolve_sp(sp: &Node, theme: &Theme, scale: f64) -> SpStyle {
    let sppr = sp.child("p:spPr");
    let style = sp.child("p:style");
    let reference = |name: &str| -> Option<(usize, Option<Color>)> {
        let r = style?.child(name)?;
        let idx = r.attr("idx").and_then(|v| v.parse().ok()).unwrap_or(0);
        Some((idx, first_color(r, theme, None)))
    };

    // Fill: the shape's own, else its fillRef into the format scheme.
    let own_fill = sppr.and_then(|p| fill_among(p, theme, None));
    let fill = own_fill.or_else(|| {
        let (idx, color) = reference("a:fillRef")?;
        let entry = match idx {
            0 => return Some(Fill::None),
            1001.. => theme.style("a:bgFillStyleLst", idx - 1000)?,
            n => theme.style("a:fillStyleLst", n)?,
        };
        fill_of(entry, theme, color)
    });
    let (fill, gradient) = match fill {
        Some(Fill::Solid(c)) => (Some(c), None),
        Some(Fill::Gradient(g)) => (g.mean(), Some(g)),
        Some(Fill::None) | None => (None, None),
    };

    // Line: the shape's own a:ln (colour and width each), else lnRef's.
    let (mut color, mut width) = sppr.and_then(|p| p.child("a:ln")).map_or((None, None), |ln| line_of(ln, theme, None));
    if color.is_none() || width.is_none() {
        if let Some((idx, ref_color)) = reference("a:lnRef") {
            if idx == 0 {
                color = color.or(Some(None));
            } else if let Some(entry) = theme.style("a:lnStyleLst", idx) {
                let (c, w) = line_of(entry, theme, ref_color);
                color = color.or(c);
                width = width.or(w);
            }
        }
    }
    // A stated outline with no width anywhere is DrawingML's default, 9525 EMU.
    let stroke = color.flatten().map(|color| Stroke { color, width: width.unwrap_or(9525.0) * scale });

    let round_adj = sppr
        .and_then(|p| p.find("a:gd"))
        .filter(|gd| gd.attr("name") == Some("adj"))
        .and_then(|gd| gd.attr("fmla")?.strip_prefix("val ")?.trim().parse::<f64>().ok())
        .map(|v| v / 100_000.0);

    SpStyle { style: ShapeStyle { fill, gradient, stroke }, round_adj }
}

/// The painting of every `p:sp` in a slide, layout or master part, in
/// document order. `scale` converts EMU to model units (for line widths).
pub(crate) fn sp_styles(xml: &str, theme: &Theme, scale: f64) -> Vec<SpStyle> {
    let root = parse_tree(xml);
    let mut shapes = Vec::new();
    root.find_all("p:sp", &mut shapes);
    shapes.into_iter().map(|sp| resolve_sp(sp, theme, scale)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 2007 Office theme python-pptx ships: fill style 3 is a gradient.
    const THEME_2007: &str = r#"<a:theme xmlns:a="a"><a:themeElements><a:clrScheme name="Office">
        <a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>
        <a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>
        <a:accent1><a:srgbClr val="4F81BD"/></a:accent1></a:clrScheme>
        <a:fmtScheme name="Office"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
        <a:gradFill><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="50000"/></a:schemeClr></a:gs></a:gsLst></a:gradFill>
        <a:gradFill rotWithShape="1"><a:gsLst>
          <a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="100000"/><a:shade val="100000"/><a:satMod val="130000"/></a:schemeClr></a:gs>
          <a:gs pos="100000"><a:schemeClr val="phClr"><a:tint val="50000"/><a:shade val="100000"/><a:satMod val="350000"/></a:schemeClr></a:gs>
        </a:gsLst><a:lin ang="16200000" scaled="0"/></a:gradFill></a:fillStyleLst>
        <a:lnStyleLst><a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"><a:shade val="95000"/><a:satMod val="105000"/></a:schemeClr></a:solidFill></a:ln>
        <a:ln w="25400"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst>
        </a:fmtScheme></a:themeElements></a:theme>"#;

    fn sp(sppr: &str, style: &str) -> String {
        format!("<p:sp><p:nvSpPr/><p:spPr>{sppr}</p:spPr>{style}</p:sp>")
    }

    const REFS: &str = r#"<p:style><a:lnRef idx="1"><a:schemeClr val="accent1"/></a:lnRef>
        <a:fillRef idx="3"><a:schemeClr val="accent1"/></a:fillRef><a:effectRef idx="2"><a:schemeClr val="accent1"/></a:effectRef>
        <a:fontRef idx="minor"><a:schemeClr val="lt1"/></a:fontRef></p:style>"#;

    #[test]
    fn the_theme_part_overrides_the_default_slots() {
        let t = theme(THEME_2007);
        assert_eq!(t.slot("accent1"), Some(Color(0x4F, 0x81, 0xBD)));
        assert_eq!(t.slot("tx1"), Some(Color(0, 0, 0)));
        assert_eq!(t.slot("accent2"), Some(Color(0xED, 0x7D, 0x31)), "unstated slots keep the default");
    }

    #[test]
    fn a_fill_reference_resolves_through_the_format_scheme_with_ph_clr() {
        let xml = sp(r#"<a:prstGeom prst="rect"/>"#, REFS);
        let s = &sp_styles(&xml, &theme(THEME_2007), 1.0)[0].style;
        let g = s.gradient.as_ref().expect("fill style 3 is a gradient");
        assert_eq!(g.stops.len(), 2);
        assert!((g.angle - 270.0).abs() < 1e-9, "16200000 is bottom to top");
        // The start stop is accent 1 (tint/shade 100%, more saturated); the
        // end stop is tinted halfway to white, so lighter.
        assert!(g.stops[1].color.0 > g.stops[0].color.0);
        assert_eq!(s.fill, g.mean());
        // Line style 1: 9525 EMU of accent 1 shaded 95%.
        let st = s.stroke.unwrap();
        assert!((st.width - 9525.0).abs() < 1e-9);
        assert_eq!(st.color, Color(0x4F, 0x81, 0xBD).shade(95_000).sat_mod(105_000));
    }

    #[test]
    fn an_explicit_fill_and_line_win_over_the_style_references() {
        let xml = sp(
            r#"<a:prstGeom prst="rect"/><a:solidFill><a:srgbClr val="DC2828"/></a:solidFill>
               <a:ln w="25400"><a:solidFill><a:srgbClr val="000000"/></a:solidFill></a:ln>"#,
            REFS,
        );
        let s = &sp_styles(&xml, &theme(THEME_2007), 0.1)[0].style;
        assert_eq!((s.fill, s.gradient.is_none()), (Some(Color(0xDC, 0x28, 0x28)), true));
        let st = s.stroke.unwrap();
        assert_eq!(st.color, Color(0, 0, 0));
        assert!((st.width - 2540.0).abs() < 1e-9, "25400 EMU at scale 0.1");
    }

    #[test]
    fn a_line_width_alone_takes_its_colour_from_the_reference() {
        let xml = sp(r#"<a:ln w="38100"/>"#, REFS);
        let st = sp_styles(&xml, &theme(THEME_2007), 1.0)[0].style.stroke.unwrap();
        assert!((st.width - 38100.0).abs() < 1e-9);
        assert_eq!(st.color, Color(0x4F, 0x81, 0xBD).shade(95_000).sat_mod(105_000));
    }

    #[test]
    fn no_fill_and_no_line_are_kept_as_none() {
        let xml = sp(r#"<a:noFill/><a:ln><a:noFill/></a:ln>"#, REFS);
        let s = &sp_styles(&xml, &Theme::default(), 1.0)[0].style;
        assert_eq!((s.fill, s.stroke), (None, None));
        let bare = sp("", "");
        let s = &sp_styles(&bare, &Theme::default(), 1.0)[0].style;
        assert_eq!((s.fill, s.stroke), (None, None));
    }

    #[test]
    fn text_colours_are_not_fills_and_round_rect_adjustments_are_read() {
        let xml = format!(
            "{}{}",
            sp(
                r#"<a:prstGeom prst="roundRect"><a:avLst><a:gd name="adj" fmla="val 25000"/></a:avLst></a:prstGeom>
                   <a:solidFill><a:schemeClr val="accent1"><a:lumMod val="75000"/></a:schemeClr></a:solidFill>"#,
                ""
            ),
            r#"<p:sp><p:spPr/><p:txBody><a:p><a:r><a:rPr><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></a:rPr></a:r></a:p></p:txBody></p:sp>"#,
        );
        let styles = sp_styles(&xml, &Theme::default(), 1.0);
        assert_eq!(styles.len(), 2);
        assert_eq!(styles[0].round_adj, Some(0.25));
        assert_eq!(styles[0].style.fill, Some(Color(0x44, 0x72, 0xC4).lum(Some(75_000), None)));
        assert_eq!(styles[1].style.fill, None);
    }
}
