// charts.rs — Pure Rust chart rendering via Cairo.
use cairo::{Context, Format, ImageSurface};

#[allow(dead_code)]
pub enum ChartType {
    Bar,
    Line,
    Pie,
}

#[allow(dead_code)]
pub fn render_chart(
    data: &[(String, f64)],
    chart_type: ChartType,
    width: i32,
    height: i32,
) -> ImageSurface {
    let surface = ImageSurface::create(Format::ARgb32, width, height).unwrap();
    let cr = Context::new(&surface).unwrap();
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.paint().unwrap();
    match chart_type {
        ChartType::Bar => draw_bars(&cr, data, width, height),
        ChartType::Line => draw_line(&cr, data, width, height),
        ChartType::Pie => draw_pie(&cr, data, width, height),
    }
    surface.flush();
    surface
}

#[allow(dead_code)]
fn draw_bars(cr: &Context, data: &[(String, f64)], w: i32, h: i32) {
    /* same as before */
    let n = data.len() as f64;
    let max_val = data.iter().map(|d| d.1).fold(0.0, f64::max).max(1.0);
    let bar_w = (w as f64 * 0.7) / n;
    let margin = w as f64 * 0.15 / n;
    for (i, (label, val)) in data.iter().enumerate() {
        let x = margin + i as f64 * (bar_w + margin * 2.0);
        let bar_h = (val / max_val) * (h as f64 * 0.7);
        let y = h as f64 * 0.85 - bar_h;
        cr.set_source_rgb(0.2, 0.5, 0.9);
        cr.rectangle(x, y, bar_w, bar_h);
        cr.fill().unwrap();
        cr.set_source_rgb(0.1, 0.1, 0.1);
        cr.move_to(x, h as f64 * 0.92);
        cr.show_text(label).unwrap();
    }
}

#[allow(dead_code)]
fn draw_line(cr: &Context, data: &[(String, f64)], w: i32, h: i32) {
    /* same */
    let n = data.len();
    let max_val = data.iter().map(|d| d.1).fold(0.0, f64::max).max(1.0);
    let step_x = w as f64 * 0.8 / (n as f64 - 1.0).max(1.0);
    let ox = w as f64 * 0.1;
    cr.set_source_rgb(0.9, 0.3, 0.2);
    cr.set_line_width(2.0);
    for (i, item) in data.iter().enumerate() {
        let x = ox + i as f64 * step_x;
        let y = h as f64 * 0.85 - (item.1 / max_val) * (h as f64 * 0.7);
        if i == 0 {
            cr.move_to(x, y);
        } else {
            cr.line_to(x, y);
        }
    }
    cr.stroke().unwrap();
    for (i, (label, _)) in data.iter().enumerate() {
        cr.set_source_rgb(0.1, 0.1, 0.1);
        cr.move_to(ox + i as f64 * step_x, h as f64 * 0.92);
        cr.show_text(label).unwrap();
    }
}

#[allow(dead_code)]
fn draw_pie(cr: &Context, data: &[(String, f64)], w: i32, h: i32) {
    /* same */
    let total: f64 = data.iter().map(|d| d.1).sum();
    if total == 0.0 {
        return;
    }
    let cx = w as f64 / 2.0;
    let cy = h as f64 / 2.0;
    let r = (w.min(h) as f64) * 0.35;
    let colors = [
        (0.2, 0.5, 0.9),
        (0.9, 0.3, 0.2),
        (0.2, 0.8, 0.3),
        (0.9, 0.7, 0.1),
        (0.7, 0.2, 0.8),
    ];
    let mut angle = -std::f64::consts::FRAC_PI_2;
    for (i, (label, val)) in data.iter().enumerate() {
        let sweep = (val / total) * 2.0 * std::f64::consts::PI;
        let (rc, gc, bc) = colors[i % colors.len()];
        cr.set_source_rgb(rc, gc, bc);
        cr.move_to(cx, cy);
        cr.arc(cx, cy, r, angle, angle + sweep);
        cr.close_path();
        cr.fill().unwrap();
        let mid = angle + sweep / 2.0;
        cr.set_source_rgb(0.1, 0.1, 0.1);
        cr.move_to(cx + r * 0.6 * mid.cos(), cy + r * 0.6 * mid.sin());
        cr.show_text(label).unwrap();
        angle += sweep;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_bar() {
        let d = vec![("A".into(), 10.0)];
        let s = render_chart(&d, ChartType::Bar, 400, 300);
        assert!(s.width() == 400);
    }
    #[test]
    fn test_pie() {
        let d = vec![("X".into(), 40.0)];
        let s = render_chart(&d, ChartType::Pie, 300, 300);
        assert!(s.width() == 300);
    }
}
