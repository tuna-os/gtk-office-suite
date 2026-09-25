// odp_graphics.rs — how an odp shape is painted: its graphic style (fill,
// gradient, outline), and the named linear gradients those styles use.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Written by odp.rs's writer (`gradient_defs`, `gradient_attrs`) and read
// back by its reader (`GraphicDefs`). Before this, the writer wrote a
// gradient as its mean colour and the reader dropped a shape's paint
// altogether, so a themed deck (decks_core::templates) lost its
// decorations to one odp save and reopen.
//
// Gradients: ODF's `draw:gradient` has two colours and an angle. A
// gradient of more stops is written with its first and last. The angle is
// counter-clockwise from top-to-bottom (ODF), where DrawingML's and the
// model's is clockwise from left-to-right, so odf = 90 - model.
//
// A style's properties are inherited through `style:parent-style-name`
// and then the part's `style:default-style`, which is where LibreOffice
// keeps its default blue fill.

use std::collections::HashMap;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::engine::shape::{Color, GradientStop, LinearGradient, ShapeKind, ShapeStyle, Stroke};
use crate::engine::{Deck, SlideObject};

fn attr(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    crate::odp::attr_of(e, name)
}

fn hex(c: Color) -> String {
    format!("#{}", c.to_hex().to_lowercase())
}

fn color(v: &str) -> Option<Color> {
    let v = v.trim().trim_start_matches('#');
    if v.len() != 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&v[i..i + 2], 16).ok();
    Some(Color(p(0)?, p(2)?, p(4)?))
}

/// The deck's gradients in first-use order (slides, then masters): the
/// `draw:gradient`s styles.xml declares, named `decksGradient<n>`.
pub(crate) fn deck_gradients(deck: &Deck) -> Vec<LinearGradient> {
    let mut out: Vec<LinearGradient> = Vec::new();
    let objects = deck.slides.iter().flat_map(|s| &s.objects).chain(deck.masters.iter().flat_map(|m| &m.shapes));
    for o in objects {
        if let SlideObject::Shape { style: ShapeStyle { gradient: Some(g), .. }, .. } = o {
            if !out.contains(g) {
                out.push(g.clone());
            }
        }
    }
    out
}

fn gradient_name(i: usize) -> String {
    format!("decksGradient{}", i + 1)
}

/// ODF's angle for a model (DrawingML) angle, in degrees within 0..360.
fn odf_angle(model: f64) -> f64 {
    (90.0 - model).rem_euclid(360.0)
}

/// The `draw:gradient` declarations, for styles.xml's `office:styles`.
pub(crate) fn gradient_defs(gradients: &[LinearGradient]) -> String {
    gradients
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let first = g.stops.first().map(|s| s.color).unwrap_or(Color(0, 0, 0));
            let last = g.stops.last().map(|s| s.color).unwrap_or(first);
            format!(
                "<draw:gradient draw:name=\"{}\" draw:style=\"linear\" draw:start-color=\"{}\" draw:end-color=\"{}\" \
                 draw:start-intensity=\"100%\" draw:end-intensity=\"100%\" draw:angle=\"{}deg\" draw:border=\"0%\"/>",
                gradient_name(i),
                hex(first),
                hex(last),
                (odf_angle(g.angle) * 1e4).round() / 1e4
            )
        })
        .collect()
}

/// The fill attributes of a graphic style painted `style`: a named
/// gradient when it has one (`gradients` is `deck_gradients`), else its
/// solid fill, else none.
pub(crate) fn fill_attrs(style: &ShapeStyle, gradients: &[LinearGradient]) -> String {
    if let Some(i) = style.gradient.as_ref().and_then(|g| gradients.iter().position(|x| x == g)) {
        return format!("draw:fill=\"gradient\" draw:fill-gradient-name=\"{}\"", gradient_name(i));
    }
    match style.fill {
        Some(c) => format!("draw:fill=\"solid\" draw:fill-color=\"{}\"", hex(c)),
        None => "draw:fill=\"none\"".to_string(),
    }
}

/// One graphic style's own properties, before inheritance.
#[derive(Clone, Debug, Default)]
struct Props {
    parent: Option<String>,
    fill: Option<String>,
    fill_color: Option<Color>,
    gradient: Option<String>,
    stroke: Option<String>,
    stroke_color: Option<Color>,
    stroke_width_pt: Option<f64>,
}

impl Props {
    fn over(&mut self, base: &Props) {
        self.fill = self.fill.take().or_else(|| base.fill.clone());
        self.fill_color = self.fill_color.or(base.fill_color);
        self.gradient = self.gradient.take().or_else(|| base.gradient.clone());
        self.stroke = self.stroke.take().or_else(|| base.stroke.clone());
        self.stroke_color = self.stroke_color.or(base.stroke_color);
        self.stroke_width_pt = self.stroke_width_pt.or(base.stroke_width_pt);
    }
}

/// The graphic styles and gradients of an odp package (content.xml and
/// styles.xml both read in).
#[derive(Debug, Default)]
pub(crate) struct GraphicDefs {
    styles: HashMap<String, Props>,
    default: Props,
    gradients: HashMap<String, LinearGradient>,
}

impl GraphicDefs {
    pub(crate) fn read(&mut self, xml: &str) {
        let mut reader = Reader::from_str(xml);
        // The graphic style being read (None: the default style).
        let mut cur: Option<Option<String>> = None;
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.name().as_ref() {
                    "style:style" => {
                        cur = (attr(&e, "style:family").as_deref() == Some("graphic")).then(|| attr(&e, "style:name"));
                        if let Some(Some(name)) = &cur {
                            self.styles.entry(name.clone()).or_default().parent = attr(&e, "style:parent-style-name");
                        }
                    }
                    "style:default-style" => {
                        cur = (attr(&e, "style:family").as_deref() == Some("graphic")).then_some(None);
                    }
                    "style:graphic-properties" => {
                        let Some(which) = &cur else { continue };
                        let p = match which {
                            Some(name) => self.styles.entry(name.clone()).or_default(),
                            None => &mut self.default,
                        };
                        if let Some(v) = attr(&e, "draw:fill") {
                            p.fill = Some(v);
                        }
                        if let Some(c) = attr(&e, "draw:fill-color").as_deref().and_then(color) {
                            p.fill_color = Some(c);
                        }
                        if let Some(v) = attr(&e, "draw:fill-gradient-name") {
                            p.gradient = Some(v);
                        }
                        if let Some(v) = attr(&e, "draw:stroke") {
                            p.stroke = Some(v);
                        }
                        if let Some(c) = attr(&e, "svg:stroke-color").as_deref().and_then(color) {
                            p.stroke_color = Some(c);
                        }
                        if let Some(w) = attr(&e, "svg:stroke-width").and_then(|v| crate::odp::parse_length_pt(&v)) {
                            p.stroke_width_pt = Some(w);
                        }
                    }
                    "draw:gradient" => {
                        let (Some(name), Some(start), Some(end)) = (
                            attr(&e, "draw:name"),
                            attr(&e, "draw:start-color").as_deref().and_then(color),
                            attr(&e, "draw:end-color").as_deref().and_then(color),
                        ) else {
                            continue;
                        };
                        let angle = attr(&e, "draw:angle").map(|v| parse_angle(&v)).unwrap_or(0.0);
                        let g = LinearGradient {
                            stops: vec![GradientStop { pos: 0.0, color: start }, GradientStop { pos: 1.0, color: end }],
                            angle: (90.0 - angle).rem_euclid(360.0),
                        };
                        self.gradients.insert(name, g);
                    }
                    _ => {}
                },
                Ok(Event::End(e)) if matches!(e.name().as_ref(), "style:style" | "style:default-style") => cur = None,
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
    }

    /// The paint of graphic style `name`, its outline scaled by `k` (model
    /// units per point). `None` when the package defines no such style.
    pub(crate) fn style(&self, name: &str, k: f64) -> Option<ShapeStyle> {
        let mut p = self.styles.get(name)?.clone();
        let mut parent = p.parent.clone();
        let mut seen = 0;
        while let Some(n) = parent.take() {
            let Some(base) = self.styles.get(&n) else { break };
            p.over(base);
            parent = base.parent.clone();
            seen += 1;
            if seen > 16 {
                break; // a cycle
            }
        }
        p.over(&self.default);
        let gradient = match p.fill.as_deref() {
            Some("gradient") => p.gradient.as_ref().and_then(|g| self.gradients.get(g)).cloned(),
            _ => None,
        };
        let fill = match p.fill.as_deref() {
            Some("solid") => p.fill_color,
            Some("gradient") => gradient.as_ref().and_then(LinearGradient::mean),
            _ => None,
        };
        let stroke = match p.stroke.as_deref() {
            Some("solid") | Some("dash") => p.stroke_color.map(|color| Stroke { color, width: p.stroke_width_pt.unwrap_or(0.0) * k }),
            _ => None,
        };
        Some(ShapeStyle { fill, gradient, stroke })
    }
}

/// An ODF angle: degrees with a unit, or tenths of a degree without one
/// (ODF 1.2's form, which LibreOffice still writes).
fn parse_angle(v: &str) -> f64 {
    let v = v.trim();
    if let Some(d) = v.strip_suffix("deg") {
        return d.trim().parse().unwrap_or(0.0);
    }
    if let Some(r) = v.strip_suffix("rad") {
        return r.trim().parse::<f64>().map(f64::to_degrees).unwrap_or(0.0);
    }
    if let Some(g) = v.strip_suffix("grad") {
        return g.trim().parse::<f64>().map(|g| g * 0.9).unwrap_or(0.0);
    }
    v.parse::<f64>().map(|t| t / 10.0).unwrap_or(0.0)
}

/// The model's shape kind for an ODF shape: a `draw:rect` (rounded when it
/// has a corner radius, `radius` its fraction of the shorter side), a
/// `draw:ellipse`, or a custom shape's `draw:type`.
pub(crate) fn kind_of(element: &str, custom_type: Option<&str>, radius: Option<f64>) -> ShapeKind {
    match element {
        "draw:rect" => match radius {
            Some(r) if r > 0.0 => ShapeKind::RoundRect { radius: r.clamp(0.0, 0.5) },
            _ => ShapeKind::Rect,
        },
        "draw:ellipse" | "draw:circle" => ShapeKind::Ellipse,
        _ => match custom_type.unwrap_or("rectangle") {
            "isosceles-triangle" => ShapeKind::Triangle,
            "diamond" => ShapeKind::Diamond,
            "ellipse" => ShapeKind::Ellipse,
            "rectangle" => ShapeKind::Rect,
            "round-rectangle" => ShapeKind::RoundRect { radius: 1.0 / 6.0 },
            t => match t.strip_prefix("ooxml-") {
                Some(prst) => ShapeKind::from_prst(prst),
                None => ShapeKind::Other(t.to_string()),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_convert_both_ways() {
        for model in [0.0, 45.0, 90.0, 180.0, 270.0, 315.0] {
            let odf = odf_angle(model);
            assert_eq!((90.0 - odf).rem_euclid(360.0), model);
        }
        assert_eq!(odf_angle(90.0), 0.0, "top to bottom is ODF's 0");
        assert_eq!(parse_angle("900"), 90.0);
        assert_eq!(parse_angle("45deg"), 45.0);
    }

    #[test]
    fn a_style_inherits_from_its_parent_and_the_default() {
        let xml = r##"<office:document-styles>
            <office:styles>
            <draw:gradient draw:name="g1" draw:style="linear" draw:start-color="#000000" draw:end-color="#ffffff" draw:angle="0deg"/>
            <style:default-style style:family="graphic"><style:graphic-properties draw:fill="solid" draw:fill-color="#729fcf" draw:stroke="solid" svg:stroke-color="#3465a4" svg:stroke-width="1pt"/></style:default-style>
            <style:style style:name="parent" style:family="graphic"><style:graphic-properties draw:stroke="none"/></style:style>
            <style:style style:name="a" style:family="graphic" style:parent-style-name="parent"><style:graphic-properties draw:fill-color="#ff0000"/></style:style>
            <style:style style:name="b" style:family="graphic"><style:graphic-properties draw:fill="gradient" draw:fill-gradient-name="g1"/></style:style>
            </office:styles></office:document-styles>"##;
        let mut d = GraphicDefs::default();
        d.read(xml);
        let a = d.style("a", 1.0).unwrap();
        assert_eq!(a.fill, Some(Color(255, 0, 0)), "own colour, default's solid fill");
        assert_eq!(a.stroke, None, "the parent's no outline");
        let b = d.style("b", 2.0).unwrap();
        let g = b.gradient.unwrap();
        assert_eq!(g.angle, 90.0);
        assert_eq!(g.stops.len(), 2);
        assert_eq!(b.stroke, Some(Stroke { color: Color(0x34, 0x65, 0xa4), width: 2.0 }), "the default's outline, scaled");
        assert!(d.style("nope", 1.0).is_none());
    }
}
