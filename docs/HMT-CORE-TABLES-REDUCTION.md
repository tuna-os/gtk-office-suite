# HMT Core Tables vertical multipage reduction (#1351)

The `tables/tall-unprinted-sheet` fixture isolates the vertical multipage
pagination mismatch observed in HM Treasury's Annual Report and Accounts Core
Tables 2025-26 (`tables/real-hmt-annual-report-and-accounts-core-tables-2025`).

## Source document and observed defect

- **Source file**: `Core_tables.ods` (sha256 `f98b077a6cbd3e019c4c7fc95a13f607c6004d42db2bcf232482e6e653644a64`, OGL-UK-3.0).
- **Structure**: A single sheet (`Sheet1`) holding 74 rows of data.
- **LibreOffice headless PDF**: Headless LibreOffice Calc splits the 74 rows
  across two A4 pages:
  - `lo-1.png`: rows 1 to 54
  - `lo-2.png`: rows 55 to 74
- **Tables viewport**: Tables has no page layout or pagination model
  (`docs/TABLES-EXPORT-PARITY.md` Section 5 explicitly excludes pagination /
  multi-page sheets). The `test-render-dump` action captures the entire used
  extent of the sheet (rows 1 to 74) in a single image `A-1.png` (height 1520px).
- **Scoring mismatch in `compare.py`**:
  - The document omits printed headings (`print_options.headings = False`), so
    `lo_cell_box(ref)` returns `None`.
  - `pick_sheet_page()` selects candidate page 1 (`lo-1.png`) and discards `lo-2.png`.
  - `align()` does not crop Tables' screen row and column header bands `(ox=40, oy=20)`.
  - `align()` resizes `A-1.png` (74 rows) down to `lo-1.png`'s dimensions (54 rows).
  - This vertically compresses 74 rows into 54 rows, displacing every row by up to
    270px (`disp_pt = 80.0pt`), yielding `scale = 1.33×`, `SSIM = 0.20`, and 3 lost lines.
- **Secondary gap**: `styles.xml` defines conditional number formatting
  (`<number:text-style style:name="N36">` with `<style:map>` branches for
  positive/negative integers and dash for zero). In `tables-core/src/io/ods_numfmt.rs`,
  `<number:text-style>` is currently parsed as `NumberFormatKind::Text`.

## Synthetic reduction

`tables/tall-unprinted-sheet` isolates the multipage split and unprinted headings
without proprietary data or complex conditional formatting:

- Single worksheet with 74 rows (title row, header row at row 3, 71 data rows).
- Headings unprinted (`sheet.print_options.headings = False`) and gridlines off.
- 6 columns with clean text and integer values.

## Reproduction

```sh
tools/render-lab/run.sh --app tables
# Inspect tables/tall-unprinted-sheet in render-lab-out/report.html
```

## Remaining acceptance work

The reduction fixture isolates the defect and allows CI to measure the synthetic
case independently. `baseline-real.json` remains unchanged pending measured
runs on CI. Issue #1351 remains open until the real document reaches green in
Tiers A and B without regressing any other real document.
