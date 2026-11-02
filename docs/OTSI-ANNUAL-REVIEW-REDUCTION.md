# OTSI Annual Review wide borderless hyperlinks reduction (#1344)

The `tables/wide-borderless-hyperlinks` fixture isolates the wide borderless
single-column cover sheet with hyperlinked contents and unprinted headings observed
in OTSI Annual Review 2025-2026: Tables (`tables/real-otsi-annual-review-2025-2026-tables`).

## Source document and observed defect

- **Source file**: `OTSI_Annual_Review_25-26_Tables.ods` (sha256 `ec6bce2467cbcd39af8ea879ce2e8fb759866c8efbcebaecfa30624884175d2a`, OGL-UK-3.0).
- **Structure**: Worksheet 1 (`Cover_sheet`) contains 15 active rows and only Column A populated, with the remaining 16,383 columns blank and borderless.
- **Dimensions & Styles**:
  - Column A is styled with width 23.71 cm (`column-width="23.7066666666667cm"`, ~896 px at 96 DPI) with no right or cell borders.
  - Row 1: Title "OTSI Annual Review 2025/26 Tables" (Arial 16pt, bold, color `#000000`, row height 20.25pt).
  - Row 2: "This dataset contains nine tables for OTSI's annual review" (Arial 12pt, text-wrap, row height 15pt).
  - Row 3: "Contents" (Arial 12pt, bold, color `#000000`, row height 15.75pt).
  - Rows 4–15: 12 hyperlinks to sheets within the workbook (`Cover sheet`, `Notes`, `Table A1` through `Table C1`, `Contact Sheet`), styled with cell style `ce5`: Arial 12pt, font color `#0563C1` (RGB 5, 99, 193), single underline, row height 15.75pt.
- **LibreOffice headless PDF**:
  - Because Column A has no right border, LibreOffice's headless PDF exporter emits ink only where text glyphs exist.
  - `ink_mask(ref)` crops `ref` to its text bounding box (~412 px wide by ~286 px tall).
  - LibreOffice Calc's headless PDF converter overrides the font color in hyperlinked text (`<text:a>` / hyperlinked cells) with its global application unvisited link color `#000080` (RGB 0, 0, 128).
- **Tables viewport**:
  - Tables renders the spreadsheet on screen without artificial ink-only cropping.
  - The `test-render-dump` action snapshots the full used range: Column A width (896 px) plus on-screen row header band (40 px) = 936 px, and rows 1..15 height (324 px) plus on-screen column header band (20 px) = 344 px (`A-1.png`).
- **Scoring mismatch in `compare.py`**:
  - The document disables printed headings (`sheet.print_options.headings = False`), so `lo_cell_box(ref)` returns `None`.
  - Because `lo_cell_box(ref)` is `None`, `align()` skips cropping Tables' on-screen header bands (`ox=40, oy=20`).
  - `align()` computes `scale = (936/412 + 344/286) / 2 = 1.74×` and resamples `ours` down to 412 × 286 px.
  - This compresses Column A's text horizontally by 2.27× (936 / 412), squashing letterforms.
  - This horizontal squash causes:
    - Text displacement inflates to `12.5pt` (threshold `<= 6.0pt`)
    - Structural similarity drops to `SSIM = 0.67` (threshold `>= 0.75`)
    - OCR word recall drops to `84%` (threshold `>= 90%`)
  - In addition, LibreOffice's salient color is `#000080` while Tables renders `#0563C1`, resulting in `colors = 0%` (threshold `>= 90%`).
  - Result: Amber in both Tier A and Tier B.

## Synthetic reduction

`tables/wide-borderless-hyperlinks` isolates the wide borderless column extent, hyperlinked table of contents, and unprinted headings without external dependencies:

- Single worksheet with 15 rows:
  - Row 1: Title "OTSI Annual Review 2025/26 Tables" (Arial 16pt, bold, color `000000`, row height 27).
  - Row 2: Subtitle "This dataset contains nine tables for OTSI's annual review" (Arial 12pt, row height 20).
  - Row 3: Section heading "Contents" (Arial 12pt, bold, color `000000`, row height 21).
  - Rows 4–15: 12 hyperlink cells (`Cover sheet`, `Notes`, `Table A1` through `Table C1`, `Contact Sheet`) targeting sheet anchors (e.g. `#'Cover sheet'!A1`), styled with Arial 12pt, font color `0563C1`, single underline, and row height 21.
- Column A width set to 115 (~23.7 cm / ~896 px), borderless.
- Headings unprinted (`sheet.print_options.headings = False`) and gridlines off (`sheet.print_options.gridLines = False`).
- Reproduces the column geometry vs ink extent mismatch, missing heading boxes in `compare.py`, and hyperlink color override independently of the large multi-sheet workbook.

## Reproduction

```sh
tools/render-lab/run.sh --app tables
# Inspect tables/wide-borderless-hyperlinks in render-lab-out/report.html
```

## Remaining acceptance work

The reduction fixture isolates the defect and allows CI to measure the synthetic
case independently. `baseline-real.json` remains unchanged pending measured
runs on CI. Issue #1344 remains open until the real document reaches green in
Tiers A and B without regressing any other real document.
