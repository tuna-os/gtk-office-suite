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

use super::shape::{Color, ColorModulation, GradientStop, LinearGradient, ShapeStyle, Stroke};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::HashMap;

// ── A minimal element tree ────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub(super) struct Node {
    pub(super) name: String,
    pub(super) attrs: Vec<(String, String)>,
    pub(super) children: Vec<Node>,
    /// Text content (kept for `a:t`; whitespace-significant).
    pub(super) text: String,
}

impl Node {
    pub(super) fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
    pub(super) fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
    pub(super) fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
    /// Every descendant named `name`, in document order, not descending
    /// into matches. A walk with its own stack: a slide of nested groups is
    /// as deep as its author made it, and recursion overflowed the thread's
    /// stack on one 100,000 deep (#1206).
    pub(super) fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a Node>) {
        let mut stack = vec![self.children.iter()];
        while let Some(level) = stack.last_mut() {
            match level.next() {
                None => {
                    stack.pop();
                }
                Some(c) if c.name == name => out.push(c),
                Some(c) => stack.push(c.children.iter()),
            }
        }
    }
    pub(super) fn find(&self, name: &str) -> Option<&Node> {
        let mut v = Vec::new();
        self.find_all(name, &mut v);
        v.into_iter().next()
    }
}

/// Freed a level at a time: the derived drop recurses once per level of
/// nesting, which overflows on a deep enough part before anything reads it.
impl Drop for Node {
    fn drop(&mut self) {
        let mut pending = std::mem::take(&mut self.children);
        while let Some(mut node) = pending.pop() {
            pending.append(&mut node.children);
        }
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
        text: String::new(),
    }
}

/// The elements whose text the tree keeps: a run's text, and the GUID of
/// the style a table names.
fn keeps_text(name: &str) -> bool {
    matches!(name, "a:t" | "a:tableStyleId")
}

/// The element tree of `xml`. Text is kept only inside `a:t` (and a
/// table's `a:tableStyleId`), untrimmed: a run ending in a space is a word
/// boundary.
pub(super) fn parse_tree(xml: &str) -> Node {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
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
            Ok(Event::Text(ref t)) => {
                if let Some(top) = stack.last_mut().filter(|n| keeps_text(&n.name)) {
                    top.text.push_str(&super::parse::unescape_text(t));
                }
            }
            Ok(Event::GeneralRef(ref r)) => {
                if let Some(top) = stack.last_mut().filter(|n| keeps_text(&n.name)) {
                    top.text.push_str(&super::parse::resolve_general_ref(r));
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
#[derive(Debug, Clone)]
pub(crate) struct Theme {
    colors: HashMap<String, Color>,
    fmt: Node,
    /// `a:fontScheme`'s heading (`+mj-lt`) and body (`+mn-lt`) Latin
    /// typefaces, when the part names them.
    major_font: Option<String>,
    minor_font: Option<String>,
    /// The theme part's `name`, retained for the writer to carry back.
    name: String,
    /// A master's colour map (`p:clrMap`) over this theme: presentation
    /// names (`bg1`, `tx1` …) to the slots they draw from. Read-time
    /// only (the model keeps the map on the master beside the theme id,
    /// DCK-006); empty is the identity every package without one assumes.
    remap: HashMap<String, String>,
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
            fmt: std::mem::take(&mut parse_tree(
                r#"<a:fmtScheme><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
                <a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst>
                <a:lnStyleLst><a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>
                <a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>
                <a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst></a:fmtScheme>"#,
            )
            .children)
            .into_iter()
            .next()
            .unwrap_or_default(),
            major_font: None,
            minor_font: None,
            name: "Office".to_string(),
            remap: HashMap::new(),
        }
    }
}

impl Theme {
    /// A `schemeClr` value. A master's colour map (`remap`, from its
    /// `p:clrMap`) replaces the standard mapping of presentation names
    /// onto the dark/light slots where it states one; anything neither
    /// names resolves directly.
    pub(super) fn slot(&self, name: &str) -> Option<Color> {
        let slot = match self.remap.get(name) {
            Some(mapped) => mapped.as_str(),
            None => match name {
                "tx1" => "dk1",
                "bg1" => "lt1",
                "tx2" => "dk2",
                "bg2" => "lt2",
                other => other,
            },
        };
        self.colors.get(slot).copied()
    }

    /// A run's `a:latin typeface`, with the theme references `+mj-lt`
    /// (heading font) and `+mn-lt` (body font) resolved. `None` for a
    /// reference the theme doesn't answer, or an empty name.
    pub(super) fn typeface(&self, name: &str) -> Option<String> {
        let name = match name {
            "+mj-lt" => self.major_font.as_deref()?,
            "+mn-lt" => self.minor_font.as_deref()?,
            n if n.starts_with('+') => return None,
            n => n,
        };
        let name = name.trim();
        (!name.is_empty()).then(|| name.to_string())
    }

    /// The `n`th (1-based) child of a format-scheme list such as
    /// `a:fillStyleLst`.
    fn style(&self, list: &str, n: usize) -> Option<&Node> {
        self.fmt.child(list)?.children.get(n.checked_sub(1)?)
    }

    /// This theme resolving under a master's colour map (`p:clrMap`,
    /// presentation names to slots): a read-time copy (DCK-006), so the
    /// retained model theme keeps the part's own slots while masters
    /// with different maps resolve differently.
    pub(crate) fn with_remap(&self, map: &std::collections::BTreeMap<String, String>) -> Theme {
        let mut out = Theme {
            colors: self.colors.clone(),
            fmt: clone_node(&self.fmt),
            major_font: self.major_font.clone(),
            minor_font: self.minor_font.clone(),
            name: self.name.clone(),
            remap: HashMap::new(),
        };
        out.remap = map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        out
    }

    /// The heading (`+mj-lt`) and body (`+mn-lt`) Latin typefaces, when
    /// the part names them. The pptx reader carries these onto the
    /// master's `default_font` without claiming an absent font is Sans.
    pub(crate) fn fonts(&self) -> (Option<String>, Option<String>) {
        (self.major_font.clone(), self.minor_font.clone())
    }

    /// The retained model theme (engine::theme) for `Deck.themes`: the
    /// part's own slots, fonts and name with its source part path. The
    /// id is stamped by the caller from the session space (DCK-005).
    pub(crate) fn to_model(&self, id: u64, source: &str) -> super::theme::Theme {
        super::theme::Theme {
            id,
            name: self.name.clone(),
            colors: self.colors.iter().map(|(k, v)| (k.clone(), v.to_hex())).collect(),
            major_font: self.major_font.clone(),
            minor_font: self.minor_font.clone(),
            source: source.to_string(),
        }
    }
}

/// The theme in a theme part (`ppt/theme/themeN.xml`). Slots or lists the
/// part doesn't state keep the default theme's.
pub(crate) fn theme(theme_xml: &str) -> Theme {
    let mut theme = Theme::default();
    let root = parse_tree(theme_xml);
    if let Some(name) = root.child("a:theme").and_then(|t| t.attr("name")).map(str::trim).filter(|n| !n.is_empty()) {
        theme.name = name.to_string();
    }
    if let Some(scheme) = root.find("a:clrScheme") {
        for slot in &scheme.children {
            let value = slot.children.first().and_then(|c| match c.name.as_str() {
                "a:srgbClr" => c.attr("val").and_then(Color::from_hex),
                "a:sysClr" => system_color(c),
                _ => None,
            });
            if let Some(c) = value {
                theme.colors.insert(slot.name.trim_start_matches("a:").to_string(), c);
            }
        }
    }
    if let Some(fonts) = root.find("a:fontScheme") {
        let latin = |n: &str| {
            fonts
                .child(n)
                .and_then(|f| f.child("a:latin"))
                .and_then(|l| l.attr("typeface"))
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(String::from)
        };
        theme.major_font = latin("a:majorFont");
        theme.minor_font = latin("a:minorFont");
    }
    if let Some(fmt) = root.find("a:fmtScheme") {
        // Keep the part's own lists; fall back to the default's per list.
        let mut merged = Node::default();
        merged.name = fmt.name.clone();
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

/// A deep copy, built with its own stack: the theme part it copies from is
/// as deep as its author made it (#1206).
fn clone_node(n: &Node) -> Node {
    let shallow = |n: &Node| {
        let mut copy = Node::default();
        copy.name = n.name.clone();
        copy.attrs = n.attrs.clone();
        copy.text = n.text.clone();
        copy
    };
    // Each entry: a source node, its copy so far, and the next child to copy.
    let mut stack: Vec<(&Node, Node, usize)> = vec![(n, shallow(n), 0)];
    loop {
        let (source, _, next) = stack.last_mut().expect("the root is popped last");
        let source: &Node = source;
        if let Some(child) = source.children.get(*next) {
            *next += 1;
            stack.push((child, shallow(child), 0));
            continue;
        }
        let (_, done, _) = stack.pop().expect("checked above");
        match stack.last_mut() {
            Some((_, parent, _)) => parent.children.push(done),
            None => return done,
        }
    }
}

// ── Colours, fills, lines ─────────────────────────────────────────────────

const COLOR_ELEMENTS: [&str; 4] = ["a:srgbClr", "a:schemeClr", "a:sysClr", "a:prstClr"];

/// A system colour (`a:sysClr`): the colour the system answers for it,
/// not the one the author's machine last had (`lastClr`). The window and
/// its text are the ones a theme names (lt1/dk1); LibreOffice and a stock
/// PowerPoint draw them white and black. A theme saved on a machine in a
/// dark mode records `window` with `lastClr="202020"`, and taking that
/// turned every bg1 background near-black (the 2024-2025 delegated
/// authorities report, DCK-004). Any other system colour falls back to
/// its recorded `lastClr`.
pub(crate) fn system_color(node: &Node) -> Option<Color> {
    match node.attr("val") {
        Some("window") => Some(Color(0xFF, 0xFF, 0xFF)),
        Some("windowText") => Some(Color(0, 0, 0)),
        _ => node.attr("lastClr").and_then(Color::from_hex),
    }
}

/// A colour element with its modifiers applied, `phClr` standing for
/// `placeholder` (the `p:style` reference's own colour).
fn color_of(node: &Node, theme: &Theme, placeholder: Option<Color>) -> Option<Color> {
    let base = match node.name.as_str() {
        "a:srgbClr" => node.attr("val").and_then(Color::from_hex),
        "a:sysClr" => system_color(node),
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
pub(super) fn first_color(node: &Node, theme: &Theme, placeholder: Option<Color>) -> Option<Color> {
    node.children
        .iter()
        .find(|c| COLOR_ELEMENTS.contains(&c.name.as_str()))
        .and_then(|c| color_of(c, theme, placeholder))
}

/// A resolved fill.
#[derive(Clone)]
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

/// A direct `a:solidFill` colour statement as its source states it
/// (DCK-006): a theme token with its modulation, or a literal sRGB.
/// System and preset colours resolve to their hex (the same values
/// `color_of` draws); `phClr` — the style reference's own colour — is
/// not a token of this statement, so `None`, resolved by the caller.
/// The caller passes the `a:solidFill` node itself.
fn direct_color_ref(node: &Node) -> Option<super::theme::ColorRef> {
    use super::theme::{ColorMod, ColorRef, ThemeColor};
    let c = node.children.iter().find(|c| COLOR_ELEMENTS.contains(&c.name.as_str()))?;
    match c.name.as_str() {
        "a:srgbClr" => c.attr("val").and_then(Color::from_hex).map(|c| ColorRef::Rgb(c.to_hex())),
        "a:sysClr" => system_color(c).map(|c| ColorRef::Rgb(c.to_hex())),
        "a:prstClr" => match c.attr("val") {
            Some("black") => Some(ColorRef::Rgb("000000".to_string())),
            Some("white") => Some(ColorRef::Rgb("FFFFFF".to_string())),
            _ => None,
        },
        "a:schemeClr" => {
            let slot = c.attr("val")?;
            // `phClr` is the style reference's own colour, not a token of
            // this statement: no reference, resolved by the caller.
            if slot == "phClr" {
                return None;
            }
            let mut mods = Vec::new();
            for m in &c.children {
                let v: Option<i32> = m.attr("val").and_then(|v| v.parse().ok());
                let Some(v) = v else { continue };
                mods.push(match m.name.as_str() {
                    "a:tint" => ColorMod::Tint(v),
                    "a:shade" => ColorMod::Shade(v),
                    "a:satMod" => ColorMod::SatMod(v),
                    "a:lumMod" => ColorMod::LumMod(v),
                    "a:lumOff" => ColorMod::LumOff(v),
                    _ => continue,
                });
            }
            Some(ColorRef::Theme(ThemeColor { slot: slot.to_string(), mods }))
        }
        _ => None,
    }
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

fn resolve_sp(sp: &Node, theme: &Theme, scale: f64, group_fill: Option<&Fill>) -> SpStyle {
    use super::theme::{FillSrc, StrokeSrc};
    let sppr = sp.child("p:spPr");
    let style = sp.child("p:style");
    let reference = |name: &str| -> Option<(usize, Option<Color>)> {
        let r = style?.child(name)?;
        let idx = r.attr("idx").and_then(|v| v.parse().ok()).unwrap_or(0);
        Some((idx, first_color(r, theme, None)))
    };

    // The shape's own fill statement, as stated (DCK-006): an explicit
    // no-fill, or a direct literal or theme-token colour. Anything else
    // — a style or group reference, a gradient, no statement — leaves
    // no reference and the resolved cache stays canonical.
    let fill_src = match sppr {
        Some(p) if p.child("a:grpFill").is_some() => None,
        Some(p) => match p.children.iter().find(|c| matches!(c.name.as_str(), "a:noFill" | "a:solidFill")) {
            Some(n) if n.name == "a:noFill" => Some(FillSrc::NoFill),
            Some(n) => direct_color_ref(n).map(FillSrc::Color),
            None => None,
        },
        None => None,
    };
    // Fill: the shape's own, else its fillRef into the format scheme.
    let own_fill = match sppr {
        // `a:grpFill`: the fill of the group the shape is in.
        Some(p) if p.child("a:grpFill").is_some() => group_fill.cloned(),
        _ => sppr.and_then(|p| fill_among(p, theme, None)),
    };
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

    // The shape's own outline statement, as stated: an explicit no-line,
    // or a direct colour. A width-only line, a style reference or no
    // statement leaves no reference (see `fill_src`).
    let stroke_src = match sppr.and_then(|p| p.child("a:ln")) {
        Some(ln) => match ln.children.iter().find(|c| matches!(c.name.as_str(), "a:noFill" | "a:solidFill")) {
            Some(n) if n.name == "a:noFill" => Some(StrokeSrc::NoLine),
            Some(n) => direct_color_ref(n).map(StrokeSrc::Color),
            None => None,
        },
        None => None,
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

    SpStyle { style: ShapeStyle { fill, gradient, stroke, fill_src, stroke_src }, round_adj }
}

// ── Tables ────────────────────────────────────────────────────────────────

/// A table found in a `p:graphicFrame`: its box (EMU, before scaling) and
/// content. `index` is the frame's position among all `p:graphicFrame`s in
/// the part, so the slide walker can place it in document order.
pub(crate) struct FrameTable {
    pub index: usize,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub table: super::table::TableData,
}

/// A table cell's runs. `links` are the part's external hyperlink
/// relationships, for an `a:hlinkClick` on a run (#1998).
fn runs_of(tx_body: &Node, links: &std::collections::HashMap<String, String>) -> Vec<suite_common_core::document::Run> {
    use suite_common_core::document::{Run, RunStyle};
    let mut runs = Vec::new();
    for (pi, p) in tx_body.children_named("a:p").enumerate() {
        if pi > 0 {
            runs.push(Run { text: "\n".into(), style: RunStyle::default() });
        }
        for r in p.children.iter().filter(|c| c.name == "a:r" || c.name == "a:br") {
            // A line break inside the paragraph, on the run before it.
            if r.name == "a:br" {
                match runs.last_mut().filter(|last: &&mut Run| !last.text.ends_with('\n')) {
                    Some(last) => last.text.push(super::LINE_BREAK),
                    None => runs.push(Run { text: super::LINE_BREAK.to_string(), style: RunStyle::default() }),
                }
                continue;
            }
            let mut style = RunStyle::default();
            if let Some(rpr) = r.child("a:rPr") {
                let on = |k: &str| matches!(rpr.attr(k), Some("1") | Some("true"));
                style.bold = on("b");
                style.italic = on("i");
                if let Some(sz) = rpr.attr("sz").and_then(|v| v.parse::<u32>().ok()) {
                    style.font_size_hp = Some((sz / 50) as u16);
                }
                style.link = rpr
                    .child("a:hlinkClick")
                    .filter(|h| h.attr("action").is_none_or(str::is_empty))
                    .and_then(|h| h.attr("r:id"))
                    .and_then(|id| links.get(id))
                    .filter(|t| !t.is_empty())
                    .cloned();
            }
            let text: String = r.children_named("a:t").map(|t| t.text.as_str()).collect();
            if !text.is_empty() {
                runs.push(Run { text, style });
            }
        }
    }
    runs
}

/// Every table in a part's graphic frames.
/// `links` are the part's external hyperlink relationships, by id.
pub(crate) fn frame_tables(xml: &str, theme: &Theme, links: &std::collections::HashMap<String, String>) -> Vec<FrameTable> {
    use super::table::{TableCell, TableData};
    let root = parse_tree(xml);
    let mut frames = Vec::new();
    root.find_all("p:graphicFrame", &mut frames);
    frames
        .into_iter()
        .enumerate()
        .filter_map(|(index, frame)| {
            let xfrm = frame.child("p:xfrm")?;
            let num = |n: Option<&Node>, k: &str| n.and_then(|n| n.attr(k)).and_then(|v| v.parse::<f64>().ok());
            let (off, ext) = (xfrm.child("a:off"), xfrm.child("a:ext"));
            let tbl = frame.find("a:tbl")?;
            let tbl_pr = tbl.child("a:tblPr");
            let flag = |k: &str| matches!(tbl_pr.and_then(|p| p.attr(k)), Some("1") | Some("true"));
            let style_id = tbl_pr.and_then(|p| p.child("a:tableStyleId")).map(|n| n.text.trim().to_string());
            let style = style_id.as_deref().and_then(super::table::builtin_style);
            let col_widths = tbl
                .child("a:tblGrid")
                .map(|g| g.children_named("a:gridCol").filter_map(|c| c.attr("w")?.parse().ok()).collect())
                .unwrap_or_default();
            let mut row_heights = Vec::new();
            let rows = tbl
                .children_named("a:tr")
                .map(|tr| {
                    row_heights.push(tr.attr("h").and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0));
                    tr.children_named("a:tc")
                        .map(|tc| {
                            let span = |k: &str| tc.attr(k).and_then(|v| v.parse::<u32>().ok()).unwrap_or(1).clamp(1, 1000);
                            let set = |k: &str| matches!(tc.attr(k), Some("1" | "true"));
                            TableCell {
                                runs: tc.child("a:txBody").map(|b| runs_of(b, links)).unwrap_or_default(),
                                fill: tc
                                    .child("a:tcPr")
                                    .and_then(|p| p.child("a:solidFill"))
                                    .and_then(|f| first_color(f, theme, None)),
                                col_span: span("gridSpan"),
                                row_span: span("rowSpan"),
                                covered: set("hMerge") || set("vMerge"),
                            }
                        })
                        .collect()
                })
                .collect();
            Some(FrameTable {
                index,
                x: num(off, "x").unwrap_or(0.0),
                y: num(off, "y").unwrap_or(0.0),
                w: num(ext, "cx").unwrap_or(0.0),
                h: num(ext, "cy").unwrap_or(0.0),
                table: TableData {
                    col_widths,
                    row_heights,
                    rows,
                    first_row: flag("firstRow"),
                    band_row: flag("bandRow"),
                    // The style's own colour: an accent, or the dark text
                    // colour for a family's plain style.
                    accent: theme.slot(style.map_or("accent1", |(_, slot)| slot)),
                    cell_margins: None,
                    style_id: style_id.filter(|_| style.is_some()),
                },
            })
        })
        .collect()
}

// ── Charts ────────────────────────────────────────────────────────────────

/// A chart found in a `p:graphicFrame`: its box (EMU, before scaling) and
/// the relationship that names its chart part. `index` is the frame's
/// position among all `p:graphicFrame`s in the part, as a table's is.
pub(crate) struct FrameChart {
    pub index: usize,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub rel_id: String,
}

/// Every chart in a part's graphic frames: a `c:chart` (in any prefix)
/// with an `r:id`.
pub(crate) fn frame_charts(xml: &str) -> Vec<FrameChart> {
    let root = parse_tree(xml);
    let mut frames = Vec::new();
    root.find_all("p:graphicFrame", &mut frames);
    fn chart_ref(n: &Node) -> Option<&Node> {
        n.children.iter().find_map(|c| {
            let local = c.name.rsplit(':').next().unwrap_or(&c.name);
            if local == "chart" && c.attr("r:id").is_some() { Some(c) } else { chart_ref(c) }
        })
    }
    frames
        .into_iter()
        .enumerate()
        .filter_map(|(index, frame)| {
            let xfrm = frame.child("p:xfrm")?;
            let num = |n: Option<&Node>, k: &str| n.and_then(|n| n.attr(k)).and_then(|v| v.parse::<f64>().ok());
            let (off, ext) = (xfrm.child("a:off"), xfrm.child("a:ext"));
            let rel_id = chart_ref(frame.child("a:graphic")?)?.attr("r:id")?.to_string();
            Some(FrameChart {
                index,
                x: num(off, "x").unwrap_or(0.0),
                y: num(off, "y").unwrap_or(0.0),
                w: num(ext, "cx").unwrap_or(0.0),
                h: num(ext, "cy").unwrap_or(0.0),
                rel_id,
            })
        })
        .collect()
}

/// The painting of every `p:sp` in a slide, layout or master part, in
/// document order. `scale` converts EMU to model units (for line widths).
pub(crate) fn sp_styles(xml: &str, theme: &Theme, scale: f64) -> Vec<SpStyle> {
    styles_of(xml, "p:sp", theme, scale)
}

/// The painting of every connector (`p:cxnSp`) in a part, in document
/// order: its outline from `a:ln` over the theme's `a:lnRef`, as a shape's.
/// Counted apart from the `p:sp`s so their order is untouched.
pub(crate) fn cxn_styles(xml: &str, theme: &Theme, scale: f64) -> Vec<SpStyle> {
    styles_of(xml, "p:cxnSp", theme, scale)
}

fn styles_of(xml: &str, tag: &str, theme: &Theme, scale: f64) -> Vec<SpStyle> {
    let root = parse_tree(xml);
    // Every p:sp in document order, as `find_all` finds them, each with the
    // fill of the group it is in: a shape filled `a:grpFill` takes it.
    let mut out = Vec::new();
    let mut stack: Vec<(std::slice::Iter<'_, Node>, Option<Fill>)> = vec![(root.children.iter(), None)];
    while let Some((level, group_fill)) = stack.last_mut() {
        let group_fill = group_fill.clone();
        match level.next() {
            None => {
                stack.pop();
            }
            Some(c) if c.name == tag => out.push(resolve_sp(c, theme, scale, group_fill.as_ref())),
            Some(c) => {
                // A group's own fill, or the one around it, for its shapes.
                let fill = if c.name == "p:grpSp" {
                    c.child("p:grpSpPr").and_then(|p| fill_among(p, theme, None)).or(group_fill)
                } else {
                    group_fill
                };
                stack.push((c.children.iter(), fill));
            }
        }
    }
    out
}

/// Each p:sp's custom geometry (`a:custGeom`), in document order like
/// `sp_styles`: its paths, or `None` for a preset or a geometry we cannot
/// draw.
pub(crate) fn sp_geometries(xml: &str) -> Vec<Option<Vec<super::freeform::FreePath>>> {
    let root = parse_tree(xml);
    let mut sps = Vec::new();
    root.find_all("p:sp", &mut sps);
    sps.into_iter().map(|sp| sp.child("p:spPr").and_then(|p| p.child("a:custGeom")).and_then(cust_geom)).collect()
}

/// The paths of one `a:custGeom`: each `a:path` in `a:pathLst`. A
/// coordinate given by a guide's name rather than a number makes the
/// geometry one we cannot draw, and `None`; so does one with no path. A
/// path of no size (`w`, `h` 0 or absent) is in the shape's own EMU, which
/// the caller knows: it is left 0.
pub(super) fn cust_geom(geom: &Node) -> Option<Vec<super::freeform::FreePath>> {
    use super::freeform::{FreePath, PathCmd};
    let num = |n: &Node, k: &str| n.attr(k).and_then(|v| v.parse::<f64>().ok());
    let mut paths = Vec::new();
    for path in geom.child("a:pathLst")?.children_named("a:path") {
        let mut cmds = Vec::new();
        for c in &path.children {
            let pts: Option<Vec<(f64, f64)>> = c.children_named("a:pt").map(|p| Some((num(p, "x")?, num(p, "y")?))).collect();
            let cmd = match (c.name.as_str(), pts?.as_slice()) {
                ("a:moveTo", [a]) => PathCmd::Move(a.0, a.1),
                ("a:lnTo", [a]) => PathCmd::Line(a.0, a.1),
                ("a:cubicBezTo", [a, b, d]) => PathCmd::Cubic(a.0, a.1, b.0, b.1, d.0, d.1),
                ("a:quadBezTo", [a, b]) => PathCmd::Quad(a.0, a.1, b.0, b.1),
                ("a:arcTo", []) => PathCmd::Arc {
                    wr: num(c, "wR")?,
                    hr: num(c, "hR")?,
                    start: num(c, "stAng")? / 60_000.0,
                    swing: num(c, "swAng")? / 60_000.0,
                },
                ("a:close", []) => PathCmd::Close,
                _ => return None,
            };
            cmds.push(cmd);
        }
        if !cmds.is_empty() {
            paths.push(FreePath {
                w: num(path, "w").unwrap_or(0.0),
                h: num(path, "h").unwrap_or(0.0),
                cmds,
                fill: path.attr("fill") != Some("none"),
                stroke: !matches!(path.attr("stroke"), Some("0" | "false")),
            });
        }
    }
    (!paths.is_empty()).then_some(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table names its style by GUID: the reader keeps it and takes the
    /// style's own colour (dark 1 for a plain style).
    #[test]
    fn a_table_keeps_its_built_in_style() {
        let xml = r#"<p:spTree><p:graphicFrame><p:xfrm><a:off x="0" y="0"/><a:ext cx="100" cy="100"/></p:xfrm><a:graphic><a:graphicData><a:tbl>
            <a:tblPr firstRow="1" bandRow="1"><a:tableStyleId>{616DA210-FB5B-4158-B5E0-FEB733F419BA}</a:tableStyleId></a:tblPr>
            <a:tblGrid><a:gridCol w="100"/></a:tblGrid><a:tr h="50"><a:tc><a:txBody><a:p><a:r><a:t>Year</a:t></a:r></a:p></a:txBody></a:tc></a:tr>
            </a:tbl></a:graphicData></a:graphic></p:graphicFrame></p:spTree>"#;
        let tables = frame_tables(xml, &Theme::default(), &Default::default());
        let t = &tables[0].table;
        assert_eq!(t.style_id.as_deref(), Some("{616DA210-FB5B-4158-B5E0-FEB733F419BA}"));
        assert_eq!(t.family(), super::super::table::StyleFamily::LightStyle3);
    }

    #[test]
    fn a_custom_geometry_reads_as_its_paths_and_back() {
        use super::super::freeform::{tests::wedge, to_cust_geom};
        let xml = format!("<p:spTree><p:sp><p:spPr>{}</p:spPr></p:sp><p:sp><p:spPr><a:prstGeom prst=\"rect\"/></p:spPr></p:sp></p:spTree>", to_cust_geom(&wedge()));
        assert_eq!(sp_geometries(&xml), [Some(wedge()), None]);
    }

    /// A coordinate named by a guide is not one we can place: the shape is
    /// left to its box rather than drawn wrong.
    #[test]
    fn a_guide_coordinate_is_not_drawn() {
        let xml = r#"<p:sp><p:spPr><a:custGeom><a:pathLst><a:path w="10" h="10"><a:moveTo><a:pt x="l" y="t"/></a:moveTo><a:lnTo><a:pt x="10" y="10"/></a:lnTo></a:path></a:pathLst></a:custGeom></p:spPr></p:sp>"#;
        assert_eq!(sp_geometries(xml), [None]);
    }

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
    fn a_graphic_frame_table_is_read_with_its_grid_text_and_style_flags() {
        let xml = r#"<p:spTree><p:graphicFrame><p:xfrm><a:off x="914400" y="914400"/><a:ext cx="7315200" cy="2743200"/></p:xfrm>
          <a:graphic><a:graphicData><a:tbl><a:tblPr firstRow="1" bandRow="1"/>
          <a:tblGrid><a:gridCol w="2438400"/><a:gridCol w="2438400"/></a:tblGrid>
          <a:tr h="914400"><a:tc><a:txBody><a:p><a:r><a:rPr b="1"/><a:t>Head </a:t></a:r><a:r><a:t>one</a:t></a:r></a:p></a:txBody><a:tcPr/></a:tc>
            <a:tc><a:txBody><a:p><a:r><a:t>R1C2</a:t></a:r></a:p></a:txBody><a:tcPr><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></a:tcPr></a:tc></a:tr>
          </a:tbl></a:graphicData></a:graphic></p:graphicFrame></p:spTree>"#;
        let t = frame_tables(xml, &theme(THEME_2007), &Default::default());
        assert_eq!(t.len(), 1);
        let f = &t[0];
        assert_eq!((f.index, f.x, f.w), (0, 914400.0, 7315200.0));
        let d = &f.table;
        assert_eq!(d.col_widths, vec![2438400.0, 2438400.0]);
        assert_eq!(d.row_heights, vec![914400.0]);
        assert!(d.first_row && d.band_row);
        assert_eq!(d.rows[0][0].text(), "Head one");
        assert!(d.rows[0][0].runs[0].style.bold);
        assert_eq!(d.rows[0][1].fill, Some(Color(0xFF, 0, 0)));
        assert_eq!(d.accent, Some(Color(0x4F, 0x81, 0xBD)));
    }

    #[test]
    fn the_theme_part_overrides_the_default_slots() {
        let t = theme(THEME_2007);
        assert_eq!(t.slot("accent1"), Some(Color(0x4F, 0x81, 0xBD)));
        assert_eq!(t.slot("tx1"), Some(Color(0, 0, 0)));
        assert_eq!(t.slot("accent2"), Some(Color(0xED, 0x7D, 0x31)), "unstated slots keep the default");
    }

    /// DCK-004: the 2024-2025 delegated authorities report's theme was
    /// saved in a dark mode, `lt1` recorded as `window` with
    /// `lastClr="202020"`. The window is white (as LibreOffice draws it),
    /// so a master's `bg1` background stays white rather than near-black.
    #[test]
    fn a_dark_mode_last_colour_does_not_become_the_window_colour() {
        let xml = r#"<a:theme name="t"><a:themeElements><a:clrScheme name="s">
            <a:dk1><a:sysClr val="windowText" lastClr="FFFFFF"/></a:dk1>
            <a:lt1><a:sysClr val="window" lastClr="202020"/></a:lt1>
            <a:dk2><a:sysClr val="highlight" lastClr="0078D7"/></a:dk2>
            </a:clrScheme></a:themeElements></a:theme>"#;
        let t = theme(xml);
        assert_eq!(t.slot("bg1"), Some(Color(0xFF, 0xFF, 0xFF)));
        assert_eq!(t.slot("tx1"), Some(Color(0, 0, 0)));
        assert_eq!(t.slot("dk2"), Some(Color(0x00, 0x78, 0xD7)), "other system colours keep their last colour");
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
    fn direct_colours_keep_their_source_reference() {
        use super::super::theme::{ColorRef, FillSrc, StrokeSrc, ThemeColor};
        let xml = sp(
            r#"<a:prstGeom prst="rect"/><a:solidFill><a:schemeClr val="accent1"><a:shade val="50000"/></a:schemeClr></a:solidFill>
               <a:ln w="12700"><a:solidFill><a:srgbClr val="112233"/></a:solidFill></a:ln>"#,
            "",
        );
        let s = &sp_styles(&xml, &Theme::default(), 1.0)[0].style;
        assert_eq!(
            &s.fill_src,
            &Some(FillSrc::Color(ColorRef::Theme(ThemeColor {
                slot: "accent1".into(),
                mods: vec![super::super::theme::ColorMod::Shade(50_000)],
            })))
        );
        assert_eq!(s.fill, Some(Color(0x44, 0x72, 0xC4).shade(50_000)), "the resolved cache agrees with the reference");
        assert_eq!(&s.stroke_src, &Some(StrokeSrc::Color(ColorRef::Rgb("112233".into()))));
    }

    #[test]
    fn an_explicit_no_fill_is_not_an_absent_fill() {
        use super::super::theme::{FillSrc, StrokeSrc};
        let xml = sp(r#"<a:noFill/><a:ln><a:noFill/></a:ln>"#, "");
        let s = &sp_styles(&xml, &Theme::default(), 1.0)[0].style;
        assert_eq!((&s.fill_src, &s.stroke_src), (&Some(FillSrc::NoFill), &Some(StrokeSrc::NoLine)));
        let bare = sp("", "");
        let s = &sp_styles(&bare, &Theme::default(), 1.0)[0].style;
        assert_eq!((&s.fill_src, &s.stroke_src), (&None, &None));
    }

    #[test]
    fn a_colour_map_remaps_presentation_names() {
        let base = theme(THEME_2007);
        let map: std::collections::BTreeMap<String, String> =
            [("tx1", "accent1"), ("bg1", "lt1")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let themed = base.with_remap(&map);
        assert_eq!(themed.slot("tx1"), base.slot("accent1"));
        assert_eq!(themed.slot("bg1"), base.slot("lt1"));
        assert_eq!(Theme::default().slot("tx1"), Some(Color(0, 0, 0)), "no map: the standard mapping");
    }

    #[test]
    fn a_theme_part_converts_to_the_retained_model() {
        let t = theme(THEME_2007).to_model(7, "ppt/theme/theme2.xml");
        assert_eq!((t.id, t.source.as_str()), (7, "ppt/theme/theme2.xml"));
        assert_eq!(t.colors.get("accent1").map(String::as_str), Some("4F81BD"));
        // `sysClr` slots keep their `lastClr`, as resolution does.
        assert_eq!(t.colors.get("dk1").map(String::as_str), Some("000000"));
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
