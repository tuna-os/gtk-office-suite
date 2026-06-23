use gtk4 as gtk;
use gtk::prelude::*;
use cairo::{Context, Format, ImageSurface};

const ROWS: usize = 50;
const COLS: usize = 10;
const CELL_W: f64 = 80.0;
const CELL_H: f64 = 26.0;
const ROW_HEADER_W: f64 = 40.0;

pub struct TablesWindow { window: gtk::ApplicationWindow, grid: gtk::DrawingArea, model: std::cell::RefCell<Vec<Vec<String>>>, selection: std::cell::Cell<(usize, usize)> }

impl TablesWindow {
    pub fn new(app: &gtk::Application) -> Self {
        let win = gtk::ApplicationWindow::builder().application(app).build();
        win.set_title(Some("Tables"));
        win.set_default_size(900, 600);

        let header = suite_common::make_header_bar();
        let toolbar = suite_common::make_toolbar();
        let formula = gtk::Entry::new();
        formula.set_placeholder_text(Some("Formula…"));

        // Data model
        let mut data = vec![vec![String::new(); COLS]; ROWS];
        for c in 0..COLS { data[0][c] = (b'A' + c as u8).to_string(); }
        let model = std::cell::RefCell::new(data);

        let grid = gtk::DrawingArea::new();
        grid.set_can_focus(true);
        grid.set_focusable(true);
        grid.set_hexpand(true);
        grid.set_vexpand(true);
        let total_w = (COLS as f64 * CELL_W + ROW_HEADER_W) as i32;
        let total_h = (ROWS as f64 * CELL_H + CELL_H) as i32;
        grid.set_content_width(total_w);
        grid.set_content_height(total_h);

        let model_ref = model.clone();
        let sel = std::cell::Cell::new((0usize, 0usize));
        let sel_ref = sel.clone();
        grid.set_draw_func(move |_area, cr, w, h| {
            let data = model_ref.borrow();
            let (sr, sc) = sel_ref.get();
            // Background
            cr.set_source_rgb(1.0, 1.0, 1.0); cr.paint().unwrap();
            // Column headers
            cr.set_source_rgb(0.9, 0.9, 0.92);
            cr.rectangle(0.0, 0.0, w as f64, CELL_H); cr.fill().unwrap();
            cr.rectangle(0.0, 0.0, ROW_HEADER_W, h as f64); cr.fill().unwrap();
            // Grid lines + cells
            let vis_cols = ((w as f64 - ROW_HEADER_W) / CELL_W) as usize + 1;
            let vis_rows = ((h as f64 - CELL_H) / CELL_H) as usize + 1;
            cr.set_font_size(12.0);
            for r in 0..vis_rows {
                if r >= ROWS { break; }
                for c in 0..vis_cols {
                    if c >= COLS { break; }
                    let x = ROW_HEADER_W + c as f64 * CELL_W;
                    let y = CELL_H + r as f64 * CELL_H;
                    // Selection
                    if r == sr && c == sc { cr.set_source_rgba(0.3, 0.6, 0.9, 0.2); cr.rectangle(x, y, CELL_W, CELL_H); cr.fill().unwrap(); }
                    // Border
                    cr.set_source_rgb(0.85, 0.85, 0.85); cr.set_line_width(0.5);
                    cr.rectangle(x, y, CELL_W, CELL_H); cr.stroke().unwrap();
                    // Text
                    cr.set_source_rgb(0.1, 0.1, 0.1);
                    let txt = &data[r][c];
                    if !txt.is_empty() {
                        cr.move_to(x + 4.0, y + CELL_H - 7.0);
                        let _ = cr.show_text(&txt[..txt.len().min(12)]);
                    }
                }
            }
            // Column letters
            cr.set_source_rgb(0.2, 0.2, 0.2); cr.set_font_size(11.0);
            for c in 0..vis_cols {
                if c >= COLS { break; }
                cr.move_to(ROW_HEADER_W + c as f64 * CELL_W + CELL_W / 2.0 - 4.0, CELL_H - 6.0);
                let _ = cr.show_text(&(b'A' + c as u8).to_string());
            }
            // Row numbers
            for r in 0..vis_rows {
                if r >= ROWS { break; }
                cr.move_to(ROW_HEADER_W / 2.0 - 8.0, CELL_H + r as f64 * CELL_H + CELL_H / 2.0 + 4.0);
                let _ = cr.show_text(&(r + 1).to_string());
            }
        });

        // Click to select
        let sel2 = sel.clone();
        let click = gtk::GestureClick::new();
        let model2 = model.clone();
        click.connect_pressed(move |gesture, _n_press, x, y| {
            let col = ((x - ROW_HEADER_W) / CELL_W) as usize;
            let row = ((y - CELL_H) / CELL_H) as usize;
            if row < ROWS && col < COLS { sel2.set((row, col)); }
            gesture.widget().queue_draw();
        });
        grid.add_controller(click);

        // Keyboard navigation
        let sel3 = sel.clone();
        let grid_ref = grid.clone();
        let key = gtk::EventControllerKey::new();
        key.connect_key_pressed(move |_ctrl, keyval, _keycode, _state| {
            let (mut r, mut c) = sel3.get();
            match keyval.name() {
                "Left" => { if c > 0 { c -= 1; } }
                "Right" => { if c < COLS - 1 { c += 1; } }
                "Up" => { if r > 0 { r -= 1; } }
                "Down" => { if r < ROWS - 1 { r += 1; } }
                _ => return gtk::glib::Propagation::Proceed,
            }
            sel3.set((r, c));
            grid_ref.queue_draw();
            gtk::glib::Propagation::Stop
        });
        grid.add_controller(key);

        let scroll = gtk::ScrolledWindow::new();
        scroll.set_child(Some(&grid)); scroll.set_vexpand(true);

        let main = gtk::Box::new(gtk::Orientation::Vertical, 2);
        main.append(&toolbar); main.append(&formula); main.append(&scroll);

        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        container.append(&header); container.append(&main);
        win.set_child(Some(&container));

        Self { window: win, grid, model, selection: sel }
    }
    pub fn present(&self) { self.window.present(); }
}
