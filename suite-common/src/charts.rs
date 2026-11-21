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

/// As [`render_chart_named`], with any number of series over the same
/// categories (see [`draw_chart_series`]).
pub fn render_chart_series(categories: &[String], series: &[Series], kind: ChartKind, width: i32, height: i32) -> ImageSurface {
    let surface = ImageSurface::create(Format::ARgb32, width.max(1), height.max(1)).unwrap();
    let cr = Context::new(&surface).unwrap();
    crate::use_ui_font_rendering(&cr);
    draw_chart_series(&cr, categories, series, kind, width as f64, height as f64);
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
    draw_chart_with(cr, categories, series, kind, width, height, &Options::default());
}

/// How a chart's series are laid out beyond their kind, as a presentation's
/// chart part names it (#2327). The default is the plain chart.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Options<'a> {
    /// A column chart's series stacked in each category, one on another,
    /// instead of side by side.
    pub stacked: bool,
    /// Each column's value written on it: in the middle of a stacked
    /// segment, over a clustered column.
    pub labels: bool,
    /// No value axis: neither its labels nor its gridlines.
    pub hide_value_axis: bool,
    /// A doughnut's hole, as a fraction of its radius; 0 is a pie.
    pub hole: f64,
    /// A colour per point, by category: a pie's slices, as the file
    /// colours them (`c:dPt`); `None` keeps the accent.
    pub point_colors: &'a [Option<(f64, f64, f64)>],
    /// No legend, where the file has none.
    pub no_legend: bool,
    /// No white background: the slide shows through.
    pub transparent: bool,
    /// A bar chart's bars lie horizontally, categories down the left.
    pub horizontal: bool,
    /// The categories run the other way: a horizontal chart's first at the
    /// top, a column chart's first at the right.
    pub reversed: bool,
    /// Data labels as percentages: 0.72 reads "72%".
    pub percent: bool,
    /// The chart's text colour (`c:txPr`), where the file sets one; black
    /// otherwise.
    pub text_color: Option<(f64, f64, f64)>,
    /// The chart's text size in the drawing's units, where the file sets
    /// one; `CHART_TEXT_PX` otherwise.
    pub text_px: Option<f64>,
    /// A colour per series, the first included, as the file colours them;
    /// `None` keeps the accent.
    pub series_colors: &'a [Option<(f64, f64, f64)>],
    /// A bar chart's gap between categories in percent of one bar; `None`
    /// is Excel's default 150.
    pub gap: Option<f64>,
    /// No category labels: the file deletes the category axis.
    pub hide_category_axis: bool,
}

/// As [`draw_chart_series`], laid out as `opts` asks. Stacking and labels
/// apply to column charts; a hidden value axis to columns, lines and areas.
pub fn draw_chart_with(cr: &Context, categories: &[String], series: &[Series], kind: ChartKind, width: f64, height: f64, opts: &Options) {
    let _ = cr.save();
    if !opts.transparent {
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.rectangle(0.0, 0.0, width, height);
        let _ = cr.fill();
    }
    // Chart text is Calibri 10 pt (Carlito where Calibri isn't
    // installed), as Excel and Calc draw a chart that names no font.
    cr.select_font_face("Calibri", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    cr.set_font_size(opts.text_px.unwrap_or(CHART_TEXT_PX));
    TEXT.with(|t| t.set(opts.text_color.unwrap_or(CHART_TEXT)));
    let (w, h) = (width, height);
    if !series.is_empty() {
        match kind {
            ChartKind::Bar => draw_bars(cr, categories, series, w, h, opts),
            ChartKind::Line => draw_category(cr, categories, series, w, h, Style::Line, opts),
            ChartKind::Area => draw_category(cr, categories, series, w, h, Style::Area, opts),
            ChartKind::Pie => draw_pie(cr, categories, &series[0], w, h, opts),
            ChartKind::Scatter => draw_scatter(cr, categories, series, w, h),
        }
    }
    TEXT.with(|t| t.set(CHART_TEXT));
    let _ = cr.restore();
}

/// Where a chart's legend sits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LegendPos {
    /// No legend.
    None,
    Top,
    Bottom,
    Left,
    /// Beside the plot at its right, where every chart's legend sat.
    #[default]
    Right,
}

/// What surrounds a chart's plot: a title over it, a title for each axis
/// and a legend where asked. A pie's legend names its categories and stays
/// at its right whatever is asked, and it has no axes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Decor<'a> {
    pub title: &'a str,
    pub x_title: Option<&'a str>,
    pub y_title: Option<&'a str>,
    pub legend: LegendPos,
}

/// As [`render_chart_series`], with the title, axis titles and legend
/// position of `decor` (see [`draw_chart_decorated`]).
pub fn render_chart_decorated(categories: &[String], series: &[Series], kind: ChartKind, width: i32, height: i32, decor: &Decor) -> ImageSurface {
    let surface = ImageSurface::create(Format::ARgb32, width.max(1), height.max(1)).unwrap();
    let cr = Context::new(&surface).unwrap();
    crate::use_ui_font_rendering(&cr);
    draw_chart_decorated(&cr, categories, series, kind, width as f64, height as f64, decor);
    drop(cr);
    surface.flush();
    surface
}

/// The height a title takes over the plot, an axis title beside it, and a
/// legend row above or below it.
const TITLE_H: f64 = 30.0;
const AXIS_TITLE: f64 = 22.0;
const LEGEND_STRIP: f64 = 26.0;
/// The gap between legend entries in a row.
const LEGEND_GAP: f64 = 18.0;

/// As [`draw_chart_series`], inside the room `decor` leaves: the title
/// centred over the plot, the x title under it and the y title turned up
/// its left side, and the legend where `decor.legend` puts it, in a row for
/// the top and bottom and a column for the left. With nothing asked for
/// (no titles, a legend at the right) it is [`draw_chart_series`] exactly.
pub fn draw_chart_decorated(cr: &Context, categories: &[String], series: &[Series], kind: ChartKind, width: f64, height: f64, decor: &Decor) {
    let title = decor.title.trim();
    let x_title = decor.x_title.map(str::trim).filter(|t| !t.is_empty());
    let y_title = decor.y_title.map(str::trim).filter(|t| !t.is_empty());
    // A pie has no axes and keeps its legend where it is.
    let pie = kind == ChartKind::Pie;
    let (x_title, y_title) = if pie { (None, None) } else { (x_title, y_title) };
    let legend = if pie { LegendPos::Right } else { decor.legend };
    if title.is_empty() && x_title.is_none() && y_title.is_none() && legend == LegendPos::Right {
        return draw_chart_series(cr, categories, series, kind, width, height);
    }

    let _ = cr.save();
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.rectangle(0.0, 0.0, width, height);
    let _ = cr.fill();
    cr.select_font_face("Calibri", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    cr.set_font_size(CHART_TEXT_PX);

    // A legend outside the plot takes its names from the plot's own.
    let key = match kind {
        ChartKind::Line => Key::Line,
        ChartKind::Scatter => Key::Marker,
        _ => Key::Square,
    };
    let entries = legend_entries(series);
    let own_legend = matches!(legend, LegendPos::Top | LegendPos::Bottom | LegendPos::Left);
    let plot_series: Vec<Series> = if legend == LegendPos::Right { series.to_vec() } else { series.iter().map(|s| Series { name: None, ..*s }).collect() };

    let mut margin = (if title.is_empty() { 0.0 } else { TITLE_H }, 0.0_f64, if y_title.is_some() { AXIS_TITLE } else { 0.0 }); // top, bottom, left
    if x_title.is_some() {
        margin.1 += AXIS_TITLE;
    }
    if own_legend && !entries.is_empty() {
        match legend {
            LegendPos::Top => margin.0 += LEGEND_STRIP,
            LegendPos::Bottom => margin.1 += LEGEND_STRIP,
            _ => margin.2 += legend_width(cr, &entries, key) + 14.0,
        }
    }
    let (plot_w, plot_h) = ((width - margin.2).max(1.0), (height - margin.0 - margin.1).max(1.0));

    // The plot, in its own rectangle.
    let _ = cr.save();
    cr.translate(margin.2, margin.0);
    cr.rectangle(0.0, 0.0, plot_w, plot_h);
    cr.clip();
    if !series.is_empty() {
        match kind {
            ChartKind::Bar => draw_bars(cr, categories, &plot_series, plot_w, plot_h, &Options::default()),
            ChartKind::Line => draw_category(cr, categories, &plot_series, plot_w, plot_h, Style::Line, &Options::default()),
            ChartKind::Area => draw_category(cr, categories, &plot_series, plot_w, plot_h, Style::Area, &Options::default()),
            ChartKind::Pie => draw_pie(cr, categories, &plot_series[0], plot_w, plot_h, &Options::default()),
            ChartKind::Scatter => draw_scatter(cr, categories, &plot_series, plot_w, plot_h),
        }
    }
    let _ = cr.restore();

    // The legend outside the plot.
    if own_legend && !entries.is_empty() {
        match legend {
            LegendPos::Left => draw_legend(cr, 8.0 + if y_title.is_some() { AXIS_TITLE } else { 0.0 }, margin.0 + plot_h / 2.0, &entries, key),
            _ => {
                let offset = |k: Key| match k {
                    Key::Square => 12.0,
                    Key::Marker => 14.0,
                    Key::Line => 34.0,
                };
                let row_w: f64 = entries.iter().map(|(n, _)| offset(key) + text_w(cr, n)).sum::<f64>() + LEGEND_GAP * (entries.len() - 1) as f64;
                let mid = if legend == LegendPos::Top { margin.0 - LEGEND_STRIP / 2.0 } else { height - if x_title.is_some() { AXIS_TITLE } else { 0.0 } - LEGEND_STRIP / 2.0 };
                let mut x = margin.2 + (plot_w - row_w) / 2.0;
                for entry in &entries {
                    draw_legend(cr, x, mid, std::slice::from_ref(entry), key);
                    x += offset(key) + text_w(cr, entry.0) + LEGEND_GAP;
                }
            }
        }
    }

    // Titles last, in the room reserved for them.
    if !title.is_empty() {
        cr.select_font_face("Calibri", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Bold);
        cr.set_font_size(CHART_TEXT_PX * 1.3);
        show(cr, (width - text_w(cr, title)) / 2.0, TITLE_H * 0.68, title);
        cr.select_font_face("Calibri", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
        cr.set_font_size(CHART_TEXT_PX);
    }
    if let Some(t) = x_title {
        show(cr, margin.2 + (plot_w - text_w(cr, t)) / 2.0, height - 6.0, t);
    }
    if let Some(t) = y_title {
        let _ = cr.save();
        cr.translate(AXIS_TITLE * 0.7, margin.0 + (plot_h + text_w(cr, t)) / 2.0);
        cr.rotate(-std::f64::consts::FRAC_PI_2);
        show(cr, 0.0, 0.0, t);
        let _ = cr.restore();
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

thread_local! {
    /// The text colour of the chart being drawn: the file's, or
    /// `CHART_TEXT`. Set by `draw_chart_with`, read by `show`.
    static TEXT: std::cell::Cell<(f64, f64, f64)> = const { std::cell::Cell::new(CHART_TEXT) };
}

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
    show_in(cr, TEXT.with(|c| c.get()), x, y, t);
}

/// `t` at (x, y) in `color`.
fn show_in(cr: &Context, color: (f64, f64, f64), x: f64, y: f64, t: &str) {
    set(cr, color);
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
    legend_in(series, &Options::default())
}

/// The legend rows in each series' colour under `opts`.
fn legend_in<'a>(series: &[Series<'a>], opts: &Options) -> Vec<(&'a str, (f64, f64, f64))> {
    series.iter().enumerate().filter_map(|(i, s)| Some((s.name?, series_color(opts, i)))).collect()
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

/// A data label's text: the value as a cache writes it, a whole number
/// without a fraction.
fn value_label(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v}") }
}

/// A data label as the chart's number format reads it: a percentage
/// rounded to a whole percent, else the value.
fn label_for(v: f64, opts: &Options) -> String {
    if opts.percent { format!("{}%", (v * 100.0).round() as i64) } else { value_label(v) }
}

/// The colour of series `k`'s bar in category `i`: a one-series chart's
/// bars take their points' own colours, as the file sets them.
fn bar_color(series: &[Series], opts: &Options, k: usize, i: usize) -> (f64, f64, f64) {
    if series.len() == 1 {
        opts.point_colors.get(i).copied().flatten().unwrap_or_else(|| series_color(opts, 0))
    } else {
        series_color(opts, k)
    }
}

/// Series `k`'s colour: the file's, else the accent.
fn series_color(opts: &Options, k: usize) -> (f64, f64, f64) {
    opts.series_colors.get(k).copied().flatten().unwrap_or_else(|| accent(k))
}

/// The largest value of any series.
fn max_of(series: &[Series]) -> f64 {
    series.iter().map(Series::max).fold(0.0, f64::max)
}

/// A clustered column chart: value axis from 0 with round ticks and
/// horizontal gridlines, categories under the bars, and the legend at the
/// right when a series has a name.
fn draw_bars(cr: &Context, categories: &[String], series: &[Series], w: f64, h: f64, opts: &Options) {
    if opts.horizontal {
        return draw_horizontal_bars(cr, categories, series, w, h, opts);
    }
    // Stacked, the axis reaches the tallest category's sum of positives.
    let max = if opts.stacked {
        (0..categories.len())
            .map(|i| series.iter().filter_map(|s| s.at(i)).filter(|v| *v > 0.0).sum::<f64>())
            .fold(0.0, f64::max)
    } else {
        max_of(series)
    };
    let axis = ValueAxis::new(max);
    let legend = if opts.no_legend { Vec::new() } else { legend_in(series, opts) };
    let legend_w = legend.iter().map(|(n, _)| text_w(cr, n) + 30.0).fold(0.0, f64::max);
    let axis_w = if opts.hide_value_axis { 0.0 } else { axis.widest(cr) + 4.0 };
    let (left, right, top_y, bottom) = (axis_w + 10.0, w - 10.0 - legend_w, 12.0, h - CHART_TEXT_PX - 14.0);
    if right <= left || bottom <= top_y || categories.is_empty() {
        return;
    }
    let y_of = |v: f64| bottom - (v / axis.top) * (bottom - top_y);
    if !opts.hide_value_axis {
        draw_value_axis(cr, &axis, left, right, top_y, bottom);
    }

    // Each category's slot holds the cluster and a gap of 150% of one bar,
    // as Excel's default: one series' bar is 40% of the slot. Stacked, the
    // slot holds one column the width of one series' bar.
    let slot = (right - left) / categories.len() as f64;
    let n = if opts.stacked { 1.0 } else { series.len() as f64 };
    let bar_w = slot / (n + opts.gap.unwrap_or(150.0) / 100.0);
    for (i, cat) in categories.iter().enumerate() {
        let at = if opts.reversed { categories.len() - 1 - i } else { i };
        let start = left + at as f64 * slot + (slot - n * bar_w) / 2.0;
        let mut base = 0.0;
        for (k, s) in series.iter().enumerate() {
            let Some(val) = s.at(i) else { continue };
            let (x, from, to) = if opts.stacked {
                let from = base;
                base += val.max(0.0);
                (start, from, base)
            } else {
                (start + k as f64 * bar_w, 0.0, val)
            };
            set(cr, bar_color(series, opts, k, i));
            cr.rectangle(x, y_of(to), bar_w, y_of(from) - y_of(to));
            cr.fill().unwrap();
            if opts.labels {
                let t = label_for(val, opts);
                let tw = text_w(cr, &t);
                if opts.stacked {
                    // In the segment, white on the accent, as PowerPoint's
                    // default centred label reads.
                    show_in(cr, (1.0, 1.0, 1.0), x + (bar_w - tw) / 2.0, (y_of(from) + y_of(to)) / 2.0 + CHART_TEXT_PX * 0.35, &t);
                } else {
                    show(cr, x + (bar_w - tw) / 2.0, y_of(to) - 3.0, &t);
                }
            }
        }
        if !opts.hide_category_axis {
            show(cr, left + at as f64 * slot + (slot - text_w(cr, cat)) / 2.0, bottom + 4.0 + CHART_TEXT_PX, cat);
        }
    }

    draw_legend(cr, right + 12.0, (top_y + bottom) / 2.0, &legend, Key::Square);
}

/// `text` broken at spaces into lines no wider than `max_w` where it can
/// be: a single word wider than that stays whole on its line.
fn wrap_to(cr: &Context, text: &str, max_w: f64) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if text_w(cr, &format!("{line} {word}")) <= max_w => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// A horizontal bar chart (`c:barDir val="bar"`): categories down the left,
/// right-aligned against the plot, values along the bottom with vertical
/// gridlines, and each bar's label just past its end. The first category is
/// at the bottom, as Excel draws it, unless the chart is reversed.
fn draw_horizontal_bars(cr: &Context, categories: &[String], series: &[Series], w: f64, h: f64, opts: &Options) {
    let max = if opts.stacked {
        (0..categories.len())
            .map(|i| series.iter().filter_map(|s| s.at(i)).filter(|v| *v > 0.0).sum::<f64>())
            .fold(0.0, f64::max)
    } else {
        max_of(series)
    };
    let axis = ValueAxis::new(max);
    let legend = if opts.no_legend { Vec::new() } else { legend_in(series, opts) };
    let legend_w = legend.iter().map(|(n, _)| text_w(cr, n) + 30.0).fold(0.0, f64::max);
    // Category labels wrap at spaces to fit 45% of the width, as Calc and
    // PowerPoint break a long one over lines.
    let max_w = w * 0.45;
    let wrapped: Vec<Vec<String>> = categories.iter().map(|c| wrap_to(cr, c, max_w)).collect();
    let cat_w = if opts.hide_category_axis { 0.0 } else { wrapped.iter().flatten().map(|l| text_w(cr, l)).fold(0.0, f64::max) };
    let line_h = cr.font_matrix().xx() * 1.15;
    // Room past the longest bar for its label.
    let label_w = if opts.labels { text_w(cr, &label_for(axis.top, opts)) + 6.0 } else { 0.0 };
    let axis_h = if opts.hide_value_axis { 0.0 } else { CHART_TEXT_PX + 8.0 };
    let (left, right, top_y, bottom) = (cat_w + 16.0, w - 10.0 - legend_w - label_w, 10.0, h - 10.0 - axis_h);
    if right <= left || bottom <= top_y || categories.is_empty() {
        return;
    }
    let x_of = |v: f64| left + (v / axis.top) * (right - left);
    if !opts.hide_value_axis {
        cr.set_line_width(1.0);
        for i in 0..=axis.ticks() {
            let x = (left + i as f64 / axis.ticks() as f64 * (right - left)).round() + 0.5;
            set(cr, GRID);
            cr.move_to(x, top_y);
            cr.line_to(x, bottom);
            cr.stroke().unwrap();
            let t = axis.label(i);
            show(cr, x - text_w(cr, &t) / 2.0, bottom + 4.0 + CHART_TEXT_PX, &t);
        }
    }
    // The category axis line, where the bars start.
    set(cr, GRID);
    cr.set_line_width(1.0);
    cr.move_to(left.round() + 0.5, top_y);
    cr.line_to(left.round() + 0.5, bottom);
    cr.stroke().unwrap();

    let count = categories.len();
    let slot = (bottom - top_y) / count as f64;
    let n = if opts.stacked { 1.0 } else { series.len() as f64 };
    let bar_h = slot / (n + opts.gap.unwrap_or(150.0) / 100.0);
    for (i, cat) in categories.iter().enumerate() {
        let row = if opts.reversed { i } else { count - 1 - i };
        let start = top_y + row as f64 * slot + (slot - n * bar_h) / 2.0;
        let mut base = 0.0;
        for (k, s) in series.iter().enumerate() {
            let Some(val) = s.at(i) else { continue };
            let (y, from, to) = if opts.stacked {
                let from = base;
                base += val.max(0.0);
                (start, from, base)
            } else {
                (start + k as f64 * bar_h, 0.0, val)
            };
            set(cr, bar_color(series, opts, k, i));
            cr.rectangle(x_of(from), y, x_of(to) - x_of(from), bar_h);
            cr.fill().unwrap();
            if opts.labels {
                let t = label_for(val, opts);
                let ty = y + bar_h / 2.0 + CHART_TEXT_PX * 0.35;
                if opts.stacked {
                    let tw = text_w(cr, &t);
                    show_in(cr, (1.0, 1.0, 1.0), (x_of(from) + x_of(to) - tw) / 2.0, ty, &t);
                } else {
                    show(cr, x_of(to) + 4.0, ty, &t);
                }
            }
        }
        let lines = &wrapped[i];
        let first = top_y + row as f64 * slot + slot / 2.0 - (lines.len() as f64 - 1.0) * line_h / 2.0 + line_h * 0.3;
        for (j, line) in lines.iter().enumerate().filter(|_| !opts.hide_category_axis) {
            show(cr, left - 8.0 - text_w(cr, line), first + j as f64 * line_h, line);
        }
        let _ = cat;
    }

    draw_legend(cr, w - legend_w + 2.0, (top_y + bottom) / 2.0, &legend, Key::Square);
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
fn draw_category(cr: &Context, categories: &[String], series: &[Series], w: f64, h: f64, style: Style, opts: &Options) {
    let axis = ValueAxis::new(max_of(series));
    let key = if style == Style::Line { Key::Line } else { Key::Square };
    let legend = if opts.no_legend { Vec::new() } else { legend_in(series, opts) };
    let legend_w = legend_width(cr, &legend, key);
    let axis_w = if opts.hide_value_axis { 0.0 } else { axis.widest(cr) + 4.0 };
    let (left, right, top_y, bottom) = (axis_w + 10.0, w - 10.0 - legend_w, 12.0, h - CHART_TEXT_PX - 14.0);
    if right <= left || bottom <= top_y || categories.is_empty() {
        return;
    }
    let y_of = |v: f64| bottom - (v.max(0.0) / axis.top) * (bottom - top_y);
    let n = categories.len();
    let x_of = |i: usize| match style {
        Style::Line => left + (i as f64 + 0.5) * (right - left) / n as f64,
        Style::Area => left + i as f64 * (right - left) / (n - 1).max(1) as f64,
    };
    if !opts.hide_value_axis {
        draw_value_axis(cr, &axis, left, right, top_y, bottom);
    }

    for (k, s) in series.iter().enumerate() {
        set(cr, series_color(opts, k));
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
        if !opts.hide_category_axis {
            show(cr, x_of(i) - text_w(cr, cat) / 2.0, bottom + 4.0 + CHART_TEXT_PX, cat);
        }
    }
    draw_legend(cr, right + 12.0, h / 2.0 + 3.0, &legend, key);
}

/// A pie: slices clockwise from twelve o'clock in the theme's accents, and
/// a legend naming each category at the right, as in Excel and Calc.
fn draw_pie(cr: &Context, categories: &[String], series: &Series, w: f64, h: f64, opts: &Options) {
    let data: Vec<(&String, f64)> = categories.iter().enumerate().map(|(i, c)| (c, series.at(i).unwrap_or(0.0))).collect();
    let total: f64 = data.iter().map(|d| d.1.max(0.0)).sum();
    if total <= 0.0 {
        return;
    }
    let color = |i: usize| opts.point_colors.get(i).copied().flatten().unwrap_or_else(|| accent(i));
    let legend_w = if opts.no_legend { 0.0 } else { data.iter().map(|(c, _)| text_w(cr, c)).fold(0.0, f64::max) + 30.0 };
    let plot_w = (w - legend_w - 10.0).max(1.0);
    let r = ((plot_w.min(h) - 27.0) / 2.0).max(1.0);
    let (cx, cy) = (plot_w / 2.0 + 5.0, h / 2.0);
    let inner = r * opts.hole.clamp(0.0, 0.9);
    let mut angle = -std::f64::consts::FRAC_PI_2;
    for (i, (_, val)) in data.iter().enumerate() {
        let sweep = val.max(0.0) / total * std::f64::consts::TAU;
        set(cr, color(i));
        if inner > 0.0 {
            // A ring segment: out along the rim, back along the hole.
            cr.new_sub_path();
            cr.arc(cx, cy, r, angle, angle + sweep);
            cr.arc_negative(cx, cy, inner, angle + sweep, angle);
            cr.close_path();
        } else {
            cr.move_to(cx, cy);
            cr.arc(cx, cy, r, angle, angle + sweep);
            cr.close_path();
        }
        cr.fill().unwrap();
        if opts.labels && *val > 0.0 {
            // On the segment, halfway across the ring (or the slice).
            let mid = angle + sweep / 2.0;
            let rad = (r + inner) / 2.0 + if inner > 0.0 { 0.0 } else { r * 0.15 };
            let t = label_for(*val, opts);
            show(cr, cx + rad * mid.cos() - text_w(cr, &t) / 2.0, cy + rad * mid.sin() + CHART_TEXT_PX * 0.35, &t);
        }
        angle += sweep;
    }
    if opts.no_legend {
        return;
    }
    // The legend: one row per category, centred on the pie.
    let row = LEGEND_ROW;
    let lx = w - legend_w + 8.0;
    let y0 = cy - row * data.len() as f64 / 2.0 + row * 0.5 + 2.0;
    for (i, (cat, _)) in data.iter().enumerate() {
        let y = y0 + i as f64 * row;
        set(cr, color(i));
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

    fn draw_with(categories: &[String], series: &[Series], kind: ChartKind, opts: &Options) -> ImageSurface {
        let s = ImageSurface::create(Format::ARgb32, 400, 260).unwrap();
        {
            let cr = Context::new(&s).unwrap();
            crate::use_ui_font_rendering(&cr);
            draw_chart_with(&cr, categories, series, kind, 400.0, 260.0, opts);
        }
        s
    }

    /// The x columns that hold `rgb` anywhere, left to right.
    fn columns_with(s: &mut ImageSurface, (r, g, b): (u8, u8, u8)) -> Vec<usize> {
        let (w, h, stride) = (s.width() as usize, s.height() as usize, s.stride() as usize);
        let px = s.data().unwrap();
        (0..w).filter(|x| (0..h).any(|y| px[y * stride + x * 4..][..3] == [b, g, r])).collect()
    }

    /// Default options are the plain chart, pixel for pixel.
    #[test]
    fn default_options_are_the_plain_chart() {
        let categories: Vec<String> = ["1", "2", "3"].iter().map(|s| s.to_string()).collect();
        let (a, b) = ([3.0, 5.0, 4.0], [4.0, 2.0, 6.0]);
        let two = [Series { name: Some("North"), values: &a }, Series { name: Some("South"), values: &b }];
        for kind in [ChartKind::Bar, ChartKind::Line, ChartKind::Area] {
            let mut plain = draw(&categories, &two, kind);
            let mut with = draw_with(&categories, &two, kind, &Options::default());
            assert!(plain.data().unwrap().iter().eq(with.data().unwrap().iter()), "{kind:?}");
        }
    }

    /// Stacked, both series share each category's one column: the second
    /// accent sits in the same x columns as the first, not beside it (#2327).
    #[test]
    fn stacked_series_share_one_column() {
        let categories: Vec<String> = ["May", "June"].iter().map(|s| s.to_string()).collect();
        let (a, b) = ([951.0, 3623.0], [123.0, 587.0]);
        let two = [Series { name: Some("$48"), values: &a }, Series { name: Some("$418"), values: &b }];
        let stacked = Options { stacked: true, ..Options::default() };
        let mut s = draw_with(&categories, &two, ChartKind::Bar, &stacked);
        let (first, second) = (columns_with(&mut s, (0x4f, 0x81, 0xbd)), columns_with(&mut s, (0xc0, 0x50, 0x4d)));
        // The legend's keys carry both accents too; compare the plot's columns.
        let plot = |v: Vec<usize>| v.into_iter().filter(|x| *x < 300).collect::<Vec<_>>();
        assert!(!plot(second.clone()).is_empty());
        assert_eq!(plot(first), plot(second), "stacked segments sit in the same columns");
        let mut side = draw_with(&categories, &two, ChartKind::Bar, &Options::default());
        let (f, g) = (columns_with(&mut side, (0x4f, 0x81, 0xbd)), columns_with(&mut side, (0xc0, 0x50, 0x4d)));
        assert_ne!(plot(f), plot(g), "clustered columns stand side by side");
    }

    /// A doughnut leaves its centre unpainted, its slices take the file's
    /// colours, and with no background the rest of the surface stays clear.
    #[test]
    fn a_transparent_coloured_doughnut() {
        let categories: Vec<String> = ["Done", "Left"].iter().map(|s| s.to_string()).collect();
        let a = [1.0, 3.0];
        let one = [Series { name: None, values: &a }];
        let colors = [Some((0.0, 176.0 / 255.0, 240.0 / 255.0)), None];
        let opts = Options { hole: 0.5, point_colors: &colors, no_legend: true, transparent: true, ..Options::default() };
        let mut s = draw_with(&categories, &one, ChartKind::Pie, &opts);
        assert!(count(&mut s, (0x00, 0xb0, 0xf0)) > 0, "the file's slice colour");
        assert!(count(&mut s, (0xc0, 0x50, 0x4d)) > 0, "the uncoloured slice keeps its accent");
        let (w, h, stride) = (s.width() as usize, s.height() as usize, s.stride() as usize);
        let px = s.data().unwrap();
        let alpha = |x: usize, y: usize| px[y * stride + x * 4 + 3];
        // The pie's centre with no legend: (w - 10) / 2 + 5 across, h / 2 down.
        assert_eq!(alpha((w - 10) / 2 + 5, h / 2), 0, "the hole");
        assert_eq!(alpha(1, 1), 0, "no background");
    }

    /// A hidden value axis draws no gridlines: no grid grey anywhere.
    #[test]
    fn a_hidden_value_axis_draws_no_gridlines() {
        let categories: Vec<String> = ["1", "2"].iter().map(|s| s.to_string()).collect();
        let a = [3.0, 5.0];
        let one = [Series { name: None, values: &a }];
        let grid = |s: &mut ImageSurface| {
            let (r, g, b) = GRID;
            count(s, ((r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8))
        };
        let mut shown = draw_with(&categories, &one, ChartKind::Bar, &Options::default());
        let mut hidden = draw_with(&categories, &one, ChartKind::Bar, &Options { hide_value_axis: true, ..Options::default() });
        assert!(grid(&mut shown) > 0);
        assert_eq!(grid(&mut hidden), 0);
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

    // ── The title, axis titles and legend position (#1658) ──────────────

    /// The surface's bytes.
    fn bytes(s: &mut ImageSurface) -> Vec<u8> {
        s.data().unwrap().to_vec()
    }

    /// The rows and columns `rgb` is drawn in: (first row, last row, first
    /// column, last column, count), or None.
    fn extent(s: &mut ImageSurface, (r, g, b): (u8, u8, u8)) -> Option<(usize, usize, usize, usize, usize)> {
        let (w, h, stride) = (s.width() as usize, s.height() as usize, s.stride() as usize);
        let px = s.data().unwrap();
        let mut found: Option<(usize, usize, usize, usize, usize)> = None;
        for y in 0..h {
            for x in 0..w {
                if px[y * stride + x * 4..][..3] == [b, g, r] {
                    found = Some(match found {
                        None => (y, y, x, x, 1),
                        Some((y0, y1, x0, x1, n)) => (y0.min(y), y1.max(y), x0.min(x), x1.max(x), n + 1),
                    });
                }
            }
        }
        found
    }

    fn decorated(series: &[Series], kind: ChartKind, decor: &Decor) -> ImageSurface {
        let cats: Vec<String> = (1..=4).map(|i| format!("Q{i}")).collect();
        render_chart_decorated(&cats, series, kind, 400, 260, decor)
    }

    fn two() -> (Vec<f64>, Vec<f64>) {
        (vec![3.0, 7.0, 5.0, 9.0], vec![0.0; 4])
    }

    /// The grid colour as the renderer draws it, for finding gridlines.
    fn grid_colour() -> (u8, u8, u8) {
        let mut s = ImageSurface::create(Format::ARgb32, 4, 4).unwrap();
        {
            let cr = Context::new(&s).unwrap();
            set(&cr, GRID);
            cr.paint().unwrap();
        }
        let px = s.data().unwrap();
        (px[2], px[1], px[0])
    }

    /// The first and last rows that hold a gridline.
    fn grid_rows(s: &mut ImageSurface) -> (usize, usize) {
        let (r, g, b) = grid_colour();
        let (w, h, stride) = (s.width() as usize, s.height() as usize, s.stride() as usize);
        let px = s.data().unwrap();
        let rows: Vec<usize> = (0..h).filter(|y| (0..w).filter(|x| px[y * stride + x * 4..][..3] == [b, g, r]).count() >= 100).collect();
        (*rows.first().expect("no gridline"), *rows.last().unwrap())
    }

    /// With nothing asked for, a decorated chart is the chart that was
    /// always drawn: Tables' charts and the render lab's baselines don't
    /// move. A pie, which has no axes and keeps its legend, is the same
    /// whatever legend position it is given.
    #[test]
    fn with_nothing_asked_it_is_draw_chart_series_exactly() {
        let (a, b) = two();
        let series = [Series { name: Some("Sales"), values: &a }, Series { name: Some("Plan"), values: &b }];
        let cats: Vec<String> = (1..=4).map(|i| format!("Q{i}")).collect();
        for kind in [ChartKind::Bar, ChartKind::Line, ChartKind::Area, ChartKind::Scatter, ChartKind::Pie] {
            let mut plain = draw(&cats, &series, kind);
            // draw() is 400x260, as decorated() is.
            let mut same = decorated(&series, kind, &Decor::default());
            assert_eq!(bytes(&mut plain), bytes(&mut same), "{kind:?}");
        }
        let mut pie_bottom = decorated(&series, ChartKind::Pie, &Decor { legend: LegendPos::Bottom, ..Decor::default() });
        let mut pie_right = decorated(&series, ChartKind::Pie, &Decor::default());
        assert_eq!(bytes(&mut pie_bottom), bytes(&mut pie_right), "a pie's legend stays at its right");
    }

    /// A title takes the room over the plot and an x title the room under
    /// it: the first gridline moves down and the last moves up.
    #[test]
    fn a_title_and_an_x_title_make_room_for_themselves() {
        let (a, b) = two();
        let series = [Series { name: Some("Sales"), values: &a }, Series { name: Some("Plan"), values: &b }];
        let (first, last) = grid_rows(&mut decorated(&series, ChartKind::Bar, &Decor::default()));
        let (t_first, t_last) = grid_rows(&mut decorated(&series, ChartKind::Bar, &Decor { title: "Sales by quarter", ..Decor::default() }));
        assert!(t_first >= first + 25 && t_last == last, "title: {first}..{last} -> {t_first}..{t_last}");
        let (x_first, x_last) = grid_rows(&mut decorated(&series, ChartKind::Bar, &Decor { x_title: Some("Quarter"), ..Decor::default() }));
        assert!(x_first == first && x_last + 15 <= last, "x title: {first}..{last} -> {x_first}..{x_last}");
    }

    /// The legend's keys are where the position says. The second series is
    /// all zeros, so its colour is drawn nowhere but in its legend key.
    #[test]
    fn the_legend_sits_where_it_is_asked() {
        let (a, b) = two();
        let series = [Series { name: Some("Sales"), values: &a }, Series { name: Some("Plan"), values: &b }];
        let key = (0xc0, 0x50, 0x4d);
        let at = |pos| extent(&mut decorated(&series, ChartKind::Bar, &Decor { legend: pos, ..Decor::default() }), key);
        assert_eq!(at(LegendPos::None), None, "no legend, no key");
        let right = at(LegendPos::Right).expect("a key at the right");
        assert!(right.2 > 300, "right: {right:?}");
        let left = at(LegendPos::Left).expect("a key at the left");
        assert!(left.3 < 100, "left: {left:?}");
        let top = at(LegendPos::Top).expect("a key above");
        assert!(top.1 < 30, "top: {top:?}");
        let bottom = at(LegendPos::Bottom).expect("a key below");
        assert!(bottom.0 > 260 - 30, "bottom: {bottom:?}");
        // A row is centred under the plot, not pushed into a corner.
        assert!((100..300).contains(&(bottom.2 + (bottom.3 - bottom.2) / 2)), "bottom: {bottom:?}");
    }

    /// Titles, long ones included, and a legend row wider than the chart
    /// stay inside it and never panic at any size.
    #[test]
    fn long_titles_and_tiny_charts_do_not_panic() {
        let (a, b) = two();
        let series = [Series { name: Some("A series with a very long name indeed"), values: &a }, Series { name: Some("Another one"), values: &b }];
        let long = "A chart title far too long to fit in the width this chart has, with 見出し and שלום in it";
        for kind in [ChartKind::Bar, ChartKind::Line, ChartKind::Area, ChartKind::Scatter, ChartKind::Pie] {
            for legend in [LegendPos::None, LegendPos::Top, LegendPos::Bottom, LegendPos::Left, LegendPos::Right] {
                let decor = Decor { title: long, x_title: Some(long), y_title: Some(long), legend };
                let cats: Vec<String> = (1..=4).map(|i| format!("Q{i}")).collect();
                for (w, h) in [(400, 260), (60, 40), (1, 1)] {
                    render_chart_decorated(&cats, &series, kind, w, h, &decor);
                }
                render_chart_decorated(&[], &[], kind, 200, 100, &decor);
            }
        }
    }
}
