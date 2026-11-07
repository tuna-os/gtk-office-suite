// Geometry and slide-space helpers for the PPTX reader.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
pub(super) fn parse_coords(
    e: &BytesStart,
    k1: &str,
    k2: &str
) -> (Option<f64>, Option<f64>) {
    let mut v1 = None;
    let mut v2 = None;
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == k1 {
            if let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
                v1 = val.parse::<f64>().ok();
            }
        } else if attr.key.as_ref() == k2 {
            if let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
                v2 = val.parse::<f64>().ok();
            }
        }
    }
    (v1, v2)
}

pub(super) fn parse_blip_embed(e: &BytesStart) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == "r:embed" {
            if let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
                return Some(val.into_owned());
            }
        }
    }
    None
}

pub(super) fn parse_prst_geom(e: &BytesStart) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == "prst" {
            if let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
                return Some(val.into_owned());
            }
        }
    }
    None
}

pub(super) fn is_tx_box_attr(e: &BytesStart) -> bool {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == "txBox" {
            if let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) {
                return val.as_ref() == "1";
            }
        }
    }
    false
}

/// `a:xfrm`'s `rot`, in degrees. OOXML stores it in sixtieth-thousandths of
/// a degree and omits the attribute for an unrotated shape, so absent and
/// zero mean the same thing here.
///
/// The writer emitted no `rot` at all until the model's rotation was
/// threaded through it, and this reader hardcoded `rotation: 0.0` at every
/// construction site — so a rotated shape saved as pptx came back square,
/// in the format an unsaved deck (and therefore its crash snapshot) uses by
/// default.
pub(super) fn parse_rotation(e: &BytesStart) -> Option<f64> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() != "rot" {
            continue;
        }
        let val = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?;
        let deg = val.trim().parse::<f64>().ok()? / 60_000.0;
        return if deg.is_finite() { Some(deg) } else { None };
    }
    None
}

/// How a document's own slide size maps onto the model's 960x540 space.
///
/// The model's units are abstract (ADR 0004: "960x540 model units"), so a
/// coordinate only means something relative to the slide it sits on. Both
/// readers used to divide by a fixed constant instead — 9525 EMU per unit
/// here, one point per unit in odp — which is the *same* thing only for a
/// slide of exactly the size our own writer emits. A deck from current
/// PowerPoint or Impress declares 13.333in x 7.5in
/// (`sldSz cx="12192000" cy="6858000"`), and a full-bleed shape on one read
/// back as 1280x720 in a 960x540 space: a third too large, running off the
/// canvas. Every test missed it because every test round-trips through our
/// own writer, which always emits the one size the constant assumed.
#[derive(Clone, Copy, Debug)]
pub struct SlideScale {
    pub x: f64,
    pub y: f64,
}

impl SlideScale {
    /// The slide size our own writer emits: 9144000 x 5143500 EMU
    /// (10in x 5.625in), which is what dividing by 9525 assumed.
    pub const DEFAULT_EMU: (f64, f64) = (9144000.0, 5143500.0);

    /// Each axis is normalised against that axis's own extent, so a
    /// full-bleed shape stays full-bleed whatever the source's aspect
    /// ratio. For any 16:9 source — both common PowerPoint sizes — the two
    /// factors are equal and this is a uniform scale; only a 4:3 import is
    /// stretched, which is the best a fixed 16:9 model space can do with
    /// one, and is still an improvement on today's silent overflow.
    pub fn from_emu(cx: f64, cy: f64) -> Self {
        let (dx, dy) = Self::DEFAULT_EMU;
        let cx = if cx > 0.0 { cx } else { dx };
        let cy = if cy > 0.0 { cy } else { dy };
        SlideScale { x: 960.0 / cx, y: 540.0 / cy }
    }
}

impl SlideScale {
    /// The factor text sizes take onto the model's slide: the same one the
    /// horizontal geometry takes. The model is always our writer's
    /// 10-inch-wide slide, so a 13.33-inch deck's shapes shrink to fit it;
    /// its point sizes, left alone, came out 1.33x too big beside them
    /// (render lab decks/text-styles, decks/table).
    pub fn text_factor(&self) -> f64 {
        self.x * 9525.0
    }

    /// `runs` with every size mapped onto the model's slide. A run with no
    /// size of its own has the 18 pt default, so on a slide of another size
    /// it gets the scaled default explicitly.
    pub fn text_runs(&self, runs: &[Run]) -> Vec<Run> {
        let k = self.text_factor();
        if (k - 1.0).abs() < 1e-6 {
            return runs.to_vec();
        }
        runs.iter()
            .map(|r| {
                let mut r = r.clone();
                let hp = r.style.font_size_hp.unwrap_or(36) as f64;
                r.style.font_size_hp = Some((hp * k).round().clamp(1.0, u16::MAX as f64) as u16);
                r
            })
            .collect()
    }
}

impl Default for SlideScale {
    fn default() -> Self {
        let (cx, cy) = Self::DEFAULT_EMU;
        Self::from_emu(cx, cy)
    }
}

/// The slide extent a package declares, from `p:sldSz` in presentation.xml.
pub fn parse_slide_size(xml: &str) -> Option<(f64, f64)> {
    if xml.is_empty() {
        return None;
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e))
                if e.name().as_ref() == "p:sldSz" =>
            {
                let (cx, cy) = parse_coords(e, "cx", "cy");
                if let (Some(cx), Some(cy)) = (cx, cy) {
                    if cx > 0.0 && cy > 0.0 {
                        return Some((cx, cy));
                    }
                }
                return None;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    None
}
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct GroupFrame {
    off: Option<(f64, f64)>,
    ext: Option<(f64, f64)>,
    ch_off: Option<(f64, f64)>,
    ch_ext: Option<(f64, f64)>,
}

impl GroupFrame {
    /// Take an `a:off`, `a:ext`, `a:chOff` or `a:chExt` of the group's own.
    pub(super) fn note(&mut self, e: &BytesStart) {
        let pair = |a: &str, b: &str| match parse_coords(e, a, b) {
            (Some(x), Some(y)) => Some((x, y)),
            _ => None,
        };
        match e.name().as_ref() {
            "a:off" => self.off = pair("x", "y"),
            "a:ext" => self.ext = pair("cx", "cy"),
            "a:chOff" => self.ch_off = pair("x", "y"),
            "a:chExt" => self.ch_ext = pair("cx", "cy"),
            _ => {}
        }
    }

    /// A rectangle in the group's child frame, on the slide (or in the
    /// frame of the group around it).
    pub(super) fn place(&self, (x, y, w, h): (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
        let (Some(off), Some(ext)) = (self.off, self.ext) else { return (x, y, w, h) };
        let ch_off = self.ch_off.unwrap_or(off);
        let ch_ext = self.ch_ext.unwrap_or(ext);
        let sx = if ch_ext.0 > 0.0 { ext.0 / ch_ext.0 } else { 1.0 };
        let sy = if ch_ext.1 > 0.0 { ext.1 / ch_ext.1 } else { 1.0 };
        (off.0 + (x - ch_off.0) * sx, off.1 + (y - ch_off.1) * sy, w * sx, h * sy)
    }
}

/// Whether a slide part is hidden: `show="0"` (or `false`) on its `p:sld`,
/// which PowerPoint writes for "Hide Slide".
pub(super) fn slide_is_hidden(slide_xml: &str) -> bool {
    let Some(at) = slide_xml.find("<p:sld") else { return false };
    let Some(len) = slide_xml[at..].find('>') else { return false };
    let tag = &slide_xml[at..at + len];
    tag.contains(" show=\"0\"") || tag.contains(" show=\"false\"")
}

/// A rectangle given in the innermost of `groups`, on the slide.
pub(super) fn in_groups(groups: &[GroupFrame], rect: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    groups.iter().rev().fold(rect, |r, g| g.place(r))
}
