// grid_render.rs — Cairo grid drawing for the spreadsheet canvas.
use gtk4::cairo::{self, Context};
use std::cell::RefCell;
use std::rc::Rc;
use tables_core::sheet::{SheetModel, CellBorder, BorderStyle, xy_to_cell, col_label};

/// Auto-fit column width to content using PangoLayout text measurement.
/// Lives here (not tables-core) because it needs Cairo/Pango to measure.
pub fn auto_fit_column(cr: &Context, sheet: &mut SheetModel, col: usize, _scroll_x: f64) {
    let layout = pangocairo::functions::create_layout(cr);
    let mut max_w: f64 = 30.0;
    let label = col_label(col);
    layout.set_text(&label);
    let (tw, _) = layout.pixel_size();
    max_w = max_w.max(tw as f64 + 16.0);
    for r in 0..sheet.rows {
        let val = sheet.cell(r, col);
        if val.is_empty() { continue; }
        layout.set_text(val);
        let (tw, _) = layout.pixel_size();
        max_w = max_w.max(tw as f64 + 12.0);
    }
    sheet.set_col_width(col, max_w.clamp(30.0, 500.0));
}

const ROW_HEIGHT: f64 = 28.0;
const COL_WIDTH: f64 = 90.0;
const ROW_HEADER_WIDTH: f64 = 50.0;
const COL_HEADER_HEIGHT: f64 = 26.0;
const HEADER_BG: (f64, f64, f64) = (0.95, 0.95, 0.95);
const HEADER_BG_DARK: (f64, f64, f64) = (0.25, 0.25, 0.25);
const SELECTION_COLOR: (f64, f64, f64) = (0.21, 0.52, 0.89);
const ACTIVE_CELL_BORDER: (f64, f64, f64) = (0.13, 0.38, 0.77);
const GRID_LINE: (f64, f64, f64) = (0.85, 0.85, 0.85);

pub fn draw_border_edges(cr: &Context, x: f64, y: f64, w: f64, h: f64, border: &CellBorder) {
    let lw = 1.5;
    draw_border_line(cr, &border.top, x, y, x + w, y);
    draw_border_line(cr, &border.bottom, x, y + h, x + w, y + h);
    draw_border_line(cr, &border.left, x, y, x, y + h);
    draw_border_line(cr, &border.right, x + w, y, x + w, y + h);
}

fn draw_border_line(cr: &Context, style: &BorderStyle, x1: f64, y1: f64, x2: f64, y2: f64) {
    if *style == BorderStyle::None { return; }
    cr.save().unwrap();
    cr.set_source_rgb(0.0, 0.0, 0.0);
    cr.set_line_width(1.5);
    match style {
        BorderStyle::Dotted => cr.set_dash(&[2.0, 2.0], 0.0),
        BorderStyle::Dashed => cr.set_dash(&[4.0, 4.0], 0.0),
        BorderStyle::Double => { cr.set_line_width(3.0); cr.set_dash(&[1.0, 3.0], 0.0); }
        _ => cr.set_dash(&[], 0.0),
    }
    cr.move_to(x1, y1);
    cr.line_to(x2, y2);
    cr.stroke().unwrap();
    cr.restore().unwrap();
}

pub fn draw_grid(
    cr: &Context, state: &Rc<RefCell<crate::window::AppState>>,
    width: f64, height: f64, scroll_x: f64, scroll_y: f64,
) {
    let st = state.borrow();
    let sheet = &st.sheets[st.active_sheet].borrow();
    let is_dark = false; // detect from theme
    let hdr_bg = if is_dark { HEADER_BG_DARK } else { HEADER_BG };

    // Background
    cr.set_source_rgb(0.5, 0.5, 0.5);
    cr.rectangle(0.0, 0.0, width, height);
    cr.fill().unwrap();

    let start_col = (scroll_x / COL_WIDTH).max(0.0) as usize;
    let start_row = (scroll_y / ROW_HEIGHT).max(0.0) as usize;

    // Corner cell — same shade as the headers, not the canvas grey.
    cr.set_source_rgb(hdr_bg.0, hdr_bg.1, hdr_bg.2);
    cr.rectangle(0.0, 0.0, ROW_HEADER_WIDTH, COL_HEADER_HEIGHT);
    cr.fill().unwrap();

    // Column headers
    cr.save().unwrap();
    cr.rectangle(ROW_HEADER_WIDTH, 0.0, width - ROW_HEADER_WIDTH, COL_HEADER_HEIGHT);
    cr.clip();
    cr.set_source_rgb(hdr_bg.0, hdr_bg.1, hdr_bg.2);
    cr.rectangle(ROW_HEADER_WIDTH, 0.0, width, COL_HEADER_HEIGHT);
    cr.fill().unwrap();
    let mut cx = ROW_HEADER_WIDTH - scroll_x;
    for c in start_col..sheet.cols {
        let cw = sheet.col_width(c);
        if cx + cw < ROW_HEADER_WIDTH { cx += cw - scroll_x; continue; }
        if cx > width { break; }
        let label = col_label(c);
        cr.set_source_rgb(0.3, 0.3, 0.3);
        let ext = cr.text_extents(&label).unwrap();
        cr.move_to(cx + (cw - ext.width()) / 2.0, 18.0);
        let _ = cr.show_text(&label);
        cr.set_source_rgb(0.8, 0.8, 0.8);
        cr.set_line_width(0.5);
        cr.move_to(cx + cw, 0.0);
        cr.line_to(cx + cw, COL_HEADER_HEIGHT);
        cr.stroke().unwrap();
        cx += cw;
    }
    cr.restore().unwrap();

    // Row headers
    cr.save().unwrap();
    cr.rectangle(0.0, COL_HEADER_HEIGHT, ROW_HEADER_WIDTH, height - COL_HEADER_HEIGHT);
    cr.clip();
    cr.set_source_rgb(hdr_bg.0, hdr_bg.1, hdr_bg.2);
    cr.rectangle(0.0, COL_HEADER_HEIGHT, ROW_HEADER_WIDTH, height);
    cr.fill().unwrap();
    let mut ry = COL_HEADER_HEIGHT - scroll_y;
    for r in start_row..sheet.rows.min(start_row + (height / ROW_HEIGHT) as usize + 1) {
        cr.set_source_rgb(0.3, 0.3, 0.3);
        let label = (r + 1).to_string();
        let ext = cr.text_extents(&label).unwrap();
        cr.move_to(ROW_HEADER_WIDTH - 6.0 - ext.width(), ry + 18.0);
        let _ = cr.show_text(&label);
        ry += ROW_HEIGHT;
    }
    cr.restore().unwrap();

    // Cells
    cr.save().unwrap();
    cr.rectangle(ROW_HEADER_WIDTH, COL_HEADER_HEIGHT, width - ROW_HEADER_WIDTH, height - COL_HEADER_HEIGHT);
    cr.clip();
    let mut cy = COL_HEADER_HEIGHT - scroll_y;
    for r in start_row..sheet.rows.min(start_row + (height / ROW_HEIGHT) as usize + 1) {
        cx = ROW_HEADER_WIDTH - scroll_x;
        for c in start_col..sheet.cols {
            let cw = sheet.col_width(c);
            if cx + cw < ROW_HEADER_WIDTH { cx += cw; continue; }
            if cx > width { break; }
            let is_sel = r == sheet.selected_row && c == sheet.selected_col;
            let (sr0, sc0, sr1, sc1) = sheet.selection_rect();
            let in_range = r >= sr0 && r <= sr1 && c >= sc0 && c <= sc1;
            let border = &sheet.borders[r][c];

            // Cell bg — whole selection range gets the accent wash.
            if in_range {
                cr.set_source_rgb(0.8, 0.85, 0.95);
            } else {
                cr.set_source_rgb(1.0, 1.0, 1.0);
            }
            cr.rectangle(cx, cy, cw, ROW_HEIGHT);
            cr.fill().unwrap();

            // Grid line
            cr.set_source_rgb(GRID_LINE.0, GRID_LINE.1, GRID_LINE.2);
            cr.set_line_width(0.5);
            cr.move_to(cx + cw, cy);
            cr.line_to(cx + cw, cy + ROW_HEIGHT);
            cr.stroke().unwrap();
            cr.move_to(cx, cy + ROW_HEIGHT);
            cr.line_to(cx + cw, cy + ROW_HEIGHT);
            cr.stroke().unwrap();

            // Cell border
            if border.top != BorderStyle::None || border.bottom != BorderStyle::None
                || border.left != BorderStyle::None || border.right != BorderStyle::None {
                draw_border_edges(cr, cx, cy, cw, ROW_HEIGHT, border);
            }

            // Active cell border
            if is_sel {
                cr.set_source_rgb(ACTIVE_CELL_BORDER.0, ACTIVE_CELL_BORDER.1, ACTIVE_CELL_BORDER.2);
                cr.set_line_width(2.0);
                cr.rectangle(cx, cy, cw, ROW_HEIGHT);
                cr.stroke().unwrap();
            }

            // Text
            let val = sheet.cell(r, c);
            if !val.is_empty() {
                cr.set_source_rgb(0.0, 0.0, 0.0);
                cr.move_to(cx + 4.0, cy + 19.0);
                let display = if val.len() > 10 { &val[..10] } else { val };
                let _ = cr.show_text(display);
            }
            cx += cw;
        }
        cy += ROW_HEIGHT;
    }
    cr.restore().unwrap();

    // Selection range outline (2px accent around the whole rectangle).
    let (sr0, sc0, sr1, sc1) = sheet.selection_rect();
    let px_x = |col: usize| -> f64 {
        ROW_HEADER_WIDTH - scroll_x + (0..col).map(|c| sheet.col_width(c)).sum::<f64>()
    };
    let x0 = px_x(sc0);
    let x1 = px_x(sc1 + 1);
    let y0 = COL_HEADER_HEIGHT - scroll_y + sr0 as f64 * ROW_HEIGHT;
    let y1 = COL_HEADER_HEIGHT - scroll_y + (sr1 + 1) as f64 * ROW_HEIGHT;
    cr.save().unwrap();
    cr.rectangle(ROW_HEADER_WIDTH, COL_HEADER_HEIGHT, width - ROW_HEADER_WIDTH, height - COL_HEADER_HEIGHT);
    cr.clip();
    cr.set_source_rgb(SELECTION_COLOR.0, SELECTION_COLOR.1, SELECTION_COLOR.2);
    cr.set_line_width(2.0);
    cr.rectangle(x0, y0, x1 - x0, y1 - y0);
    cr.stroke().unwrap();
    cr.restore().unwrap();
}
