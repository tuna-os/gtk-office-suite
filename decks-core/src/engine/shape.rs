// shape.rs — what a drawn shape is: its preset geometry, fill and outline.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The Phase 1 "Decks shape-style model" (docs/RENDER-PARITY-ROADMAP.md). The
// deck model used to have `Rect` and `Circle` with no style at all, and the
// canvas painted every rectangle blue and every ellipse red, whatever the file
// said. An ellipse could only be a circle, and a rounded rectangle came out
// square (render lab `decks/shapes`). A `Shape` carries its DrawingML preset
// and its own fill and outline, read from pptx/odp and written back.

/// An sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Color(pub u8, pub u8, pub u8);

impl Color {
    /// From `RRGGBB`, `#RRGGBB` or `AARRGGBB` hex.
    pub fn from_hex(hex: &str) -> Option<Color> {
        let hex = hex.trim().trim_start_matches('#');
        let hex = match hex.len() {
            8 => &hex[2..],
            6 => hex,
            _ => return None,
        };
        let c = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some(Color(c(0)?, c(2)?, c(4)?))
    }

    /// `RRGGBB`, upper case, as DrawingML writes it.
    pub fn to_hex(self) -> String {
        format!("{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }

    /// Components as 0.0–1.0, for Cairo.
    pub fn to_f64(self) -> (f64, f64, f64) {
        (self.0 as f64 / 255.0, self.1 as f64 / 255.0, self.2 as f64 / 255.0)
    }

    /// DrawingML `tint`: toward white by `1 - val`, in linear light as
    /// Office and LibreOffice apply it (val in 1/100000ths).
    pub fn tint(self, val: i32) -> Color {
        let t = (val as f64 / 100_000.0).clamp(0.0, 1.0);
        self.map_linear(|c| c * t + (1.0 - t))
    }

    /// DrawingML `shade`: toward black by `1 - val`, in linear light.
    pub fn shade(self, val: i32) -> Color {
        let t = (val as f64 / 100_000.0).clamp(0.0, 1.0);
        self.map_linear(|c| c * t)
    }

    /// DrawingML `satMod`: saturation scaled by `val`, in HSL.
    pub fn sat_mod(self, val: i32) -> Color {
        let (h, s, l) = rgb_to_hsl(self);
        hsl_to_rgb(h, (s * val as f64 / 100_000.0).clamp(0.0, 1.0), l)
    }

    fn map_linear(self, f: impl Fn(f64) -> f64) -> Color {
        let to_lin = |c: u8| {
            let c = c as f64 / 255.0;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        let to_srgb = |c: f64| {
            let c = c.clamp(0.0, 1.0);
            let v = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
            (v * 255.0).round() as u8
        };
        Color(to_srgb(f(to_lin(self.0))), to_srgb(f(to_lin(self.1))), to_srgb(f(to_lin(self.2))))
    }

    /// DrawingML `lumMod`/`lumOff` on this colour, in HSL as the spec
    /// defines them (values in 1/100000ths).
    pub fn lum(self, lum_mod: Option<i32>, lum_off: Option<i32>) -> Color {
        if lum_mod.is_none() && lum_off.is_none() {
            return self;
        }
        let (h, s, l) = rgb_to_hsl(self);
        let l = l * lum_mod.unwrap_or(100_000) as f64 / 100_000.0 + lum_off.unwrap_or(0) as f64 / 100_000.0;
        hsl_to_rgb(h, s, l.clamp(0.0, 1.0))
    }
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
/// kept by name, so it is written back unchanged, and drawn as its bounding
/// rectangle until it gets an outline of its own.
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
    match kind {
        ShapeKind::Rect | ShapeKind::Other(_) => Some(vec![(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]),
        ShapeKind::Triangle => Some(vec![(w / 2.0, 0.0), (w, h), (0.0, h)]),
        ShapeKind::Diamond => Some(vec![(w / 2.0, 0.0), (w, h / 2.0), (w / 2.0, h), (0.0, h / 2.0)]),
        ShapeKind::RoundRect { .. } | ShapeKind::Ellipse => None,
    }
}

/// Whether the point (`px`, `py`), in the shape's own box, is inside it.
pub fn contains(kind: &ShapeKind, w: f64, h: f64, px: f64, py: f64) -> bool {
    if px < 0.0 || py < 0.0 || px > w || py > h {
        return false;
    }
    match kind {
        ShapeKind::Ellipse => {
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
            let poly = polygon(kind, w, h).unwrap_or_default();
            // Even-odd ray cast.
            let mut inside = false;
            let n = poly.len();
            for i in 0..n {
                let (x1, y1) = poly[i];
                let (x2, y2) = poly[(i + 1) % n];
                if (y1 > py) != (y2 > py) && px < (x2 - x1) * (py - y1) / (y2 - y1) + x1 {
                    inside = !inside;
                }
            }
            inside
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
