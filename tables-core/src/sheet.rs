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
                value.parse::<i64>().ok().is_some_and(|v| {
                    min.is_none_or(|m| v >= m) && max.is_none_or(|m| v <= m)
                })
            }
            ValidationRule::Decimal { min, max } => {
                value.parse::<f64>().ok().is_some_and(|v| {
                    min.is_none_or(|m| v >= m) && max.is_none_or(|m| v <= m)
                })
            }
            ValidationRule::TextLength { min, max } => {
                let len = value.len();
                min.is_none_or(|m| len >= m) && max.is_none_or(|m| len <= m)
            }
            ValidationRule::Regex(pattern) => {
                regex::Regex::new(pattern).is_ok_and(|re| re.is_match(value))
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

/// Parse a cell reference like "A1", "b3", "AA10" → (row, col), 0-based.
pub fn parse_cell_ref(s: &str) -> Option<(usize, usize)> {
    let s = s.trim();
    let letters: String = s.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let digits = &s[letters.len()..];
    if letters.is_empty() || digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut col: usize = 0;
    for ch in letters.chars() {
        col = col * 26 + (ch.to_ascii_uppercase() as usize - 'A' as usize) + 1;
    }
    let row: usize = digits.parse().ok()?;
    if row == 0 { return None; }
    Some((row - 1, col - 1))
}

/// Parse a defined-name formula like `"Sheet1!$D$3"` or
/// `"Sheet1!$A$1:$B$3"` — the exact shape `WorkbookController::define_name`
/// writes — back into a `(top, left, bottom, right)` selection rect,
/// 0-based inclusive. `None` for anything this app doesn't itself
/// generate (multi-sheet ranges, unparseable formulas).
pub fn parse_defined_name_range(formula: &str) -> Option<(usize, usize, usize, usize)> {
    let refs = formula.rsplit('!').next()?;
    let mut parts = refs.split(':');
    let (r0, c0) = parse_cell_ref(&parts.next()?.replace('$', ""))?;
    match parts.next() {
        Some(second) => {
            let (r1, c1) = parse_cell_ref(&second.replace('$', ""))?;
            Some((r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1)))
        }
        None => Some((r0, c0, r0, c0)),
    }
}

/// Summary statistics over the numeric cells of a selection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionStats {
    /// Cells in the selection containing a parseable number.
    pub count: usize,
    pub sum: f64,
    pub avg: f64,
}

pub fn hit_col_divider(x: f64, y: f64, scroll_x: f64, sheet: &SheetModel) -> Option<usize> {
    if !(0.0..=COL_HEADER_HEIGHT).contains(&y) { return None; }
    let cx = x - ROW_HEADER_WIDTH + scroll_x;
    if cx < 0.0 { return None; }
    let mut accum = 0.0;
    for c in 0..sheet.cols { accum += sheet.col_width(c); if (cx - accum).abs() < 5.0 { return Some(c); } }
    None
}

/// Row-header equivalent of [[hit_col_divider]]: `(x, y)` widget-local,
/// `y` scroll-adjusted via `scroll_y` since (unlike the fixed column
/// header strip) the row header scrolls vertically with the content.
/// Hidden rows contribute no divider (their height collapses to zero).
pub fn hit_row_divider(x: f64, y: f64, scroll_y: f64, sheet: &SheetModel) -> Option<usize> {
    if !(0.0..=ROW_HEADER_WIDTH).contains(&x) { return None; }
    let cy = y - COL_HEADER_HEIGHT + scroll_y;
    if cy < 0.0 { return None; }
    let mut accum = 0.0;
    for r in 0..sheet.rows {
        if sheet.is_row_hidden(r) { continue; }
        accum += sheet.row_height(r);
        if (cy - accum).abs() < 5.0 { return Some(r); }
    }
    None
}

/// Half-width in pixels of the fill-handle hit zone (issue #113) — a
/// small square at the selection's bottom-right corner. Slightly larger
/// than the drawn handle itself so it's easy to grab with a mouse.
pub const FILL_HANDLE_HALF: f64 = 5.0;

/// Screen position of the fill-handle's center, given the selection's
/// bottom-right cell `(bottom, right)` and current scroll offsets.
/// Shared by the renderer (draws it) and the hit-tester (checks presses
/// against it), so they can never disagree about where it is.
pub fn fill_handle_center(
    bottom: usize,
    right: usize,
    scroll_x: f64,
    scroll_y: f64,
    sheet: &SheetModel,
) -> (f64, f64) {
    let x = ROW_HEADER_WIDTH - scroll_x + (0..=right).map(|c| sheet.col_width(c)).sum::<f64>();
    let y = row_y(bottom, scroll_y, sheet) + sheet.row_height(bottom);
    (x, y)
}

/// Whether `(x, y)` (widget-local, not scroll-adjusted) presses the fill
/// handle of a selection ending at `(bottom, right)`.
pub fn hit_fill_handle(
    x: f64,
    y: f64,
    bottom: usize,
    right: usize,
    scroll_x: f64,
    scroll_y: f64,
    sheet: &SheetModel,
) -> bool {
    let (hx, hy) = fill_handle_center(bottom, right, scroll_x, scroll_y, sheet);
    (x - hx).abs() <= FILL_HANDLE_HALF && (y - hy).abs() <= FILL_HANDLE_HALF
}

/// Screen y-coordinate of the top of `row`, in widget-local (scroll-
/// adjusted) space — sums each preceding visible row's own height
/// (hidden rows collapse to zero, per-row heights per [[SheetModel::row_height]]).
/// Shared by the renderer and hit-testers so they can never disagree
/// about where a row actually falls on screen.
pub fn row_y(row: usize, scroll_y: f64, sheet: &SheetModel) -> f64 {
    let mut y = COL_HEADER_HEIGHT - scroll_y;
    for r in 0..row {
        if !sheet.is_row_hidden(r) {
            y += sheet.row_height(r);
        }
    }
    y
}

/// Inverse of the row-position half of [[row_y]]: the row whose visible
/// band contains content-space offset `offset` (i.e. `y - COL_HEADER_HEIGHT`
/// at zero scroll), or `None` past the last visible row.
fn row_at_content_offset(offset: f64, sheet: &SheetModel) -> Option<usize> {
    let mut accum = 0.0;
    for r in 0..sheet.rows {
        if sheet.is_row_hidden(r) {
            continue;
        }
        let h = sheet.row_height(r);
        if offset < accum + h {
            return Some(r);
        }
        accum += h;
    }
    None
}

pub fn xy_to_cell(x: f64, y: f64, scroll_x: f64, sheet: &SheetModel) -> Option<(usize, usize)> {
    let col_x = x - ROW_HEADER_WIDTH + scroll_x;
    if col_x < 0.0 || y < COL_HEADER_HEIGHT { return None; }
    let row = row_at_content_offset(y - COL_HEADER_HEIGHT, sheet)?;
    let mut accum = 0.0;
    for c in 0..sheet.cols { accum += sheet.col_width(c); if col_x < accum { return Some((c, row)); } }
    None
}

/// A cell-value conditional-formatting rule (ADR 0003 §4): when a
/// numeric cell in `range` satisfies the comparison, paint `fill`.
#[derive(Clone, Debug, PartialEq)]
pub struct CondRule {
    /// (first_row, first_col, last_row, last_col), zero-based inclusive.
    pub range: (usize, usize, usize, usize),
    pub op: CondOp,
    pub value: f64,
    /// Upper bound, used by Between only.
    pub value2: f64,
    /// Fill color as RRGGBB hex (no '#').
    pub fill: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CondOp {
    Greater,
    Less,
    Equal,
    Between,
}

impl CondRule {
    /// Does `v` satisfy this rule?
    pub fn matches(&self, v: f64) -> bool {
        match self.op {
            CondOp::Greater => v > self.value,
            CondOp::Less => v < self.value,
            CondOp::Equal => (v - self.value).abs() < 1e-9,
            CondOp::Between => v >= self.value.min(self.value2) && v <= self.value.max(self.value2),
        }
    }

    /// Is (row, col) inside this rule's range?
    pub fn contains(&self, row: usize, col: usize) -> bool {
        let (r0, c0, r1, c1) = self.range;
        row >= r0 && row <= r1 && col >= c0 && col <= c1
    }
}

/// One embedded chart: kind, title, and the data it draws, as
/// zero-based inclusive cell ranges on this sheet.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartSpec {
    pub kind: ChartKind,
    pub title: String,
    /// Category labels range (first_row, col, last_row).
    pub cat: (usize, usize, usize),
    /// Values range (first_row, col, last_row).
    pub val: (usize, usize, usize),
    /// Anchor cell (row, col) of the chart's top-left corner.
    pub anchor: (usize, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartKind {
    Bar,
    Line,
    Pie,
}

#[derive(Clone)]
pub struct SheetModel {
    pub name: String,
    pub data: Vec<Vec<String>>,
    pub rows: usize,
    pub cols: usize,
    pub selected_row: usize,
    pub selected_col: usize,
    /// Far corner of the selection rectangle (equals selected_* for a
    /// single-cell selection). Kept valid by select_cell/extend_selection.
    pub sel_end_row: usize,
    pub sel_end_col: usize,
    pub col_widths: Vec<f64>,
    /// Per-row heights (#113), mirroring col_widths — a row not yet
    /// resized uses ROW_HEIGHT (see row_height()).
    pub row_heights: Vec<f64>,
    pub formulas: Vec<Vec<bool>>,
    pub formats: Vec<Vec<NumberFormat>>,
    pub sorted_col: Option<(usize, SortDirection)>,
    pub borders: Vec<Vec<CellBorder>>,
    pub frozen_rows: usize,
    pub frozen_cols: usize,
    pub merges: Vec<(usize, usize, usize, usize)>,
    /// Charts anchored on this sheet, persisted into xlsx (ADR 0003 §3).
    pub charts: Vec<ChartSpec>,
    /// Conditional-formatting rules (ADR 0003 §4).
    pub cond_rules: Vec<CondRule>,
    pub validations: Vec<Vec<Option<ValidationRule>>>,
    /// Rows currently hidden by a column-value filter (#113). Purely a
    /// display concern — data, formulas, and formatting for a hidden row
    /// are untouched; rendering and hit-testing are expected to skip
    /// indices in this set.
    pub hidden_rows: std::collections::HashSet<usize>,
    /// Print area (#113): (top, left, bottom, right), 0-based inclusive.
    /// `None` means "print the whole used range" (export's existing
    /// default). Purely an export-time concern — doesn't affect editing.
    pub print_area: Option<(usize, usize, usize, usize)>,
    /// Page orientation/size/margins for PDF export (#113).
    pub page_setup: suite_common_core::print::PageSetup,
    /// Stable IronCalc worksheet identity. Unlike the sheet's position in
    /// `WorkbookState::sheets`, this never changes when other sheets are
    /// added, deleted, or reordered — undo commands key off this instead of
    /// a positional index so they keep targeting the right sheet.
    pub sheet_id: u32,
}

impl SheetModel {
    pub fn new(name: &str, rows: usize, cols: usize, sheet_id: u32) -> Self {
        SheetModel {
            name: name.to_string(),
            data: vec![vec![String::new(); cols]; rows],
            rows, cols,
            selected_row: 0, selected_col: 0,
            sel_end_row: 0, sel_end_col: 0,
            col_widths: vec![COL_WIDTH; cols],
            row_heights: vec![ROW_HEIGHT; rows],
            formulas: vec![vec![false; cols]; rows],
            formats: vec![vec![NumberFormat::default(); cols]; rows],
            sorted_col: None,
            borders: vec![vec![CellBorder::none(); cols]; rows],
            frozen_rows: 0, frozen_cols: 0,
            merges: Vec::new(),
            charts: Vec::new(),
            cond_rules: Vec::new(),
            validations: vec![vec![None; cols]; rows],
            hidden_rows: std::collections::HashSet::new(),
            print_area: None,
            page_setup: suite_common_core::print::PageSetup::default(),
            sheet_id,
        }
    }

    /// Whether `row` should be skipped by rendering/hit-testing.
    pub fn is_row_hidden(&self, row: usize) -> bool {
        self.hidden_rows.contains(&row)
    }

    /// Collapse the selection to a single cell.
    pub fn select_cell(&mut self, r: usize, c: usize) {
        self.selected_row = r;
        self.selected_col = c;
        self.sel_end_row = r;
        self.sel_end_col = c;
    }

    /// Extend the selection rectangle from the anchor to (r, c).
    pub fn extend_selection(&mut self, r: usize, c: usize) {
        self.sel_end_row = r;
        self.sel_end_col = c;
    }

    /// Normalized selection rectangle: (row0, col0, row1, col1), inclusive.
    pub fn selection_rect(&self) -> (usize, usize, usize, usize) {
        (
            self.selected_row.min(self.sel_end_row),
            self.selected_col.min(self.sel_end_col),
            self.selected_row.max(self.sel_end_row).min(self.rows.saturating_sub(1)),
            self.selected_col.max(self.sel_end_col).min(self.cols.saturating_sub(1)),
        )
    }

    /// True when more than one cell is selected.
    pub fn has_range_selection(&self) -> bool {
        self.selected_row != self.sel_end_row || self.selected_col != self.sel_end_col
    }

    /// Sum/avg/count over numeric cells in the selection. Formula cells
    /// count through their displayed value when it parses as a number.
    pub fn selection_stats(&self) -> SelectionStats {
        let (r0, c0, r1, c1) = self.selection_rect();
        let mut count = 0usize;
        let mut sum = 0f64;
        for r in r0..=r1 {
            for c in c0..=c1 {
                if let Ok(v) = self.cell(r, c).trim().parse::<f64>() {
                    count += 1;
                    sum += v;
                }
            }
        }
        let avg = if count > 0 { sum / count as f64 } else { 0.0 };
        SelectionStats { count, sum, avg }
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
        if c < self.col_widths.len() { self.col_widths[c] = w.clamp(30.0, 500.0); }
    }

    pub fn row_height(&self, r: usize) -> f64 {
        if r < self.row_heights.len() { self.row_heights[r] } else { ROW_HEIGHT }
    }

    pub fn set_row_height(&mut self, r: usize, h: f64) {
        if r < self.row_heights.len() { self.row_heights[r] = h.clamp(12.0, 300.0); }
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
        self.data = indices.iter().map(|&old_r| old[old_r].clone()).collect();
    }

    pub fn toggle_merge(&mut self) {
        let r = self.selected_row; let c = self.selected_col;
        let pos = self.merges.iter().position(|&(mr, mc, _, _)| mr == r && mc == c);
        if let Some(p) = pos { self.merges.remove(p); }
        else { self.merges.push((r, c, 2, 2)); }
    }

    #[allow(clippy::needless_range_loop)]
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

#[cfg(test)]
mod selection_tests {
    use super::*;

    fn sheet() -> SheetModel {
        let mut s = SheetModel::new("t", 10, 10, 0);
        s.data[1][1] = "10".into();
        s.data[1][2] = "20".into();
        s.data[2][1] = "x".into();
        s.data[2][2] = " 30 ".into();
        s
    }

    #[test]
    fn fill_handle_hit_test_centers_on_selection_corner() {
        let s = sheet();
        let (hx, hy) = fill_handle_center(2, 2, 0.0, 0.0, &s);
        assert!(hit_fill_handle(hx, hy, 2, 2, 0.0, 0.0, &s));
        assert!(hit_fill_handle(hx + FILL_HANDLE_HALF, hy - FILL_HANDLE_HALF, 2, 2, 0.0, 0.0, &s));
        assert!(!hit_fill_handle(hx + FILL_HANDLE_HALF + 1.0, hy, 2, 2, 0.0, 0.0, &s));
        assert!(!hit_fill_handle(hx, hy + FILL_HANDLE_HALF + 1.0, 2, 2, 0.0, 0.0, &s));
    }

    #[test]
    fn fill_handle_center_tracks_scroll_offset() {
        let s = sheet();
        let (x0, y0) = fill_handle_center(2, 2, 0.0, 0.0, &s);
        let (x1, y1) = fill_handle_center(2, 2, 20.0, 15.0, &s);
        assert_eq!(x1, x0 - 20.0);
        assert_eq!(y1, y0 - 15.0);
    }

    #[test]
    fn row_height_defaults_to_row_height_constant_and_is_settable() {
        let mut s = sheet();
        assert_eq!(s.row_height(0), ROW_HEIGHT);
        s.set_row_height(0, 60.0);
        assert_eq!(s.row_height(0), 60.0);
        assert_eq!(s.row_height(1), ROW_HEIGHT, "other rows unaffected");
    }

    #[test]
    fn row_height_clamps_to_a_sane_range() {
        let mut s = sheet();
        s.set_row_height(0, 1.0);
        assert_eq!(s.row_height(0), 12.0);
        s.set_row_height(0, 10_000.0);
        assert_eq!(s.row_height(0), 300.0);
    }

    #[test]
    fn hit_row_divider_finds_the_boundary_between_rows() {
        let s = sheet();
        // Row 0's default height is ROW_HEIGHT, so its bottom boundary
        // sits at COL_HEADER_HEIGHT + ROW_HEIGHT.
        let y = COL_HEADER_HEIGHT + ROW_HEIGHT;
        assert_eq!(hit_row_divider(ROW_HEADER_WIDTH / 2.0, y, 0.0, &s), Some(0));
        assert_eq!(hit_row_divider(ROW_HEADER_WIDTH / 2.0, y + 20.0, 0.0, &s), None);
    }

    #[test]
    fn hit_row_divider_outside_the_row_header_column_is_none() {
        let s = sheet();
        let y = COL_HEADER_HEIGHT + ROW_HEIGHT;
        assert_eq!(hit_row_divider(ROW_HEADER_WIDTH + 5.0, y, 0.0, &s), None);
    }

    #[test]
    fn row_y_accounts_for_a_resized_row_above() {
        let mut s = sheet();
        let y_before = row_y(3, 0.0, &s);
        s.set_row_height(1, ROW_HEIGHT * 2.0);
        let y_after = row_y(3, 0.0, &s);
        assert_eq!(y_after, y_before + ROW_HEIGHT);
    }

    #[test]
    fn xy_to_cell_accounts_for_a_resized_row_above() {
        let mut s = sheet();
        s.set_row_height(0, ROW_HEIGHT * 2.0);
        // Row 0 now spans 2x the normal height, so a y just past one
        // normal ROW_HEIGHT still lands in row 0, not row 1.
        let y = COL_HEADER_HEIGHT + ROW_HEIGHT * 1.5;
        let x = ROW_HEADER_WIDTH + 5.0;
        assert_eq!(xy_to_cell(x, y, 0.0, &s), Some((0, 0)));
    }

    #[test]
    fn row_y_collapses_hidden_rows_to_zero_height() {
        let mut s = sheet();
        let y_before = row_y(3, 0.0, &s);
        s.hidden_rows.insert(1);
        let y_after = row_y(3, 0.0, &s);
        assert_eq!(y_after, y_before - ROW_HEIGHT);
    }

    #[test]
    fn xy_to_cell_skips_hidden_rows() {
        let mut s = sheet();
        s.hidden_rows.insert(1);
        // Screen slot 1 (second visible row) is now data row 2, not 1.
        let y = COL_HEADER_HEIGHT + ROW_HEIGHT * 1.5;
        let x = ROW_HEADER_WIDTH + 5.0;
        assert_eq!(xy_to_cell(x, y, 0.0, &s), Some((0, 2)));
    }

    #[test]
    fn xy_to_cell_past_the_last_visible_row_is_none() {
        let s = sheet();
        let y = COL_HEADER_HEIGHT + ROW_HEIGHT * (s.rows as f64 + 5.0);
        let x = ROW_HEADER_WIDTH + 5.0;
        assert_eq!(xy_to_cell(x, y, 0.0, &s), None);
    }

    #[test]
    fn fill_handle_center_accounts_for_hidden_rows_above() {
        let mut s = sheet();
        let (_, y_before) = fill_handle_center(2, 2, 0.0, 0.0, &s);
        s.hidden_rows.insert(1);
        let (_, y_after) = fill_handle_center(2, 2, 0.0, 0.0, &s);
        assert_eq!(y_after, y_before - ROW_HEIGHT);
    }

    #[test]
    fn select_cell_collapses_range() {
        let mut s = sheet();
        s.extend_selection(5, 5);
        assert!(s.has_range_selection());
        s.select_cell(2, 2);
        assert!(!s.has_range_selection());
        assert_eq!(s.selection_rect(), (2, 2, 2, 2));
    }

    #[test]
    fn selection_rect_normalizes_and_clamps() {
        let mut s = sheet();
        s.select_cell(4, 4);
        s.extend_selection(1, 99);
        assert_eq!(s.selection_rect(), (1, 4, 4, 9));
    }

    #[test]
    fn stats_over_numeric_cells_only() {
        let mut s = sheet();
        s.select_cell(1, 1);
        s.extend_selection(2, 2);
        let st = s.selection_stats();
        assert_eq!(st.count, 3);
        assert_eq!(st.sum, 60.0);
        assert_eq!(st.avg, 20.0);
    }

    #[test]
    fn stats_empty_selection() {
        let mut s = sheet();
        s.select_cell(5, 5);
        let st = s.selection_stats();
        assert_eq!(st.count, 0);
        assert_eq!(st.sum, 0.0);
        assert_eq!(st.avg, 0.0);
    }

    #[test]
    fn parse_cell_refs() {
        assert_eq!(parse_cell_ref("A1"), Some((0, 0)));
        assert_eq!(parse_cell_ref("b3"), Some((2, 1)));
        assert_eq!(parse_cell_ref("AA10"), Some((9, 26)));
        assert_eq!(parse_cell_ref(" C7 "), Some((6, 2)));
        assert_eq!(parse_cell_ref("A0"), None);
        assert_eq!(parse_cell_ref("1A"), None);
        assert_eq!(parse_cell_ref(""), None);
        assert_eq!(parse_cell_ref("hello"), None);
    }

    #[test]
    fn parse_defined_name_range_single_cell() {
        assert_eq!(parse_defined_name_range("Sheet1!$D$3"), Some((2, 3, 2, 3)));
    }

    #[test]
    fn parse_defined_name_range_a_rectangle() {
        assert_eq!(parse_defined_name_range("Sheet1!$A$1:$B$3"), Some((0, 0, 2, 1)));
    }

    #[test]
    fn parse_defined_name_range_rejects_garbage() {
        assert_eq!(parse_defined_name_range("not a range"), None);
        assert_eq!(parse_defined_name_range(""), None);
    }

    #[test]
    fn col_label_round_trips_through_parse() {
        for c in [0usize, 1, 25, 26, 27, 51, 52, 701, 702] {
            let label = format!("{}1", col_label(c));
            assert_eq!(parse_cell_ref(&label), Some((0, c)), "col {c} label {label}");
        }
    }
}

#[cfg(test)]
mod sort_tests {
    use super::*;

    #[test]
    fn toggle_sort_does_not_panic_and_sorts() {
        let mut s = SheetModel::new("t", 3, 2, 0);
        s.data[0][0] = "3".into();
        s.data[1][0] = "1".into();
        s.data[2][0] = "2".into();
        s.toggle_sort(0);
        let col: Vec<&str> = (0..3).map(|r| s.cell(r, 0)).collect();
        assert_eq!(col, vec!["1", "2", "3"]);
        s.toggle_sort(0);
        let col: Vec<&str> = (0..3).map(|r| s.cell(r, 0)).collect();
        assert_eq!(col, vec!["3", "2", "1"]);
    }
}
