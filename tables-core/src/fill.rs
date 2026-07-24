// fill.rs — fill-handle copy logic (#113, first slice).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Scope: literal copy/tile only. Dragging the fill handle repeats the
// selected pattern into the target range — the same behavior spreadsheets
// give a non-numeric or mixed selection. Series detection (1,2,3 -> 4,5,6)
// and relative-reference adjustment for formulas (=A1 -> =A2 when filled
// down a row) are real, separate features, deliberately out of scope for
// this slice — a formula cell's literal text is copied unadjusted, same
// as any other cell's content. Tracked as follow-up under #113.

/// One cell's fillable content: its input text (formula text including
/// the leading `=`, or a plain value) and whether it's a formula.
pub type FillCell = (String, bool);

/// Tile `source` (the pattern being dragged from) to cover `target_len`
/// cells, repeating from the start once exhausted. `source` must be
/// non-empty; an empty target is a no-op (empty result).
pub fn tile_fill(source: &[FillCell], target_len: usize) -> Vec<FillCell> {
    if source.is_empty() {
        return Vec::new();
    }
    (0..target_len).map(|i| source[i % source.len()].clone()).collect()
}

/// Direction inferred from how the drag extended the selection relative
/// to its own bounds. Fill only ever extends in one axis at a time —
/// same as every spreadsheet's fill handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillDirection {
    Down,
    Up,
    Right,
    Left,
}

/// Where a fill-handle drag landed, relative to the original selection
/// `(sel_top, sel_left, sel_bottom, sel_right)` (inclusive), determines
/// both the direction and how far the fill extends. Returns `None` if
/// the drag didn't leave the selection (no fill to do).
pub fn infer_fill(
    sel: (usize, usize, usize, usize),
    drag_row: usize,
    drag_col: usize,
) -> Option<(FillDirection, usize)> {
    let (top, left, bottom, right) = sel;
    if drag_row > bottom && drag_col >= left && drag_col <= right {
        Some((FillDirection::Down, drag_row - bottom))
    } else if drag_row < top && drag_col >= left && drag_col <= right {
        Some((FillDirection::Up, top - drag_row))
    } else if drag_col > right && drag_row >= top && drag_row <= bottom {
        Some((FillDirection::Right, drag_col - right))
    } else if drag_col < left && drag_row >= top && drag_row <= bottom {
        Some((FillDirection::Left, left - drag_col))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> FillCell {
        (s.to_string(), false)
    }

    #[test]
    fn tile_fill_repeats_single_cell() {
        let source = vec![v("42")];
        assert_eq!(tile_fill(&source, 3), vec![v("42"), v("42"), v("42")]);
    }

    #[test]
    fn tile_fill_tiles_a_multi_cell_pattern() {
        let source = vec![v("1"), v("2")];
        assert_eq!(
            tile_fill(&source, 5),
            vec![v("1"), v("2"), v("1"), v("2"), v("1")]
        );
    }

    #[test]
    fn tile_fill_of_empty_source_is_empty() {
        assert_eq!(tile_fill(&[], 5), Vec::<FillCell>::new());
    }

    #[test]
    fn tile_fill_target_shorter_than_source_truncates() {
        let source = vec![v("a"), v("b"), v("c")];
        assert_eq!(tile_fill(&source, 2), vec![v("a"), v("b")]);
    }

    #[test]
    fn infer_fill_detects_downward_drag() {
        // Selection is row 2..=4, col 1..=1; drag lands at row 6 -> down by 2.
        let got = infer_fill((2, 1, 4, 1), 6, 1);
        assert_eq!(got, Some((FillDirection::Down, 2)));
    }

    #[test]
    fn infer_fill_detects_rightward_drag() {
        let got = infer_fill((0, 0, 0, 2), 0, 5);
        assert_eq!(got, Some((FillDirection::Right, 3)));
    }

    #[test]
    fn infer_fill_detects_upward_and_leftward_drag() {
        assert_eq!(infer_fill((5, 0, 8, 0), 3, 0), Some((FillDirection::Up, 2)));
        assert_eq!(infer_fill((0, 5, 0, 8), 0, 2), Some((FillDirection::Left, 3)));
    }

    #[test]
    fn infer_fill_inside_selection_is_none() {
        assert_eq!(infer_fill((2, 2, 4, 4), 3, 3), None);
    }

    #[test]
    fn infer_fill_diagonal_off_axis_is_none() {
        // Landing outside both row range and col range isn't a valid
        // single-axis fill drag.
        assert_eq!(infer_fill((2, 2, 4, 4), 6, 6), None);
    }
}
