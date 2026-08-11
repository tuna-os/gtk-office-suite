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

/// One cell/range reference found inside a formula, for reference
/// highlighting while editing (#113) — a colored outline drawn around
/// each referenced cell/range, the standard spreadsheet convention.
#[derive(Debug, Clone, PartialEq)]
pub struct FormulaRef {
    /// `None` means the same sheet the formula lives on. `Some(name)`
    /// for an explicit `Sheet2!A1`-style prefix — recognized so the
    /// parser doesn't misfire on it, but only same-sheet references are
    /// meaningful to highlight (the other sheet isn't visible).
    pub sheet_name: Option<String>,
    /// (top, left, bottom, right), 0-based inclusive.
    pub rect: (usize, usize, usize, usize),
    /// The raw matched text of a single-cell reference (e.g. "A1" or
    /// "Tax1"), `$` signs stripped — `None` for a range (a defined name
    /// can't be a range in this app). Lets callers with access to a
    /// defined-names list exclude a match that's actually a name that
    /// happens to look like a cell ref, which this pure parser can't do
    /// on its own.
    pub single_cell_text: Option<String>,
}

/// Find every cell/range reference in formula text (leading `=` optional
/// — works on either `"=A1+B2"` or `"A1+B2"`). A simple regex scan, not
/// a real tokenizer: guards against the two collisions that matter in
/// practice — a function call like `LOG10(` (an ident that happens to
/// look like a cell ref, followed immediately by `(`) is excluded, but
/// disambiguating a defined name that happens to look like a cell ref
/// (e.g. a name literally called `Tax1`) needs the caller's own
/// defined-names list, which this pure parser doesn't have access to.
pub fn parse_formula_references(formula: &str) -> Vec<FormulaRef> {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"(?:('(?:[^']|'')+'|[A-Za-z_][A-Za-z0-9_.]*)!)?(\$?[A-Za-z]{1,3}\$?[0-9]+)(?::(\$?[A-Za-z]{1,3}\$?[0-9]+))?",
        )
        .unwrap()
    });

    let mut out = Vec::new();
    for cap in re.captures_iter(formula) {
        let whole = cap.get(0).unwrap();
        if formula[whole.end()..].starts_with('(') {
            continue; // a function call, e.g. LOG10(...), not a cell ref
        }
        let sheet_name = cap.get(1).map(|m| m.as_str().trim_matches('\'').replace("''", "'"));
        let first_text = cap.get(2).unwrap().as_str().replace('$', "");
        let Some((r0, c0)) = parse_cell_ref(&first_text) else {
            continue;
        };
        let (r1, c1, single_cell_text) = match cap.get(3) {
            Some(m) => match parse_cell_ref(&m.as_str().replace('$', "")) {
                Some(rc) => (rc.0, rc.1, None),
                None => continue,
            },
            None => (r0, c0, Some(first_text)),
        };
        out.push(FormulaRef {
            sheet_name,
            rect: (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1)),
            single_cell_text,
        });
    }
    out
}

/// Summary statistics over the numeric cells of a selection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionStats {
    /// Cells in the selection containing a parseable number.
    pub count: usize,
    pub sum: f64,
    pub avg: f64,
}

/// Screen x-coordinate of the left edge of `col`, in widget-local
/// (scroll-adjusted) space — column analog of [[row_y]]: sums each
/// preceding visible column's own width, hidden columns collapsing to
/// zero. Shared by the renderer and hit-testers.
pub fn col_x(col: usize, scroll_x: f64, sheet: &SheetModel) -> f64 {
    let mut x = ROW_HEADER_WIDTH - scroll_x;
    for c in 0..col {
        if !sheet.is_col_hidden(c) {
            x += sheet.col_width(c);
        }
    }
    x
}

/// Column analog of [[row_at_content_offset]].
fn col_at_content_offset(offset: f64, sheet: &SheetModel) -> Option<usize> {
    let mut accum = 0.0;
    for c in 0..sheet.cols {
        if sheet.is_col_hidden(c) {
            continue;
        }
        let w = sheet.col_width(c);
        if offset < accum + w {
            return Some(c);
        }
        accum += w;
    }
    None
}

pub fn hit_col_divider(x: f64, y: f64, scroll_x: f64, sheet: &SheetModel) -> Option<usize> {
    if !(0.0..=COL_HEADER_HEIGHT).contains(&y) { return None; }
    let cx = x - ROW_HEADER_WIDTH + scroll_x;
    if cx < 0.0 { return None; }
    let mut accum = 0.0;
    for c in 0..sheet.cols {
        if sheet.is_col_hidden(c) { continue; }
        accum += sheet.col_width(c);
        if (cx - accum).abs() < 5.0 { return Some(c); }
    }
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
    let x = col_x(right + 1, scroll_x, sheet);
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
    let content_x = x - ROW_HEADER_WIDTH + scroll_x;
    if content_x < 0.0 || y < COL_HEADER_HEIGHT { return None; }
    let row = row_at_content_offset(y - COL_HEADER_HEIGHT, sheet)?;
    let col = col_at_content_offset(content_x, sheet)?;
    Some((col, row))
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegendPosition {
    None,
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChartSeries {
    pub name: String,
    /// Category range: (first_row, col, last_row)
    pub cat: (usize, usize, usize),
    /// Values range: (first_row, col, last_row)
    pub val: (usize, usize, usize),
    pub color: Option<String>,
}

/// One embedded chart: kind, title, series, axes, legend, and placement.
#[derive(Clone, Debug, PartialEq)]
pub struct ChartSpec {
    pub kind: ChartKind,
    pub title: String,
    pub x_axis_title: Option<String>,
    pub y_axis_title: Option<String>,
    pub legend_position: LegendPosition,
    pub series: Vec<ChartSeries>,
    /// Primary category range (for backwards compatibility / single series)
    pub cat: (usize, usize, usize),
    /// Primary values range (for backwards compatibility / single series)
    pub val: (usize, usize, usize),
    /// Anchor cell (row, col) of top-left corner
    pub anchor: (usize, usize),
    pub width_px: f64,
    pub height_px: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartKind {
    Bar,
    Line,
    Pie,
    Scatter,
    Area,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PivotAggFunc {
    Sum,
    Count,
    Average,
    Min,
    Max,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PivotField {
    pub col_index: usize,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PivotDataField {
    pub col_index: usize,
    pub name: String,
    pub func: PivotAggFunc,
}

/// Pivot Table specification model.
#[derive(Clone, Debug, PartialEq)]
pub struct PivotTableSpec {
    pub name: String,
    /// Source data range: (top, left, bottom, right), 0-based inclusive
    pub source_range: (usize, usize, usize, usize),
    /// Destination anchor (row, col)
    pub target_cell: (usize, usize),
    pub row_fields: Vec<PivotField>,
    pub col_fields: Vec<PivotField>,
    pub data_fields: Vec<PivotDataField>,
}

/// Cell protection settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellProtection {
    pub locked: bool,
    pub hidden_formula: bool,
}

impl Default for CellProtection {
    fn default() -> Self {
        Self { locked: true, hidden_formula: false }
    }
}

/// Sheet protection specification model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetProtection {
    pub protected: bool,
    pub password_hash: Option<String>,
    pub allow_select_locked: bool,
    pub allow_select_unlocked: bool,
    pub allow_format_cells: bool,
    pub allow_insert_rows: bool,
    pub allow_delete_rows: bool,
}

impl Default for SheetProtection {
    fn default() -> Self {
        Self {
            protected: false,
            password_hash: None,
            allow_select_locked: true,
            allow_select_unlocked: true,
            allow_format_cells: false,
            allow_insert_rows: false,
            allow_delete_rows: false,
        }
    }
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
    /// Pivot tables defined on this sheet.
    pub pivot_tables: Vec<PivotTableSpec>,
    /// Sheet protection status.
    pub protection: SheetProtection,
    pub cell_protections: Vec<Vec<CellProtection>>,
    /// Conditional-formatting rules (ADR 0003 §4).
    pub cond_rules: Vec<CondRule>,
    pub validations: Vec<Vec<Option<ValidationRule>>>,
    /// Rows currently hidden by a column-value filter (#113). Purely a
    /// display concern — data, formulas, and formatting for a hidden row
    /// are untouched; rendering and hit-testing are expected to skip
    /// indices in this set.
    pub hidden_rows: std::collections::HashSet<usize>,
    /// Rows/columns manually hidden by the user (#113 "row/column
    /// hiding"), independent of `hidden_rows`'s filter — clearing a
    /// filter must not reveal a row the user deliberately hid, and vice
    /// versa. `is_row_hidden`/`is_col_hidden` fold both concepts together
    /// for rendering/hit-testing, which don't need to distinguish why a
    /// row or column is hidden, only that it is.
    pub hidden_rows_manual: std::collections::HashSet<usize>,
    pub hidden_cols: std::collections::HashSet<usize>,
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
            pivot_tables: Vec::new(),
            protection: SheetProtection::default(),
            cell_protections: vec![vec![CellProtection::default(); cols]; rows],
            cond_rules: Vec::new(),
            validations: vec![vec![None; cols]; rows],
            hidden_rows: std::collections::HashSet::new(),
            hidden_rows_manual: std::collections::HashSet::new(),
            hidden_cols: std::collections::HashSet::new(),
            print_area: None,
            page_setup: suite_common_core::print::PageSetup::default(),
            sheet_id,
        }
    }

    /// Whether `row` should be skipped by rendering/hit-testing — hidden
    /// either by the active filter or by a manual hide.
    pub fn is_row_hidden(&self, row: usize) -> bool {
        self.hidden_rows.contains(&row) || self.hidden_rows_manual.contains(&row)
    }

    /// Whether `col` should be skipped by rendering/hit-testing.
    pub fn is_col_hidden(&self, col: usize) -> bool {
        self.hidden_cols.contains(&col)
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
    fn col_x_collapses_hidden_cols_to_zero_width() {
        let mut s = sheet();
        let x_before = col_x(3, 0.0, &s);
        s.hidden_cols.insert(1);
        let x_after = col_x(3, 0.0, &s);
        assert_eq!(x_after, x_before - COL_WIDTH);
    }

    #[test]
    fn xy_to_cell_skips_hidden_cols() {
        let mut s = sheet();
        s.hidden_cols.insert(1);
        // Screen slot 1 (second visible col) is now data col 2, not 1.
        let x = ROW_HEADER_WIDTH + COL_WIDTH * 1.5;
        let y = COL_HEADER_HEIGHT + 5.0;
        assert_eq!(xy_to_cell(x, y, 0.0, &s), Some((2, 0)));
    }

    #[test]
    fn fill_handle_center_accounts_for_hidden_cols_to_the_left() {
        let mut s = sheet();
        let (x_before, _) = fill_handle_center(2, 2, 0.0, 0.0, &s);
        s.hidden_cols.insert(1);
        let (x_after, _) = fill_handle_center(2, 2, 0.0, 0.0, &s);
        assert_eq!(x_after, x_before - COL_WIDTH);
    }

    #[test]
    fn is_row_hidden_is_true_for_either_filter_or_manual_hide() {
        let mut s = sheet();
        assert!(!s.is_row_hidden(1));
        s.hidden_rows_manual.insert(1);
        assert!(s.is_row_hidden(1));
        s.hidden_rows_manual.remove(&1);
        s.hidden_rows.insert(1);
        assert!(s.is_row_hidden(1));
    }

    #[test]
    fn manually_hiding_a_row_does_not_affect_the_filter_set_and_vice_versa() {
        // Regression guard: clearing a filter must not reveal a row the
        // user deliberately hid, and hiding a row manually must not be
        // undone by clearing an unrelated filter.
        let mut s = sheet();
        s.hidden_rows_manual.insert(1);
        s.hidden_rows.insert(2);
        assert!(!s.hidden_rows.contains(&1));
        assert!(!s.hidden_rows_manual.contains(&2));
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
    fn parse_formula_references_finds_single_cells() {
        let refs = parse_formula_references("=A1+B2*3");
        assert_eq!(refs.len(), 2);
        assert_eq!(
            refs[0],
            FormulaRef { sheet_name: None, rect: (0, 0, 0, 0), single_cell_text: Some("A1".into()) }
        );
        assert_eq!(
            refs[1],
            FormulaRef { sheet_name: None, rect: (1, 1, 1, 1), single_cell_text: Some("B2".into()) }
        );
    }

    #[test]
    fn parse_formula_references_finds_a_range() {
        let refs = parse_formula_references("=SUM(A1:A3)");
        assert_eq!(
            refs,
            vec![FormulaRef { sheet_name: None, rect: (0, 0, 2, 0), single_cell_text: None }]
        );
    }

    #[test]
    fn parse_formula_references_handles_absolute_refs() {
        let refs = parse_formula_references("=$A$1*2");
        assert_eq!(
            refs,
            vec![FormulaRef { sheet_name: None, rect: (0, 0, 0, 0), single_cell_text: Some("A1".into()) }]
        );
    }

    #[test]
    fn parse_formula_references_recognizes_a_cross_sheet_reference() {
        let refs = parse_formula_references("=Sheet2!A1+1");
        assert_eq!(
            refs,
            vec![FormulaRef {
                sheet_name: Some("Sheet2".to_string()),
                rect: (0, 0, 0, 0),
                single_cell_text: Some("A1".into()),
            }]
        );
    }

    #[test]
    fn parse_formula_references_exposes_raw_text_for_defined_name_disambiguation() {
        // "Tax1" parses as a syntactically valid cell ref (col "TAX", row
        // 1), but callers with a defined-names list can check
        // single_cell_text against it to tell the difference.
        let refs = parse_formula_references("=Tax1*2");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].single_cell_text.as_deref(), Some("Tax1"));
    }

    #[test]
    fn parse_formula_references_does_not_mistake_a_function_call_for_a_ref() {
        // LOG10( looks like a cell ref (3 letters + digits) followed by
        // an open paren — a function call, not a reference to cell LOG10.
        let refs = parse_formula_references("=LOG10(100)");
        assert!(refs.is_empty(), "{refs:?}");
    }

    #[test]
    fn parse_formula_references_of_plain_text_is_empty() {
        assert!(parse_formula_references("hello world").is_empty());
        assert!(parse_formula_references("").is_empty());
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
