// style.rs — what a cell looks like, apart from its number format and borders.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// The Phase 1 "Tables cell-style model" (docs/RENDER-PARITY-ROADMAP.md): font,
// fill, alignment and wrap, per cell, GTK-free. The xlsx reader fills it from
// styles.xml, the writer carries it back out, and the grid draws it. Before
// this, a workbook's bold headers, coloured fills and centred or wrapped cells
// all opened as plain left-aligned text in one font.

/// An sRGB colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// From `RRGGBB` or `AARRGGBB` hex, as xlsx writes colours.
    pub fn from_hex(hex: &str) -> Option<Rgb> {
        let hex = hex.trim().trim_start_matches('#');
        let hex = match hex.len() {
            8 => &hex[2..],
            6 => hex,
            _ => return None,
        };
        let c = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some(Rgb(c(0)?, c(2)?, c(4)?))
    }

    /// `RRGGBB`, upper case.
    pub fn to_hex(self) -> String {
        format!("{:02X}{:02X}{:02X}", self.0, self.1, self.2)
    }

    /// Components as 0.0–1.0, for Cairo.
    pub fn to_f64(self) -> (f64, f64, f64) {
        (self.0 as f64 / 255.0, self.1 as f64 / 255.0, self.2 as f64 / 255.0)
    }
}

/// Horizontal alignment. `General` is the spreadsheet default: numbers
/// right, text left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HAlign {
    #[default]
    General,
    Left,
    Center,
    Right,
}

/// Vertical alignment. Spreadsheets default to the bottom of the cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VAlign {
    Top,
    Center,
    #[default]
    Bottom,
}

/// Everything about a cell's appearance that isn't its number format or
/// its borders. `None` means "the workbook default".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CellStyle {
    pub font_family: Option<String>,
    /// Points.
    pub font_size: Option<f64>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub color: Option<Rgb>,
    pub fill: Option<Rgb>,
    pub h_align: HAlign,
    pub v_align: VAlign,
    pub wrap: bool,
    /// Indent level; one level is three characters' width, as in Excel.
    pub indent: u8,
}

impl CellStyle {
    pub fn is_default(&self) -> bool {
        *self == CellStyle::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_parse_from_xlsx_hex() {
        assert_eq!(Rgb::from_hex("FFC00000"), Some(Rgb(0xC0, 0, 0)));
        assert_eq!(Rgb::from_hex("ffc7ce"), Some(Rgb(0xFF, 0xC7, 0xCE)));
        assert_eq!(Rgb::from_hex("#00FF00"), Some(Rgb(0, 0xFF, 0)));
        assert_eq!(Rgb::from_hex("xyz"), None);
        assert_eq!(Rgb(0xC0, 0, 0x0A).to_hex(), "C0000A");
    }

    #[test]
    fn the_default_style_is_the_spreadsheet_default() {
        let s = CellStyle::default();
        assert!(s.is_default());
        assert_eq!((s.h_align, s.v_align, s.wrap), (HAlign::General, VAlign::Bottom, false));
    }
}
