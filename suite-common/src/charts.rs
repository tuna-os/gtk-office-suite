// SPDX-License-Identifier: GPL-3.0-or-later
//! Chart rendering with Cairo: column, line, area, pie and XY scatter, drawn
//! as Excel and LibreOffice Calc draw a chart that names no style. The
//! render lab compares each kind against LibreOffice (`tables/chart`,
//! `tables/chart-line`, `-area`, `-pie`, `-scatter`).
//!
//! Shared by Tables (charts on the grid, `tables::charts` re-exports this)
//! and Decks (charts on a slide). One series or several: [`draw_chart`] is
//! [`draw_chart_series`] with one, drawn exactly as before.

use gtk4::cairo::{Context, Format, ImageSurface};
use suite_common_core::charts::{nice_axis, ChartKind};

pub fn render_chart(data: &[(String, f64)], kind: ChartKind, width: i32, height: i32) -> ImageSurface {
    render_chart_named(data, kind, width, height, None)
}

/// As [`render_chart`], with a legend entry for the series when it has a
/// name (charts on the grid do; the dialog's preview doesn't need one). A
/// pie's legend names its categories instead. For a scatter chart, each
/// point's label is its x value.
pub fn render_chart_named(
    data: &[(String, f64)],
    kind: ChartKind,
    width: i32,
    height: i32,
    series_name: Option<&str>,
) -> ImageSurface {
    let surface = ImageSurface::create(Format::ARgb32, width.max(1), height.max(1)).unwrap();
    let cr = Context::new(&surface).unwrap();
    crate::use_ui_font_rendering(&cr);
    draw_chart(&cr, data, kind, width as f64, height as f64, series_name);
    drop(cr);
    surface.flush();
    surface
}

/// Draw the chart into `cr` at its origin, `width` x `height` in its
/// units, on a white ground: what [`render_chart_named`] draws onto a
/// surface, for a caller with a context of its own (a slide on the canvas
/// or in a PDF, where it stays vector).
pub fn draw_chart(cr: &Context, data: &[(String, f64)], kind: ChartKind, width: f64, height: f64, series_name: Option<&str>) {
    let categories: Vec<String> = data.iter().map(|d| d.0.clone()).collect();
    let values: Vec<f64> = data.iter().map(|d| d.1).collect();
    draw_chart_series(cr, &categories, &[Series { name: series_name, values: &values }], kind, width, height);
}

/// One series of a chart: its values, one per category (a missing one is
/// a gap), and its name for the legend.
#[derive(Clone, Copy, Debug)]
pub struct Series<'a> {
    pub name: Option<&'a str>,
    pub values: &'a [f64],
}

impl Series<'_> {
    fn at(&self, i: usize) -> Option<f64> {
        self.values.get(i).copied().filter(|v| v.is_finite())
    }
    fn max(&self) -> f64 {
        self.values.iter().copied().filter(|v| v.is_finite()).fold(0.0, f64::max)
    }
}

/// As [`draw_chart`], with any number of series over the same categories,
/// as Excel and Calc draw them: columns clustered in each category's slot,
/// a line, area or set of markers per series, each series in the next of
/// the theme's accents, and a legend row per named series. A pie draws
/// its first series. With one series it is exactly [`draw_chart`].
pub fn draw_chart_series(cr: &Context, categories: &[String], series: &[Series], kind: ChartKind, width: f64, height: f64) {
    let _ = cr.save();
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.rectangle(0.0, 0.0, width, height);
    let _ = cr.fill();
    // Chart text is Calibri 10 pt (Carlito where Calibri isn't
    // installed), as Excel and Calc draw a chart that names no font.
    cr.select_font_face("Calibri", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    cr.set_font_size(CHART_TEXT_PX);
    let (w, h) = (width, height);
    if !series.is_empty() {
        match kind {
            ChartKind::Bar => draw_bars(cr, categories, series, w, h),
            ChartKind::Line => draw_category(cr, categories, series, w, h, Style::Line),
            ChartKind::Area => draw_category(cr, categories, series, w, h, Style::Area),
            ChartKind::Pie => draw_pie(cr, categories, &series[0], w, h),
            ChartKind::Scatter => draw_scatter(cr, categories, series, w, h),
        }
    }
    let _ = cr.restore();
}

/// 10 pt at 96 DPI.
const CHART_TEXT_PX: f64 = 10.0 * 96.0 / 72.0;
/// Axis and legend text: black, as Calc draws a chart that names no text
/// colour.
const CHART_TEXT: (f64, f64, f64) = (0.0, 0.0, 0.0);
/// Gridlines.
const GRID: (f64, f64, f64) = (0.7, 0.7, 0.7);

/// Office's default theme accents 1 to 6, which Excel and LibreOffice give
/// series (and a pie's slices) that name no colour.
const ACCENTS: [(u8, u8, u8); 6] = [
    (0x4f, 0x81, 0xbd),
    (0xc0, 0x50, 0x4d),
    (0x9b, 0xbb, 0x59),
    (0x80, 0x64, 0xa2),
    (0x4b, 0xac, 0xc6),
    (0xf7, 0x96, 0x46),
];

fn accent(i: usize) -> (f64, f64, f64) {
    let (r, g, b) = ACCENTS[i % ACCENTS.len()];
    (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0)
}

fn set(cr: &Context, (r, g, b): (f64, f64, f64)) {
    cr.set_source_rgb(r, g, b);
}

fn text_w(cr: &Context, t: &str) -> f64 {
    cr.text_extents(t).map(|e| e.x_advance()).unwrap_or(0.0)
}

fn show(cr: &Context, x: f64, y: f64, t: &str) {
    set(cr, CHART_TEXT);
    cr.move_to(x, y);
    cr.show_text(t).unwrap();
}

/// How a series' legend key looks.
#[derive(Clone, Copy)]
enum Key {
    Square,
    Line,
    Marker,
}

/// The named series' legend entries: name and colour.
fn legend_entries<'a>(series: &[Series<'a>]) -> Vec<(&'a str, (f64, f64, f64))> {
    series.iter().enumerate().filter_map(|(i, s)| Some((s.name?, accent(i)))).collect()
}

/// The legend's width for the widest of `names`, at the right of the
/// plot, measured against LibreOffice's (render lab `tables/chart-line`,
/// `-scatter`); none without names.
fn legend_width(cr: &Context, names: &[(&str, (f64, f64, f64))], key: Key) -> f64 {
    let sample = match key {
        Key::Line => 34.0,
        Key::Square => 12.0,
        Key::Marker => 14.0,
    };
    names.iter().map(|(n, _)| text_w(cr, n) + sample + 13.0).fold(0.0, f64::max)
}

/// A scatter point's marker in `color`: a filled circle with a darker rim,
/// the size Calc draws a 7 pt marker.
fn marker(cr: &Context, x: f64, y: f64, color: (f64, f64, f64)) {
    // A path of its own: after text, the current point would otherwise
    // join the circle with a line from wherever the text ended.
    cr.new_path();
    cr.arc(x, y, 5.0, 0.0, std::f64::consts::TAU);
    set(cr, color);
    cr.fill_preserve().unwrap();
    set(cr, (color.0 * 0.75, color.1 * 0.75, color.2 * 0.75));
    cr.set_line_width(1.0);
    cr.stroke().unwrap();
}

/// The distance between legend rows (and a pie legend's).
const LEGEND_ROW: f64 = 19.0;

/// A legend of `entries` at `x`, its rows centred on `mid` (one entry
/// sits at `mid`).
fn draw_legend(cr: &Context, x: f64, mid: f64, entries: &[(&str, (f64, f64, f64))], key: Key) {
    // One entry exactly at `mid`, as the single-series legend always sat.
    let y0 = if entries.len() == 1 { mid } else { mid - LEGEND_ROW * entries.len() as f64 / 2.0 + LEGEND_ROW * 0.5 };
    for (k, (name, color)) in entries.iter().enumerate() {
        let mid = y0 + k as f64 * LEGEND_ROW;
        set(cr, *color);
        let text_x = match key {
            Key::Square => {
                cr.rectangle(x, mid - 7.0, 8.0, 8.0);
                cr.fill().unwrap();
                x + 12.0
            }
            Key::Marker => {
                marker(cr, x + 5.0, mid - 3.0, *color);
                x + 14.0
            }
            Key::Line => {
                cr.set_line_width(2.5);
                cr.move_to(x, mid - 3.0);
                cr.line_to(x + 30.0, mid - 3.0);
                cr.stroke().unwrap();
                x + 34.0
            }
        };
        show(cr, text_x, mid + 1.0, name);
    }
}

/// A value axis from 0: where its ticks are and how they're labelled.
struct ValueAxis {
    top: f64,
    step: f64,
}

impl ValueAxis {
    fn new(max: f64) -> Self {
        let (top, step) = nice_axis(max);
        ValueAxis { top, step }
    }
    fn ticks(&self) -> usize {
        (self.top / self.step).round() as usize
    }
    fn label(&self, i: usize) -> String {
        format!("{:.*}", if self.step < 1.0 { 1 } else { 0 }, i as f64 * self.step)
    }
    fn widest(&self, cr: &Context) -> f64 {
        (0..=self.ticks()).map(|i| text_w(cr, &self.label(i))).fold(0.0, f64::max)
    }
}

/// Horizontal gridlines and the value labels at their left, for a value
/// axis drawn from `bottom` (0) up to `top_y` (`axis.top`).
fn draw_value_axis(cr: &Context, axis: &ValueAxis, left: f64, right: f64, top_y: f64, bottom: f64) {
    cr.set_line_width(1.0);
    for i in 0..=axis.ticks() {
        let y = (bottom - i as f64 / axis.ticks() as f64 * (bottom - top_y)).round() + 0.5;
        set(cr, GRID);
        cr.move_to(left, y);
        cr.line_to(right, y);
        cr.stroke().unwrap();
        let t = axis.label(i);
        show(cr, left - 6.0 - text_w(cr, &t), y + CHART_TEXT_PX * 0.35, &t);
    }
}

/// The largest value of any series.
fn max_of(series: &[Series]) -> f64 {
    series.iter().map(Series::max).fold(0.0, f64::max)
}

/// A clustered column chart: value axis from 0 with round ticks and
/// horizontal gridlines, categories under the bars, and the legend at the
/// right when a series has a name.
fn draw_bars(cr: &Context, categories: &[String], series: &[Series], w: f64, h: f64) {
    let axis = ValueAxis::new(max_of(series));
    let legend = legend_entries(series);
    let legend_w = legend.iter().map(|(n, _)| text_w(cr, n) + 30.0).fold(0.0, f64::max);
    let (left, right, top_y, bottom) = (axis.widest(cr) + 14.0, w - 10.0 - legend_w, 12.0, h - CHART_TEXT_PX - 14.0);
    if right <= left || bottom <= top_y || categories.is_empty() {
        return;
    }
    let y_of = |v: f64| bottom - (v / axis.top) * (bottom - top_y);
    draw_value_axis(cr, &axis, left, right, top_y, bottom);

    // Each category's slot holds the cluster and a gap of 150% of one bar,
    // as Excel's default: one series' bar is 40% of the slot.
    let slot = (right - left) / categories.len() as f64;
    let n = series.len() as f64;
    let bar_w = slot / (n + 1.5);
    for (i, cat) in categories.iter().enumerate() {
        let start = left + i as f64 * slot + (slot - n * bar_w) / 2.0;
        for (k, s) in series.iter().enumerate() {
            let Some(val) = s.at(i) else { continue };
            set(cr, accent(k));
            cr.rectangle(start + k as f64 * bar_w, y_of(val), bar_w, bottom - y_of(val));
            cr.fill().unwrap();
        }
        show(cr, left + i as f64 * slot + (slot - text_w(cr, cat)) / 2.0, bottom + 4.0 + CHART_TEXT_PX, cat);
    }

    draw_legend(cr, right + 12.0, (top_y + bottom) / 2.0, &legend, Key::Square);
}

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Line,
    Area,
}

/// A line or area chart over categories. A line's points sit in the middle
/// of each category's slot; an area's categories sit on the axis ticks, the
/// first at the left edge and the last at the right, as in Excel and Calc.
/// Series are drawn in order, a later area in front of an earlier one.
fn draw_category(cr: &Context, categories: &[String], series: &[Series], w: f64, h: f64, style: Style) {
    let axis = ValueAxis::new(max_of(series));
    let key = if style == Style::Line { Key::Line } else { Key::Square };
    let legend = legend_entries(series);
    let legend_w = legend_width(cr, &legend, key);
    let (left, right, top_y, bottom) = (axis.widest(cr) + 14.0, w - 10.0 - legend_w, 12.0, h - CHART_TEXT_PX - 14.0);
    if right <= left || bottom <= top_y || categories.is_empty() {
        return;
    }
    let y_of = |v: f64| bottom - (v.max(0.0) / axis.top) * (bottom - top_y);
    let n = categories.len();
    let x_of = |i: usize| match style {
        Style::Line => left + (i as f64 + 0.5) * (right - left) / n as f64,
        Style::Area => left + i as f64 * (right - left) / (n - 1).max(1) as f64,
    };
    draw_value_axis(cr, &axis, left, right, top_y, bottom);

    for (k, s) in series.iter().enumerate() {
        set(cr, accent(k));
        match style {
            Style::Area => {
                // A missing value is 0, as Calc fills a gap in an area.
                cr.move_to(x_of(0), bottom);
                for i in 0..n {
                    cr.line_to(x_of(i), y_of(s.at(i).unwrap_or(0.0)));
                }
                cr.line_to(x_of(n - 1), bottom);
                cr.close_path();
                cr.fill().unwrap();
            }
            Style::Line => {
                cr.set_line_width(2.5);
                cr.set_line_join(gtk4::cairo::LineJoin::Round);
                // A missing value breaks the line.
                let mut drawing = false;
                for i in 0..n {
                    match s.at(i) {
                        Some(v) if drawing => cr.line_to(x_of(i), y_of(v)),
                        Some(v) => {
                            cr.move_to(x_of(i), y_of(v));
                            drawing = true;
                        }
                        None => drawing = false,
                    }
                }
                cr.stroke().unwrap();
            }
        }
    }
    for (i, cat) in categories.iter().enumerate() {
        show(cr, x_of(i) - text_w(cr, cat) / 2.0, bottom + 4.0 + CHART_TEXT_PX, cat);
    }
    draw_legend(cr, right + 12.0, h / 2.0 + 3.0, &legend, key);
}

/// A pie: slices clockwise from twelve o'clock in the theme's accents, and
/// a legend naming each category at the right, as in Excel and Calc.
fn draw_pie(cr: &Context, categories: &[String], series: &Series, w: f64, h: f64) {
    let data: Vec<(&String, f64)> = categories.iter().enumerate().map(|(i, c)| (c, series.at(i).unwrap_or(0.0))).collect();
    let total: f64 = data.iter().map(|d| d.1.max(0.0)).sum();
    if total <= 0.0 {
        return;
    }
    let legend_w = data.iter().map(|(c, _)| text_w(cr, c)).fold(0.0, f64::max) + 30.0;
    let plot_w = (w - legend_w - 10.0).max(1.0);
    let r = ((plot_w.min(h) - 27.0) / 2.0).max(1.0);
    let (cx, cy) = (plot_w / 2.0 + 5.0, h / 2.0);
    let mut angle = -std::f64::consts::FRAC_PI_2;
    for (i, (_, val)) in data.iter().enumerate() {
        let sweep = val.max(0.0) / total * std::f64::consts::TAU;
        set(cr, accent(i));
        cr.move_to(cx, cy);
        cr.arc(cx, cy, r, angle, angle + sweep);
        cr.close_path();
        cr.fill().unwrap();
        angle += sweep;
    }
    // The legend: one row per category, centred on the pie.
    let row = LEGEND_ROW;
    let lx = w - legend_w + 8.0;
    let y0 = cy - row * data.len() as f64 / 2.0 + row * 0.5 + 2.0;
    for (i, (cat, _)) in data.iter().enumerate() {
        let y = y0 + i as f64 * row;
        set(cr, accent(i));
        cr.rectangle(lx, y - 7.0, 8.0, 8.0);
        cr.fill().unwrap();
        show(cr, lx + 12.0, y + 1.0, cat);
    }
}

/// An XY scatter: numeric axes from 0 with gridlines both ways, and each
/// point a marker, as Calc and Excel draw one without lines. The
/// categories are the x values every series shares.
fn draw_scatter(cr: &Context, categories: &[String], series: &[Series], w: f64, h: f64) {
    let xs: Vec<Option<f64>> = categories.iter().map(|c| c.trim().parse::<f64>().ok().filter(|x| x.is_finite())).collect();
    let points: Vec<Vec<(f64, f64)>> = series
        .iter()
        .map(|s| xs.iter().enumerate().filter_map(|(i, x)| Some((((*x)?), s.at(i)?))).collect())
        .collect();
    let y_axis = ValueAxis::new(points.iter().flatten().map(|p| p.1).fold(0.0, f64::max));
    let x_axis = ValueAxis::new(points.iter().flatten().map(|p| p.0).fold(0.0, f64::max));
    let legend = legend_entries(series);
    let legend_w = legend_width(cr, &legend, Key::Marker);
    let (left, right, top_y, bottom) = (y_axis.widest(cr) + 14.0, w - 10.0 - legend_w, 12.0, h - CHART_TEXT_PX - 14.0);
    if right <= left || bottom <= top_y {
        return;
    }
    draw_value_axis(cr, &y_axis, left, right, top_y, bottom);
    let x_of = |v: f64| left + (v / x_axis.top) * (right - left);
    let y_of = |v: f64| bottom - (v / y_axis.top) * (bottom - top_y);
    cr.set_line_width(1.0);
    for i in 0..=x_axis.ticks() {
        let x = (left + i as f64 / x_axis.ticks() as f64 * (right - left)).round() + 0.5;
        set(cr, GRID);
        cr.move_to(x, top_y);
        cr.line_to(x, bottom);
        cr.stroke().unwrap();
        let t = x_axis.label(i);
        show(cr, x - text_w(cr, &t) / 2.0, bottom + 4.0 + CHART_TEXT_PX, &t);
    }
    for (k, pts) in points.iter().enumerate() {
        for (x, y) in pts {
            marker(cr, x_of(*x), y_of(*y), accent(k));
        }
    }
    draw_legend(cr, right + 12.0, h / 2.0 + 3.0, &legend, Key::Marker);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> Vec<(String, f64)> {
        vec![("1".into(), 3.0), ("2.5".into(), 7.0), ("4".into(), 5.0), ("6".into(), 9.0)]
    }

    /// Every kind draws something in the series colour inside the frame,
    /// and survives no data at all.
    #[test]
    fn every_kind_draws_and_none_panics_on_nothing() {
        for kind in [ChartKind::Bar, ChartKind::Line, ChartKind::Area, ChartKind::Pie, ChartKind::Scatter] {
            let mut s = render_chart_named(&data(), kind, 340, 227, Some("Column B"));
            let (w, stride) = (s.width() as usize, s.stride() as usize);
            let px = s.data().unwrap();
            let blue = (0..px.len() / 4).any(|i| {
                let (b, g, r) = (px[i * 4], px[i * 4 + 1], px[i * 4 + 2]);
                (r, g, b) == (0x4f, 0x81, 0xbd) && (i * 4) % stride / 4 < w
            });
            assert!(blue, "{kind:?} drew no series colour");
            render_chart(&[], kind, 100, 80);
        }
    }

    /// Pixels of exactly `rgb` in a drawn surface.
    fn count(s: &mut ImageSurface, (r, g, b): (u8, u8, u8)) -> usize {
        let (w, h, stride) = (s.width() as usize, s.height() as usize, s.stride() as usize);
        let px = s.data().unwrap();
        (0..h).map(|y| (0..w).filter(|x| px[y * stride + x * 4..][..3] == [b, g, r]).count()).sum()
    }

    fn draw(categories: &[String], series: &[Series], kind: ChartKind) -> ImageSurface {
        let s = ImageSurface::create(Format::ARgb32, 400, 260).unwrap();
        {
            let cr = Context::new(&s).unwrap();
            crate::use_ui_font_rendering(&cr);
            draw_chart_series(&cr, categories, series, kind, 400.0, 260.0);
        }
        s
    }

    /// One series drawn as a series is the single-series chart, pixel for
    /// pixel: Tables' charts, and the render lab's baselines, don't move.
    #[test]
    fn one_series_is_the_single_series_chart() {
        let d = data();
        let categories: Vec<String> = d.iter().map(|p| p.0.clone()).collect();
        let values: Vec<f64> = d.iter().map(|p| p.1).collect();
        for kind in [ChartKind::Bar, ChartKind::Line, ChartKind::Area, ChartKind::Pie, ChartKind::Scatter] {
            let mut a = render_chart_named(&d, kind, 400, 260, Some("Column B"));
            let mut b = draw(&categories, &[Series { name: Some("Column B"), values: &values }], kind);
            assert!(a.data().unwrap().iter().eq(b.data().unwrap().iter()), "{kind:?}");
        }
    }

    /// Two series: each in its own accent (the second in accent 2), the
    /// columns of a category side by side and each narrower than a lone
    /// series' column, and a pie of the first series alone.
    #[test]
    fn a_second_series_is_drawn_in_the_second_accent() {
        let categories: Vec<String> = ["1", "2", "3"].iter().map(|s| s.to_string()).collect();
        let (a, b) = ([3.0, 5.0, 4.0], [4.0, 2.0, 6.0]);
        let two = [Series { name: Some("North"), values: &a }, Series { name: Some("South"), values: &b }];
        for kind in [ChartKind::Bar, ChartKind::Line, ChartKind::Area, ChartKind::Scatter] {
            let mut s = draw(&categories, &two, kind);
            assert!(count(&mut s, (0x4f, 0x81, 0xbd)) > 0, "{kind:?}: the first series");
            assert!(count(&mut s, (0xc0, 0x50, 0x4d)) > 0, "{kind:?}: the second series");
        }
        // Clustered columns: two series' bars together are 2/3.5 of a slot
        // where one series' is 1/2.5, so each is narrower than alone.
        let mut one = draw(&categories, &two[..1], ChartKind::Bar);
        let mut both = draw(&categories, &two, ChartKind::Bar);
        let alone = count(&mut one, (0x4f, 0x81, 0xbd));
        let clustered = count(&mut both, (0x4f, 0x81, 0xbd));
        assert!(clustered < alone, "{clustered} < {alone}");
        // A pie draws its first series only.
        let mut pie = draw(&categories, &two, ChartKind::Pie);
        let mut first = draw(&categories, &two[..1], ChartKind::Pie);
        assert!(pie.data().unwrap().iter().eq(first.data().unwrap().iter()));
    }

    /// A missing value is a gap, not a zero line or a panic.
    #[test]
    fn a_short_series_leaves_gaps() {
        let categories: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        let short = [2.0, f64::NAN];
        for kind in [ChartKind::Bar, ChartKind::Line, ChartKind::Area, ChartKind::Pie, ChartKind::Scatter] {
            draw(&categories, &[Series { name: None, values: &short }], kind);
            draw(&categories, &[], kind);
        }
    }
}
