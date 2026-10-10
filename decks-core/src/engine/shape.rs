// shape.rs — what a drawn shape is: its preset geometry, fill and outline.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The Phase 1 "Decks shape-style model" (docs/RENDER-PARITY-ROADMAP.md). The
// deck model used to have `Rect` and `Circle` with no style at all, and the
// canvas painted every rectangle blue and every ellipse red, whatever the file
// said. An ellipse could only be a circle, and a rounded rectangle came out
// square (render lab `decks/shapes`). A `Shape` carries its DrawingML preset
// and its own fill and outline, read from pptx/odp and written back.

/// An sRGB colour: the canonical [`suite_common_core::color::Color`].
/// The implementation (parsing, printing, channel maths) lives in
/// suite-common-core; the DrawingML modulation below stays here,
/// where the spec knowledge lives.
pub use suite_common_core::color::Color;

/// DrawingML colour modulation (`a:tint`, `a:shade`, `a:satMod`,
/// `a:lumMod`/`a:lumOff`). An extension trait rather than inherent
/// methods because `Color` is defined in suite-common-core; the maths
/// below is unchanged.
pub trait ColorModulation {
    /// `tint`: toward white by `1 - val`, in linear light as Office
    /// and LibreOffice apply it (val in 1/100000ths).
    fn tint(self, val: i32) -> Color;
    /// `shade`: toward black by `1 - val`, in linear light.
    fn shade(self, val: i32) -> Color;
    /// `satMod`: saturation scaled by `val`, in HSL.
    fn sat_mod(self, val: i32) -> Color;
    /// `lumMod`/`lumOff`, in HSL as the spec defines them (values in
    /// 1/100000ths).
    fn lum(self, lum_mod: Option<i32>, lum_off: Option<i32>) -> Color;
}

impl ColorModulation for Color {
    fn tint(self, val: i32) -> Color {
        let t = (val as f64 / 100_000.0).clamp(0.0, 1.0);
        map_linear(self, |c| c * t + (1.0 - t))
    }

    fn shade(self, val: i32) -> Color {
        let t = (val as f64 / 100_000.0).clamp(0.0, 1.0);
        map_linear(self, |c| c * t)
    }

    fn sat_mod(self, val: i32) -> Color {
        let (h, s, l) = rgb_to_hsl(self);
        hsl_to_rgb(h, (s * val as f64 / 100_000.0).clamp(0.0, 1.0), l)
    }

    fn lum(self, lum_mod: Option<i32>, lum_off: Option<i32>) -> Color {
        if lum_mod.is_none() && lum_off.is_none() {
            return self;
        }
        let (h, s, l) = rgb_to_hsl(self);
        let l = l * lum_mod.unwrap_or(100_000) as f64 / 100_000.0 + lum_off.unwrap_or(0) as f64 / 100_000.0;
        hsl_to_rgb(h, s, l.clamp(0.0, 1.0))
    }
}

fn map_linear(c: Color, f: impl Fn(f64) -> f64) -> Color {
    let to_lin = |c: u8| {
        let c = c as f64 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let to_srgb = |c: f64| {
        let c = c.clamp(0.0, 1.0);
        let v = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (v * 255.0).round() as u8
    };
    Color(to_srgb(f(to_lin(c.0))), to_srgb(f(to_lin(c.1))), to_srgb(f(to_lin(c.2))))
}

fn rgb_to_hsl(c: Color) -> (f64, f64, f64) {
    let (r, g, b) = c.to_f64();
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if (max - min).abs() < f64::EPSILON {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h / 6.0, s, l)
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> Color {
    let to = |v: f64| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    if s == 0.0 {
        return Color(to(l), to(l), to(l));
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hue = |mut t: f64| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    Color(to(hue(h + 1.0 / 3.0)), to(hue(h)), to(hue(h - 1.0 / 3.0)))
}

/// A DrawingML preset geometry (`<a:prstGeom prst="…">`). The ones drawn
/// with their true outline have their own variant; any other preset is
/// kept by name, so it is written back unchanged, and drawn with its
/// outline from `preset_polygon` or BetterOffice's presets (`outline`), or
/// as its bounding rectangle when neither has one.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ShapeKind {
    Rect,
    /// Corner radius as a fraction of the shorter side (DrawingML `adj`,
    /// default 16667 = 1/6).
    RoundRect { radius: f64 },
    Ellipse,
    Triangle,
    Diamond,
    Other(String),
}

impl ShapeKind {
    pub fn from_prst(prst: &str) -> ShapeKind {
        match prst {
            "rect" => ShapeKind::Rect,
            "roundRect" => ShapeKind::RoundRect { radius: 1.0 / 6.0 },
            "ellipse" => ShapeKind::Ellipse,
            "triangle" => ShapeKind::Triangle,
            "diamond" => ShapeKind::Diamond,
            other => ShapeKind::Other(other.to_string()),
        }
    }

    /// The DrawingML preset name to write back.
    pub fn prst(&self) -> &str {
        match self {
            ShapeKind::Rect => "rect",
            ShapeKind::RoundRect { .. } => "roundRect",
            ShapeKind::Ellipse => "ellipse",
            ShapeKind::Triangle => "triangle",
            ShapeKind::Diamond => "diamond",
            ShapeKind::Other(name) => name,
        }
    }
}

/// An outline.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Stroke {
    pub color: Color,
    /// In model units, like x/y/w/h (so it scales with the slide as they
    /// do): readers scale it from the file's units exactly as coordinates.
    pub width: f64,
}

/// A stop in a gradient: `pos` from 0.0 (start) to 1.0 (end).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GradientStop {
    pub pos: f64,
    pub color: Color,
}

/// A linear gradient across a shape's box.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LinearGradient {
    pub stops: Vec<GradientStop>,
    /// Direction the colours run, in degrees clockwise from left-to-right
    /// (DrawingML `a:lin ang`): 90 runs top to bottom, 270 bottom to top.
    pub angle: f64,
}

impl LinearGradient {
    /// The colour halfway along, for consumers that can only paint one
    /// colour (a format with no gradient, a thumbnail).
    pub fn mean(&self) -> Option<Color> {
        let n = self.stops.len() as f64;
        if n == 0.0 {
            return None;
        }
        let avg = |f: fn(&Color) -> u8| (self.stops.iter().map(|s| f(&s.color) as f64).sum::<f64>() / n).round() as u8;
        Some(Color(avg(|c| c.0), avg(|c| c.1), avg(|c| c.2)))
    }
}

/// How a shape is painted. `fill: None` is no fill (not a default fill),
/// `stroke: None` no outline. When `gradient` is set it is what is drawn,
/// and `fill` holds its mean colour for consumers that can't draw one.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ShapeStyle {
    pub fill: Option<Color>,
    pub gradient: Option<LinearGradient>,
    pub stroke: Option<Stroke>,
}

impl Default for ShapeStyle {
    /// A new shape as the default Office theme draws one: accent 1 fill
    /// with a darker outline of the same hue.
    fn default() -> Self {
        let accent1 = Color(0x44, 0x72, 0xC4);
        ShapeStyle {
            fill: Some(accent1),
            gradient: None,
            stroke: Some(Stroke { color: accent1.lum(Some(50_000), None), width: 1.0 }),
        }
    }
}

/// A point on a shape's outline, in the shape's own box (0..w, 0..h).
pub type Point = (f64, f64);

/// The outline of `kind` in a `w`×`h` box, as the canvas draws it and as
/// hit-testing tests it: `Some(polygon)` for straight-edged kinds, `None`
/// for the curved ones (ellipse, rounded rectangle), which the canvas draws
/// with arcs and `contains` tests analytically.
pub fn polygon(kind: &ShapeKind, w: f64, h: f64) -> Option<Vec<Point>> {
    if is_elliptical(kind) {
        return None;
    }
    match kind {
        ShapeKind::Other(prst) => Some(
            preset_polygon(prst, w, h)
                .or_else(|| borrowed_outline(prst, w, h).filter(|layers| layers.len() == 1 && layers[0].len() == 1).map(|mut layers| layers.remove(0).remove(0)))
                .unwrap_or_else(|| vec![(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]),
        ),
        ShapeKind::Rect => Some(vec![(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]),
        ShapeKind::Triangle => Some(vec![(w / 2.0, 0.0), (w, h), (0.0, h)]),
        ShapeKind::Diamond => Some(vec![(w / 2.0, 0.0), (w, h / 2.0), (w / 2.0, h), (0.0, h / 2.0)]),
        ShapeKind::RoundRect { .. } | ShapeKind::Ellipse => None,
    }
}

/// The outline of `kind` in a `w`×`h` box as layers, each one or more
/// closed subpaths filled by winding (a donut's hole is wound against its
/// ring, so it stays open) and painted over the one before (a cube's faces),
/// for the kinds `polygon` draws; `None` for the curved kinds, as there.
pub fn outline(kind: &ShapeKind, w: f64, h: f64) -> Option<Vec<Vec<Vec<Point>>>> {
    if let ShapeKind::Other(prst) = kind {
        if !is_elliptical(kind) && preset_polygon(prst, w, h).is_none() {
            if let Some(layers) = borrowed_outline(prst, w, h) {
                return Some(layers);
            }
        }
    }
    polygon(kind, w, h).map(|poly| vec![vec![poly]])
}

/// Presets BetterOffice draws, but not as Office and LibreOffice do: these
/// stay their box rather than take a wrong outline.
const BORROWED_WRONG: [&str; 3] = ["upDownArrow", "flowChartManualInput", "flowChartOffpageConnector"];

/// Segments a curve is flattened into: under a pixel off the true curve
/// for any shape on a slide.
const CURVE_STEPS: usize = 16;

/// The outline of preset `prst` from BetterOffice's DrawingML crate
/// (Apache-2.0, see `THIRD-PARTY-NOTICES.md`), at the preset's default
/// adjustments, for the presets our own `preset_polygon` doesn't draw:
/// its filled layers, with curves flattened, scaled to the box. `None` when
/// it has no filled outline for `prst` either.
fn borrowed_outline(prst: &str, w: f64, h: f64) -> Option<Vec<Vec<Vec<Point>>>> {
    use ooxml_drawingml::PresetPathFill;
    if !(w > 0.0 && h > 0.0) || BORROWED_WRONG.contains(&prst) {
        return None;
    }
    let adjustments = ooxml_drawingml::preset_geometry_default_adjustments(prst);
    let aspect = w / h;
    let layers: Vec<Vec<ooxml_drawingml::GeometryPathCommand>> = match ooxml_drawingml::preset_geometry_layers(prst, &adjustments, aspect) {
        Some(layers) => layers.into_iter().filter(|layer| layer.fill != PresetPathFill::None).map(|layer| layer.commands).collect(),
        None => vec![ooxml_drawingml::preset_geometry_to_path(prst, &adjustments, aspect)?],
    };
    let layers: Vec<Vec<Vec<Point>>> = layers.into_iter().map(|commands| flatten(commands, w, h)).filter(|layer| !layer.is_empty()).collect();
    (!layers.is_empty()).then_some(layers)
}

/// `commands`, in the unit square, as closed polygons in a `w`×`h` box.
fn flatten(commands: Vec<ooxml_drawingml::GeometryPathCommand>, w: f64, h: f64) -> Vec<Vec<Point>> {
    use ooxml_drawingml::GeometryPathCommand as C;
    let mut subpaths: Vec<Vec<Point>> = Vec::new();
    let mut current: Vec<Point> = Vec::new();
    let mut at = (0.0, 0.0);
    for command in commands {
        match command {
            C::Move { x, y } => {
                if current.len() > 2 {
                    subpaths.push(std::mem::take(&mut current));
                }
                current.clear();
                at = (x, y);
                current.push(at);
            }
            C::Line { x, y } => {
                at = (x, y);
                current.push(at);
            }
            C::Quad { cpx, cpy, x, y } => {
                let (x0, y0) = at;
                for i in 1..=CURVE_STEPS {
                    let t = i as f64 / CURVE_STEPS as f64;
                    let u = 1.0 - t;
                    current.push((u * u * x0 + 2.0 * u * t * cpx + t * t * x, u * u * y0 + 2.0 * u * t * cpy + t * t * y));
                }
                at = (x, y);
            }
            C::Cubic { cp1x, cp1y, cp2x, cp2y, x, y } => {
                let (x0, y0) = at;
                for i in 1..=CURVE_STEPS {
                    let t = i as f64 / CURVE_STEPS as f64;
                    let u = 1.0 - t;
                    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    current.push((a * x0 + b * cp1x + c * cp2x + d * x, a * y0 + b * cp1y + c * cp2y + d * y));
                }
                at = (x, y);
            }
            C::Close => {
                if current.len() > 2 {
                    subpaths.push(std::mem::take(&mut current));
                }
                current.clear();
            }
        }
    }
    if current.len() > 2 {
        subpaths.push(current);
    }
    subpaths
        .into_iter()
        .map(|poly| poly.into_iter().filter(|(x, y)| x.is_finite() && y.is_finite()).map(|(x, y)| (x * w, y * h)).collect::<Vec<_>>())
        .filter(|poly| poly.len() > 2)
        .collect()
}

/// Whether `kind` is drawn as an ellipse filling its box: the ellipse, and
/// the presets that are one under another name (a flowchart connector).
pub fn is_elliptical(kind: &ShapeKind) -> bool {
    match kind {
        ShapeKind::Ellipse => true,
        ShapeKind::Other(prst) => prst == "flowChartConnector",
        _ => false,
    }
}

/// The outline of DrawingML preset `prst` in a `w`×`h` box, at the
/// preset's default adjustments (presetShapeDefinitions.xml), for the
/// straight-edged presets real decks use; `None` for any other, which is
/// drawn as its box. `ss` is the shorter side, as the definitions use it.
pub fn preset_polygon(prst: &str, w: f64, h: f64) -> Option<Vec<Point>> {
    let ss = w.min(h);
    let (cx, cy) = (w / 2.0, h / 2.0);
    Some(match prst {
        "flowChartProcess" => vec![(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)],
        "flowChartDecision" => vec![(cx, 0.0), (w, cy), (cx, h), (0.0, cy)],
        "rtTriangle" => vec![(0.0, 0.0), (w, h), (0.0, h)],
        "upArrow" | "downArrow" => {
            // adj1 50000: the shaft is half the width; adj2 50000: the
            // head is half the shorter side long.
            let (dx, head) = (w * 0.25, ss * 0.5);
            let up = vec![(cx, 0.0), (w, head), (cx + dx, head), (cx + dx, h), (cx - dx, h), (cx - dx, head), (0.0, head)];
            if prst == "upArrow" { up } else { up.into_iter().map(|(x, y)| (x, h - y)).collect() }
        }
        "rightArrow" | "leftArrow" => {
            let (dy, head) = (h * 0.25, ss * 0.5);
            let right = vec![(0.0, cy - dy), (w - head, cy - dy), (w - head, 0.0), (w, cy), (w - head, h), (w - head, cy + dy), (0.0, cy + dy)];
            if prst == "rightArrow" { right } else { right.into_iter().map(|(x, y)| (w - x, y)).collect() }
        }
        "homePlate" => {
            let x = w - ss * 0.5;
            vec![(0.0, 0.0), (x, 0.0), (w, cy), (x, h), (0.0, h)]
        }
        "chevron" => {
            let x = ss * 0.5;
            vec![(0.0, 0.0), (w - x, 0.0), (w, cy), (w - x, h), (0.0, h), (x, cy)]
        }
        "parallelogram" => {
            let x = ss * 0.25;
            vec![(x, 0.0), (w, 0.0), (w - x, h), (0.0, h)]
        }
        "trapezoid" => {
            let x = ss * 0.25;
            vec![(0.0, h), (x, 0.0), (w - x, 0.0), (w, h)]
        }
        "hexagon" => {
            let x = ss * 0.25;
            vec![(0.0, cy), (x, 0.0), (w - x, 0.0), (w, cy), (w - x, h), (x, h)]
        }
        "octagon" => {
            let x = ss * 0.29289;
            vec![(x, 0.0), (w - x, 0.0), (w, x), (w, h - x), (w - x, h), (x, h), (0.0, h - x), (0.0, x)]
        }
        "plus" => {
            let x = ss * 0.25;
            vec![(x, 0.0), (w - x, 0.0), (w - x, x), (w, x), (w, h - x), (w - x, h - x), (w - x, h), (x, h), (x, h - x), (0.0, h - x), (0.0, x), (x, x)]
        }
        "mathPlus" => {
            // adj1 23520: the bars' thickness; they reach 73490/100000 of
            // each side.
            let (dx1, dy1, t) = (w * 0.36745, h * 0.36745, ss * 0.1176);
            let (x1, x2, x3, x4) = (cx - dx1, cx - t, cx + t, cx + dx1);
            let (y1, y2, y3, y4) = (cy - dy1, cy - t, cy + t, cy + dy1);
            vec![(x1, y2), (x2, y2), (x2, y1), (x3, y1), (x3, y2), (x4, y2), (x4, y3), (x3, y3), (x3, y4), (x2, y4), (x2, y3), (x1, y3)]
        }
        "snip2DiagRect" => {
            // adj1 0 snips the top-left and bottom-right corners (not at
            // all), adj2 16667 the other two.
            let d = ss * 0.16667;
            vec![(0.0, 0.0), (w - d, 0.0), (w, d), (w, h), (d, h), (0.0, h - d)]
        }
        "pentagon" | "star5" => {
            // A regular pentagon, or star, point up, stretched to the box.
            let outer: Vec<(f64, f64)> = (0..5).map(|k| angle_point(-90.0 + 72.0 * k as f64, 1.0)).collect();
            let points: Vec<(f64, f64)> = if prst == "pentagon" {
                outer.clone()
            } else {
                (0..10).map(|k| angle_point(-90.0 + 36.0 * k as f64, if k % 2 == 0 { 1.0 } else { 0.381966 })).collect()
            };
            let (x0, x1) = outer.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
            let (y0, y1) = outer.iter().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
            points.into_iter().map(|(x, y)| ((x - x0) / (x1 - x0) * w, (y - y0) / (y1 - y0) * h)).collect()
        }
        _ => return None,
    })
}

/// The point at `degrees` (clockwise from east) on a circle of radius `r`.
fn angle_point(degrees: f64, r: f64) -> (f64, f64) {
    let a = degrees.to_radians();
    (r * a.cos(), r * a.sin())
}

/// Whether the point (`px`, `py`), in the shape's own box, is inside it.
pub fn contains(kind: &ShapeKind, w: f64, h: f64, px: f64, py: f64) -> bool {
    if px < 0.0 || py < 0.0 || px > w || py > h {
        return false;
    }
    match kind {
        k if is_elliptical(k) => {
            let (rx, ry) = (w / 2.0, h / 2.0);
            if rx <= 0.0 || ry <= 0.0 {
                return false;
            }
            let (dx, dy) = ((px - rx) / rx, (py - ry) / ry);
            dx * dx + dy * dy <= 1.0
        }
        ShapeKind::RoundRect { radius } => {
            let r = radius.clamp(0.0, 0.5) * w.min(h);
            let cx = px.clamp(r, w - r);
            let cy = py.clamp(r, h - r);
            (px - cx).powi(2) + (py - cy).powi(2) <= r * r + 1e-9
        }
        _ => {
            // In any layer, by its subpaths' winding number, as the canvas
            // fills them (for a simple polygon, the same as even-odd).
            outline(kind, w, h).unwrap_or_default().iter().any(|layer| {
                let mut winding = 0;
                for poly in layer {
                    let n = poly.len();
                    for i in 0..n {
                        let (x1, y1) = poly[i];
                        let (x2, y2) = poly[(i + 1) % n];
                        if (y1 > py) != (y2 > py) && px < (x2 - x1) * (py - y1) / (y2 - y1) + x1 {
                            winding += if y2 > y1 { 1 } else { -1 };
                        }
                    }
                }
                winding != 0
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_parse_and_print_as_drawingml_hex() {
        assert_eq!(Color::from_hex("C00000"), Some(Color(0xC0, 0, 0)));
        assert_eq!(Color::from_hex("#2a9d3f"), Some(Color(0x2A, 0x9D, 0x3F)));
        assert_eq!(Color::from_hex("FF123456"), Some(Color(0x12, 0x34, 0x56)));
        assert_eq!(Color(0x12, 0xAB, 0x0F).to_hex(), "12AB0F");
        assert_eq!(Color::from_hex("nope"), None);
    }

    #[test]
    fn luminance_modulation_darkens_and_lightens_in_hsl() {
        // Against the values Office itself shows, to within rounding.
        let near = |a: Color, b: Color| {
            let d = |x: u8, y: u8| (x as i16 - y as i16).abs();
            assert!(d(a.0, b.0) <= 1 && d(a.1, b.1) <= 1 && d(a.2, b.2) <= 1, "{a:?} vs {b:?}");
        };
        let accent = Color(0x44, 0x72, 0xC4);
        assert_eq!(accent.lum(None, None), accent);
        // lumMod 50%: Office's "darker 50%" of accent 1.
        near(accent.lum(Some(50_000), None), Color(0x1F, 0x38, 0x64));
        // lumMod 20% + lumOff 80%: "lighter 80%".
        near(accent.lum(Some(20_000), Some(80_000)), Color(0xDA, 0xE3, 0xF3));
    }

    #[test]
    fn tint_and_shade_mix_toward_white_and_black_in_linear_light() {
        let c = Color(0x4F, 0x81, 0xBD);
        assert_eq!(c.tint(100_000), c);
        assert_eq!(c.shade(100_000), c);
        assert_eq!(c.tint(0), Color(255, 255, 255));
        assert_eq!(c.shade(0), Color(0, 0, 0));
        // A half tint is lighter than the colour and not white.
        let t = c.tint(50_000);
        assert!(t.0 > c.0 && t.1 > c.1 && t.2 > c.2 && t != Color(255, 255, 255));
        // satMod 0 is grey at the same lightness.
        let g = c.sat_mod(0);
        assert!(g.0 == g.1 && g.1 == g.2);
    }

    /// The presets real decks use draw their own outline, not their box:
    /// every point lies in the box and the outline is not the rectangle.
    #[test]
    fn presets_real_decks_use_have_their_own_outline() {
        let (w, h) = (200.0, 100.0);
        let rect = polygon(&ShapeKind::Rect, w, h).unwrap();
        for p in ["upArrow", "downArrow", "leftArrow", "rightArrow", "homePlate", "chevron", "parallelogram", "trapezoid", "rtTriangle", "hexagon", "octagon", "plus", "mathPlus", "snip2DiagRect", "pentagon", "star5", "flowChartDecision"] {
            let poly = polygon(&ShapeKind::Other(p.into()), w, h).unwrap();
            assert_ne!(poly, rect, "{p}");
            for (x, y) in &poly {
                assert!((-1e-9..=w + 1e-9).contains(x) && (-1e-9..=h + 1e-9).contains(y), "{p}: ({x}, {y})");
            }
        }
        assert_eq!(polygon(&ShapeKind::Other("star5".into()), w, h).unwrap().len(), 10);
        // An unknown preset is still drawn as its box.
        assert_eq!(polygon(&ShapeKind::Other("cloud".into()), w, h).unwrap(), rect);
    }

    /// The presets only BetterOffice draws have their own outline too,
    /// inside their box, and a donut keeps its hole.
    #[test]
    fn borrowed_presets_have_their_own_outline() {
        let (w, h) = (200.0, 100.0);
        let rect = vec![vec![(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]];
        for p in ["star4", "star6", "star8", "star12", "star24", "heptagon", "decagon", "dodecagon", "leftRightArrow", "bentArrow", "corner", "foldedCorner", "noSmoking", "donut", "cube", "ribbon", "ellipseRibbon", "flowChartTerminator", "wedgeRoundRectCallout", "wedgeEllipseCallout", "cloudCallout", "mathMultiply"] {
            let layers = outline(&ShapeKind::Other(p.into()), w, h).unwrap_or_else(|| panic!("{p}: no outline"));
            assert_ne!(layers, vec![rect.clone()], "{p}");
            for (x, y) in layers.iter().flatten().flatten() {
                // Callouts reach outside their box with their tail.
                if !p.contains("Callout") {
                    assert!((-1e-6..=w + 1e-6).contains(x) && (-1e-6..=h + 1e-6).contains(y), "{p}: ({x}, {y})");
                }
            }
        }
        let donut = ShapeKind::Other("donut".into());
        assert_eq!(outline(&donut, w, h).unwrap(), vec![outline(&donut, w, h).unwrap()[0].clone()]);
        assert_eq!(outline(&donut, w, h).unwrap()[0].len(), 2);
        assert!(contains(&donut, w, h, 10.0, 50.0), "the ring");
        assert!(!contains(&donut, w, h, 100.0, 50.0), "the hole");
        // Our own outlines still come first.
        assert_eq!(outline(&ShapeKind::Other("chevron".into()), w, h).unwrap(), vec![vec![polygon(&ShapeKind::Other("chevron".into()), w, h).unwrap()]]);
        // A ribbon crosses itself; the crossing is filled, not a hole.
        let ribbon = ShapeKind::Other("ribbon".into());
        assert!(contains(&ribbon, w, h, 100.0, 60.0), "the front");
        // A cube is its faces, one layer each.
        assert_eq!(outline(&ShapeKind::Other("cube".into()), w, h).unwrap().len(), 3);
        // The presets BetterOffice gets wrong stay their box.
        for p in BORROWED_WRONG {
            assert_eq!(outline(&ShapeKind::Other(p.into()), w, h).unwrap(), vec![rect.clone()], "{p}");
        }
    }

    /// An up arrow is its point and its shaft: the shaft's foot is inside,
    /// the box's bottom corners are not.
    #[test]
    fn an_up_arrow_is_its_head_and_shaft() {
        let up = ShapeKind::Other("upArrow".into());
        let (w, h) = (40.0, 80.0);
        assert!(contains(&up, w, h, 20.0, 2.0), "the point");
        assert!(contains(&up, w, h, 20.0, 78.0), "the shaft's foot");
        assert!(!contains(&up, w, h, 1.0, 79.0) && !contains(&up, w, h, 39.0, 79.0), "beside the shaft");
        assert!(!contains(&up, w, h, 1.0, 1.0), "beside the point");
    }

    /// A flowchart connector is a circle under another name.
    #[test]
    fn a_flowchart_connector_is_an_ellipse() {
        let c = ShapeKind::Other("flowChartConnector".into());
        assert!(is_elliptical(&c) && polygon(&c, 10.0, 10.0).is_none());
        assert!(contains(&c, 10.0, 10.0, 5.0, 5.0) && !contains(&c, 10.0, 10.0, 0.5, 0.5));
    }

    #[test]
    fn presets_round_trip_by_name_and_unknown_ones_are_kept() {
        for p in ["rect", "roundRect", "ellipse", "triangle", "diamond", "star5"] {
            assert_eq!(ShapeKind::from_prst(p).prst(), p);
        }
        assert_eq!(ShapeKind::from_prst("star5"), ShapeKind::Other("star5".into()));
    }

    #[test]
    fn hit_testing_follows_the_true_outline() {
        // An ellipse misses its bounding box's corners.
        assert!(contains(&ShapeKind::Ellipse, 200.0, 100.0, 100.0, 50.0));
        assert!(!contains(&ShapeKind::Ellipse, 200.0, 100.0, 5.0, 5.0));
        // A rounded rectangle misses the very corner, hits just inside the curve.
        let rr = ShapeKind::RoundRect { radius: 1.0 / 6.0 };
        assert!(!contains(&rr, 120.0, 60.0, 0.5, 0.5));
        assert!(contains(&rr, 120.0, 60.0, 10.0, 10.0));
        // A triangle's apex is at the top centre.
        assert!(contains(&ShapeKind::Triangle, 100.0, 100.0, 50.0, 60.0));
        assert!(!contains(&ShapeKind::Triangle, 100.0, 100.0, 5.0, 5.0));
        assert!(!contains(&ShapeKind::Rect, 10.0, 10.0, 11.0, 5.0));
    }
}
