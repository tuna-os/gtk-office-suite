// color.rs — the one canonical sRGB colour for the whole suite.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Previously implemented three times over: tables-core `Rgb`,
// decks-core `Color`, and letters-core's private `parse_hex` all
// carried byte-identical `from_hex`/`to_hex`/`to_f64` semantics. They
// are moved here so rendering stays pixel-identical while the
// implementation exists once. This is a move, not a redesign: the
// accepted inputs and the rounding are unchanged. Decks' DrawingML
// tint/shade/satMod/lumMod modulation stays in decks-core
// (`engine::shape`), where the spec knowledge lives.

use serde::{Deserialize, Serialize};

/// An sRGB colour: one byte per channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Color(pub u8, pub u8, pub u8);

impl Color {
    /// From `RRGGBB`, `#RRGGBB` or `AARRGGBB` hex (a leading alpha byte
    /// is ignored), as xlsx styles and DrawingML write colours.
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

    /// `RRGGBB`, upper case.
    pub fn to_hex(self) -> String {
        format!("{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }

    /// Components as 0.0–1.0, for Cairo.
    pub fn to_f64(self) -> (f64, f64, f64) {
        (self.0 as f64 / 255.0, self.1 as f64 / 255.0, self.2 as f64 / 255.0)
    }

    /// Components as 16-bit channels (`v * 257`), for Pango attributes.
    pub fn to_u16(self) -> (u16, u16, u16) {
        (u16::from(self.0) * 257, u16::from(self.1) * 257, u16::from(self.2) * 257)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_hex_forms_the_apps_write() {
        // 8-digit AARRGGBB as xlsx writes colours (alpha ignored).
        assert_eq!(Color::from_hex("FFC00000"), Some(Color(0xC0, 0, 0)));
        assert_eq!(Color::from_hex("FF123456"), Some(Color(0x12, 0x34, 0x56)));
        // Plain and #-prefixed RRGGBB, either case.
        assert_eq!(Color::from_hex("ffc7ce"), Some(Color(0xFF, 0xC7, 0xCE)));
        assert_eq!(Color::from_hex("#00FF00"), Some(Color(0, 0xFF, 0)));
        assert_eq!(Color::from_hex("#2a9d3f"), Some(Color(0x2A, 0x9D, 0x3F)));
        // Anything else is not a colour.
        assert_eq!(Color::from_hex("xyz"), None);
        assert_eq!(Color::from_hex("nope"), None);
        assert_eq!(Color::from_hex("12345"), None);
    }

    #[test]
    fn prints_upper_case_rrggbb() {
        assert_eq!(Color(0xC0, 0, 0x0A).to_hex(), "C0000A");
        assert_eq!(Color(0x12, 0xAB, 0x0F).to_hex(), "12AB0F");
    }

    #[test]
    fn converts_to_cairo_and_pango_channels() {
        let (r, g, b) = Color(255, 128, 0).to_f64();
        assert_eq!((r, g, b), (1.0, 128.0 / 255.0, 0.0));
        assert_eq!(Color(0xFF, 0x00, 0x80).to_u16(), (0xFFFF, 0x0000, 0x8080));
        assert_eq!(Color(0, 0, 0).to_u16(), (0, 0, 0));
    }

    #[test]
    fn round_trips_through_hex() {
        for c in [Color(0, 0, 0), Color(255, 255, 255), Color(0x44, 0x72, 0xC4), Color(0x12, 0xAB, 0x0F)] {
            assert_eq!(Color::from_hex(&c.to_hex()), Some(c));
        }
    }
}
