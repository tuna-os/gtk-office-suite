// SPDX-License-Identifier: GPL-3.0-or-later
//
// TablesWindow — Spreadsheet window with Cairo grid, formula bar, sheet tabs, and file I/O.
// GNOME GUI spec: AdwApplicationWindow + AdwToolbarView + AdwBreakpoint.

use gtk4::cairo::{self, Context};
use gtk4::{self as gtk, gio, glib, prelude::*};
use libadwaita as adw;
use adw::prelude::{AdwDialogExt, AlertDialogExt};
use std::cell::RefCell;
use std::rc::Rc;

use crate::engine::TablesEngine;

// ── Constants ──────────────────────────────────────────────────────────
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
const GRID_LINE: (f64, f64, f64) = (0.85, 0.85, 0.85);

// ── Column label helper ────────────────────────────────────────────────
fn col_label(c: usize) -> String {
    let mut n = c;
    let mut s = String::new();
    loop {
        let d = (n % 26) as u8;
        s.insert(0, (b'A' + d) as char);
        n /= 26;
        if n == 0 { break; }
        n -= 1;
    }
    s
}

// ── Per-sheet display model (Cairo rendering cache) ────────────────────
#[derive(Clone)]
pub struct SheetModel {
    pub name: String,
    pub data: Vec<Vec<String>>,
    pub rows: usize,
    pub cols: usize,
    pub selected_row: usize,
    pub selected_col: usize,
    pub col_widths: Vec<f64>,
    pub formulas: Vec<Vec<bool>>, // tracks which cells have formulas
    engine_idx: usize,            // index into the IronCalc workbook
}

impl SheetModel {
    pub fn new(name: &str, rows: usize, cols: usize, engine_idx: usize) -> Self {
        SheetModel {
            name: name.to_string(),
            data: vec![vec![String::new(); cols]; rows],
            rows, cols,
            selected_row: 0, selected_col: 0,
            col_widths: vec![COL_WIDTH; cols],
            formulas: vec![vec![false; cols]; rows],
            engine_idx,
        }
    }

    pub fn cell(&self, r: usize, c: usize) -> &str {
        if r < self.rows && c < self.cols { &self.data[r][c] } else { "" }
    }

    pub fn set_cell(&mut self, r: usize, c: usize, val: String) {
        if r < self.rows && c < self.cols { self.data[r][c] = val; }
    }

    pub fn is_formula(&self, r: usize, c: usize) -> bool {
        r < self.rows && c < self.cols && self.formulas[r][c]
    }

    pub fn col_width(&self, c: usize) -> f64 {
        if c < self.col_widths.len() { self.col_widths[c] } else { COL_WIDTH }
    }

    pub fn set_col_width(&mut self, c: usize, w: f64) {
        if c < self.col_widths.len() { self.col_widths[c] = w.max(30.0).min(500.0); }
    }

    /// Sync display data from the given IronCalc engine.
    pub fn sync_from_engine(&mut self, engine: &TablesEngine) {
        for r in 0..self.rows.min(engine.rows) {
            for c in 0..self.cols.min(engine.cols) {
                self.data[r][c] = engine.cell(r, c);
                self.formulas[r][c] = engine.has_formula(r, c);
            }
        }
    }
}

// ── Shared state ─────────────────────────────────────────────────────
struct AppState {
    sheets: Vec<Rc<RefCell<SheetModel>>>,
    active_sheet: usize,
    engine: TablesEngine,
}

impl AppState {
    fn sheet(&self) -> std::cell::Ref<SheetModel> {
        self.sheets[self.active_sheet].borrow()
    }

    fn sheet_mut(&self) -> std::cell::RefMut<SheetModel> {
        self.sheets[self.active_sheet].borrow_mut()
    }

    fn grid_data(&self) -> Vec<Vec<String>> {
        let mut grid = Vec::new();
        let s = self.sheet();
        for r in 0..s.rows {
            let mut row = Vec::new();
            for c in 0..s.cols {
                row.push(s.data[r][c].clone());
            }
            grid.push(row);
        }
        grid
    }
}

// ── File I/O helpers ────────────────────────────────────────────────────

/// Load spreadsheet file into the engine. Returns (rows, cols) or error string.
fn load_file_into_engine(path: &str, engine: &mut TablesEngine) -> Result<(usize, usize), String> {
    let ext = std::path::Path::new(path)
        .extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    match ext.as_str() {
        "xlsx" | "xls" | "xlsm" | "xlsb" => {
            use calamine::{open_workbook, Reader, Xlsx};
            let mut wb: Xlsx<_> = open_workbook(path)
                .map_err(|e| format!("Cannot open file: {}", e))?;

            let sheet_names = wb.sheet_names().to_vec();
            if sheet_names.is_empty() { return Err("No sheets found".into()); }

            let range = wb.worksheet_range(&sheet_names[0])
                .map_err(|e| format!("Cannot read sheet: {}", e))?;
            let (rows, cols) = (range.height(), range.width());

            for (r, row) in range.rows().enumerate() {
                for (c, cell) in row.iter().enumerate() {
                    let val = match cell {
                        calamine::Data::String(s) => s.clone(),
                        calamine::Data::Float(f) => f.to_string(),
                        calamine::Data::Int(i) => i.to_string(),
                        calamine::Data::Bool(b) => b.to_string(),
                        calamine::Data::DateTime(d) => d.to_string(),
                        calamine::Data::Error(e) => format!("#{}", e),
                        _ => String::new(),
                    };
                    engine.set_cell_text(r, c, &val);
                }
            }
            engine.evaluate();
            Ok((rows.max(1), cols.max(1)))
        }
        "ods" => {
            use calamine::{open_workbook, Reader, Ods};
            let mut wb: Ods<_> = open_workbook(path)
                .map_err(|e| format!("Cannot open file: {}", e))?;
            let sheet_names = wb.sheet_names().to_vec();
            if sheet_names.is_empty() { return Err("No sheets found".into()); }
            let range = wb.worksheet_range(&sheet_names[0])
                .map_err(|e| format!("Cannot read sheet: {}", e))?;
            let (rows, cols) = (range.height(), range.width());
            for (r, row) in range.rows().enumerate() {
                for (c, cell) in row.iter().enumerate() {
                    let val = format!("{:?}", cell);
                    engine.set_cell_text(r, c, &val);
                }
            }
            engine.evaluate();
            Ok((rows.max(1), cols.max(1)))
        }
        "csv" | "tsv" => {
            let content = std::fs::read_to_string(path)
                .map_err(|e| format!("Cannot read file: {}", e))?;
            let delim = if ext == "tsv" { '\t' } else { ',' };
            let mut max_rows = 0usize;
            let mut max_cols = 0usize;
            for (r, line) in content.lines().enumerate() {
                let cols: Vec<&str> = line.split(delim).collect();
                max_cols = max_cols.max(cols.len());
                for (c, val) in cols.iter().enumerate() {
                    let trimmed = val.trim().trim_matches('"');
                    engine.set_cell_text(r, c, trimmed);
                }
                max_rows = r + 1;
            }
            engine.evaluate();
            Ok((max_rows.max(1), max_cols.max(1)))
        }
        _ => Err(format!("Unsupported format: .{}", ext)),
    }
}

/// Save engine data to XLSX file.
fn save_engine_to_xlsx(path: &str, state: &AppState) -> Result<(), String> {
    use rust_xlsxwriter::*;
    let mut workbook = Workbook::new();
    for (i, sheet_rc) in state.sheets.iter().enumerate() {
        let sh = sheet_rc.borrow();
        let ws_name = &sh.name;
        let sheet = if i == 0 {
            workbook.add_worksheet()
        } else {
            workbook.add_worksheet()
        };
        sheet.set_name(ws_name).map_err(|e| format!("Sheet name: {}", e))?;
        for r in 0..sh.rows {
            for c in 0..sh.cols {
                let val = &sh.data[r][c];
                if val.is_empty() { continue; }
                // Try number first
                if let Ok(n) = val.parse::<f64>() {
                    sheet.write_number(r as u32, c as u16, n)
                        .map_err(|e| format!("Write error: {}", e))?;
                } else {
                    sheet.write_string(r as u32, c as u16, val)
                        .map_err(|e| format!("Write error: {}", e))?;
                }
            }
        }
    }
    workbook.save(path).map_err(|e| format!("Save error: {}", e))?;
    Ok(())
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
        let engine = TablesEngine::new(DEFAULT_ROWS, DEFAULT_COLS)
            .expect("Failed to create spreadsheet engine");

        let sheet = SheetModel::new("Sheet1", DEFAULT_ROWS, DEFAULT_COLS, 0);
        let state = Rc::new(RefCell::new(AppState {
            sheets: vec![Rc::new(RefCell::new(sheet))],
            active_sheet: 0,
            engine,
        }));

        // ── Scrolling ──────────────────────────────────────────────────
        let h_adj = gtk4::Adjustment::new(0.0, 0.0, 5000.0, 10.0, 50.0, 500.0);
        let v_adj = gtk4::Adjustment::new(0.0, 0.0, 5000.0, 10.0, 50.0, 500.0);

        // ── Drawing area ────────────────────────────────────────────────
        let drawing_area = gtk4::DrawingArea::new();
        drawing_area.set_vexpand(true);
        drawing_area.set_hexpand(true);

        {
            let da_state = state.clone();
            let da_h = h_adj.clone();
            let da_v = v_adj.clone();
            drawing_area.set_draw_func(move |_da, cr, width, height| {
                draw_grid(cr, &da_state, width as f64, height as f64,
                          da_h.value(), da_v.value());
            });
        }

        h_adj.connect_value_changed({ let da = drawing_area.clone(); move |_| da.queue_draw() });
        v_adj.connect_value_changed({ let da = drawing_area.clone(); move |_| da.queue_draw() });

        // ── Formula bar ─────────────────────────────────────────────────
        let fx_label = gtk4::Label::new(Some(" fx"));
        fx_label.add_css_class("dim-label");
        fx_label.set_width_chars(5);
        let fx_entry = gtk4::Entry::new();
        fx_entry.set_hexpand(true);
        fx_entry.set_placeholder_text(Some("Formula or value\u{2026}"));

        let fx_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        fx_bar.set_margin_start(6); fx_bar.set_margin_end(6);
        fx_bar.set_margin_top(2); fx_bar.set_margin_bottom(2);
        fx_bar.set_halign(gtk4::Align::Fill);
        fx_bar.append(&fx_label);
        fx_bar.append(&fx_entry);

        // Wire formula bar: Enter commits
        {
            let s = state.clone();
            let da = drawing_area.clone();
            let fx = fx_entry.clone();
            fx_entry.connect_activate(move |_| {
                let val = fx.text().to_string();
                let mut st = s.borrow_mut();
                let r = st.sheet().selected_row;
                let c = st.sheet().selected_col;
                // Update both engine and display cache
                st.engine.set_cell_text(r, c, &val);
                {
                    let mut sh = st.sheets[st.active_sheet].borrow_mut();
                    sh.data[r][c] = val.clone();
                    sh.formulas[r][c] = val.starts_with('=');
                }
                // Sync engine results back to display
                st.sheet_mut().sync_from_engine(&st.engine);
                da.queue_draw();
            });
        }

        // ── Mouse: single-click select ──────────────────────────────────
        {
            let s = state.clone();
            let da = drawing_area.clone();
            let fx = fx_entry.clone();
            let h = h_adj.clone();
            let v = v_adj.clone();
            let click = gtk4::GestureClick::new();
            click.connect_pressed(move |_g, _n, x, y| {
                let wx = x + h.value();
                let wy = y + v.value();
                if let Some((col, row)) = xy_to_cell(wx, wy) {
                    let mut st = s.borrow_mut();
                    let mut sh = st.sheet_mut();
                    sh.selected_row = row;
                    sh.selected_col = col;
                    // Show cell content in formula bar
                    let val = &sh.data[row][col];
                    if sh.is_formula(row, col) {
                        // For formulas, we'd need to retrieve the formula string
                        fx.set_text(val);
                    } else {
                        fx.set_text(val);
                    }
                    da.queue_draw();
                }
            });
            drawing_area.add_controller(click);
        }

        // ── Double-click: inline edit ───────────────────────────────────
        {
            let s = state.clone();
            let da = drawing_area.clone();
            let h = h_adj.clone();
            let v = v_adj.clone();
            let dbl = gtk4::GestureClick::new();
            dbl.set_button(1);
            dbl.set_touch_only(false);
            dbl.connect_pressed(move |_g, n, x, y| {
                if n < 2 { return; }
                let wx = x + h.value();
                let wy = y + v.value();
                if let Some((col, row)) = xy_to_cell(wx, wy) {
                    let mut st = s.borrow_mut();
                    let val = st.sheet().data[row][col].clone();
                    drop(st);
                    // Position entry overlay at cell
                    let sx = ROW_HEADER_WIDTH + col as f64 * COL_WIDTH - h.value();
                    let sy = COL_HEADER_HEIGHT + row as f64 * ROW_HEIGHT - v.value();
                    let entry = gtk4::Entry::new();
                    entry.set_text(&val);
                    entry.set_size_request(COL_WIDTH as i32 - 4, ROW_HEIGHT as i32 - 2);
                    let overlay = gtk4::Fixed::new();
                    overlay.put(&entry, sx, sy);
                    entry.grab_focus();
                    entry.select_region(0, -1);

                    let s2 = s.clone();
                    let da2 = da.clone();
                    entry.connect_activate(move |e| {
                        let new_val = e.text().to_string();
                        let mut st = s2.borrow_mut();
                        let r = row; let c = col;
                        st.engine.set_cell_text(r, c, &new_val);
                        {
                            let mut sh = st.sheet_mut();
                            sh.data[r][c] = new_val.clone();
                            sh.formulas[r][c] = new_val.starts_with('=');
                        }
                        st.sheet_mut().sync_from_engine(&st.engine);
                        e.parent().map(|p| { p.unparent(); });
                        da2.queue_draw();
                    });
                    let focus_ctrl = gtk4::EventControllerFocus::new();
                    let e2 = entry.clone();
                    focus_ctrl.connect_leave(move |_| {
                        e2.parent().map(|p| { p.unparent(); });
                    });
                    entry.add_controller(focus_ctrl);
                }
            });
            drawing_area.add_controller(dbl);
        }

        // ── Sheet switcher (dynamic) ────────────────────────────────────
        let sheet_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        sheet_bar.set_margin_start(6); sheet_bar.set_margin_end(6);
        sheet_bar.set_margin_top(2); sheet_bar.set_margin_bottom(2);

        let sheet_model = gtk4::StringList::new(&["Sheet1"]);
        let sheet_switcher = gtk4::DropDown::builder()
            .model(&sheet_model)
            .tooltip_text("Switch sheet")
            .build();
        sheet_bar.append(&sheet_switcher);

        let add_btn = gtk4::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("Add sheet")
            .build();
        add_btn.set_css_classes(&["flat", "circular"]);
        sheet_bar.append(&add_btn);

        // Add sheet action
        {
            let s = state.clone();
            let sm = sheet_model.clone();
            let sd = sheet_switcher.clone();
            let da = drawing_area.clone();
            add_btn.connect_clicked(move |_| {
                let mut st = s.borrow_mut();
                let idx = st.sheets.len();
                let name = format!("Sheet{}", idx + 1);
                let sheet = SheetModel::new(&name, DEFAULT_ROWS, DEFAULT_COLS, idx);
                let rc = Rc::new(RefCell::new(sheet));
                st.sheets.push(rc);
                st.active_sheet = idx;
                sm.append(&name);
                sd.set_selected(idx as u32);
                da.queue_draw();
            });
        }

        // Switch sheet
        {
            let s = state.clone();
            let da = drawing_area.clone();
            let fx = fx_entry.clone();
            sheet_switcher.connect_selected_notify(move |dd| {
                let idx = dd.selected() as usize;
                let mut st = s.borrow_mut();
                if idx < st.sheets.len() {
                    st.active_sheet = idx;
                    // Also sync engine state
                    st.sheet_mut().sync_from_engine(&st.engine);
                    fx.set_text("");
                    da.queue_draw();
                }
            });
        }

        // ── Content stack ───────────────────────────────────────────────
        let scroll_grid = gtk4::Grid::new();
        scroll_grid.set_vexpand(true);
        scroll_grid.set_hexpand(true);
        let h_scroll = gtk4::Scrollbar::new(gtk4::Orientation::Horizontal, Some(&h_adj));
        let v_scroll = gtk4::Scrollbar::new(gtk4::Orientation::Vertical, Some(&v_adj));
        scroll_grid.attach(&drawing_area, 0, 0, 1, 1);
        scroll_grid.attach(&v_scroll, 1, 0, 1, 1);
        scroll_grid.attach(&h_scroll, 0, 1, 1, 1);

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
        stack.add_titled(&scroll_grid, Some("editor"), "Editor");
        stack.set_visible_child_name("empty");

        // Actions for toolbars
        let s_bar = state.clone();
        let show_bar_chart = Box::new({
            let s = s_bar.clone();
            move || {
                let st = s.borrow();
                let active = st.active_sheet;
                let sheet = st.sheets[active].borrow();
                let col = sheet.selected_col;
                let mut data = Vec::new();
                for r in 0..sheet.rows {
                    let label = sheet.data[r][0].clone();
                    let val_str = &sheet.data[r][col];
                    if let Ok(val) = val_str.parse::<f64>() {
                        let lbl = if label.is_empty() { format!("Row {}", r + 1) } else { label };
                        data.push((lbl, val));
                    }
                }
                if data.is_empty() {
                    return;
                }
                let win = gtk4::Window::builder()
                    .title("Bar Chart Preview")
                    .default_width(600)
                    .default_height(400)
                    .modal(true)
                    .build();
                let surface = crate::charts::render_chart(&data, crate::charts::ChartType::Bar, 600, 400);
                let drawing_area = gtk4::DrawingArea::new();
                drawing_area.set_draw_func(move |_, cr, _, _| {
                    cr.set_source_surface(&surface, 0.0, 0.0).unwrap();
                    cr.paint().unwrap();
                });
                win.set_child(Some(&drawing_area));
                win.present();
            }
        });

        let s_pie = state.clone();
        let show_pie_chart = Box::new({
            let s = s_pie.clone();
            move || {
                let st = s.borrow();
                let active = st.active_sheet;
                let sheet = st.sheets[active].borrow();
                let col = sheet.selected_col;
                let mut data = Vec::new();
                for r in 0..sheet.rows {
                    let label = sheet.data[r][0].clone();
                    let val_str = &sheet.data[r][col];
                    if let Ok(val) = val_str.parse::<f64>() {
                        let lbl = if label.is_empty() { format!("Row {}", r + 1) } else { label };
                        data.push((lbl, val));
                    }
                }
                if data.is_empty() {
                    return;
                }
                let win = gtk4::Window::builder()
                    .title("Pie Chart Preview")
                    .default_width(600)
                    .default_height(400)
                    .modal(true)
                    .build();
                let surface = crate::charts::render_chart(&data, crate::charts::ChartType::Pie, 600, 400);
                let drawing_area = gtk4::DrawingArea::new();
                drawing_area.set_draw_func(move |_, cr, _, _| {
                    cr.set_source_surface(&surface, 0.0, 0.0).unwrap();
                    cr.paint().unwrap();
                });
                win.set_child(Some(&drawing_area));
                win.present();
            }
        });

        let s_pdf = state.clone();
        let win_ref = Rc::new(RefCell::new(None));
        let export_pdf = Box::new({
            let s = s_pdf.clone();
            let wr = win_ref.clone();
            move || {
                let parent_win = wr.borrow().clone();
                let s2 = s.clone();
                let dlg = gtk4::FileDialog::new();
                let f = gtk4::FileFilter::new();
                f.add_pattern("*.pdf");
                f.set_name(Some("PDF Documents"));
                let fl = gio::ListStore::new::<gtk4::FileFilter>();
                fl.append(&f);
                dlg.set_filters(Some(&fl));
                dlg.set_initial_name(Some("Spreadsheet.pdf"));
                
                let wr2 = wr.clone();
                dlg.save(parent_win.as_ref(), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                let path_str = path.to_string_lossy().to_string();
                                let mut st = s2.borrow_mut();
                                // Sync sheet data to engine first
                                let active = st.active_sheet;
                                let sheet_model = st.sheets[active].borrow().clone();
                                for r in 0..sheet_model.rows {
                                    for c in 0..sheet_model.cols {
                                        st.engine.set_cell_text(r, c, &sheet_model.data[r][c]);
                                    }
                                }
                                st.engine.evaluate();
                                let parent_win = wr2.borrow().clone();
                                if let Err(err_msg) = crate::export::to_pdf(&st.engine, &path_str) {
                                    let alert = adw::AlertDialog::builder()
                                        .heading("Export Failed")
                                        .body(&err_msg)
                                        .build();
                                    alert.add_response("ok", "OK");
                                    alert.present(parent_win.as_ref());
                                } else {
                                    println!("PDF exported successfully to {}", path_str);
                                }
                            }
                        }
                    },
                );
            }
        });

        let extended_toolbar: Vec<(&'static str, &'static str, Box<dyn Fn() + 'static>)> = vec![
            ("insert-object-symbolic", "Show Bar Chart", show_bar_chart),
            ("insert-image-symbolic", "Show Pie Chart", show_pie_chart),
            ("document-send-symbolic", "Export PDF", export_pdf),
        ];

        let suite_win = suite_common::SuiteWindow::new(app, "Tables", vec![], extended_toolbar);
        *win_ref.borrow_mut() = Some(suite_win.window.clone());

        suite_win.add_top_bar(&fx_bar);
        suite_win.set_content(&stack);
        suite_win.add_bottom_bar(&sheet_bar);

        // ── App actions ─────────────────────────────────────────────────
        let st = stack.clone();
        let act = gtk4::gio::SimpleAction::new("open-file", None);
        act.connect_activate(move |_, _| st.set_visible_child_name("editor"));
        app.add_action(&act);

        let st = stack.clone();
        let act = gtk4::gio::SimpleAction::new("new-document", None);
        act.connect_activate(move |_, _| st.set_visible_child_name("editor"));
        app.add_action(&act);

        // File Open action
        {
            let s = state.clone();
            let st = stack.clone();
            let w = suite_win.window.clone();
            let fx = fx_entry.clone();
            let da = drawing_area.clone();
            let sm = sheet_model.clone();
            let sd = sheet_switcher.clone();
            let act = gtk4::gio::SimpleAction::new("open-file-dialog", None);
            act.connect_activate(move |_, _| {
                let dlg = gtk4::FileDialog::new();
                let f = gtk4::FileFilter::new();
                f.add_pattern("*.xlsx"); f.add_pattern("*.xls");
                f.add_pattern("*.ods"); f.add_pattern("*.csv");
                f.set_name(Some("Spreadsheets"));
                let fl = gio::ListStore::new::<gtk4::FileFilter>();
                fl.append(&f);
                dlg.set_filters(Some(&fl));
                let s = s.clone(); let st = st.clone();
                let w2 = w.clone(); let fx = fx.clone();
                let da = da.clone(); let sm = sm.clone(); let sd = sd.clone();
                dlg.open(Some(&w), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                let path_str = path.to_string_lossy().to_string();
                                match load_file_into_engine(&path_str, &mut s.borrow_mut().engine) {
                                    Ok((rows, cols)) => {
                                        let mut ss = s.borrow_mut();
                                        // Replace with loaded data
                                        let mut sheet = SheetModel::new(
                                            "Sheet1", rows.max(DEFAULT_ROWS),
                                            cols.max(DEFAULT_COLS), 0);
                                        sheet.sync_from_engine(&ss.engine);
                                        ss.sheets.clear();
                                        ss.sheets.push(Rc::new(RefCell::new(sheet)));
                                        ss.active_sheet = 0;
                                        // Update sheet switcher
                                        sm.splice(0, sm.n_items(), &[]);
                                        sm.append("Sheet1");
                                        sd.set_selected(0);
                                        fx.set_text("");
                                        st.set_visible_child_name("editor");
                                        da.queue_draw();
                                    }
                                    Err(e) => {
                                        let err = adw::AlertDialog::builder()
                                            .heading("Error opening file")
                                            .body(&e)
                                            .build();
                                        err.add_response("ok", "OK");
                                        err.set_default_response(Some("ok"));
                                        err.present(Some(&w2));
                                    }
                                }
                            }
                        }
                    },
                );
            });
            app.add_action(&act);
        }

        // File Save action
        {
            let s = state.clone();
            let w = suite_win.window.clone();
            let act = gtk4::gio::SimpleAction::new("save-file-dialog", None);
            act.connect_activate(move |_, _| {
                let dlg = gtk4::FileDialog::new();
                let f = gtk4::FileFilter::new();
                f.add_suffix("xlsx");
                f.set_name(Some("Excel Workbook (.xlsx)"));
                let fl = gio::ListStore::new::<gtk4::FileFilter>();
                fl.append(&f);
                dlg.set_filters(Some(&fl));
                dlg.set_initial_name(Some("Untitled.xlsx"));
                let s = s.clone(); let w2 = w.clone();
                dlg.save(Some(&w), None::<&gio::Cancellable>,
                    move |result: Result<gio::File, glib::Error>| {
                        if let Ok(file) = result {
                            if let Some(path) = file.path() {
                                let path_str = path.to_string_lossy().to_string();
                                let ss = s.borrow();
                                match save_engine_to_xlsx(&path_str, &ss) {
                                    Ok(()) => {
                                        let toast = adw::Toast::new("Spreadsheet saved");
                                        // toast via overlay — suite_common doesn't expose it directly
                                    }
                                    Err(e) => {
                                        let err = adw::AlertDialog::builder()
                                            .heading("Error saving file")
                                            .body(&e)
                                            .build();
                                        err.add_response("ok", "OK");
                                        err.set_default_response(Some("ok"));
                                        err.present(Some(&w2));
                                    }
                                }
                            }
                        }
                    },
                );
            });
            app.add_action(&act);
        }

        // ── Keyboard shortcuts ──────────────────────────────────────────
        {
            let s = state.clone();
            let da = drawing_area.clone();
            let key = gtk4::EventControllerKey::new();
            key.connect_key_pressed(move |_, key, _code, _mod| {
                if key == gtk::gdk::Key::Delete || key == gtk::gdk::Key::BackSpace {
                    let mut st = s.borrow_mut();
                    let r = st.sheet().selected_row;
                    let c = st.sheet().selected_col;
                    st.engine.set_cell_text(r, c, "");
                    st.sheet_mut().data[r][c] = String::new();
                    st.sheet_mut().formulas[r][c] = false;
                    da.queue_draw();
                }
                gtk4::glib::Propagation::Proceed
            });
            drawing_area.add_controller(key);
        }

        Self { window: suite_win.window, drawing_area, h_adj, v_adj, fx_entry, stack }
    }

    pub fn present(&self) { self.window.present(); }
}

// ── Coordinate conversion ─────────────────────────────────────────────
fn xy_to_cell(x: f64, y: f64) -> Option<(usize, usize)> {
    if x < ROW_HEADER_WIDTH || y < COL_HEADER_HEIGHT { return None; }
    let col = ((x - ROW_HEADER_WIDTH) / COL_WIDTH) as usize;
    let row = ((y - COL_HEADER_HEIGHT) / ROW_HEIGHT) as usize;
    Some((col, row))
}

// ── Cairo grid rendering ────────────────────────────────────────────────

fn draw_grid(cr: &Context, state: &Rc<RefCell<AppState>>, width: f64, height: f64,
             scroll_x: f64, scroll_y: f64) {
    let st = state.borrow();
    let sh = st.sheet();
    let is_dark = adw::StyleManager::default().is_dark();
    let (header_bg, _) = if is_dark { (HEADER_BG_DARK, HEADER_BG) } else { (HEADER_BG, HEADER_BG_DARK) };

    // Background
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.paint().unwrap();

    // Column header background
    cr.set_source_rgb(header_bg.0, header_bg.1, header_bg.2);
    cr.rectangle(ROW_HEADER_WIDTH, 0.0, width - ROW_HEADER_WIDTH, COL_HEADER_HEIGHT);
    cr.fill().unwrap();

    // Column headers
    let start_col = (scroll_x / COL_WIDTH) as usize;
    let end_col = ((scroll_x + width) / COL_WIDTH) as usize + 1;
    for col in start_col..end_col.min(sh.cols) {
        let x = ROW_HEADER_WIDTH + col as f64 * COL_WIDTH - scroll_x;
        cr.set_source_rgb(0.3, 0.3, 0.3);
        let label = col_label(col);
        cr.move_to(x + 6.0, COL_HEADER_HEIGHT - 7.0);
        cr.show_text(&label).unwrap();
        // Divider
        if col > start_col {
            cr.set_source_rgb(0.7, 0.7, 0.7);
            cr.set_line_width(0.5);
            cr.move_to(x, 0.0); cr.line_to(x, COL_HEADER_HEIGHT); cr.stroke().unwrap();
        }
    }

    // Row header background
    cr.set_source_rgb(header_bg.0, header_bg.1, header_bg.2);
    cr.rectangle(0.0, COL_HEADER_HEIGHT, ROW_HEADER_WIDTH, height - COL_HEADER_HEIGHT);
    cr.fill().unwrap();

    // Row headers
    let start_row = (scroll_y / ROW_HEIGHT) as usize;
    let end_row = ((scroll_y + height) / ROW_HEIGHT) as usize + 1;
    for row in start_row..end_row.min(sh.rows) {
        let y = COL_HEADER_HEIGHT + row as f64 * ROW_HEIGHT - scroll_y;
        cr.set_source_rgb(0.3, 0.3, 0.3);
        cr.move_to(ROW_HEADER_WIDTH - 8.0, y + ROW_HEIGHT - 7.0);
        cr.show_text(&(row + 1).to_string()).unwrap();
        // Divider
        cr.set_source_rgb(0.7, 0.7, 0.7);
        cr.set_line_width(0.5);
        cr.move_to(0.0, y); cr.line_to(ROW_HEADER_WIDTH, y); cr.stroke().unwrap();
    }

    // Cells
    for row in start_row..end_row.min(sh.rows) {
        let y = COL_HEADER_HEIGHT + row as f64 * ROW_HEIGHT - scroll_y;
        for col in start_col..end_col.min(sh.cols) {
            let x = ROW_HEADER_WIDTH + col as f64 * COL_WIDTH - scroll_x;
            let val = sh.cell(row, col);

            // Cell background for formula cells (light green tint)
            if sh.is_formula(row, col) {
                cr.set_source_rgba(0.8, 1.0, 0.8, 0.3);
                cr.rectangle(x, y, COL_WIDTH, ROW_HEIGHT);
                cr.fill().unwrap();
            }

            // Cell text
            if !val.is_empty() {
                cr.set_source_rgb(0.1, 0.1, 0.1);
                cr.move_to(x + 4.0, y + ROW_HEIGHT - 8.0);
                let display = if val.len() > 15 { &val[..14] } else { val };
                cr.show_text(display).unwrap();
            }

            // Grid lines
            cr.set_source_rgb(GRID_LINE.0, GRID_LINE.1, GRID_LINE.2);
            cr.set_line_width(0.5);
            cr.rectangle(x, y, COL_WIDTH, ROW_HEIGHT);
            cr.stroke().unwrap();
        }
    }

    // Selection highlight
    let sx = ROW_HEADER_WIDTH + sh.selected_col as f64 * COL_WIDTH - scroll_x;
    let sy = COL_HEADER_HEIGHT + sh.selected_row as f64 * ROW_HEIGHT - scroll_y;
    cr.set_source_rgb(SELECTION_COLOR.0, SELECTION_COLOR.1, SELECTION_COLOR.2);
    cr.set_line_width(2.5);
    cr.rectangle(sx, sy, COL_WIDTH, ROW_HEIGHT);
    cr.stroke().unwrap();
    cr.set_source_rgba(SELECTION_COLOR.0, SELECTION_COLOR.1, SELECTION_COLOR.2, 0.15);
    cr.rectangle(sx, sy, COL_WIDTH, ROW_HEIGHT);
    cr.fill().unwrap();
}
