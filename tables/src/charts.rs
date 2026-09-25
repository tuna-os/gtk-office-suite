// SPDX-License-Identifier: GPL-3.0-or-later
//! Chart rendering with Cairo: column, line, area, pie and XY scatter, drawn
//! as Excel and LibreOffice Calc draw a chart that names no style. The
//! render lab compares each kind against LibreOffice (`tables/chart`,
//! `tables/chart-line`, `-area`, `-pie`, `-scatter`).

use gtk4::cairo::{Context, Format, ImageSurface};
use tables_core::sheet::ChartKind;

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
    suite_common::use_ui_font_rendering(&cr);
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.paint().unwrap();
    // Chart text is Calibri 10 pt (Carlito where Calibri isn't
    // installed), as Excel and Calc draw a chart that names no font.
    cr.select_font_face("Calibri", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    cr.set_font_size(CHART_TEXT_PX);
    let (w, h) = (width as f64, height as f64);
    match kind {
        ChartKind::Bar => draw_bars(&cr, data, width, height, series_name),
        ChartKind::Line => draw_category(&cr, data, w, h, series_name, Style::Line),
        ChartKind::Area => draw_category(&cr, data, w, h, series_name, Style::Area),
        ChartKind::Pie => draw_pie(&cr, data, w, h),
        ChartKind::Scatter => draw_scatter(&cr, data, w, h, series_name),
    }
    surface.flush();
    surface
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

/// The first series colour (accent 1).
const SERIES_1: (f64, f64, f64) = (0x4f as f64 / 255.0, 0x81 as f64 / 255.0, 0xbd as f64 / 255.0);

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

/// The legend's width for `name`, at the right of the plot.
fn legend_width(cr: &Context, name: Option<&str>, key: Key) -> f64 {
    let sample = match key {
        Key::Line => 34.0,
        Key::Square | Key::Marker => 12.0,
    };
    name.map_or(0.0, |n| text_w(cr, n) + sample + 18.0)
}

/// A one-entry legend, vertically centred at `(x, mid)`.
fn draw_legend(cr: &Context, x: f64, mid: f64, name: &str, key: Key) {
    set(cr, SERIES_1);
    let text_x = match key {
        Key::Square => {
            cr.rectangle(x, mid - 7.0, 8.0, 8.0);
            cr.fill().unwrap();
            x + 12.0
        }
        Key::Marker => {
            cr.arc(x + 4.0, mid - 3.0, 3.5, 0.0, std::f64::consts::TAU);
            cr.fill().unwrap();
            x + 12.0
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

/// A value axis from 0: where its ticks are and how they're labelled.
struct ValueAxis {
    top: f64,
    step: f64,
}

impl ValueAxis {
    fn new(max: f64) -> Self {
        let (top, step) = tables_core::sheet::nice_axis(max);
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

/// A clustered column chart: value axis from 0 with round ticks and
/// horizontal gridlines, categories under the bars, and the legend at the
/// right when the series has a name.
fn draw_bars(cr: &Context, data: &[(String, f64)], w: i32, h: i32, series_name: Option<&str>) {
    let (w, h) = (w as f64, h as f64);
    let axis = ValueAxis::new(data.iter().map(|d| d.1).fold(0.0, f64::max));
    let legend_w = series_name.map_or(0.0, |n| text_w(cr, n) + 30.0);
    let (left, right, top_y, bottom) = (axis.widest(cr) + 14.0, w - 10.0 - legend_w, 12.0, h - CHART_TEXT_PX - 14.0);
    if right <= left || bottom <= top_y || data.is_empty() {
        return;
    }
    let y_of = |v: f64| bottom - (v / axis.top) * (bottom - top_y);
    draw_value_axis(cr, &axis, left, right, top_y, bottom);

    // Bars: each category's slot is split 150% gap to 100% bar, as Excel's default.
    let slot = (right - left) / data.len() as f64;
    let bar_w = slot / 2.5;
    for (i, (cat, val)) in data.iter().enumerate() {
        let x = left + i as f64 * slot + (slot - bar_w) / 2.0;
        set(cr, SERIES_1);
        cr.rectangle(x, y_of(*val), bar_w, bottom - y_of(*val));
        cr.fill().unwrap();
        show(cr, left + i as f64 * slot + (slot - text_w(cr, cat)) / 2.0, bottom + 4.0 + CHART_TEXT_PX, cat);
    }

    if let Some(name) = series_name {
        let lx = right + 12.0;
        let ly = (top_y + bottom) / 2.0;
        set(cr, SERIES_1);
        cr.rectangle(lx, ly - 7.0, 8.0, 8.0);
        cr.fill().unwrap();
        show(cr, lx + 12.0, ly + 1.0, name);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Line,
    Area,
}

/// A line or area chart over categories. A line's points sit in the middle
/// of each category's slot; an area's categories sit on the axis ticks, the
/// first at the left edge and the last at the right, as in Excel and Calc.
fn draw_category(cr: &Context, data: &[(String, f64)], w: f64, h: f64, series_name: Option<&str>, style: Style) {
    let axis = ValueAxis::new(data.iter().map(|d| d.1).fold(0.0, f64::max));
    let key = if style == Style::Line { Key::Line } else { Key::Square };
    let legend_w = legend_width(cr, series_name, key);
    let (left, right, top_y, bottom) = (axis.widest(cr) + 14.0, w - 10.0 - legend_w, 12.0, h - CHART_TEXT_PX - 14.0);
    if right <= left || bottom <= top_y || data.is_empty() {
        return;
    }
    let y_of = |v: f64| bottom - (v.max(0.0) / axis.top) * (bottom - top_y);
    let n = data.len();
    let x_of = |i: usize| match style {
        Style::Line => left + (i as f64 + 0.5) * (right - left) / n as f64,
        Style::Area => left + i as f64 * (right - left) / (n - 1).max(1) as f64,
    };
    draw_value_axis(cr, &axis, left, right, top_y, bottom);

    set(cr, SERIES_1);
    match style {
        Style::Area => {
            cr.move_to(x_of(0), bottom);
            for (i, (_, v)) in data.iter().enumerate() {
                cr.line_to(x_of(i), y_of(*v));
            }
            cr.line_to(x_of(n - 1), bottom);
            cr.close_path();
            cr.fill().unwrap();
        }
        Style::Line => {
            cr.set_line_width(2.5);
            cr.set_line_join(gtk4::cairo::LineJoin::Round);
            for (i, (_, v)) in data.iter().enumerate() {
                if i == 0 {
                    cr.move_to(x_of(i), y_of(*v));
                } else {
                    cr.line_to(x_of(i), y_of(*v));
                }
            }
            cr.stroke().unwrap();
        }
    }
    for (i, (cat, _)) in data.iter().enumerate() {
        show(cr, x_of(i) - text_w(cr, cat) / 2.0, bottom + 4.0 + CHART_TEXT_PX, cat);
    }
    if let Some(name) = series_name {
        draw_legend(cr, right + 12.0, (top_y + bottom) / 2.0, name, key);
    }
}

/// A pie: slices clockwise from twelve o'clock in the theme's accents, and
/// a legend naming each category at the right, as in Excel and Calc.
fn draw_pie(cr: &Context, data: &[(String, f64)], w: f64, h: f64) {
    let total: f64 = data.iter().map(|d| d.1.max(0.0)).sum();
    if total <= 0.0 {
        return;
    }
    let legend_w = data.iter().map(|(c, _)| text_w(cr, c)).fold(0.0, f64::max) + 30.0;
    let plot_w = (w - legend_w - 10.0).max(1.0);
    let r = ((plot_w.min(h) - 24.0) / 2.0).max(1.0);
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
    let row = CHART_TEXT_PX * 1.5;
    let lx = w - legend_w + 8.0;
    let y0 = cy - row * data.len() as f64 / 2.0 + row * 0.7;
    for (i, (cat, _)) in data.iter().enumerate() {
        let y = y0 + i as f64 * row;
        set(cr, accent(i));
        cr.rectangle(lx, y - 7.0, 8.0, 8.0);
        cr.fill().unwrap();
        show(cr, lx + 12.0, y + 1.0, cat);
    }
}

/// An XY scatter: numeric axes from 0 with gridlines both ways, and each
/// point a marker, as Calc and Excel draw one without lines.
fn draw_scatter(cr: &Context, data: &[(String, f64)], w: f64, h: f64, series_name: Option<&str>) {
    let points: Vec<(f64, f64)> =
        data.iter().filter_map(|(x, y)| Some((x.trim().parse::<f64>().ok()?, *y))).filter(|(x, y)| x.is_finite() && y.is_finite()).collect();
    let y_axis = ValueAxis::new(points.iter().map(|p| p.1).fold(0.0, f64::max));
    let x_axis = ValueAxis::new(points.iter().map(|p| p.0).fold(0.0, f64::max));
    let legend_w = legend_width(cr, series_name, Key::Marker);
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
    set(cr, SERIES_1);
    for (x, y) in points {
        cr.arc(x_of(x), y_of(y), 4.0, 0.0, std::f64::consts::TAU);
        cr.fill().unwrap();
    }
    if let Some(name) = series_name {
        draw_legend(cr, right + 12.0, (top_y + bottom) / 2.0, name, Key::Marker);
    }
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
}
