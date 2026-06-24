// SPDX-License-Identifier: GPL-3.0-or-later
//
// TablesWindow — Spreadsheet window with Cairo grid, formula bar, sheet tabs, and file I/O.
// GNOME GUI spec: AdwApplicationWindow + AdwToolbarView + AdwBreakpoint.

use gtk4::cairo::{self, Context};
use gtk4::{self as gtk, gio, glib, prelude::*};
use libadwaita as adw;
use adw::prelude::{AdwDialogExt, AlertDialogExt};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::engine::TablesEngine;
use suite_common::format::{NumberFormat, NumberFormatKind};
use suite_common::undo::UndoManager;
use crate::undo::SheetState;

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

#[derive(Clone, Copy, PartialEq)]
pub enum SortDirection { Ascending, Descending }

#[derive(Clone, Debug, PartialEq)]
pub enum BorderStyle { None, Solid, Dotted, Dashed, Double }

#[derive(Clone, Debug)]
pub enum ValidationRule {
    List(Vec<String>),
    WholeNumber { min: Option<i64>, max: Option<i64> },
    Decimal { min: Option<f64>, max: Option<f64> },
    TextLength { min: Option<usize>, max: Option<usize> },
    Regex(String),
}

impl ValidationRule {
    pub fn validate(&self, value: &str) -> bool {
        match self {
            ValidationRule::List(items) => items.is_empty() || items.iter().any(|i| i == value),
            ValidationRule::WholeNumber { min, max } => {
                value.parse::<i64>().ok().map_or(false, |v| {
                    min.map_or(true, |m| v >= m) && max.map_or(true, |m| v <= m)
                })
            }
            ValidationRule::Decimal { min, max } => {
                value.parse::<f64>().ok().map_or(false, |v| {
                    min.map_or(true, |m| v >= m) && max.map_or(true, |m| v <= m)
                })
            }
            ValidationRule::TextLength { min, max } => {
                let len = value.len();
                min.map_or(true, |m| len >= m) && max.map_or(true, |m| len <= m)
            }
            ValidationRule::Regex(pattern) => {
                regex::Regex::new(pattern).map_or(false, |re| re.is_match(value))
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct CellBorder {
    pub top: BorderStyle,
    pub bottom: BorderStyle,
    pub left: BorderStyle,
    pub right: BorderStyle,
    pub color: (f64, f64, f64),
}

impl CellBorder {
    pub fn none() -> Self {
        Self { top: BorderStyle::None, bottom: BorderStyle::None,
               left: BorderStyle::None, right: BorderStyle::None,
               color: (0.0, 0.0, 0.0) }
    }

    pub fn all(style: BorderStyle, color: (f64, f64, f64)) -> Self {
        Self { top: style.clone(), bottom: style.clone(), left: style.clone(), right: style, color }
    }

    pub fn outline(style: BorderStyle, color: (f64, f64, f64)) -> Self {
        Self { top: style.clone(), bottom: style.clone(), left: style.clone(), right: style, color }
    }
}

// ── Column divider hit test ─────────────────────────────────────────

fn hit_col_divider(x: f64, y: f64, scroll_x: f64, sheet: &SheetModel) -> Option<usize> {
    // Only active in the column header zone
    if y < 0.0 || y > COL_HEADER_HEIGHT { return None; }
    let cx = x - ROW_HEADER_WIDTH + scroll_x;
    if cx < 0.0 { return None; }
    let mut accum = 0.0;
    for c in 0..sheet.cols {
        accum += sheet.col_width(c);
        if (cx - accum).abs() < 5.0 {
            return Some(c);
        }
    }
    None
}

/// Auto-fit column width to content using PangoLayout text measurement.
fn auto_fit_column(cr: &Context, sheet: &mut SheetModel, col: usize, scroll_x: f64) {
    let layout = pangocairo::functions::create_layout(cr);
    let mut max_w: f64 = 30.0;
    // Measure header label
    let label = col_label(col);
    layout.set_text(&label);
    let (tw, _) = layout.pixel_size();
    max_w = max_w.max(tw as f64 + 16.0);
    // Measure visible cells in this column
    let start_row = 0usize;
    let end_row = sheet.rows;
    for r in start_row..end_row {
        let val = sheet.cell(r, col);
        if val.is_empty() { continue; }
        layout.set_text(val);
        let (tw, _) = layout.pixel_size();
        max_w = max_w.max(tw as f64 + 12.0);
    }
    sheet.set_col_width(col, max_w.clamp(30.0, 500.0));
}

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
    pub formulas: Vec<Vec<bool>>,
    pub formats: Vec<Vec<NumberFormat>>,
    pub sorted_col: Option<(usize, SortDirection)>,
    pub borders: Vec<Vec<CellBorder>>,
    pub frozen_rows: usize,
    pub frozen_cols: usize,
    pub merges: Vec<(usize, usize, usize, usize)>,
    pub validations: Vec<Vec<Option<ValidationRule>>>,
    engine_idx: usize,
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
            formats: vec![vec![NumberFormat::default(); cols]; rows],
            sorted_col: None,
            borders: vec![vec![CellBorder::none(); cols]; rows],
            frozen_rows: 0,
            frozen_cols: 0,
            merges: Vec::new(),
            validations: vec![vec![None; cols]; rows],
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

    /// Sort rows by column. Cycles: None → Ascending → Descending → None.
    pub fn toggle_sort(&mut self, col: usize) {
        let new_dir = match self.sorted_col {
            Some((c, SortDirection::Ascending)) if c == col => SortDirection::Descending,
            Some((c, SortDirection::Descending)) if c == col => {
                self.sorted_col = None;
                return;
            }
            _ => SortDirection::Ascending,
        };
        self.sorted_col = Some((col, new_dir));
        // Gather row indices
        let mut indices: Vec<usize> = (0..self.rows).collect();
        match new_dir {
            SortDirection::Ascending => {
                indices.sort_by(|a, b| {
                    let va = &self.data[*a][col];
                    let vb = &self.data[*b][col];
                    // Numeric sort if both parse as numbers
                    if let (Ok(na), Ok(nb)) = (va.parse::<f64>(), vb.parse::<f64>()) {
                        na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        va.cmp(vb)
                    }
                });
            }
            SortDirection::Descending => {
                indices.sort_by(|a, b| {
                    let va = &self.data[*a][col];
                    let vb = &self.data[*b][col];
                    if let (Ok(na), Ok(nb)) = (va.parse::<f64>(), vb.parse::<f64>()) {
                        nb.partial_cmp(&na).unwrap_or(std::cmp::Ordering::Equal)
                    } else {
                        vb.cmp(va)
                    }
                });
            }
        }
        // Reorder data, formulas, and formats by index
        let old_data = self.data.clone();
        let old_formulas = self.formulas.clone();
        let old_formats = self.formats.clone();
        let old_borders = self.borders.clone();
        for (new_row, &old_row) in indices.iter().enumerate() {
            self.data[new_row] = old_data[old_row].clone();
            self.formulas[new_row] = old_formulas[old_row].clone();
            self.formats[new_row] = old_formats[old_row].clone();
            self.borders[new_row] = old_borders[old_row].clone();
        }
    }

    /// Merge the selected cell with cell to its right. Toggle merges/unmerges.
    pub fn toggle_merge(&mut self) {
        let r = self.selected_row;
        let c = self.selected_col;
        if c + 1 >= self.cols { return; }
        if self.is_merged(r, c).is_some() {
            self.unmerge(r, c);
            return;
        }
        self.merges.push((r, c, r, c + 1));
    }

    pub fn is_merged(&self, r: usize, c: usize) -> Option<(usize, usize, usize, usize)> {
        self.merges.iter().find(|&&(r1, c1, r2, c2)| {
            r >= r1 && r <= r2 && c >= c1 && c <= c2
        }).copied()
    }

    pub fn unmerge(&mut self, r: usize, c: usize) {
        self.merges.retain(|&(r1, c1, r2, c2)| {
            !(r >= r1 && r <= r2 && c >= c1 && c <= c2)
        });
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
    toast_overlay: Rc<adw::ToastOverlay>,
    undo: Rc<RefCell<UndoManager<SheetState>>>,
}

impl TablesWindow {
    pub fn new(app: &adw::Application) -> Self {
        let engine = TablesEngine::new(DEFAULT_ROWS, DEFAULT_COLS)
            .expect("Failed to create spreadsheet engine");

        let sheet = SheetModel::new("Sheet1", DEFAULT_ROWS, DEFAULT_COLS, 0);
        let sheet_clone = sheet.clone();
        let state = Rc::new(RefCell::new(AppState {
            sheets: vec![Rc::new(RefCell::new(sheet))],
            active_sheet: 0,
            engine,
        }));

        // ── Undo manager ─────────────────────────────────────────────
        let undo_state = Rc::new(RefCell::new(SheetState {
            sheets: vec![sheet_clone],
            active_sheet: 0,
        }));
        let undo_mgr: Rc<RefCell<UndoManager<SheetState>>> = Rc::new(RefCell::new(UndoManager::new(undo_state)));

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
            let t = toast_rc.clone();
            fx_entry.connect_activate(move |_| {
                let val = fx.text().to_string();
                let mut st = s.borrow_mut();
                let r = st.sheet().selected_row;
                let c = st.sheet().selected_col;
                // Validate
                let sh = st.sheet();
                if let Some(rule) = &sh.validations[r][c] {
                    if !rule.validate(&val) {
                        let toast = adw::Toast::new("Invalid input — value rejected");
                        toast.set_timeout(3);
                        t.add_toast(toast);
                        return;
                    }
                }
                drop(sh);
                st.engine.set_cell_text(r, c, &val);
                {
                    let mut sh = st.sheets[st.active_sheet].borrow_mut();
                    sh.data[r][c] = val.clone();
                    sh.formulas[r][c] = val.starts_with('=');
                }
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
                let st = s.borrow();
                let sh = st.sheet();
                // Check if click is in column header zone
                if wy < COL_HEADER_HEIGHT && wx > ROW_HEADER_WIDTH {
                    // Find which column was clicked
                    let mut cx = ROW_HEADER_WIDTH;
                    let mut clicked_col = None;
                    for c in 0..sh.cols {
                        cx += sh.col_width(c);
                        if wx < cx { clicked_col = Some(c); break; }
                    }
                    if let Some(col) = clicked_col {
                        drop(sh); drop(st);
                        let mut st = s.borrow_mut();
                        st.sheet_mut().toggle_sort(col);
                        da.queue_draw();
                        return;
                    }
                }
                if let Some((col, row)) = xy_to_cell(wx, wy, h.value(), &*sh) {
                    drop(sh); drop(st);
                    let mut st = s.borrow_mut();
                    let mut sh = st.sheet_mut();
                    sh.selected_row = row;
                    sh.selected_col = col;
                    let val = sh.data[row][col].clone();
                    if sh.is_formula(row, col) {
                        fx.set_text(&val);
                    } else {
                        fx.set_text(&val);
                    }
                    da.queue_draw();
                }
            });
            drawing_area.add_controller(click);
        }

        // ── Column resize: drag divider in header ───────────────────────
        {
            let s = state.clone();
            let da = drawing_area.clone();
            let h = h_adj.clone();
            let drag_col = Rc::new(Cell::new(None::<(usize, f64)>));
            let drag = gtk4::GestureDrag::new();
            drag.set_button(1);
            let dc2 = drag_col.clone();
            let dc3 = drag_col.clone();
            let s2 = s.clone();
            let h2 = h.clone();
            drag.connect_drag_begin(move |_g, x, y| {
                let st = s.borrow();
                let sh = st.sheet();
                if let Some(col) = hit_col_divider(x as f64, y as f64, h.value(), &*sh) {
                    dc2.set(Some((col, sh.col_width(col))));
                }
            });
            drag.connect_drag_update(move |_g, dx, _dy| {
                if let Some((col, start_w)) = drag_col.get() {
                    let new_w = (start_w + dx as f64).clamp(30.0, 500.0);
                    let mut st = s2.borrow_mut();
                    let mut sh = st.sheet_mut();
                    sh.set_col_width(col, new_w);
                    drop(sh); drop(st);
                    da.queue_draw();
                }
            });
            drag.connect_drag_end(move |_g, _dx, _dy| {
                dc3.set(None);
            });
            drawing_area.add_controller(drag);
        }

        // ── Cursor feedback: col-resize over divider ────────────────────
        {
            let s = state.clone();
            let da = drawing_area.clone();
            let h = h_adj.clone();
            let motion = gtk4::EventControllerMotion::new();
            motion.connect_motion(move |_m, x, y| {
                let st = s.borrow();
                let sh = st.sheet();
                let over_div = hit_col_divider(x as f64, y as f64, h.value(), &*sh).is_some();
                let over_head = (y as f64) < COL_HEADER_HEIGHT && (x as f64) > ROW_HEADER_WIDTH;
                if over_div {
                    da.set_cursor_from_name(Some("col-resize"));
                } else if over_head {
                    da.set_cursor_from_name(Some("pointer"));
                } else {
                    da.set_cursor_from_name(Some("default"));
                }
            });
            drawing_area.add_controller(motion);
        }

        // ── Double-click: inline edit or auto-fit column ────────────────
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
                // Check for divider double-click first (auto-fit)
                {
                    let st = s.borrow();
                    let sh = st.sheet();
                    if let Some(col) = hit_col_divider(wx, wy, h.value(), &*sh) {
                        drop(sh); drop(st);
                        // Auto-fit by temporarily setting draw func to measure
                        let s2 = s.clone();
                        let h2 = h.clone();
                        let v2 = v.clone();
                        let da2 = da.clone();
                        da.set_draw_func(move |_area, cr, width, height| {
                            let mut st = s2.borrow_mut();
                            let mut sh = st.sheet_mut();
                            auto_fit_column(cr, &mut *sh, col, h2.value());
                            drop(sh);
                            draw_grid(cr, &s2, width as f64, height as f64, h2.value(), v2.value());
                            // Restore normal draw func
                            let s3 = s2.clone();
                            let h3 = h2.clone();
                            let v3 = v2.clone();
                            da2.set_draw_func(move |_, cr, w, h| {
                                draw_grid(cr, &s3, w as f64, h as f64, h3.value(), v3.value());
                            });
                        });
                        da.queue_draw();
                        return;
                    }
                }
                let st = s.borrow();
                let sh = st.sheet();
                if let Some((col, row)) = xy_to_cell(wx, wy, h.value(), &*sh) {
                    let mut st = s.borrow_mut();
                    let val = st.sheet().data[row][col].clone();
                    // Compute cell x-offset using per-column widths
                    let cell_x = ROW_HEADER_WIDTH + (0..col).map(|cc| st.sheet().col_width(cc)).sum::<f64>();
                    let cell_w = st.sheet().col_width(col);
                    drop(st);
                    // Position entry overlay at cell
                    let sx = cell_x - h.value();
                    let sy = COL_HEADER_HEIGHT + row as f64 * ROW_HEIGHT - v.value();
                    let entry = gtk4::Entry::new();
                    entry.set_text(&val);
                    entry.set_size_request(cell_w as i32 - 4, ROW_HEIGHT as i32 - 2);
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

        // ── Chart dialog ──────────────────────────────────────────────
        let win_ref = Rc::new(RefCell::new(None::<adw::ApplicationWindow>));
        let show_chart_dialog = {
            let wr = win_ref.clone();
            let s = state.clone();
            Box::new(move || {
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
                if data.is_empty() { return; }

                let dialog = adw::Dialog::builder()
                    .title("Chart")
                    .content_width(600)
                    .content_height(480)
                    .build();

                let chart_type = Rc::new(Cell::new(crate::charts::ChartType::Bar));
                let data_rc = Rc::new(data);

                let preview = gtk::DrawingArea::new();
                preview.set_vexpand(true);
                preview.set_hexpand(true);
                let ct = chart_type.clone();
                let d = data_rc.clone();
                preview.set_draw_func(move |_, cr, w, h| {
                    let surface = crate::charts::render_chart(&d, ct.get(), w, h);
                    cr.set_source_surface(&surface, 0.0, 0.0).unwrap();
                    cr.paint().unwrap();
                });

                let type_combo = gtk::DropDown::from_strings(&["Bar", "Line", "Pie"]);
                let ct2 = chart_type.clone();
                let pv = preview.clone();
                type_combo.connect_selected_notify(move |dd| {
                    ct2.set(match dd.selected() {
                        0 => crate::charts::ChartType::Bar,
                        1 => crate::charts::ChartType::Line,
                        _ => crate::charts::ChartType::Pie,
                    });
                    pv.queue_draw();
                });

                let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                header.set_margin_start(12); header.set_margin_end(12); header.set_margin_top(6);
                header.append(&gtk::Label::new(Some("Type:")));
                header.append(&type_combo);

                let box_content = gtk::Box::new(gtk::Orientation::Vertical, 6);
                box_content.append(&header);
                box_content.append(&preview);
                dialog.set_child(Some(&box_content));
                let pw = wr.borrow().clone();
                dialog.present(pw.as_ref());
            })
        };

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

        let toggle_format = {
            let s = state.clone();
            let da = drawing_area.clone();
            Box::new(move || {
                let mut st = s.borrow_mut();
                let mut sh = st.sheet_mut();
                let r = sh.selected_row;
                let c = sh.selected_col;
                let current = &sh.formats[r][c].kind;
                let next = match current {
                    NumberFormatKind::General => NumberFormatKind::Number(2),
                    NumberFormatKind::Number(_) => NumberFormatKind::Currency("$".into(), 2),
                    NumberFormatKind::Currency(_, _) => NumberFormatKind::Percent(1),
                    NumberFormatKind::Percent(_) => NumberFormatKind::Date("%Y-%m-%d".into()),
                    NumberFormatKind::Date(_) => NumberFormatKind::Scientific(2),
                    NumberFormatKind::Scientific(_) => NumberFormatKind::General,
                    _ => NumberFormatKind::General,
                };
                sh.formats[r][c] = NumberFormat::new(next);
                da.queue_draw();
            })
        };

        let toggle_border = {
            let s = state.clone();
            let da = drawing_area.clone();
            Box::new(move || {
                let mut st = s.borrow_mut();
                let mut sh = st.sheet_mut();
                let r = sh.selected_row;
                let c = sh.selected_col;
                let current = &sh.borders[r][c].top;
                let next = match current {
                    BorderStyle::None => BorderStyle::Solid,
                    BorderStyle::Solid => BorderStyle::Dashed,
                    BorderStyle::Dashed => BorderStyle::Dotted,
                    BorderStyle::Dotted => BorderStyle::Double,
                    BorderStyle::Double => BorderStyle::None,
                };
                sh.borders[r][c] = CellBorder::outline(next, (0.0, 0.0, 0.0));
                da.queue_draw();
            })
        };

        let toggle_merge = {
            let s = state.clone();
            let da = drawing_area.clone();
            Box::new(move || {
                let mut st = s.borrow_mut();
                st.sheet_mut().toggle_merge();
                da.queue_draw();
            })
        };

        let extended_toolbar: Vec<(&'static str, &'static str, Box<dyn Fn() + 'static>)> = vec![
            ("format-justify-fill-symbolic", "Toggle Number Format", toggle_format),
            ("format-text-strikethrough-symbolic", "Toggle Cell Border", toggle_border),
            ("view-grid-symbolic", "Merge Cells", toggle_merge),
            ("insert-object-symbolic", "Chart", show_chart_dialog),
            ("document-send-symbolic", "Export PDF", export_pdf),
        ];

        let suite_win = suite_common::SuiteWindow::new(app, "Tables", vec![], extended_toolbar);
        *win_ref.borrow_mut() = Some(suite_win.window.clone());

        suite_win.add_top_bar(&fx_bar);
        let toast_overlay = adw::ToastOverlay::new();
        toast_overlay.set_child(Some(&stack));
        suite_win.set_content(&toast_overlay);
        suite_win.add_bottom_bar(&sheet_bar);
        let toast_rc = Rc::new(toast_overlay);

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
            let t = toast_rc.clone();
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
                                        t.add_toast(toast);
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
            let u = undo_mgr.clone();
            let key = gtk4::EventControllerKey::new();
            key.connect_key_pressed(move |_, keyval, _code, mods| {
                if mods.contains(gtk4::gdk::ModifierType::CONTROL_MASK) && keyval == gtk4::gdk::Key::z {
                    u.borrow_mut().undo();
                    da.queue_draw();
                    return gtk4::glib::Propagation::Stop;
                }
                if mods.contains(gtk4::gdk::ModifierType::CONTROL_MASK | gtk4::gdk::ModifierType::SHIFT_MASK) && keyval == gtk4::gdk::Key::z {
                    u.borrow_mut().redo();
                    da.queue_draw();
                    return gtk4::glib::Propagation::Stop;
                }
                if keyval == gtk::gdk::Key::Delete || keyval == gtk::gdk::Key::BackSpace {
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

        Self { window: suite_win.window, drawing_area, h_adj, v_adj, fx_entry, stack, toast_overlay: toast_rc, undo: undo_mgr }
    }

    pub fn present(&self) { self.window.present(); }
}

// ── Coordinate conversion ─────────────────────────────────────────────

fn draw_border_edges(cr: &Context, x: f64, y: f64, w: f64, h: f64, border: &CellBorder) {
    let (r, g, b) = border.color;
    cr.set_source_rgb(r, g, b);
    for (style, edge_x, edge_y, edge_w, edge_h) in [
        (&border.top, x, y, w, 0.0),
        (&border.bottom, x, y + h, w, 0.0),
        (&border.left, x, y, 0.0, h),
        (&border.right, x + w, y, 0.0, h),
    ] {
        draw_border_line(cr, style, edge_x, edge_y, edge_w.max(1.0), edge_h.max(1.0));
    }
}

fn draw_border_line(cr: &Context, style: &BorderStyle, x: f64, y: f64, w: f64, h: f64) {
    match style {
        BorderStyle::None => {},
        BorderStyle::Solid => {
            cr.set_line_width(2.0);
            cr.rectangle(x, y, w, h); cr.stroke().unwrap();
        }
        BorderStyle::Dashed => {
            cr.set_line_width(1.5);
            cr.set_dash(&[4.0, 2.0], 0.0);
            cr.rectangle(x, y, w, h); cr.stroke().unwrap();
            cr.set_dash(&[], 0.0);
        }
        BorderStyle::Dotted => {
            cr.set_line_width(1.5);
            cr.set_dash(&[2.0, 2.0], 0.0);
            cr.rectangle(x, y, w, h); cr.stroke().unwrap();
            cr.set_dash(&[], 0.0);
        }
        BorderStyle::Double => {
            cr.set_line_width(1.0);
            if w > h {
                // Horizontal double: draw 2px apart vertically
                cr.move_to(x, y); cr.line_to(x + w, y);
                cr.move_to(x, y + 3.0); cr.line_to(x + w, y + 3.0);
            } else {
                cr.move_to(x, y); cr.line_to(x, y + h);
                cr.move_to(x + 3.0, y); cr.line_to(x + 3.0, y + h);
            }
            cr.stroke().unwrap();
        }
    }
}

fn xy_to_cell(x: f64, y: f64, scroll_x: f64, sheet: &SheetModel) -> Option<(usize, usize)> {
    if x < ROW_HEADER_WIDTH || y < COL_HEADER_HEIGHT { return None; }
    // Convert x offset into per-column width accumulation
    let cx = x - ROW_HEADER_WIDTH + scroll_x;
    let mut accum = 0.0;
    for c in 0..sheet.cols {
        accum += sheet.col_width(c);
        if cx < accum {
            let row = ((y - COL_HEADER_HEIGHT) / ROW_HEIGHT) as usize;
            return Some((c, row));
        }
    }
    None
}

// ── Cairo grid rendering ────────────────────────────────────────────────

fn draw_grid(cr: &Context, state: &Rc<RefCell<AppState>>, width: f64, height: f64,
             scroll_x: f64, scroll_y: f64) {
    let st = state.borrow();
    let sh = st.sheet();
    let fr = sh.frozen_rows;
    let fc = sh.frozen_cols;

    // Compute freeze boundary in canvas coords
    let freeze_x = if fc > 0 {
        ROW_HEADER_WIDTH + (0..fc).map(|c| sh.col_width(c)).sum::<f64>()
    } else { 0.0 };
    let freeze_y = if fr > 0 {
        COL_HEADER_HEIGHT + fr as f64 * ROW_HEIGHT
    } else { 0.0 };

    // If no freeze panes, render normally in one pass
    if fr == 0 && fc == 0 {
        draw_grid_region(cr, state, width, height, scroll_x, scroll_y);
        return;
    }

    // 5-zone freeze rendering
    // Zone 1: Top-left fixed corner (row+col headers + frozen cells)
    cr.save().unwrap();
    cr.rectangle(0.0, 0.0, freeze_x, freeze_y);
    cr.clip();
    draw_grid_region(cr, state, width, height, 0.0, 0.0);
    cr.restore().unwrap();

    // Zone 2: Top scrollable band (frozen rows, scrollable columns)
    if freeze_x < width && freeze_y > 0.0 {
        cr.save().unwrap();
        cr.rectangle(freeze_x, 0.0, width - freeze_x, freeze_y);
        cr.clip();
        draw_grid_region(cr, state, width, height, scroll_x, 0.0);
        cr.restore().unwrap();
    }

    // Zone 3: Left fixed band (scrollable rows, frozen columns)
    if freeze_x > 0.0 && freeze_y < height {
        cr.save().unwrap();
        cr.rectangle(0.0, freeze_y, freeze_x, height - freeze_y);
        cr.clip();
        draw_grid_region(cr, state, width, height, 0.0, scroll_y);
        cr.restore().unwrap();
    }

    // Zone 4: Scrollable main area
    cr.save().unwrap();
    cr.rectangle(freeze_x, freeze_y, width - freeze_x, height - freeze_y);
    cr.clip();
    draw_grid_region(cr, state, width, height, scroll_x, scroll_y);
    cr.restore().unwrap();

    // Zone 5: Freeze boundary lines (darker, thicker)
    cr.set_source_rgb(0.25, 0.25, 0.25);
    cr.set_line_width(2.5);
    if fr > 0 {
        cr.move_to(0.0, freeze_y); cr.line_to(width, freeze_y); cr.stroke().unwrap();
    }
    if fc > 0 {
        cr.move_to(freeze_x, 0.0); cr.line_to(freeze_x, height); cr.stroke().unwrap();
    }
}

fn draw_grid_region(cr: &Context, state: &Rc<RefCell<AppState>>, width: f64, height: f64,
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

    // Column headers (using per-column widths)
    // Find visible column range by accumulating widths
    let mut col_acc = 0.0;
    let mut start_col = 0usize;
    for c in 0..sh.cols {
        if col_acc + sh.col_width(c) > scroll_x {
            start_col = c;
            break;
        }
        col_acc += sh.col_width(c);
    }
    let mut end_col = sh.cols;
    let mut end_acc = col_acc;
    for c in start_col..sh.cols {
        end_acc += sh.col_width(c);
        if end_acc > scroll_x + width {
            end_col = c + 1;
            break;
        }
    }

    let mut x_offset = ROW_HEADER_WIDTH;
    for c in 0..sh.cols {
        if c < start_col { x_offset += sh.col_width(c); continue; }
        if c >= end_col { break; }
        let cw = sh.col_width(c);
        let x = x_offset - scroll_x;
        cr.set_source_rgb(0.3, 0.3, 0.3);
        let label = col_label(c);
        cr.move_to(x + 6.0, COL_HEADER_HEIGHT - 7.0);
        cr.show_text(&label).unwrap();
        // Sort indicator
        if let Some((sc, dir)) = sh.sorted_col {
            if sc == c {
                cr.move_to(x + cw - 20.0, COL_HEADER_HEIGHT - 7.0);
                let arrow = match dir { SortDirection::Ascending => "▲", SortDirection::Descending => "▼" };
                cr.show_text(arrow).unwrap();
            }
        }
        // Divider at right edge of this column
        let div_x = x + cw;
        cr.set_source_rgb(0.7, 0.7, 0.7);
        cr.set_line_width(0.5);
        cr.move_to(div_x, 0.0); cr.line_to(div_x, COL_HEADER_HEIGHT); cr.stroke().unwrap();
        x_offset += cw;
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

    // Cells (using per-column widths)
    for row in start_row..end_row.min(sh.rows) {
        let y = COL_HEADER_HEIGHT + row as f64 * ROW_HEIGHT - scroll_y;
        let mut cell_x = ROW_HEADER_WIDTH;
        for col in 0..sh.cols {
            if col < start_col { cell_x += sh.col_width(col); continue; }
            if col >= end_col.min(sh.cols) { break; }
            let cw = sh.col_width(col);
            let x = cell_x - scroll_x;
            let val = sh.cell(row, col);

            // Skip cells that are inside a merge (except top-left corner)
            if let Some((mr1, mc1, mr2, mc2)) = sh.is_merged(row, col) {
                if row != mr1 || col != mc1 {
                    cell_x += cw;
                    continue; // skip rendering for secondary merged cells
                }
                // Render merged cell spanning multiple cells
                let merge_w: f64 = (mc1..=mc2).map(|cc| sh.col_width(cc)).sum();
                let merge_h = (mr2 - mr1 + 1) as f64 * ROW_HEIGHT;
                cr.rectangle(x, y, merge_w, merge_h);
                cr.set_source_rgb(1.0, 1.0, 1.0);
                cr.fill_preserve().unwrap();
                // Grid lines for merged area
                cr.set_source_rgb(GRID_LINE.0, GRID_LINE.1, GRID_LINE.2);
                cr.set_line_width(0.5);
                cr.stroke().unwrap();
                // Text centered in merged area
                if !val.is_empty() {
                    cr.set_source_rgb(0.1, 0.1, 0.1);
                    let formatted = sh.formats[row][col].format(val);
                    let ext = cr.text_extents(&formatted).unwrap();
                    cr.move_to(x + (merge_w - ext.width()) / 2.0, y + (merge_h + ext.height()) / 2.0 - 2.0);
                    cr.show_text(&formatted).unwrap();
                }
                // Cell borders for merged area
                let border = &sh.borders[row][col];
                draw_border_edges(cr, x, y, merge_w, merge_h, border);
                cell_x += merge_w;
                continue;
            }

            // Cell background for formula cells (light green tint)
            if sh.is_formula(row, col) {
                cr.set_source_rgba(0.8, 1.0, 0.8, 0.3);
                cr.rectangle(x, y, cw, ROW_HEIGHT);
                cr.fill().unwrap();
            }

            // Cell text (formatted)
            if !val.is_empty() {
                cr.set_source_rgb(0.1, 0.1, 0.1);
                cr.move_to(x + 4.0, y + ROW_HEIGHT - 8.0);
                let formatted = sh.formats[row][col].format(val);
                cr.show_text(&formatted).unwrap();
            }

            // Cell borders (drawn after text, on top of grid lines)
            let border = &sh.borders[row][col];
            draw_border_edges(cr, x, y, cw, ROW_HEIGHT, border);

            // Grid lines
            cr.set_source_rgb(GRID_LINE.0, GRID_LINE.1, GRID_LINE.2);
            cr.set_line_width(0.5);
            cr.rectangle(x, y, cw, ROW_HEIGHT);
            cr.stroke().unwrap();
            cell_x += cw;
        }
    }

    // Selection highlight (using per-column widths)
    let sx = {
        let mut acc = ROW_HEADER_WIDTH;
        for c in 0..sh.selected_col {
            acc += sh.col_width(c);
        }
        acc - scroll_x
    };
    let sy = COL_HEADER_HEIGHT + sh.selected_row as f64 * ROW_HEIGHT - scroll_y;
    cr.set_source_rgb(SELECTION_COLOR.0, SELECTION_COLOR.1, SELECTION_COLOR.2);
    let sel_w = sh.col_width(sh.selected_col);
    cr.set_line_width(2.5);
    cr.rectangle(sx, sy, sel_w, ROW_HEIGHT);
    cr.stroke().unwrap();
    cr.set_source_rgba(SELECTION_COLOR.0, SELECTION_COLOR.1, SELECTION_COLOR.2, 0.15);
    cr.rectangle(sx, sy, sel_w, ROW_HEIGHT);
    cr.fill().unwrap();
}
