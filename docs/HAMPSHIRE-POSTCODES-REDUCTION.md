# Hampshire Public Review postcodes vertical multipage reduction (#1347)

The `tables/tall-unheaded-postcodes` fixture isolates the vertical multipage
pagination and unprinted headings mismatch observed in Hampshire Public Review:
additional postcodes (`tables/real-hampshire-public-review-additional-postcodes`).

## Source document and observed defect

- **Source file**: `247_Postcodes.ods` (sha256 `731b9251d6ba4acd226206ca12ac849acccb1485e829dd66a6966c7309fd1f2e`, OGL-UK-3.0).
- **Structure**: A single worksheet (`247 Postcodes`) containing 251 rows and 2 columns (`Postcode` and `No of Premises Impacted`), formatted in regular Arial 11pt, column width 70.16pt (93.5px), row height 15.76pt (21px), with no custom borders or background fills.
- **LibreOffice headless PDF**: Headless LibreOffice Calc paginates the 251 rows across 6 portrait A4 pages:
  - `lo-1.png`: rows 1 to 50
  - `lo-2.png` to `lo-6.png`: remaining rows (50 rows per page; last page has 51 rows)
- **Tables viewport**: Tables renders a continuous grid on screen without page boundaries (`docs/TABLES-EXPORT-PARITY.md` Section 5 explicitly excludes pagination / multi-page sheets). The `test-render-dump` action snapshots the on-screen used-range rect up to drawing area bounds (`A-1.png` captures rows 1 to 73, height 1520px).
- **Scoring mismatch in `compare.py`**:
  - The document disables printed headings (`sheet.print_options.headings = False`), so `lo_cell_box(ref)` returns `None`.
  - `pick_sheet_page()` selects candidate page 1 (`lo-1.png`, 50 rows) and discards subsequent pages.
  - `align()` does not crop Tables' on-screen column (`A`, `B`) and row (`1`..`73`) header bands (`ox=40, oy=20`).
  - `align()` resizes `A-1.png` (73 rows + UI headers) down to `lo-1.png`'s dimensions (50 rows, no headers).
  - This compresses 73 rows into the vertical space of 50 rows (a 1.67× vertical squeeze), displacing text rows by over 50pt (`disp_pt = 51.9pt`), yielding `scale = 1.28×`, `SSIM = 0.26`, and dropping OCR word match to 69% (`lost_lines = 12`).

## Synthetic reduction

`tables/tall-unheaded-postcodes` isolates the vertical multipage split and unprinted headings without proprietary data:

- Single worksheet with 75 rows and 2 columns: header row at row 1 (`Postcode`, `No of Premises Impacted`) followed by 74 rows of synthetic UK-style postcodes and premises counts.
- Headings unprinted (`sheet.print_options.headings = False`) and gridlines off (`sheet.print_options.gridLines = False`).
- Clean text and integer values with standard column widths and fonts.
- Crossing the 50-row Calc page boundary reproduces the continuous-vs-paginated mismatch independently of file formats or styles.

## Reproduction

```sh
tools/render-lab/run.sh --app tables
# Inspect tables/tall-unheaded-postcodes in render-lab-out/report.html
```

## Remaining acceptance work

The reduction fixture isolates the defect and allows CI to measure the synthetic
case independently. `baseline-real.json` remains unchanged pending measured
runs on CI. Issue #1347 remains open until the real document reaches green in
Tiers A and B without regressing any other real document.
