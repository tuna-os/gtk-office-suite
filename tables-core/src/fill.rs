// fill.rs — fill-handle autofill (#113): what dragging a selection's fill
// handle puts in the cells it's dragged over, as Excel and Calc do it.
// SPDX-License-Identifier: GPL-3.0-or-later
//
// extend_fill_kind() decides: number series (1,2 → 3,4; an uneven run
// continues Excel's linear trend), dates by day, month or year, weekday
// and month names, numbered text ("Item 1" → "Item 2"), and otherwise the
// pattern repeats (tile_fill(), which paste also uses). A single number
// repeats, as in Excel, where counting on from one number is Ctrl+drag.
// Formulas repeat here and have their relative references shifted by the
// controller, which has the engine.

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

/// The plain numbers in `source`, if every cell is one.
fn numbers(source: &[FillCell]) -> Option<Vec<f64>> {
    source
        .iter()
        .map(|(text, is_formula)| (!*is_formula).then(|| text.trim().parse::<f64>().ok()).flatten())
        .collect()
}

/// The step if `values` go up (or down) by the same amount each time.
fn constant_step(values: &[f64]) -> Option<f64> {
    let step = values.get(1)? - values[0];
    values.windows(2).all(|w| (w[1] - w[0] - step).abs() < 1e-9).then_some(step)
}

/// Excel's fill of an uneven number sequence: the least-squares line
/// through the values, continued (1, 2, 4 → 5.33, 6.83…).
fn trend(values: &[f64], target_len: usize) -> Vec<f64> {
    let n = values.len() as f64;
    let mean_x = (n - 1.0) / 2.0;
    let mean_y = values.iter().sum::<f64>() / n;
    let (mut sxy, mut sxx) = (0.0, 0.0);
    for (i, y) in values.iter().enumerate() {
        let dx = i as f64 - mean_x;
        sxy += dx * (y - mean_y);
        sxx += dx * dx;
    }
    let slope = if sxx == 0.0 { 0.0 } else { sxy / sxx };
    (0..target_len).map(|i| mean_y + slope * ((values.len() + i) as f64 - mean_x)).collect()
}

/// Dates as spreadsheet serials (days since 1899-12-30).
fn to_date(serial: f64) -> Option<chrono::NaiveDate> {
    (serial.fract() == 0.0).then_some(())?;
    chrono::NaiveDate::from_ymd_opt(1899, 12, 30)?.checked_add_signed(chrono::Duration::days(serial as i64))
}

fn to_serial(date: chrono::NaiveDate) -> f64 {
    let epoch = chrono::NaiveDate::from_ymd_opt(1899, 12, 30).unwrap();
    (date - epoch).num_days() as f64
}

/// Dates continue by whole months (or years) when they fall on the same
/// day of the month, else by their day step; one date goes up a day.
fn date_series(values: &[f64], target_len: usize) -> Option<Vec<f64>> {
    use chrono::Datelike;
    let dates: Vec<chrono::NaiveDate> = values.iter().map(|v| to_date(*v)).collect::<Option<_>>()?;
    if dates.len() == 1 {
        return Some((1..=target_len).map(|i| values[0] + i as f64).collect());
    }
    let months = |d: &chrono::NaiveDate| d.year() as i64 * 12 + d.month0() as i64;
    let same_day = dates.windows(2).all(|w| w[0].day() == w[1].day());
    let month_steps: Vec<i64> = dates.windows(2).map(|w| months(&w[1]) - months(&w[0])).collect();
    if same_day && month_steps.iter().all(|m| *m == month_steps[0] && *m != 0) {
        let last = *dates.last()?;
        return (1..=target_len as i64)
            .map(|i| {
                let m = months(&last) + month_steps[0] * i;
                let (y, m0) = (m.div_euclid(12), m.rem_euclid(12));
                // A day the month doesn't have lands on its last day.
                (1..=last.day()).rev().find_map(|d| chrono::NaiveDate::from_ymd_opt(y as i32, m0 as u32 + 1, d)).map(to_serial)
            })
            .collect();
    }
    let step = constant_step(values)?;
    let last = *values.last()?;
    Some((1..=target_len).map(|i| last + step * i as f64).collect())
}

/// Weekday and month names, short and long, as Excel and Calc fill them.
const LISTS: [&[&str]; 4] = [
    &["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"],
    &["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"],
    &["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
    &["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
];

/// `name` in the case of `like`: UPPER, lower or Title.
fn in_case_of(name: &str, like: &str) -> String {
    if like.chars().all(|c| !c.is_alphabetic() || c.is_uppercase()) && like.chars().filter(|c| c.is_alphabetic()).count() > 1 {
        name.to_uppercase()
    } else if like.chars().all(|c| !c.is_alphabetic() || c.is_lowercase()) {
        name.to_lowercase()
    } else {
        name.to_string()
    }
}

/// Weekdays or months: each cell a name from one list, stepping by the
/// same number of places (one, for a single cell).
fn list_series(source: &[FillCell], target_len: usize) -> Option<Vec<FillCell>> {
    let texts: Vec<&str> = source.iter().map(|(t, f)| (!*f).then_some(t.trim())).collect::<Option<_>>()?;
    for list in LISTS {
        let positions: Option<Vec<usize>> =
            texts.iter().map(|t| list.iter().position(|n| n.eq_ignore_ascii_case(t))).collect();
        let Some(positions) = positions else { continue };
        let len = list.len() as i64;
        let step = if positions.len() == 1 { 1 } else { (positions[1] as i64 - positions[0] as i64).rem_euclid(len) };
        let consistent = positions.windows(2).all(|w| (w[1] as i64 - w[0] as i64).rem_euclid(len) == step);
        if !consistent || step == 0 {
            return None;
        }
        let last = *positions.last()? as i64;
        let like = texts.last()?;
        return Some(
            (1..=target_len as i64)
                .map(|i| (in_case_of(list[(last + step * i).rem_euclid(len) as usize], like), false))
                .collect(),
        );
    }
    None
}

/// Text with a number in it (the last run of digits): "Item 1", "Q1",
/// "Room 007". The text around the number stays; the number counts on by
/// one, or by the step between the cells, keeping its zero padding.
fn numbered_text_series(source: &[FillCell], target_len: usize) -> Option<Vec<FillCell>> {
    let parts: Vec<(String, u64, usize, String)> = source
        .iter()
        .map(|(t, f)| {
            if *f {
                return None;
            }
            let end = t.rfind(|c: char| c.is_ascii_digit())? + 1;
            let start = t[..end].rfind(|c: char| !c.is_ascii_digit()).map_or(0, |i| i + 1);
            // A bare number isn't numbered text.
            (start > 0 || end < t.len()).then_some(())?;
            let digits = &t[start..end];
            Some((t[..start].to_string(), digits.parse().ok()?, digits.len(), t[end..].to_string()))
        })
        .collect::<Option<_>>()?;
    let (prefix, _, width, suffix) = parts.last()?.clone();
    if parts.iter().any(|p| p.0 != prefix || p.3 != suffix) {
        return None;
    }
    let values: Vec<f64> = parts.iter().map(|p| p.1 as f64).collect();
    let step = if values.len() == 1 { 1.0 } else { constant_step(&values)? };
    let last = *values.last()?;
    Some(
        (1..=target_len)
            .map(|i| {
                let n = (last + step * i as f64).max(0.0) as u64;
                (format!("{prefix}{n:0width$}{suffix}"), false)
            })
            .collect(),
    )
}

/// Extend `source` to cover `target_len` cells, as Excel and Calc's
/// autofill does. In order:
/// - formulas and anything unrecognised repeat (their references shift
///   later, in the controller);
/// - dates (when `dates`: the cells are formatted as dates) continue by
///   day, month or year;
/// - two or more numbers continue their step, or Excel's linear trend
///   when the step isn't even; a single number repeats, as in Excel;
/// - weekday and month names continue, in the same case;
/// - numbered text ("Item 1", "Q1") counts on.
pub fn extend_fill_kind(source: &[FillCell], target_len: usize, dates: bool) -> Vec<FillCell> {
    if source.is_empty() || source.iter().any(|(_, f)| *f) {
        return tile_fill(source, target_len);
    }
    if let Some(values) = numbers(source) {
        if dates {
            if let Some(series) = date_series(&values, target_len) {
                return series.into_iter().map(|v| (format_series_value(v), false)).collect();
            }
        }
        if values.len() >= 2 {
            let series = match constant_step(&values) {
                Some(step) => (1..=target_len).map(|i| values[values.len() - 1] + step * i as f64).collect(),
                None => trend(&values, target_len),
            };
            return series.into_iter().map(|v| (format_series_value(v), false)).collect();
        }
        return tile_fill(source, target_len);
    }
    if let Some(series) = list_series(source, target_len) {
        return series;
    }
    if let Some(series) = numbered_text_series(source, target_len) {
        return series;
    }
    tile_fill(source, target_len)
}

/// [`extend_fill_kind`] for cells that aren't dates.
pub fn extend_fill(source: &[FillCell], target_len: usize) -> Vec<FillCell> {
    extend_fill_kind(source, target_len, false)
}

/// Formats a series value the way a spreadsheet cell would: integers
/// with no decimal point, everything else as a plain decimal (not
/// scientific notation, which f64's Display can fall back to for very
/// large/small magnitudes — a range that never occurs from filling a
/// real selection, but the explicit format keeps the guarantee local
/// rather than relying on that being true).
fn format_series_value(v: f64) -> String {
    // Steps like 0.1 accumulate binary noise (0.30000000000000004).
    let v = (v * 1e10).round() / 1e10;
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
    fn an_uneven_number_run_continues_excels_linear_trend() {
        // Excel's fill of 1, 2, 4: the least-squares line (slope 1.5), 5.33… and 6.83…
        let source = vec![v("1"), v("2"), v("4")];
        assert_eq!(extend_fill(&source, 2), vec![v("5.3333333333"), v("6.8333333333")]);
    }

    #[test]
    fn fractional_steps_keep_no_binary_noise() {
        assert_eq!(extend_fill(&[v("0.1"), v("0.2")], 2), vec![v("0.3"), v("0.4")]);
    }

    #[test]
    fn weekdays_and_months_continue_in_their_own_case() {
        assert_eq!(extend_fill(&[v("Mon")], 3), vec![v("Tue"), v("Wed"), v("Thu")]);
        assert_eq!(extend_fill(&[v("Saturday")], 2), vec![v("Sunday"), v("Monday")]);
        assert_eq!(extend_fill(&[v("JAN"), v("MAR")], 2), vec![v("MAY"), v("JUL")]);
        assert_eq!(extend_fill(&[v("december")], 2), vec![v("january"), v("february")]);
        // Not a steady step: repeats.
        assert_eq!(extend_fill(&[v("Mon"), v("Wed"), v("Thu")], 1), vec![v("Mon")]);
    }

    #[test]
    fn numbered_text_counts_on_keeping_its_padding() {
        assert_eq!(extend_fill(&[v("Item 1")], 2), vec![v("Item 2"), v("Item 3")]);
        assert_eq!(extend_fill(&[v("Q1"), v("Q3")], 2), vec![v("Q5"), v("Q7")]);
        assert_eq!(extend_fill(&[v("Room 007")], 1), vec![v("Room 008")]);
        // Different words around the numbers: repeats.
        assert_eq!(extend_fill(&[v("A1"), v("B2")], 1), vec![v("A1")]);
    }

    #[test]
    fn dates_go_by_day_month_or_year() {
        // 2024-01-31 is serial 45322.
        let day = |s: &str| extend_fill_kind(&[v(s)], 2, true);
        assert_eq!(day("45322"), vec![v("45323"), v("45324")]);
        // 2024-01-15, 2024-02-15 → 2024-03-15 (45366), 2024-04-15 (45397).
        assert_eq!(extend_fill_kind(&[v("45306"), v("45337")], 2, true), vec![v("45366"), v("45397")]);
        // Month ends: 2024-01-31 and 2024-02-29 don't share a day, so they
        // step by their 29 days.
        assert_eq!(extend_fill_kind(&[v("45322"), v("45351")], 1, true), vec![v("45380")]);
        // 2023-06-01, 2024-06-01 → 2025-06-01 (45809).
        assert_eq!(extend_fill_kind(&[v("45078"), v("45444")], 1, true), vec![v("45809")]);
        // Not dates: a lone number repeats, as in Excel.
        assert_eq!(extend_fill_kind(&[v("45322")], 1, false), vec![v("45322")]);
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
