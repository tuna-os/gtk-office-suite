# ADR 0004 — Tables Advanced Analysis: IronCalc Integration, Dynamic Arrays, Pivot Tables, and Sheet Protection Strategy

Date: 2026-08-11 · Status: accepted

## Context

Issue #114 requires scoping advanced spreadsheet capabilities for Tables:
1. **Dynamic Arrays and Advanced Functions:** Strategy for integrating with IronCalc engine capability checks without reimplementing mature calculation machinery locally.
2. **Rich Charts:** Multi-series editing, axis labels, titles, legends, custom positioning, and OOXML/xlsx persistence.
3. **Pivot Tables:** Scope, data model, and interoperability verification against fixtures before GUI expansion.
4. **Sheet & Cell Protection:** Explicit protection model (locked/unlocked flags, passwords, permission flags) and loss policy during import/export.
5. **Recalculation and Performance Budgets:** Maintaining performance gates on large workbooks.

## Decision

### 1. IronCalc Engine Strategy & Dynamic Arrays

- **Engine Boundary:** All formula evaluation, dependency tracking, and calculation logic belong strictly to the underlying calculation engine (`ironcalc_base::Model` via `TablesEngine`). `tables-core` and the GTK GUI shell MUST NOT re-implement expression evaluation or array spilling locally.
- **Capability Check:** `TablesEngine` queries IronCalc capabilities at runtime. For dynamic array formulas (e.g. `FILTER`, `UNIQUE`, `SORT`, `SEQUENCE`) and advanced functions, formula evaluation delegates directly to IronCalc.
- **Spill & Array Formula Strategy:** When IronCalc introduces array spilling, `TablesEngine` captures spilled output ranges and projects them into `SheetModel`. If an array formula attempts to spill over non-empty cells, a `#SPILL!` error is surfaced.

### 2. Rich Charts Model & Persistence

- **Extended `ChartSpec` Model:**
  - `kind`: `Bar`, `Line`, `Pie`, `Scatter`, `Area`.
  - `title`: Optional custom title string.
  - `x_axis_title` / `y_axis_title`: Custom axis title strings.
  - `legend_position`: `None`, `Top`, `Bottom`, `Left`, `Right`.
  - `series`: Vector of `ChartSeries` (`name`, `cat_range`, `val_range`, `color`).
  - `anchor` & dimensions: Position `(row, col)` and sizing `(width_px, height_px)` for flexible canvas placement.
- **XLSX Persistence:** Extended DrawingML chart XML generation in `tables-core/src/io.rs` for multi-series, axis titles, legends, and chart types.

### 3. Pivot Table Model & Fixture Verification

- **Data Model (`PivotTableSpec`):**
  - `name`: Pivot table identifier.
  - `source_range`: `(top, left, bottom, right)` data source.
  - `target_cell`: `(row, col)` location for output.
  - `row_fields`: Vector of column indices for row grouping.
  - `col_fields`: Vector of column indices for column grouping.
  - `data_fields`: Vector of `(col_index, AggregationFunc)` (Sum, Count, Average, Min, Max).
  - `filter_fields`: Vector of column indices for filtering.
- **Fixtures & Interoperability:** Model proven against golden fixture tests in `tables-core/tests/pivot_tests.rs` prior to full GUI layout builder integration.

### 4. Sheet and Cell Protection Model

- **Cell Protection (`CellProtection`):**
  - `locked: bool` (defaults to `true`, matching Excel/Calc defaults).
  - `hidden_formula: bool` (hides formula string when protected).
- **Sheet Protection (`SheetProtection`):**
  - `protected: bool`.
  - `password_hash: Option<String>` (SHA-256 or legacy OOXML hash).
  - `allow_select_locked: bool`, `allow_select_unlocked: bool`, `allow_format_cells: bool`, `allow_insert_rows: bool`, `allow_delete_rows: bool`, etc.
- **Enforcement Policy:** When `sheet.protection.protected` is `true`, `WorkbookController` enforces edit checks: attempts to mutate locked cells return `Err("Sheet is protected")`.
- **Loss Policy & Warnings:** Full round-trip preservation into XLSX `<sheetProtection>` elements. Unsupported protection features emit explicit user warnings rather than silent data loss.

### 5. Recalculation & Large-Workbook Budget

- Large workbook benchmark targets (< 100ms for 10,000 cells recalculation) are enforced in `tables-core/benches` and integration test gates.

## Consequences

- Clean architectural split preserved: calculation stays inside `ironcalc_base`, presentation in `tables` GTK shell.
- Full structural compatibility for extended charts, pivot tables, and sheet protection in XLSX I/O.
