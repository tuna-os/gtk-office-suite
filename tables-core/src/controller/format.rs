// SPDX-License-Identifier: GPL-3.0-or-later
//! Formatting the selection: the commands behind the Format inspector
//! (docs/DESIGN-UI.md, "The Format inspector"). Each is one undoable step
//! over the whole selection, merged blocks included, and a change that
//! changes nothing records no step.

use crate::sheet::{BorderStyle, CellBorder, SheetModel};
use crate::style::CellStyle;

use super::core::WorkbookController;

/// Which edges of the selection a border command draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderPreset {
    /// Remove every border inside and around the selection.
    None,
    /// Every edge of every cell.
    All,
    /// Only the selection's outer edge.
    Outline,
}

impl WorkbookController {
    /// The active cell's style: what the inspector shows.
    pub fn active_style(&self) -> (CellStyle, CellBorder) {
        let state = self.state.borrow();
        let sheet = state.sheet();
        let (r, c) = (sheet.selected_row, sheet.selected_col);
        (sheet.styles[r][c].clone(), sheet.borders[r][c].clone())
    }

    /// The active sheet's default font, `(family, points)`: what a cell
    /// with no font of its own is drawn in.
    pub fn default_font(&self) -> (String, f64) {
        let state = self.state.borrow();
        let sheet = state.sheet();
        (sheet.default_font_family.clone(), sheet.default_font_size)
    }

    /// Apply `change` to the style of every selected cell, as one undo step
    /// named `description`.
    pub fn format_selection(&mut self, description: &'static str, change: impl Fn(&mut CellStyle)) {
        let differs = {
            let state = self.state.borrow();
            let sheet = state.sheet();
            cells(&sheet).any(|(r, c)| {
                let mut s = sheet.styles[r][c].clone();
                change(&mut s);
                s != sheet.styles[r][c]
            })
        };
        if differs {
            self.mutate_sheet(description, |sheet| {
                for (r, c) in cells(sheet).collect::<Vec<_>>() {
                    change(&mut sheet.styles[r][c]);
                }
            });
        }
    }

    /// Draw (or clear) borders on the selection in `style` and `color`.
    pub fn border_selection(&mut self, preset: BorderPreset, style: BorderStyle, color: (f64, f64, f64)) {
        let mut after = {
            let state = self.state.borrow();
            let sheet = state.sheet();
            sheet.borders.clone()
        };
        let (r0, c0, r1, c1) = {
            let state = self.state.borrow();
            let sheet = state.sheet();
            sheet.selection_block()
        };
        for (r, row) in after.iter_mut().enumerate().take(r1 + 1).skip(r0) {
            for (c, b) in row.iter_mut().enumerate().take(c1 + 1).skip(c0) {
                match preset {
                    BorderPreset::None => *b = CellBorder::none(),
                    BorderPreset::All => *b = CellBorder::outline(style.clone(), color),
                    BorderPreset::Outline => {
                        if r == r0 {
                            b.top = style.clone();
                        }
                        if r == r1 {
                            b.bottom = style.clone();
                        }
                        if c == c0 {
                            b.left = style.clone();
                        }
                        if c == c1 {
                            b.right = style.clone();
                        }
                        if !b.is_none() {
                            b.color = color;
                        }
                    }
                }
            }
        }
        let unchanged = self.state.borrow().sheet().borders == after;
        if !unchanged {
            let description = match preset {
                BorderPreset::None => "Clear Borders",
                _ => "Set Borders",
            };
            self.mutate_sheet(description, move |sheet| sheet.borders = after);
        }
    }
}

/// Every cell of the selection block, merged blocks grown to their whole
/// extent.
fn cells(sheet: &SheetModel) -> impl Iterator<Item = (usize, usize)> {
    let (r0, c0, r1, c1) = sheet.selection_block();
    (r0..=r1).flat_map(move |r| (c0..=c1).map(move |c| (r, c)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{HAlign, Rgb};

    fn select(controller: &WorkbookController, r0: usize, c0: usize, r1: usize, c1: usize) {
        let state = controller.state.borrow();
        let mut sheet = state.sheet_mut();
        (sheet.selected_row, sheet.selected_col, sheet.sel_end_row, sheet.sel_end_col) = (r0, c0, r1, c1);
    }

    #[test]
    fn formatting_a_range_is_one_undo_step() {
        let mut controller = WorkbookController::new(4, 4).unwrap();
        select(&controller, 0, 0, 1, 2);
        controller.format_selection("Bold", |s| s.bold = true);
        {
            let state = controller.state.borrow();
            let sheet = state.sheet();
            for r in 0..=1 {
                for c in 0..=2 {
                    assert!(sheet.styles[r][c].bold, "({r},{c})");
                }
            }
            assert!(!sheet.styles[2][0].bold, "outside the selection");
        }
        assert_eq!(controller.undo_description(), Some("Bold"));
        assert!(controller.undo());
        assert!(!controller.state.borrow().sheet().styles[0][0].bold);
        assert!(!controller.can_undo(), "one step, not one per cell");
    }

    #[test]
    fn a_change_that_changes_nothing_records_nothing() {
        let mut controller = WorkbookController::new(2, 2).unwrap();
        controller.format_selection("Align Left", |s| s.h_align = HAlign::General);
        assert!(!controller.can_undo());
        assert!(!controller.is_dirty());
    }

    #[test]
    fn the_active_style_is_what_the_inspector_shows() {
        let mut controller = WorkbookController::new(2, 2).unwrap();
        controller.format_selection("Fill Colour", |s| s.fill = Some(Rgb(0xC6, 0xEF, 0xCE)));
        assert_eq!(controller.active_style().0.fill, Some(Rgb(0xC6, 0xEF, 0xCE)));
    }

    #[test]
    fn a_merged_block_is_formatted_whole() {
        let mut controller = WorkbookController::new(4, 4).unwrap();
        controller.mutate_sheet("Merge", |s| s.merges.push((0, 0, 2, 2)));
        select(&controller, 0, 0, 0, 0);
        controller.format_selection("Italic", |s| s.italic = true);
        assert!(controller.state.borrow().sheet().styles[1][1].italic);
    }

    #[test]
    fn outline_borders_only_the_outer_edge() {
        let mut controller = WorkbookController::new(4, 4).unwrap();
        select(&controller, 0, 0, 1, 1);
        controller.border_selection(BorderPreset::Outline, BorderStyle::Thick, (0.0, 0.0, 0.0));
        {
            let state = controller.state.borrow();
            let b = &state.sheet().borders;
            assert_eq!((&b[0][0].top, &b[0][0].left), (&BorderStyle::Thick, &BorderStyle::Thick));
            assert_eq!((&b[0][0].bottom, &b[0][0].right), (&BorderStyle::None, &BorderStyle::None));
            assert_eq!((&b[1][1].bottom, &b[1][1].right), (&BorderStyle::Thick, &BorderStyle::Thick));
            assert!(b[2][2].is_none());
        }
        controller.border_selection(BorderPreset::None, BorderStyle::None, (0.0, 0.0, 0.0));
        assert!(controller.state.borrow().sheet().borders[0][0].is_none());
        assert_eq!(controller.undo_description(), Some("Clear Borders"));
    }
}
