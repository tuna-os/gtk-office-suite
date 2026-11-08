# Record of non-lethal actions oversized unheaded instructions reduction (#1345)

The `tables/oversized-unheaded-instructions` fixture isolates the oversized
single-column instruction worksheet with vertical multipage pagination and
unprinted headings observed in Record of non-lethal actions (template)
(`tables/real-record-of-non-lethal-actions-template`).

## Source document and observed defect

- **Source file**: `Wild_birds_-_record_of_non-lethal_actions_-_template.ods` (sha256 `9654b5bda0b72d815ea0100f38d8a7aed27a4acc49e4fe9a5c827fdd901887f5`, OGL-UK-3.0).
- **Structure**: Worksheet 1 (`1_-_Instructions`) contains only Column A, styled with a width of 25.90 cm (`column-width="25.9027083333333cm"`, ~979 px at 96 dpi) and 21 rows of instructions and reference links with explicit row heights totaling 895.65 pt (~1194 px at 96 dpi).
- **LibreOffice headless PDF**: Headless LibreOffice Calc PDF export in portrait orientation (A4 with standard margins, printable dimensions ~17.0 cm × 25.7 cm):
  - Column A (25.90 cm) exceeds printable width (~17.0 cm / 728 px), so Calc cuts off the right portion of column A on `lo-1.png`.
  - Rows 1..21 (895.65 pt) exceed printable page height (~728 pt), so Calc paginates vertically: rows 1..17 appear on `lo-1.png`, and rows 18..21 appear on `lo-2.png` (`lo_pages: 11` across the 4-sheet workbook).
- **Tables viewport**: Tables renders a continuous grid on screen without page boundaries or pagination (`docs/TABLES-EXPORT-PARITY.md` Section 5 explicitly excludes multi-page spreadsheet pagination). The `test-render-dump` action snapshots the on-screen used-range rect up to drawing area bounds (`A-1.png` captures column A width 979 px + 40 px row header band = 1019 px, and rows 1..21 height 1195 px + 20 px column header band = 1215 px).
- **Scoring mismatch in `compare.py`**:
  - The document disables printed headings (`sheet.print_options.headings = False`), so `lo_cell_box(ref)` returns `None`.
  - Because `lo_cell_box(ref)` is `None`, `align()` does not crop Tables' on-screen column (`A`) and row (`1`..`21`) header bands (`ox=40, oy=20`).
  - `pick_sheet_page()` selects candidate page 1 (`lo-1.png`, rows 1..17, width ~728 px) and discards subsequent pages (`lo-2.png`).
  - `align()` resizes `A-1.png` (rows 1..21, 1019 × 1215 px) into `lo-1.png`'s dimensions (728 × 900 px).
  - This compresses 21 rows into the space of 17 rows and compresses 1019 px into 728 px (`scale = 1.37×`), displacing text vertically and horizontally by 82.1 pt (`disp_pt = 82.1pt` vs budget <= 6.0pt) and reducing structural similarity to `SSIM = 0.04` (vs budget >= 0.75), yielding amber despite 95% word recall and 100% color match.

## Synthetic reduction

`tables/oversized-unheaded-instructions` isolates the single-column oversized instruction layout and unprinted headings without external dependencies:

- Single worksheet with 21 rows and 1 column: title at row 1, guidance sections, and 20 instructions/notes across rows 2–21 with explicit heights totaling over 800 pt.
- Column A width set to 125 units (~26 cm), exceeding standard A4 printable width.
- Explicit row heights force LibreOffice Calc to paginate vertically across 2 pages.
- Headings unprinted (`sheet.print_options.headings = False`) and gridlines off (`sheet.print_options.gridLines = False`).
- Clean text with Arial fonts reproducing the layout mismatch independently of file formats or styles.

## Reproduction

```sh
tools/render-lab/run.sh --app tables
# Inspect tables/oversized-unheaded-instructions in render-lab-out/report.html
```

## Remaining acceptance work

The reduction fixture isolates the defect and allows CI to measure the synthetic
case independently. `baseline-real.json` remains unchanged pending measured
runs on CI. Issue #1345 remains open until the real document reaches green in
Tiers A and B without regressing any other real document.
