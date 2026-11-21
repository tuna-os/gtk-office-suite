// theme.rs — deck themes: the palettes and fonts masters inherit.
//
// SPDX-License-Identifier: GPL-3.0-or-later
//
// A pptx keeps each master's colours and fonts in its own theme part
// (`ppt/theme/themeN.xml`), named by the master's theme relationship
// (ECMA-376: every `p:sldMaster` has exactly one). `Deck.themes` retains
// one entry per theme part the package named, and each master names its
// theme by stable id (`MasterSlide.theme_id`) — never by part order,
// which a reorder or a second master with its own design invalidates.
// A master with no theme relationship (what this writer used to emit)
// keeps `theme_id: None` and resolves against the documented default,
// which is also what the writer emits for it, so its bytes do not change.
//
// A shape or run that states its colour as a theme slot keeps that
// reference as its canonical value (`ColorRef::Theme` with the source
// `a:tint`/`a:shade`/`a:satMod`/`a:lumMod`/`a:lumOff` modulation); the
// resolved sRGB beside it is a derived cache for renderers, exactly as
// the shared Letters `Run` stays the resolved value for text. A later
// theme edit therefore reaches every inheritor without touching the
// local overrides, which keep their own references. What the readers
// cannot name as a token — a `p:style` fill/list reference into the
// theme's format scheme, a gradient stop, a system or preset colour —
// is resolved at import to `ColorRef::Rgb` (or left to the resolved
// cache with no reference), which the writers carry back byte-stable.
// That is a recorded boundary, not a silent fallback: the reference is
// absent rather than invented.
//
// ODP has no theme parts: its masters share one document font and its
// colours are literals. An ODP import leaves `themes` empty and every
// `theme_id` unset; its fills keep `Rgb` references. A pptx-born deck
// saved as ODP resolves its theme references to hex at the border
// (odp.rs), which the loss report does not need to warn about: the
// colour on the page is unchanged, only the live link cannot survive
// in a format with no themes to point at.

use std::collections::BTreeMap;

use super::shape::{Color, ColorModulation};

/// One DrawingML colour modulation (`a:tint`, `a:shade`, `a:satMod`,
/// `a:lumMod`/`a:lumOff`), as the source states it, values in
/// 1/100000ths. Mirrors `shape_xml::color_of`'s application order so a
/// retained reference resolves to the colour the file drew.
#[derive(Clone, Debug, PartialEq, Eq)]
#[derive(serde::Serialize, serde::Deserialize)]
pub enum ColorMod {
    Tint(i32),
    Shade(i32),
    SatMod(i32),
    LumMod(i32),
    LumOff(i32),
}
impl ColorMod {
    /// The DrawingML element name for this modulation (`a:tint` …).
    pub fn xml_name(&self) -> &'static str {
        match self {
            ColorMod::Tint(_) => "a:tint",
            ColorMod::Shade(_) => "a:shade",
            ColorMod::SatMod(_) => "a:satMod",
            ColorMod::LumMod(_) => "a:lumMod",
            ColorMod::LumOff(_) => "a:lumOff",
        }
    }

    /// The modulation value, in 1/100000ths, as the source states it.
    pub fn value(&self) -> i32 {
        match *self {
            ColorMod::Tint(v) | ColorMod::Shade(v) | ColorMod::SatMod(v) | ColorMod::LumMod(v) | ColorMod::LumOff(v) => v,
        }
    }

    /// The modulated colour, replaying the reader's collection: tint,
    /// shade and saturation in order with the pending luminance pair
    /// applied last, exactly as `shape_xml::color_of` reads them.
    /// In practice files state a single modulation.
    pub fn apply_all(mods: &[ColorMod], base: Color) -> Color {
        let mut lum_mod = None;
        let mut lum_off = None;
        let mut c = base;
        for m in mods {
            match *m {
                ColorMod::Tint(v) => c = c.tint(v),
                ColorMod::Shade(v) => c = c.shade(v),
                ColorMod::SatMod(v) => c = c.sat_mod(v),
                ColorMod::LumMod(v) => lum_mod = Some(v),
                ColorMod::LumOff(v) => lum_off = Some(v),
            }
        }
        c.lum(lum_mod, lum_off)
    }
}

/// A theme-slot colour with its source modulation: `a:schemeClr`
/// `val` plus any following `a:tint`/`a:shade`/`a:satMod`/`a:lumMod`/
/// `a:lumOff` children, in document order.
#[derive(Clone, Debug, PartialEq, Eq)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ThemeColor {
    /// The slot as the file names it (`accent1`, `dk1`, `tx1`, `bg1`,
    /// `hlink` …). Presentation names (`tx1`, `bg1`) resolve through
    /// the master's colour map (`MasterSlide.color_map`) before the
    /// theme's slots, as `Theme::slot` does.
    pub slot: String,
    pub mods: Vec<ColorMod>,
}

impl ThemeColor {
    pub fn unmodulated(slot: &str) -> Self {
        ThemeColor { slot: slot.to_string(), mods: Vec::new() }
    }
}

/// A shape or outline colour as its source states it: a literal sRGB
/// or a theme reference. Absence is `None` beside it (no statement),
/// never a variant here: an absent fill is not white, and an absent
/// line is not "no line".
#[derive(Clone, Debug, PartialEq, Eq)]
#[derive(serde::Serialize, serde::Deserialize)]
pub enum ColorRef {
    /// Literal sRGB, `RRGGBB` upper case as the writers emit it.
    Rgb(String),
    /// A theme slot with modulation.
    Theme(ThemeColor),
}

impl ColorRef {
    /// The colour this reference draws as under `theme`, if it names
    /// one the theme answers.
    pub fn resolve(&self, theme: &Theme) -> Option<Color> {
        match self {
            ColorRef::Rgb(hex) => Color::from_hex(hex),
            ColorRef::Theme(t) => theme.slot(&t.slot).map(|c| ColorMod::apply_all(&t.mods, c)),
        }
    }

    /// The `a:schemeClr`/`a:srgbClr` element this writes as in a
    /// `a:solidFill`, with its modulation children. The writer prefers
    /// this over the resolved cache whenever a shape carries one, so a
    /// save keeps the live link instead of freezing the colour.
    pub fn solid_fill_xml(&self) -> String {
        match self {
            ColorRef::Rgb(hex) => format!(
                "<a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>",
                hex.to_uppercase()
            ),
            ColorRef::Theme(t) => {
                let mut mods = String::new();
                for m in &t.mods {
                    let (name, v) = match *m {
                        ColorMod::Tint(v) => ("a:tint", v),
                        ColorMod::Shade(v) => ("a:shade", v),
                        ColorMod::SatMod(v) => ("a:satMod", v),
                        ColorMod::LumMod(v) => ("a:lumMod", v),
                        ColorMod::LumOff(v) => ("a:lumOff", v),
                    };
                    mods.push_str(&format!("<{name} val=\"{v}\"/>"));
                }
                format!(
                    "<a:solidFill><a:schemeClr val=\"{}\">{mods}</a:schemeClr></a:solidFill>",
                    quick_xml::escape::escape(t.slot.as_str())
                )
            }
        }
    }
}

/// A fill as its source states it. `None` beside it (no direct
/// statement: a `p:style` reference, a group fill, or a predates-
/// retention shape) leaves the resolved cache canonical, as before.
#[derive(Clone, Debug, PartialEq, Eq)]
#[derive(serde::Serialize, serde::Deserialize)]
pub enum FillSrc {
    /// An explicit `a:noFill`: not the same as no statement. A theme
    /// edit that would fill an inheriting shape must not fill this one.
    NoFill,
    /// A direct `a:solidFill` colour, literal or theme token.
    Color(ColorRef),
}

/// An outline colour as its source states it. `None` beside it means
/// no direct `a:ln` statement was kept (see `FillSrc`).
#[derive(Clone, Debug, PartialEq, Eq)]
#[derive(serde::Serialize, serde::Deserialize)]
pub enum StrokeSrc {
    /// An explicit line with no colour (`a:noFill` inside `a:ln`, or a
    /// zero `lnRef`): drawn as no outline, and kept that way.
    NoLine,
    /// A direct line colour, literal or theme token.
    Color(ColorRef),
}

/// One theme part of the package: its colour slots and its heading
/// and body Latin typefaces. Format-scheme lists (`a:fmtScheme`) are
/// deliberately not retained: shapes keep their own references or
/// their resolved colours (see the module note), and re-emitting an
/// opaque scheme would be a second theme model beside this one.
#[derive(Clone, Debug, PartialEq, Eq)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Theme {
    /// Stable identity within the session (DCK-005, as masters and
    /// layouts): readers assign ids once at import; 0 means not
    /// assigned yet (`ops::ensure_all_ids` fills those in).
    pub id: u64,
    /// The theme part's `name`, for the writer to carry back.
    pub name: String,
    /// Slot (without any `a:` prefix: `dk1`, `accent1` …) to `RRGGBB`
    /// upper case. Slots the part does not state are absent rather
    /// than defaulted: resolution falls back per slot, visibly.
    pub colors: BTreeMap<String, String>,
    /// `a:fontScheme/a:majorFont/a:latin`, when the part names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub major_font: Option<String>,
    /// `a:fontScheme/a:minorFont/a:latin`, when the part names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minor_font: Option<String>,
    /// The part this was read from (`ppt/theme/theme2.xml`), for
    /// provenance and for matching masters back to parts on write.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
}

impl Theme {
    /// The Office 2013+ default slots, for a master with no theme of
    /// its own: the same values `shape_xml::Theme::default` resolves
    /// against, so a retained and an unretained master agree.
    pub fn default_colors() -> BTreeMap<String, String> {
        [
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
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    /// A `schemeClr` value: presentation names (`tx1`, `bg1`, `tx2`,
    /// `bg2`) onto the dark/light slots, everything else directly.
    /// A slot the theme does not state falls back to the default
    /// theme's value for that slot — the one reader that resolves
    /// without a retained theme — rather than to nothing, and an
    /// unknown name resolves to nothing rather than to black.
    pub fn slot(&self, name: &str) -> Option<Color> {
        let slot = match name {
            "tx1" => "dk1",
            "bg1" => "lt1",
            "tx2" => "dk2",
            "bg2" => "lt2",
            other => other,
        };
        if let Some(hex) = self.colors.get(slot) {
            return Color::from_hex(hex);
        }
        Self::default_colors().get(slot).and_then(|hex| Color::from_hex(hex))
    }

    /// The reference `r` draws as under this theme, if it names a
    /// colour this theme answers.
    pub fn resolve(&self, r: &ColorRef) -> Option<Color> {
        r.resolve(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme {
            id: 1,
            name: "Office".into(),
            colors: Theme::default_colors(),
            major_font: Some("Calibri Light".into()),
            minor_font: Some("Calibri".into()),
            source: "ppt/theme/theme1.xml".into(),
        }
    }

    #[test]
    fn presentation_slot_names_reach_the_dark_and_light_slots() {
        let t = theme();
        assert_eq!(t.slot("tx1"), t.slot("dk1"));
        assert_eq!(t.slot("bg1"), t.slot("lt1"));
        assert_eq!(t.slot("accent1"), Color::from_hex("4472C4"));
        assert_eq!(t.slot("no-such-slot"), None);
    }

    #[test]
    fn a_missing_slot_falls_back_to_the_default_theme_not_to_nothing() {
        let mut t = theme();
        t.colors.remove("accent1");
        assert_eq!(t.slot("accent1"), Color::from_hex("4472C4"));
    }

    #[test]
    fn modulation_replays_the_reader_order() {
        let t = theme();
        let r = ColorRef::Theme(ThemeColor { slot: "accent1".into(), mods: vec![ColorMod::Shade(50_000)] });
        let c = t.resolve(&r).expect("shaded accent1");
        let plain = Color::from_hex("4472C4").unwrap();
        assert!(c.0 <= plain.0 && c.1 <= plain.1 && c.2 <= plain.2, "shade darkens: {c:?}");
        assert_eq!(ColorRef::Rgb("ff0000".into()).resolve(&t), Color::from_hex("FF0000"));
    }

    #[test]
    fn theme_references_write_back_as_scheme_colours() {
        let xml = ColorRef::Theme(ThemeColor {
            slot: "accent1".into(),
            mods: vec![ColorMod::Shade(50_000)],
        })
        .solid_fill_xml();
        assert!(xml.contains("<a:schemeClr val=\"accent1\">"), "{xml}");
        assert!(xml.contains("<a:shade val=\"50000\"/>"), "{xml}");
        let rgb = ColorRef::Rgb("4472c4".into()).solid_fill_xml();
        assert!(rgb.contains("<a:srgbClr val=\"4472C4\"/>"), "{rgb}");
    }
}
