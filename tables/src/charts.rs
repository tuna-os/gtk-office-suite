// charts.rs — Pure Rust chart rendering via Cairo.
use gtk4::cairo::{Context, Format, ImageSurface};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChartType { Bar, Line, Pie }

pub fn render_chart(data: &[(String, f64)], chart_type: ChartType, width: i32, height: i32) -> ImageSurface {
    render_chart_named(data, chart_type, width, height, None)
}

/// As [`render_chart`], with a legend entry for the series when it has a
/// name (charts on the grid do; the dialog's preview doesn't need one).
pub fn render_chart_named(
    data: &[(String, f64)],
    chart_type: ChartType,
    width: i32,
    height: i32,
    series_name: Option<&str>,
) -> ImageSurface {
    let surface = ImageSurface::create(Format::ARgb32, width, height).unwrap();
    let cr = Context::new(&surface).unwrap();
    suite_common::use_ui_font_rendering(&cr);
    cr.set_source_rgb(1.0, 1.0, 1.0); cr.paint().unwrap();
    // Chart text is Calibri 10 pt (Carlito where Calibri isn't
    // installed), as Excel and Calc draw a chart that names no font. It was
    // 10 px of the generic sans: 7.5 pt, and too small to read at 100%.
    cr.select_font_face("Calibri", gtk4::cairo::FontSlant::Normal, gtk4::cairo::FontWeight::Normal);
    cr.set_font_size(CHART_TEXT_PX);
    match chart_type {
        ChartType::Bar => draw_bars(&cr, data, width, height, series_name),
        ChartType::Line => draw_line(&cr, data, width, height),
        ChartType::Pie => draw_pie(&cr, data, width, height),
    }
    surface.flush();
    surface
}

/// 10 pt at 96 DPI.
const CHART_TEXT_PX: f64 = 10.0 * 96.0 / 72.0;
/// Axis and legend text: black, as Calc draws a chart that names no text
/// colour.
const CHART_TEXT: (f64, f64, f64) = (0.0, 0.0, 0.0);

/// The first series colour of Office's default theme (accent 1), which
/// Excel and LibreOffice give a series that names no colour.
const SERIES_1: (f64, f64, f64) = (0x4f as f64 / 255.0, 0x81 as f64 / 255.0, 0xbd as f64 / 255.0);

/// A clustered column chart: value axis from 0 with round ticks and
/// horizontal gridlines, categories under the bars, and the legend at the
/// right when the series has a name.
fn draw_bars(cr: &Context, data: &[(String, f64)], w: i32, h: i32, series_name: Option<&str>) {
    let (w, h) = (w as f64, h as f64);
    let text_w = |t: &str| cr.text_extents(t).map(|e| e.x_advance()).unwrap_or(0.0);
    let max_val = data.iter().map(|d| d.1).fold(0.0, f64::max);
    let (top, step) = tables_core::sheet::nice_axis(max_val);
    let ticks = (top / step).round() as usize;
    let label = |v: f64| {
        let s = format!("{:.*}", if step < 1.0 { 1 } else { 0 }, v);
        s
    };
    let axis_w = (0..=ticks).map(|i| text_w(&label(i as f64 * step))).fold(0.0, f64::max);
    let legend_w = series_name.map_or(0.0, |n| text_w(n) + 30.0);
    let (left, right, top_y, bottom) = (axis_w + 14.0, w - 10.0 - legend_w, 12.0, h - CHART_TEXT_PX - 14.0);
    if right <= left || bottom <= top_y || data.is_empty() {
        return;
    }
    let y_of = |v: f64| bottom - (v / top) * (bottom - top_y);

    // Gridlines and value labels.
    cr.set_line_width(1.0);
    for i in 0..=ticks {
        let v = i as f64 * step;
        let y = y_of(v).round() + 0.5;
        cr.set_source_rgb(0.7, 0.7, 0.7);
        cr.move_to(left, y);
        cr.line_to(right, y);
        cr.stroke().unwrap();
        let t = label(v);
        cr.set_source_rgb(CHART_TEXT.0, CHART_TEXT.1, CHART_TEXT.2);
        cr.move_to(left - 6.0 - text_w(&t), y + CHART_TEXT_PX * 0.35);
        cr.show_text(&t).unwrap();
    }

    // Bars: each category's slot is split 150% gap to 100% bar, as Excel's default.
    let slot = (right - left) / data.len() as f64;
    let bar_w = slot / 2.5;
    for (i, (cat, val)) in data.iter().enumerate() {
        let x = left + i as f64 * slot + (slot - bar_w) / 2.0;
        cr.set_source_rgb(SERIES_1.0, SERIES_1.1, SERIES_1.2);
        cr.rectangle(x, y_of(*val), bar_w, bottom - y_of(*val));
        cr.fill().unwrap();
        cr.set_source_rgb(CHART_TEXT.0, CHART_TEXT.1, CHART_TEXT.2);
        cr.move_to(left + i as f64 * slot + (slot - text_w(cat)) / 2.0, bottom + 4.0 + CHART_TEXT_PX);
        cr.show_text(cat).unwrap();
    }

    // Legend.
    if let Some(name) = series_name {
        let lx = right + 12.0;
        let ly = (top_y + bottom) / 2.0;
        cr.set_source_rgb(SERIES_1.0, SERIES_1.1, SERIES_1.2);
        cr.rectangle(lx, ly - 7.0, 8.0, 8.0);
        cr.fill().unwrap();
        cr.set_source_rgb(CHART_TEXT.0, CHART_TEXT.1, CHART_TEXT.2);
        cr.move_to(lx + 12.0, ly + 1.0);
        cr.show_text(name).unwrap();
    }
}
fn draw_line(cr: &Context, data: &[(String, f64)], w: i32, h: i32) { /* same */
    let n = data.len(); let max_val = data.iter().map(|d| d.1).fold(0.0, f64::max).max(1.0);
    let step_x = w as f64 * 0.8 / (n as f64 - 1.0).max(1.0); let ox = w as f64 * 0.1;
    cr.set_source_rgb(0.9, 0.3, 0.2); cr.set_line_width(2.0);
    for (i, d) in data.iter().enumerate().take(n) { let x = ox + i as f64 * step_x; let y = h as f64 * 0.85 - (d.1 / max_val) * (h as f64 * 0.7);
        if i == 0 { cr.move_to(x, y); } else { cr.line_to(x, y); } }
    cr.stroke().unwrap();
    for (i, (label, _)) in data.iter().enumerate() { cr.set_source_rgb(0.1, 0.1, 0.1); cr.move_to(ox + i as f64 * step_x, h as f64 * 0.92); cr.show_text(label).unwrap(); }
}
fn draw_pie(cr: &Context, data: &[(String, f64)], w: i32, h: i32) { /* same */
    let total: f64 = data.iter().map(|d| d.1).sum(); if total == 0.0 { return; }
    let cx = w as f64 / 2.0; let cy = h as f64 / 2.0; let r = (w.min(h) as f64) * 0.35;
    let colors = [(0.2,0.5,0.9),(0.9,0.3,0.2),(0.2,0.8,0.3),(0.9,0.7,0.1),(0.7,0.2,0.8)];
    let mut angle = -std::f64::consts::FRAC_PI_2;
    for (i, (label, val)) in data.iter().enumerate() {
        let sweep = (val / total) * 2.0 * std::f64::consts::PI;
        let (rc,gc,bc) = colors[i % colors.len()]; cr.set_source_rgb(rc, gc, bc);
        cr.move_to(cx, cy); cr.arc(cx, cy, r, angle, angle + sweep); cr.close_path(); cr.fill().unwrap();
        let mid = angle + sweep / 2.0; cr.set_source_rgb(0.1, 0.1, 0.1);
        cr.move_to(cx + r * 0.6 * mid.cos(), cy + r * 0.6 * mid.sin()); cr.show_text(label).unwrap();
        angle += sweep;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn test_bar() { let d = vec![("A".into(),10.0)]; let s = render_chart(&d, ChartType::Bar, 400, 300); assert!(s.width() == 400); }
    #[test] fn test_pie() { let d = vec![("X".into(),40.0)]; let s = render_chart(&d, ChartType::Pie, 300, 300); assert!(s.width() == 300); }
}
