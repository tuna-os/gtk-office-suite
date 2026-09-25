// fill.rs — fill-handle copy/series logic (#113).
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Two fill strategies: series (numeric linear extrapolation — 1,2,3
// filled further gives 4,5,6) and tile (repeat the pattern — anything
// non-numeric, or a single numeric cell, since Excel's own default for
// a lone numeric cell is copy, not increment; incrementing a single
// cell is normally a Ctrl-drag, out of scope here since that needs GTK
// modifier-key awareness this crate has no business knowing about).
// extend_fill() picks whichever applies; tile_fill() stays available
// directly for callers (e.g. paste) that always want literal repetition.
//
// Relative-reference adjustment for formulas (=A1 -> =A2 when filled
// down a row) is a separate, still-deferred feature — a formula cell's
// literal text is copied/tiled unadjusted either way. Tracked as
// follow-up under #113.

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

/// If `source` is 2+ plain (non-formula) numeric cells with a constant
/// difference between consecutive values, returns that difference — the
/// series step. `None` for anything else (single cell, non-numeric,
/// formulas, or an uneven/non-arithmetic sequence), meaning the caller
/// should fall back to tile_fill.
fn arithmetic_step(source: &[FillCell]) -> Option<f64> {
    if source.len() < 2 {
        return None;
    }
    let values: Option<Vec<f64>> = source
        .iter()
        .map(|(text, is_formula)| (!*is_formula).then(|| text.trim().parse::<f64>().ok()).flatten())
        .collect();
    let values = values?;
    let step = values[1] - values[0];
    let is_arithmetic = values.windows(2).all(|w| (w[1] - w[0] - step).abs() < 1e-9);
    is_arithmetic.then_some(step)
}

/// Extend `source` to cover `target_len` cells: continues a numeric
/// arithmetic series if `source` is one, otherwise tiles it (see module
/// doc for why a single numeric cell tiles rather than increments).
pub fn extend_fill(source: &[FillCell], target_len: usize) -> Vec<FillCell> {
    if let Some(step) = arithmetic_step(source) {
        let last: f64 = source.last().unwrap().0.trim().parse().unwrap();
        return (1..=target_len)
            .map(|i| (format_series_value(last + step * i as f64), false))
            .collect();
    }
    tile_fill(source, target_len)
}

/// Formats a series value the way a spreadsheet cell would: integers
/// with no decimal point, everything else as a plain decimal (not
/// scientific notation, which f64's Display can fall back to for very
/// large/small magnitudes — a range that never occurs from filling a
/// real selection, but the explicit format keeps the guarantee local
/// rather than relying on that being true).
fn format_series_value(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
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
    // How far outside the selection the drag ended, along each axis.
    let rows_out = drag_row.saturating_sub(bottom).max(top.saturating_sub(drag_row));
    let cols_out = drag_col.saturating_sub(right).max(left.saturating_sub(drag_col));
    // The handle sits on the selection's corner, so a straight drag from
    // it easily ends a cell over on the other axis too (#520). The fill
    // goes along the axis the drag went further on; a tie goes down or
    // up, as in Excel and Calc.
    if rows_out == 0 && cols_out == 0 {
        None
    } else if rows_out >= cols_out {
        Some(if drag_row > bottom { (FillDirection::Down, rows_out) } else { (FillDirection::Up, rows_out) })
    } else {
        Some(if drag_col > right { (FillDirection::Right, cols_out) } else { (FillDirection::Left, cols_out) })
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
    fn extend_fill_continues_an_arithmetic_series() {
        let source = vec![v("1"), v("2"), v("3")];
        assert_eq!(extend_fill(&source, 3), vec![v("4"), v("5"), v("6")]);
    }

    #[test]
    fn extend_fill_continues_a_descending_series() {
        let source = vec![v("10"), v("8")];
        assert_eq!(extend_fill(&source, 3), vec![v("6"), v("4"), v("2")]);
    }

    #[test]
    fn extend_fill_handles_non_integer_steps() {
        let source = vec![v("0.5"), v("1")];
        assert_eq!(extend_fill(&source, 2), vec![v("1.5"), v("2")]);
    }

    #[test]
    fn extend_fill_falls_back_to_tile_for_a_single_cell() {
        // Matches Excel's default (no Ctrl modifier): one numeric cell
        // copies, it doesn't increment.
        let source = vec![v("5")];
        assert_eq!(extend_fill(&source, 3), vec![v("5"), v("5"), v("5")]);
    }

    #[test]
    fn extend_fill_falls_back_to_tile_for_a_non_arithmetic_sequence() {
        let source = vec![v("1"), v("2"), v("4")];
        assert_eq!(extend_fill(&source, 2), vec![v("1"), v("2")]);
    }

    #[test]
    fn extend_fill_falls_back_to_tile_for_non_numeric_or_formula_cells() {
        let mixed = vec![v("1"), v("x")];
        assert_eq!(extend_fill(&mixed, 2), vec![v("1"), v("x")]);
        let formulas = vec![("1".into(), true), ("2".into(), true)];
        assert_eq!(extend_fill(&formulas, 2), formulas[..2].to_vec());
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
    fn infer_fill_off_both_axes_follows_the_longer_one() {
        // Straight down from the corner handle, ending a column over.
        assert_eq!(infer_fill((0, 0, 0, 0), 3, 1), Some((FillDirection::Down, 3)));
        assert_eq!(infer_fill((2, 2, 4, 4), 5, 9), Some((FillDirection::Right, 5)));
        assert_eq!(infer_fill((2, 2, 4, 4), 0, 1), Some((FillDirection::Up, 2)));
        // A tie fills down or up.
        assert_eq!(infer_fill((2, 2, 4, 4), 6, 6), Some((FillDirection::Down, 2)));
    }
}
