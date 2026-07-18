// sheet.rs — Spreadsheet data model and cell types (extracted from window.rs).

use crate::engine::TablesEngine;
use suite_common_core::format::NumberFormat;

pub const DEFAULT_ROWS: usize = 100;
pub const DEFAULT_COLS: usize = 26;
pub const ROW_HEIGHT: f64 = 28.0;
pub const COL_WIDTH: f64 = 90.0;
pub const ROW_HEADER_WIDTH: f64 = 50.0;
pub const COL_HEADER_HEIGHT: f64 = 26.0;

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
    pub top: BorderStyle, pub bottom: BorderStyle,
    pub left: BorderStyle, pub right: BorderStyle,
    pub color: (f64, f64, f64),
}

impl CellBorder {
    pub fn none() -> Self {
        Self { top: BorderStyle::None, bottom: BorderStyle::None,
               left: BorderStyle::None, right: BorderStyle::None, color: (0.0, 0.0, 0.0) }
    }
    pub fn all(style: BorderStyle, color: (f64, f64, f64)) -> Self {
        Self { top: style.clone(), bottom: style.clone(), left: style.clone(), right: style, color }
    }
    pub fn outline(style: BorderStyle, color: (f64, f64, f64)) -> Self {
        Self { top: style.clone(), bottom: style.clone(), left: style.clone(), right: style, color }
    }
}

pub fn col_label(c: usize) -> String {
    let mut n = c; let mut s = String::new();
    loop { let d = (n % 26) as u8; s.insert(0, (b'A' + d) as char); n /= 26; if n == 0 { break; } n -= 1; }
    s
}

pub fn hit_col_divider(x: f64, y: f64, scroll_x: f64, sheet: &SheetModel) -> Option<usize> {
    if y < 0.0 || y > COL_HEADER_HEIGHT { return None; }
    let cx = x - ROW_HEADER_WIDTH + scroll_x;
    if cx < 0.0 { return None; }
    let mut accum = 0.0;
    for c in 0..sheet.cols { accum += sheet.col_width(c); if (cx - accum).abs() < 5.0 { return Some(c); } }
    None
}

pub fn xy_to_cell(x: f64, y: f64, scroll_x: f64, sheet: &SheetModel) -> Option<(usize, usize)> {
    let col_x = x - ROW_HEADER_WIDTH + scroll_x;
    if col_x < 0.0 || y < COL_HEADER_HEIGHT { return None; }
    let row = ((y - COL_HEADER_HEIGHT) / ROW_HEIGHT) as usize;
    let mut accum = 0.0;
    for c in 0..sheet.cols { accum += sheet.col_width(c); if col_x < accum { return Some((c, row)); } }
    None
}

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
            frozen_rows: 0, frozen_cols: 0,
            merges: Vec::new(),
            validations: vec![vec![None; cols]; rows],
            engine_idx,
        }
    }

    pub fn cell(&self, r: usize, c: usize) -> &str {
        if r < self.rows && c < self.cols { &self.data[r][c] } else { "" }
    }

    pub fn cell_mut(&mut self, r: usize, c: usize) -> &mut String {
        &mut self.data[r][c]
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

    pub fn toggle_sort(&mut self, col: usize) {
        use SortDirection::*;
        let new_dir = match self.sorted_col {
            Some((c, Ascending)) if c == col => Descending,
            Some((c, Descending)) if c == col => { self.sorted_col = None; return; }
            _ => Ascending,
        };
        self.sorted_col = Some((col, new_dir));
        let mut indices: Vec<usize> = (0..self.rows).collect();
        indices.sort_by(|&a, &b| {
            let va = self.data[a][col].parse::<f64>().ok().unwrap_or(0.0);
            let vb = self.data[b][col].parse::<f64>().ok().unwrap_or(0.0);
            if new_dir == Ascending { va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal) }
            else { vb.partial_cmp(&va).unwrap_or(std::cmp::Ordering::Equal) }
        });
        let old = std::mem::take(&mut self.data);
        for (new_r, old_r) in indices.iter().enumerate() {
            self.data[new_r] = old[*old_r].clone();
        }
    }

    pub fn toggle_merge(&mut self) {
        let r = self.selected_row; let c = self.selected_col;
        let pos = self.merges.iter().position(|&(mr, mc, _, _)| mr == r && mc == c);
        if let Some(p) = pos { self.merges.remove(p); }
        else { self.merges.push((r, c, 2, 2)); }
    }

    pub fn sync_from_engine(&mut self, engine: &TablesEngine) {
        let grid = engine.to_grid();
        for r in 0..self.rows.min(grid.len()) {
            for c in 0..self.cols.min(grid[r].len()) {
                self.data[r][c] = grid[r][c].clone();
                self.formulas[r][c] = engine.has_formula(r, c);
            }
        }
    }
}
