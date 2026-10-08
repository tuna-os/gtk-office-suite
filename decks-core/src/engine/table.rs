// table.rs — a table on a slide: its grid, its cells, and how it is painted.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// A pptx table is a `p:graphicFrame` holding an `a:tbl`: a column grid, rows
// with heights, and cells with rich text and optional fills. The deck reader
// used to skip graphic frames entirely, so a slide's table simply wasn't there
// (render lab `decks/table`). Its look comes from a table style: PowerPoint's
// default, "Medium Style 2 - Accent 1", has an accent-filled header row with
// white bold text, banded tinted rows and white rules. The model keeps the
// style flags, and `cell_paint` resolves them into what each cell draws.

use super::shape::{Color, ColorModulation};
use letters_core::model::Run;

/// One cell: its styled text and, when the file says so, its own fill. A
/// merged cell spans more than one column or row (`a:tc gridSpan`,
/// `rowSpan`); each cell it covers is still in the grid, marked `covered`
/// (`hMerge`, `vMerge`), as DrawingML keeps them.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TableCell {
    pub runs: Vec<Run>,
    pub fill: Option<Color>,
    /// Columns the cell spans; 0 and 1 are one.
    #[cfg_attr(feature = "serde", serde(default))]
    pub col_span: u32,
    /// Rows the cell spans; 0 and 1 are one.
    #[cfg_attr(feature = "serde", serde(default))]
    pub row_span: u32,
    /// Covered by a merged cell before it.
    #[cfg_attr(feature = "serde", serde(default))]
    pub covered: bool,
}

impl TableCell {
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// The columns and rows the cell spans, each at least one.
    pub fn span(&self) -> (usize, usize) {
        (self.col_span.max(1) as usize, self.row_span.max(1) as usize)
    }
}

/// A table's grid and content. Geometry is in model units; column widths
/// and row heights are as the file states them (the drawn table scales them
/// to its box, as PowerPoint does when the two disagree).
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TableData {
    pub col_widths: Vec<f64>,
    pub row_heights: Vec<f64>,
    pub rows: Vec<Vec<TableCell>>,
    /// `a:tblPr firstRow`: the first row is a header.
    pub first_row: bool,
    /// `a:tblPr bandRow`: alternate rows are banded.
    pub band_row: bool,
    /// The accent colour the table style is built from (the theme's
    /// accent 1 for the default style).
    pub accent: Option<Color>,
    /// Cell text insets (left/right, top/bottom) in model units. `None`:
    /// DrawingML's 0.1in and 0.05in on our own 10-inch slide. A reader
    /// sets it for the slide it read, since on a wider slide those
    /// inches are fewer model units (render lab `decks/table`: the text
    /// sat 3 px right of and below LibreOffice's).
    pub cell_margins: Option<(f64, f64)>,
    /// The table style it names (`a:tableStyleId`), one of PowerPoint's
    /// built-in styles by its GUID; `None` is the default, "Medium Style 2
    /// - Accent 1". `accent` is that style's colour.
    #[cfg_attr(feature = "serde", serde(default))]
    pub style_id: Option<String>,
}

/// The default table style's GUID: "Medium Style 2 - Accent 1".
pub const DEFAULT_STYLE_ID: &str = "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}";

/// The families of PowerPoint's built-in table styles this model draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleFamily {
    /// Plain cells, no rules: "No Style, No Grid".
    NoStyle,
    /// Plain cells in a black grid: "No Style, Table Grid".
    TableGrid,
    /// Rules above and below, a ruled header, tinted bands.
    LightStyle1,
    /// A box in the colour, a filled header.
    LightStyle2,
    /// A grid in the colour, a bold header, tinted bands.
    LightStyle3,
    /// Tinted cells in a white grid under a filled header (the default).
    MediumStyle2,
    /// White and grey bands between heavy rules, a filled header.
    MediumStyle3,
}

/// A built-in table style by its GUID: its family, and the theme colour it
/// is built from ("dk1" for a family's plain style, "accent1".."accent6"
/// for its accented ones). The GUIDs are PowerPoint's, in the order its
/// gallery lists them, plain style first.
pub fn builtin_style(id: &str) -> Option<(StyleFamily, &'static str)> {
    const SLOTS: [&str; 7] = ["dk1", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6"];
    const FAMILIES: [(StyleFamily, [&str; 7]); 5] = [
        (StyleFamily::LightStyle1, [
            "9D7B26C5-4107-4FEC-AEDC-1716B250A1EF", "3B4B98B0-60AC-42C2-AFA5-B58CD77FA1E5", "0E3FDE45-AF77-4B5C-9715-49D594BDF05E",
            "C083E6E3-FA7D-4D7B-A595-EF9225AFEA82", "D27102A9-8310-4765-A935-A1911B00CA55", "5FD0F851-EC5A-4D38-B0AD-8093EC10F338",
            "68D230F3-CF80-4859-8CE7-A43EE81993B5",
        ]),
        (StyleFamily::LightStyle2, [
            "7E9639D4-E3E2-4D34-9284-5A2195B3D0D7", "69012ECD-51FC-41F1-AA8D-1B2483CD663E", "72833802-FEF1-4C79-8D5D-14CF1EAF98D9",
            "F2DE63D5-997A-4646-A377-4702673A728D", "17292A2E-F333-43FB-9621-5CBBE7FDCDCB", "5A111915-BE36-4E01-A7E5-04B1672EAD32",
            "912C8C85-51F0-491E-9774-3900AFEF0FD7",
        ]),
        (StyleFamily::LightStyle3, [
            "616DA210-FB5B-4158-B5E0-FEB733F419BA", "BC89EF96-8CEA-46FF-86C4-4CE0E7609802", "5DA37D80-6434-44D0-A028-1B22A696006F",
            "8799B23B-EC83-4686-B30A-512413B5E67A", "ED083AE6-46FA-4A59-8FB0-9F97EB10719F", "BDBED569-4797-4DF1-A0F4-6AAB3CD982D8",
            "E8B1032C-EA38-4F05-BA0D-38AFFFC7BED3",
        ]),
        (StyleFamily::MediumStyle2, [
            "073A0DAA-6AF3-43AB-8588-CEC1D06C72B9", "5C22544A-7EE6-4342-B048-85BDC9FD1C3A", "21E4AEA4-8DFA-4A89-87EB-49C32662AFE0",
            "F5AB1C69-6EDB-4FF4-983F-18BD219EF322", "00A15C55-8517-42AA-B614-E9B94910E393", "7DF18680-E054-41AD-8BC1-D1AEF772440D",
            "93296810-A885-4BE3-A3E7-6D5BEEA58F35",
        ]),
        (StyleFamily::MediumStyle3, [
            "8EC20E35-A176-4012-BC5E-935CFFF8708E", "6E25E649-3F16-4E02-A733-19D2CDBF48F0", "85BE263C-DBD7-4A20-BB59-AAB30ACAA65A",
            "EB344D84-9AFB-497E-A393-DC336BA19D2E", "EB9631B5-78F2-41C9-869B-9F39066F8104", "74C1A8A3-306A-4EB7-A6B1-4F7E0EB9C5D6",
            "2A488322-F2BA-4B5B-9748-0D474271808F",
        ]),
    ];
    let guid = id.trim().trim_start_matches('{').trim_end_matches('}').to_ascii_uppercase();
    match guid.as_str() {
        "2D5ABB26-0587-4C30-8999-92F81FD0307C" => return Some((StyleFamily::NoStyle, "dk1")),
        "5940675A-B579-460E-94D1-54222C63F5DA" => return Some((StyleFamily::TableGrid, "dk1")),
        _ => {}
    }
    FAMILIES.iter().find_map(|(family, ids)| ids.iter().position(|g| *g == guid).map(|i| (*family, SLOTS[i])))
}

/// The rules a table style draws: their colour and width (points), and
/// where. `header` is the width of the rule under a header row, if any.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rules {
    pub color: Color,
    pub width_pt: f64,
    pub outer: bool,
    pub top_bottom: bool,
    pub inside_h: bool,
    pub inside_v: bool,
    pub header: Option<f64>,
}

impl TableData {
    /// For each grid position, the cell drawn there: the merged cell that
    /// covers it, or its own. Indexed `[row][col]`.
    pub fn owners(&self) -> Vec<Vec<(usize, usize)>> {
        let rows = self.rows.len();
        let cols = self.rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut owner: Vec<Vec<(usize, usize)>> = (0..rows).map(|r| (0..cols).map(|c| (r, c)).collect()).collect();
        for (r, row) in self.rows.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                if cell.covered || owner[r][c] != (r, c) {
                    continue;
                }
                let (cs, rs) = cell.span();
                for row in owner.iter_mut().skip(r).take(rs) {
                    for at in row.iter_mut().skip(c).take(cs) {
                        *at = (r, c);
                    }
                }
            }
        }
        owner
    }

    /// `cell_margins`, defaulted.
    pub fn margins(&self) -> (f64, f64) {
        self.cell_margins.unwrap_or((9.6, 4.8))
    }
}

/// How one cell is painted under the table style.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CellPaint {
    pub fill: Option<Color>,
    pub text: Color,
    pub bold: bool,
}

impl TableData {
    /// The style's family; an unknown style is drawn as the default.
    pub fn family(&self) -> StyleFamily {
        self.style_id.as_deref().and_then(builtin_style).map_or(StyleFamily::MediumStyle2, |(f, _)| f)
    }

    /// The paint of cell (`r`, `c`) under the table's style. A cell's own
    /// fill wins. "Medium Style 2", the default: the header row is the
    /// accent with white bold text, banded rows alternate accent tints (40%
    /// and 20%), and all other cells use the lighter tint. The other
    /// families as PowerPoint draws them: see `StyleFamily`.
    pub fn cell_paint(&self, r: usize, c: usize) -> CellPaint {
        let accent = self.accent.unwrap_or(Color(0x44, 0x72, 0xC4));
        let own = self.rows.get(r).and_then(|row| row.get(c)).and_then(|cell| cell.fill);
        let header = self.first_row && r == 0;
        let banded = !header && self.band_row && (if self.first_row { r - 1 } else { r }) % 2 == 0;
        let black = Color(0, 0, 0);
        let white = Color(255, 255, 255);
        // A band in the colour at 20% over white (`alpha val="20000"`).
        let wash = |c: Color| {
            let mix = |v: u8| (f64::from(v) * 0.2 + 255.0 * 0.8).round() as u8;
            Color(mix(c.0), mix(c.1), mix(c.2))
        };
        let (fill, text, bold) = match self.family() {
            StyleFamily::MediumStyle2 => (
                Some(if header { accent } else if banded { accent.tint(40_000) } else { accent.tint(20_000) }),
                if header { white } else { black },
                header,
            ),
            StyleFamily::MediumStyle3 => (
                Some(if header { accent } else if banded { wash(black) } else { white }),
                if header { white } else { black },
                header,
            ),
            StyleFamily::LightStyle1 | StyleFamily::LightStyle3 => (banded.then(|| wash(accent)), black, header),
            StyleFamily::LightStyle2 => (header.then_some(accent), if header { white } else { black }, header),
            StyleFamily::NoStyle | StyleFamily::TableGrid => (None, black, false),
        };
        CellPaint { fill: own.or(fill), text, bold }
    }

    /// The rules the table's style draws between and around its cells.
    pub fn rules(&self) -> Option<Rules> {
        let accent = self.accent.unwrap_or(Color(0x44, 0x72, 0xC4));
        let none = Rules { color: accent, width_pt: 1.0, outer: false, top_bottom: false, inside_h: false, inside_v: false, header: None };
        let header = |w: f64| self.first_row.then_some(w);
        Some(match self.family() {
            StyleFamily::NoStyle => return None,
            StyleFamily::TableGrid => Rules { color: Color(0, 0, 0), outer: true, inside_h: true, inside_v: true, ..none },
            StyleFamily::MediumStyle2 => Rules { color: Color(255, 255, 255), inside_h: true, inside_v: true, ..none },
            StyleFamily::MediumStyle3 => Rules { color: Color(0, 0, 0), width_pt: 2.0, top_bottom: true, header: header(2.0), ..none },
            StyleFamily::LightStyle1 => Rules { top_bottom: true, header: header(1.0), ..none },
            StyleFamily::LightStyle2 => Rules { outer: true, ..none },
            StyleFamily::LightStyle3 => Rules { outer: true, inside_h: true, inside_v: true, header: header(2.0), ..none },
        })
    }

    /// Column widths and row heights scaled so the grid fills a `w`×`h` box.
    pub fn fitted(&self, w: f64, h: f64) -> (Vec<f64>, Vec<f64>) {
        let fit = |sizes: &[f64], total: f64| -> Vec<f64> {
            let sum: f64 = sizes.iter().sum();
            if sum <= 0.0 {
                let n = sizes.len().max(1) as f64;
                return vec![total / n; sizes.len()];
            }
            sizes.iter().map(|s| s / sum * total).collect()
        };
        (fit(&self.col_widths, w), fit(&self.row_heights, h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(first_row: bool, band_row: bool) -> TableData {
        TableData {
            col_widths: vec![1.0, 1.0, 2.0],
            row_heights: vec![1.0; 4],
            rows: vec![vec![TableCell::default(); 3]; 4],
            first_row,
            band_row,
            accent: Some(Color(0x4F, 0x81, 0xBD)),
            cell_margins: None,
            style_id: None,
        }
    }

    /// PowerPoint's built-in styles by GUID, as its gallery orders them:
    /// each family's plain style in dark 1, then its six accents.
    #[test]
    fn built_in_styles_are_known_by_their_guid() {
        assert_eq!(builtin_style(DEFAULT_STYLE_ID), Some((StyleFamily::MediumStyle2, "accent1")));
        assert_eq!(builtin_style("{616DA210-FB5B-4158-B5E0-FEB733F419BA}"), Some((StyleFamily::LightStyle3, "dk1")));
        assert_eq!(builtin_style("{6e25e649-3f16-4e02-a733-19d2cdbf48f0}"), Some((StyleFamily::MediumStyle3, "accent1")));
        assert_eq!(builtin_style("{93296810-A885-4BE3-A3E7-6D5BEEA58F35}"), Some((StyleFamily::MediumStyle2, "accent6")));
        assert_eq!(builtin_style("{2D5ABB26-0587-4C30-8999-92F81FD0307C}"), Some((StyleFamily::NoStyle, "dk1")));
        assert_eq!(builtin_style("{00000000-0000-0000-0000-000000000000}"), None);
    }

    /// "Light Style 3": no fill but a 20% wash on banded rows, a bold
    /// header, and a grid in the style's colour with a heavier rule under
    /// the header.
    #[test]
    fn light_style_3_is_a_grid_with_washed_bands() {
        let mut t = table(true, true);
        t.style_id = Some("{616DA210-FB5B-4158-B5E0-FEB733F419BA}".into());
        t.accent = Some(Color(0, 0, 0));
        let header = t.cell_paint(0, 0);
        assert_eq!((header.fill, header.bold), (None, true));
        assert_eq!(t.cell_paint(1, 0).fill, Some(Color(204, 204, 204)), "the first band");
        assert_eq!(t.cell_paint(2, 0).fill, None);
        let rules = t.rules().unwrap();
        assert!(rules.outer && rules.inside_h && rules.inside_v && rules.header == Some(2.0));
    }

    /// "No Style, No Grid" draws nothing but the text.
    #[test]
    fn no_style_draws_no_fill_and_no_rules() {
        let mut t = table(true, true);
        t.style_id = Some("{2D5ABB26-0587-4C30-8999-92F81FD0307C}".into());
        assert_eq!(t.cell_paint(0, 0), CellPaint { fill: None, text: Color(0, 0, 0), bold: false });
        assert_eq!(t.rules(), None);
    }

    #[test]
    fn cell_margins_default_to_drawingmls_on_the_models_slide() {
        let mut t = table(false, false);
        assert_eq!(t.margins(), (9.6, 4.8));
        t.cell_margins = Some((7.2, 3.6));
        assert_eq!(t.margins(), (7.2, 3.6));
    }

    #[test]
    fn the_header_row_is_the_accent_with_white_bold_text() {
        let t = table(true, true);
        let p = t.cell_paint(0, 1);
        assert_eq!((p.fill, p.text, p.bold), (Some(Color(0x4F, 0x81, 0xBD)), Color(255, 255, 255), true));
    }

    #[test]
    fn body_rows_band_between_two_tints() {
        let t = table(true, true);
        let (a, b, c) = (t.cell_paint(1, 0).fill, t.cell_paint(2, 0).fill, t.cell_paint(3, 0).fill);
        assert_ne!(a, b, "banded rows differ");
        assert_eq!(a, c, "and alternate");
        // Without banding every body row is the same.
        let t = table(true, false);
        assert_eq!(t.cell_paint(1, 0).fill, t.cell_paint(2, 0).fill);
    }

    #[test]
    fn a_cell_fill_of_its_own_wins() {
        let mut t = table(true, true);
        t.rows[2][1].fill = Some(Color(1, 2, 3));
        assert_eq!(t.cell_paint(2, 1).fill, Some(Color(1, 2, 3)));
    }

    #[test]
    fn the_grid_scales_to_the_box() {
        let (cols, rows) = table(true, true).fitted(400.0, 200.0);
        assert_eq!(cols, vec![100.0, 100.0, 200.0]);
        assert_eq!(rows, vec![50.0; 4]);
    }
}
