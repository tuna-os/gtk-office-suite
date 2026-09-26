// SPDX-License-Identifier: GPL-3.0-or-later
//! What a chart is, shared by the apps that draw one (Tables on the grid,
//! Decks on a slide): its kind, and the value axis its values get. The
//! drawing is `suite_common::charts`. Moved from tables-core, which
//! re-exports both under their old names.

/// How a chart draws its series.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ChartKind {
    Bar,
    Line,
    Pie,
    Scatter,
    Area,
}

/// A value axis from 0 that holds `max`: `(top, step)` with the step
/// 1, 2 or 5 times a power of ten and 4 to 10 intervals, as spreadsheet
/// charts choose them (0..10 by 1 for a maximum of 9).
pub fn nice_axis(max: f64) -> (f64, f64) {
    if !max.is_finite() || max <= 0.0 {
        return (1.0, 0.2);
    }
    // The top is the first tick above `max` (headroom over the tallest bar).
    let top_for = |step: f64| ((max / step).floor() + 1.0) * step;
    let mag = 10f64.powf((max / 10.0).log10().floor());
    [1.0, 2.0, 5.0, 10.0, 20.0, 50.0]
        .iter()
        .map(|m| m * mag)
        .map(|step| (top_for(step), step))
        .find(|(top, step)| (top / step).round() <= 10.0)
        .unwrap_or((top_for(100.0 * mag), 100.0 * mag))
}
