// SPDX-License-Identifier: GPL-3.0-or-later
//
// TablesWindow — Spreadsheet window with Cairo grid, formula bar, and sheet tabs.
// GNOME GUI spec: AdwApplicationWindow + AdwToolbarView + AdwBreakpoint.

use gtk4::cairo::{self, Context};
use gtk4::{self as gtk, gio, prelude::*};
use libadwaita as adw;
use std::cell::RefCell;
use std::rc::Rc;

// Grid constants
const DEFAULT_ROWS: usize = 100;
const DEFAULT_COLS: usize = 26;
const ROW_HEIGHT: f64 = 28.0;
const COL_WIDTH: f64 = 90.0;
const ROW_HEADER_WIDTH: f64 = 50.0;
const COL_HEADER_HEIGHT: f64 = 26.0;
const HEADER_BG: (f64, f64, f64) = (0.95, 0.95, 0.95);
const HEADER_BG_DARK: (f64, f64, f64) = (0.25, 0.25, 0.25);
const SELECTION_COLOR: (f64, f64, f64) = (0.21, 0.52, 0.89);
const ACTIVE_CELL_BORDER: (f64, f64, f64) = (0.0, 0.6, 0.0);

/// Per-sheet model: stores cell data
pub struct SheetModel {
    pub name: String,
    pub data: Vec<Vec<String>>,
    pub rows: usize,
    pub cols: usize,
    pub selected_row: usize,
    pub selected_col: usize,
}

impl SheetModel {
    pub fn new(name: &str, rows: usize, cols: usize) -> Self {
        SheetModel {
            name: name.to_string(),
            data: vec![vec![String::new(); cols]; rows],
            rows,
            cols,
            selected_row: 0,
            selected_col: 0,
        }
    }

    pub fn cell(&self, r: usize, c: usize) -> &str {
        if r < self.rows && c < self.cols { &self.data[r][c] } else { "" }
    }

    pub fn set_cell(&mut self, r: usize, c: usize, val: String) {
        if r < self.rows && c < self.cols { self.data[r][c] = val; }
    }
}

/// Shared mutable state for the window.
struct AppState {
    sheets: Vec<Rc<RefCell<SheetModel>>>,
    active_sheet: usize,
}

// ── Main window ────────────────────────────────────────────────────────

pub struct TablesWindow {
    pub window: adw::ApplicationWindow,
    drawing_area: gtk4::DrawingArea,
    h_adj: gtk4::Adjustment,
    v_adj: gtk4::Adjustment,
    fx_entry: gtk4::Entry,
    stack: gtk4::Stack,
}

impl TablesWindow {
    pub fn new(app: &adw::Application) -> Self {
        let state = Rc::new(RefCell::new(AppState {
            sheets: vec![Rc::new(RefCell::new(
                SheetModel::new("Sheet1", DEFAULT_ROWS, DEFAULT_COLS)))],
            active_sheet: 0,
        }));

        // ── Scrolling adjustments ───────────────────────────────────────
        let h_adj = gtk4::Adjustment::new(0.0, 0.0, 5000.0, 10.0, 50.0, 500.0);
        let v_adj = gtk4::Adjustment::new(0.0, 0.0, 5000.0, 10.0, 50.0, 500.0);

        // ── Drawing area ────────────────────────────────────────────────
        let drawing_area = gtk4::DrawingArea::new();
        drawing_area.set_vexpand(true);
        drawing_area.set_hexpand(true);

        let da_state = state.clone();
        let da_h = h_adj.clone();
        let da_v = v_adj.clone();

        drawing_area.set_draw_func(move |_da, cr, width, height| {
            draw_grid(cr, &da_state, width as f64, height as f64,
                      da_h.value(), da_v.value());
        });

        // Redraw on scroll
        h_adj.connect_value_changed({
            let da = drawing_area.clone();
            move |_| da.queue_draw()
        });
        v_adj.connect_value_changed({
            let da = drawing_area.clone();
            move |_| da.queue_draw()
        });

        // ── Formula bar ─────────────────────────────────────────────────
        let fx_label = gtk4::Label::new(Some(" fx"));
        fx_label.add_css_class("dim-label");
        fx_label.set_width_chars(5);
        let fx_entry = gtk4::Entry::new();
        fx_entry.set_hexpand(true);
        fx_entry.set_placeholder_text(Some("Formula or value\u{2026}"));

        let fx_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        fx_bar.set_margin_start(6);
        fx_bar.set_margin_end(6);
        fx_bar.set_margin_top(2);
        fx_bar.set_margin_bottom(2);
        fx_bar.set_halign(gtk4::Align::Fill);
        fx_bar.append(&fx_label);
        fx_bar.append(&fx_entry);

        // ── Sheet switcher at bottom ────────────────────────────────────
        let sheet_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        sheet_bar.set_margin_start(6);
        sheet_bar.set_margin_end(6);
        sheet_bar.set_margin_top(2);
        sheet_bar.set_margin_bottom(2);

        let add_btn = gtk4::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Add sheet")
            .build();
        add_btn.set_css_classes(&["flat", "circular"]);
        sheet_bar.append(&add_btn);

        let sheet_switcher = gtk4::DropDown::from_strings(&["Sheet1"]);
        sheet_switcher.set_tooltip_text(Some("Switch sheet"));
        sheet_bar.append(&sheet_switcher);

        // ── Grid content ────────────────────────────────────────────────
        let h_scroll = gtk4::Scrollbar::new(gtk4::Orientation::Horizontal, Some(&h_adj));
        let v_scroll = gtk4::Scrollbar::new(gtk4::Orientation::Vertical, Some(&v_adj));

        let scroll_grid = gtk4::Grid::new();
        scroll_grid.set_vexpand(true);
        scroll_grid.set_hexpand(true);
        scroll_grid.attach(&drawing_area, 0, 0, 1, 1);
        scroll_grid.attach(&v_scroll, 1, 0, 1, 1);
        scroll_grid.attach(&h_scroll, 0, 1, 1, 1);

        let grid_area = scroll_grid;

        // ── Content stack ───────────────────────────────────────────────
        let stack = gtk4::Stack::new();
        stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
        stack.set_transition_duration(200);

        let empty_page = suite_common::make_empty_state(
            "Tables",
            "Create a new spreadsheet or open an existing one",
            "x-office-spreadsheet-symbolic",
            "Open File\u{2026}",
        );
        stack.add_titled(&empty_page, Some("empty"), "Empty");
        stack.add_titled(&grid_area, Some("editor"), "Editor");
        stack.set_visible_child_name("empty");

        // ── Window chrome ───────────────────────────────────────────────
        let suite_win = suite_common::SuiteWindow::new(app, "Tables", vec![], vec![]);
        suite_win.add_top_bar(&fx_bar);
        suite_win.set_content(&stack);
        suite_win.add_bottom_bar(&sheet_bar);

        // ── Mouse click → cell selection ────────────────────────────────
        let state_click = state.clone();
        let h_click = h_adj.clone();
        let v_click = v_adj.clone();
        let da_click = drawing_area.clone();
        let fx_click = fx_entry.clone();

        let click = gtk4::GestureClick::new();
        click.connect_pressed(move |_g, _n, x, y| {
            let wx = x + h_click.value();
            let wy = y + v_click.value();
            if let Some((col, row)) = xy_to_cell(wx, wy) {
                let s = state_click.borrow();
                if let Some(sheet) = s.sheets.get(s.active_sheet) {
                    let mut sh = sheet.borrow_mut();
                    sh.selected_row = row;
                    sh.selected_col = col;
                    fx_click.set_text(&sh.cell(row, col));
                }
                da_click.queue_draw();
            }
        });
        drawing_area.add_controller(click);

        // ── Double-click → inline cell editor ───────────────────────────
        let state_edit = state.clone();
        let h_edit = h_adj.clone();
        let v_edit = v_adj.clone();
        let da_edit = drawing_area.clone();
        let fx_edit = fx_entry.clone();
        let grid_edit = grid_area.clone();

        let dbl = gtk4::GestureClick::new();
        dbl.set_button(1);
        dbl.set_touch_only(false);

        // We use pressed and check n_press for double-click
        let state_dbl = state_edit.clone();
        let h_dbl = h_edit.clone();
        let v_dbl = v_edit.clone();
        let da_dbl = da_edit.clone();
        let fx_dbl = fx_edit.clone();

        dbl.connect_pressed(move |g, n, x, y| {
            if n != 2 { return; }
            let wx = x + h_dbl.value();
            let wy = y + v_dbl.value();
            if let Some((col, row)) = xy_to_cell(wx, wy) {
                let s = state_dbl.borrow();
                if let Some(sheet) = s.sheets.get(s.active_sheet) {
                    let sh = sheet.borrow();

                    // Position overlay entry
                    let entry = gtk4::Entry::new();
                    entry.set_text(sh.cell(row, col));
                    entry.set_has_frame(true);
                    entry.set_width_chars(12);

                    let (margin_l, margin_t) = (
                        (ROW_HEADER_WIDTH + col as f64 * COL_WIDTH - h_dbl.value()) as i32,
                        (COL_HEADER_HEIGHT + row as f64 * ROW_HEIGHT - v_dbl.value()) as i32,
                    );
                    entry.set_margin_start(margin_l.max(0));
                    entry.set_margin_top(margin_t.max(0));
                    entry.set_size_request(COL_WIDTH as i32 - 4, ROW_HEIGHT as i32 - 2);

                    let da_q = da_dbl.clone();
                    let fx_q = fx_dbl.clone();
                    let state_q = state_dbl.clone();

                    entry.connect_activate(move |e| {
                        let val = e.text().to_string();
                        {
                            let s = state_q.borrow();
                            if let Some(sheet) = s.sheets.get(s.active_sheet) {
                                let mut sh = sheet.borrow_mut();
                                sh.set_cell(row, col, val.clone());
                                fx_q.set_text(&val);
                            }
                        }
                        da_q.queue_draw();
                        if let Some(parent) = e.parent() {
                            if let Ok(box_) = parent.downcast::<gtk4::Box>() {
                                box_.remove(e);
                            }
                        }
                    });

                    // Place entry as overlay — append to parent grid area
                    if let Some(parent) = da_dbl.parent() {
                        if let Ok(box_) = parent.downcast::<gtk4::Box>() {
                            box_.append(&entry);
                        }
                    }
                }
            }
        });
        drawing_area.add_controller(dbl);

        // ── Keyboard: arrow keys move selection ─────────────────────────
        let state_key = state.clone();
        let da_key = drawing_area.clone();
        let fx_key = fx_entry.clone();

        let key = gtk4::EventControllerKey::new();
        key.connect_key_pressed(move |_k, keyval, _code, _mods| {
            let mut s = state_key.borrow_mut();
            if let Some(sheet) = s.sheets.get(s.active_sheet) {
                let mut sh = sheet.borrow_mut();
                match keyval {
                    gtk4::gdk::Key::Left if sh.selected_col > 0 => sh.selected_col -= 1,
                    gtk4::gdk::Key::Right => sh.selected_col += 1,
                    gtk4::gdk::Key::Up if sh.selected_row > 0 => sh.selected_row -= 1,
                    gtk4::gdk::Key::Down => sh.selected_row += 1,
                    _ => return gtk4::glib::Propagation::Proceed,
                }
                fx_key.set_text(&sh.cell(sh.selected_row, sh.selected_col));
                da_key.queue_draw();
            }
            gtk4::glib::Propagation::Stop
        });
        drawing_area.add_controller(key);

        // ── App actions ─────────────────────────────────────────────────
        let st = stack.clone();
        let act = gtk4::gio::SimpleAction::new("open-file", None);
        act.connect_activate(move |_, _| st.set_visible_child_name("editor"));
        app.add_action(&act);

        let st = stack.clone();
        let act = gtk4::gio::SimpleAction::new("new-document", None);
        act.connect_activate(move |_, _| st.set_visible_child_name("editor"));
        app.add_action(&act);

        // ── Copy/Paste TSV clipboard (cross-app table exchange) ──
        {
            let ss = state.clone();
            let g = drawing_area.clone();
            let a = gtk4::gio::SimpleAction::new("copy", None);
            a.connect_activate(move |_, _| {
                let app_state = ss.borrow();
                let sheet = app_state.sheets[app_state.active_sheet].borrow();
                // Copy currently visible range (full grid for now)
                let mut tsv = String::new();
                for r in 0..sheet.rows {
                    for c in 0..sheet.cols {
                        if c > 0 { tsv.push('\t'); }
                        tsv.push_str(sheet.cell(r, c));
                    }
                    tsv.push('\n');
                }
                if let Some(display) = gtk4::gdk::Display::default() {
                    display.clipboard().set_text(&tsv);
                }
            });
            app.add_action(&a);
            app.set_accels_for_action("app.copy", &["<Primary>c"]);
        }

        {
            let ss = state.clone();
            let g = drawing_area.clone();
            let a = gtk4::gio::SimpleAction::new("paste", None);
            a.connect_activate(move |_, _| {
                if let Some(display) = gtk4::gdk::Display::default() {
                    let ss = ss.clone();
                    let g = g.clone();
                    display.clipboard().read_text_async(
                        None::<&gtk4::gio::Cancellable>,
                        move |result| {
                            if let Ok(Some(text)) = result {
                                let mut app_state = ss.borrow_mut();
                                let sheet = app_state.sheets[app_state.active_sheet].clone();
                                drop(app_state);
                                let mut sh = sheet.borrow_mut();
                                for (dr, line) in text.lines().enumerate() {
                                    for (dc, cell_val) in line.split('\t').enumerate() {
                                        let r = dr;
                                        let c = dc;
                                        sh.set_cell(r, c, cell_val.to_string());
                                    }
                                }
                                g.queue_draw();
                            }
                        },
                    );
                }
            });
            app.add_action(&a);
            app.set_accels_for_action("app.paste", &["<Primary>v"]);
        }

        Self { window: suite_win.window, drawing_area, h_adj, v_adj, fx_entry, stack }
    }

    pub fn present(&self) { self.window.present(); }
}

// ── Helpers ────────────────────────────────────────────────────────────

fn xy_to_cell(x: f64, y: f64) -> Option<(usize, usize)> {
    if x < ROW_HEADER_WIDTH || y < COL_HEADER_HEIGHT { return None; }
    let col = ((x - ROW_HEADER_WIDTH) / COL_WIDTH) as usize;
    let row = ((y - COL_HEADER_HEIGHT) / ROW_HEIGHT) as usize;
    Some((col, row))
}

fn grid_line_color() -> (f64, f64, f64) {
    let is_dark = adw::StyleManager::default().is_dark();
    if is_dark { (0.3, 0.3, 0.3) } else { (0.82, 0.82, 0.82) }
}

fn is_dark_theme() -> bool {
    adw::StyleManager::default().is_dark()
}

// ── Grid drawing ───────────────────────────────────────────────────────

fn draw_grid(
    cr: &Context,
    state: &Rc<RefCell<AppState>>,
    width: f64,
    height: f64,
    scroll_x: f64,
    scroll_y: f64,
) {
    let dark = is_dark_theme();
    let (bg_r, bg_g, bg_b) = if dark { (0.12, 0.12, 0.12) } else { (1.0, 1.0, 1.0) };
    let (hdr_r, hdr_g, hdr_b) = if dark { HEADER_BG_DARK } else { HEADER_BG };
    let (gl_r, gl_g, gl_b) = grid_line_color();

    cr.set_source_rgb(bg_r, bg_g, bg_b);
    cr.paint().unwrap();

    let s = state.borrow();
    let sheet = match s.sheets.get(s.active_sheet) {
        Some(sh) => sh,
        None => return,
    };
    let sh = sheet.borrow();

    let start_col = (scroll_x / COL_WIDTH) as usize;
    let end_col = ((scroll_x + width) / COL_WIDTH) as usize + 1;
    let start_row = (scroll_y / ROW_HEIGHT) as usize;
    let end_row = ((scroll_y + height) / ROW_HEIGHT) as usize + 1;

    // Column header background
    cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
    cr.rectangle(ROW_HEADER_WIDTH, 0.0, width - ROW_HEADER_WIDTH, COL_HEADER_HEIGHT);
    cr.fill().unwrap();

    // Column headers (A, B, C...)
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(11.0);
    for col in start_col..=end_col.min(25) {
        let x = ROW_HEADER_WIDTH + col as f64 * COL_WIDTH - scroll_x;
        if x > width { break; }
        cr.set_source_rgb(gl_r, gl_g, gl_b);
        cr.set_line_width(0.5);
        cr.rectangle(x, 0.0, COL_WIDTH, COL_HEADER_HEIGHT);
        cr.stroke().unwrap();
        let letter = std::char::from_u32((b'A' + col as u8) as u32).unwrap_or('?');
        cr.set_source_rgb(0.3, 0.3, 0.3);
        cr.move_to(x + 6.0, COL_HEADER_HEIGHT - 7.0);
        cr.show_text(&letter.to_string()).unwrap();
    }

    // Row header background
    cr.set_source_rgb(hdr_r, hdr_g, hdr_b);
    cr.rectangle(0.0, COL_HEADER_HEIGHT, ROW_HEADER_WIDTH, height - COL_HEADER_HEIGHT);
    cr.fill().unwrap();

    for row in start_row..=end_row.min(sh.rows - 1) {
        let y = COL_HEADER_HEIGHT + row as f64 * ROW_HEIGHT - scroll_y;
        if y > height { break; }
        cr.set_source_rgb(gl_r, gl_g, gl_b);
        cr.set_line_width(0.5);
        cr.rectangle(0.0, y, ROW_HEADER_WIDTH, ROW_HEIGHT);
        cr.stroke().unwrap();
        cr.set_source_rgb(0.3, 0.3, 0.3);
        cr.move_to(ROW_HEADER_WIDTH - 8.0, y + ROW_HEIGHT - 7.0);
        cr.show_text(&(row + 1).to_string()).unwrap();
    }

    // Cells
    for row in start_row..=end_row.min(sh.rows - 1) {
        for col in start_col..=end_col.min(sh.cols - 1) {
            let x = ROW_HEADER_WIDTH + col as f64 * COL_WIDTH - scroll_x;
            let y = COL_HEADER_HEIGHT + row as f64 * ROW_HEIGHT - scroll_y;
            if x > width || y > height { continue; }

            if row == sh.selected_row && col == sh.selected_col {
                cr.set_source_rgb(0.9, 0.95, 1.0);
            } else {
                cr.set_source_rgb(bg_r, bg_g, bg_b);
            }
            cr.rectangle(x, y, COL_WIDTH, ROW_HEIGHT);
            cr.fill().unwrap();

            let val = sh.cell(row, col);
            if !val.is_empty() {
                cr.set_source_rgb(0.1, 0.1, 0.1);
                cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
                cr.set_font_size(12.0);
                cr.move_to(x + 4.0, y + ROW_HEIGHT - 7.0);
                cr.show_text(val).unwrap();
            }

            cr.set_source_rgb(gl_r, gl_g, gl_b);
            cr.set_line_width(0.5);
            cr.rectangle(x, y, COL_WIDTH, ROW_HEIGHT);
            cr.stroke().unwrap();
        }
    }

    // Selection highlight
    let sx = ROW_HEADER_WIDTH + sh.selected_col as f64 * COL_WIDTH - scroll_x;
    let sy = COL_HEADER_HEIGHT + sh.selected_row as f64 * ROW_HEIGHT - scroll_y;
    let (r, g, b) = ACTIVE_CELL_BORDER;
    cr.set_source_rgb(r, g, b);
    cr.set_line_width(2.5);
    cr.rectangle(sx, sy, COL_WIDTH, ROW_HEIGHT);
    cr.stroke().unwrap();

    // Selection color fill with alpha
    cr.set_source_rgba(SELECTION_COLOR.0, SELECTION_COLOR.1, SELECTION_COLOR.2, 0.15);
    cr.rectangle(sx, sy, COL_WIDTH, ROW_HEIGHT);
    cr.fill().unwrap();
}
