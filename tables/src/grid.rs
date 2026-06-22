// grid.rs — Cairo-rendered spreadsheet grid widget.
use gtk4 as gtk;
use gtk::prelude::*;
use crate::engine::Spreadsheet;

const CELL_W: f64 = 80.0;
const CELL_H: f64 = 24.0;
const HEADER_W: f64 = 40.0;

pub struct CellGrid {
    drawing_area: gtk::DrawingArea,
    sheet: std::cell::RefCell<Spreadsheet>,
    selection: std::cell::Cell<(usize, usize)>,
    scroll_offset: std::cell::Cell<(f64, f64)>,
    visible_rows: std::cell::Cell<usize>,
    visible_cols: std::cell::Cell<usize>,
}

impl CellGrid {
    pub fn new(sheet: Spreadsheet) -> Self {
        let (rows, cols) = (sheet.rows, sheet.cols);
        let sheet = std::cell::RefCell::new(sheet);
        let selection = std::cell::Cell::new((0, 0));
        let scroll_offset = std::cell::Cell::new((0.0, 0.0));

        let drawing_area = gtk::DrawingArea::new();
        drawing_area.set_hexpand(true);
        drawing_area.set_vexpand(true);
        drawing_area.set_can_focus(true);
        drawing_area.set_focusable(true);

        // Calculate visible area
        drawing_area.connect_resize({
            let w = CELL_W; let h = CELL_H;
            move |area, width, _height| {
                let vc = (width as f64 / (w + HEADER_W)) as usize + 1;
                area.set_size_request((vc as f64 * w + HEADER_W) as i32, 800);
            }
        });

        let sheet_ref = sheet.clone();
        let sel = selection.clone();
        let scroll = scroll_offset.clone();
        drawing_area.set_draw_func(move |_area, cr, width, height| {
            let sheet = sheet_ref.borrow();
            let (ox, oy) = scroll.get();
            let (sr, sc) = sel.get();

            // Background
            cr.set_source_rgb(1.0, 1.0, 1.0); cr.paint().unwrap();

            // Column headers
            cr.set_source_rgb(0.9, 0.9, 0.9);
            cr.rectangle(0.0, 0.0, width as f64, CELL_H); cr.fill().unwrap();
            cr.set_source_rgb(0.95, 0.95, 0.95);
            cr.rectangle(0.0, 0.0, HEADER_W, height as f64); cr.fill().unwrap();

            let visible_cols = ((width as f64 - HEADER_W) / CELL_W) as usize + 1;
            let visible_rows = (height as f64 / CELL_H) as usize;

            // Draw cells
            for r in 0..visible_rows {
                let data_row = r + oy as usize;
                if data_row >= sheet.rows { break; }
                for c in 0..visible_cols {
                    let data_col = c + ox as usize;
                    if data_col >= sheet.cols { break; }
                    let x = HEADER_W + c as f64 * CELL_W;
                    let y = (r + 1) as f64 * CELL_H;

                    // Selection highlight
                    if data_row == sr && data_col == sc {
                        cr.set_source_rgb(0.8, 0.9, 1.0);
                        cr.rectangle(x, y, CELL_W, CELL_H); cr.fill().unwrap();
                    }

                    // Cell border
                    cr.set_source_rgb(0.85, 0.85, 0.85);
                    cr.rectangle(x, y, CELL_W, CELL_H); cr.stroke().unwrap();

                    // Cell text
                    cr.set_source_rgb(0.1, 0.1, 0.1);
                    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
                    cr.set_font_size(13.0);
                    let val = sheet.get(data_row, data_col);
                    if !val.is_empty() {
                        cr.move_to(x + 4.0, y + CELL_H - 7.0);
                        cr.show_text(val).unwrap();
                    }
                }
            }

            // Column headers (A, B, C...)
            cr.set_source_rgb(0.2, 0.2, 0.2);
            cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
            cr.set_font_size(12.0);
            for c in 0..visible_cols {
                let data_col = c + ox as usize;
                if data_col >= 26 { break; }
                let col_char = (b'A' + data_col as u8) as char;
                let x = HEADER_W + c as f64 * CELL_W + CELL_W / 2.0 - 4.0;
                cr.move_to(x, CELL_H - 5.0);
                cr.show_text(&col_char.to_string()).unwrap();
            }

            // Row headers (1, 2, 3...)
            for r in 0..visible_rows {
                let data_row = r + oy as usize + 1;
                let y = (r + 1) as f64 * CELL_H + CELL_H / 2.0 + 4.0;
                cr.move_to(HEADER_W / 2.0 - 8.0, y);
                cr.show_text(&data_row.to_string()).unwrap();
            }
        });

        // Keyboard navigation
        let sr = selection.clone();
        let so = scroll_offset.clone();
        let key = gtk::EventControllerKey::new();
        key.connect_key_pressed(move |_, keyval, _, _| {
            let (mut r, mut c) = sr.get();
            let (mut ox, mut oy) = so.get();
            match keyval {
                65361 => { if c > 0 { c -= 1; } } // Left
                65363 => { c += 1; }              // Right
                65362 => { if r > 0 { r -= 1; } } // Up
                65364 => { r += 1; }              // Down
                _ => return gtk::Inhibit(false),
            }
            // Scroll if needed
            if c < ox as usize { ox = c as f64; }
            if c >= ox as usize + 10 { ox = c as f64 - 9.0; }
            if r < oy as usize { oy = r as f64; }
            if r >= oy as usize + 20 { oy = r as f64 - 19.0; }
            sr.set((r, c));
            so.set((ox, oy));
            drawing_area.queue_draw();
            gtk::Inhibit(false)
        });
        drawing_area.add_controller(key);

        Self { drawing_area, sheet, selection, scroll_offset, visible_rows: std::cell::Cell::new(0), visible_cols: std::cell::Cell::new(0) }
    }

    pub fn widget(&self) -> &gtk::DrawingArea { &self.drawing_area }

    pub fn set_cell(&self, r: usize, c: usize, v: &str) {
        self.sheet.borrow_mut().set(r, c, v);
        self.drawing_area.queue_draw();
    }
}
