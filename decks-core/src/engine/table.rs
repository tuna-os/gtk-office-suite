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

use super::shape::Color;
use letters_core::model::Run;

/// One cell: its styled text and, when the file says so, its own fill.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableCell {
    pub runs: Vec<Run>,
    pub fill: Option<Color>,
}

impl TableCell {
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}

/// A table's grid and content. Geometry is in model units; column widths
/// and row heights are as the file states them (the drawn table scales them
/// to its box, as PowerPoint does when the two disagree).
#[derive(Clone, Debug, Default, PartialEq)]
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
}

/// How one cell is painted under the table style.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellPaint {
    pub fill: Option<Color>,
    pub text: Color,
    pub bold: bool,
}

impl TableData {
    /// The paint of cell (`r`, `c`) under "Medium Style 2": a cell's own
    /// fill wins; otherwise the header row is the accent with white bold
    /// text, banded rows alternate accent tints (40% and 20%), and all
    /// other cells use the lighter tint.
    pub fn cell_paint(&self, r: usize, c: usize) -> CellPaint {
        let accent = self.accent.unwrap_or(Color(0x44, 0x72, 0xC4));
        let own = self.rows.get(r).and_then(|row| row.get(c)).and_then(|cell| cell.fill);
        let header = self.first_row && r == 0;
        let fill = own.or(Some(if header {
            accent
        } else {
            let body = if self.first_row { r - 1 } else { r };
            if self.band_row && body % 2 == 0 {
                accent.tint(40_000)
            } else {
                accent.tint(20_000)
            }
        }));
        CellPaint { fill, text: if header { Color(255, 255, 255) } else { Color(0, 0, 0) }, bold: header }
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
        }
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
