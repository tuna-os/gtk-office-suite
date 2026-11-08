# BICS sectoral consumption estimates horizontal multipage reduction (#1343)

The `tables/wide-unheaded-consumption-estimates` fixture isolates the horizontal multipage
pagination, 2-tier merged column header, and unprinted headings mismatch observed in
BICS sectoral consumption estimates (`tables/real-bics-sectoral-consumption-estimates`).

## Source document and observed defect

- **Source file**: `bics-sectoral-consumption-estimates.ods` (sha256 `587fff7e8414753816a6ae58c4ea576366a2ecf483137f2d133b99d76544e918`, OGL-UK-3.0).
- **Structure**: A single worksheet (`Consumption_Estimates`) containing 86 rows and 6 columns:
  - Row 1: Cell A1 ("SIC07 4-digit", rowspan 2), Cell B1 ("Sector Description", rowspan 2), Cell C1 ("Consumption Range (GWh)", colspan 4 across columns C..F).
  - Row 2: Four year sub-headers: C2 ("2027-2028"), D2 ("2028-2029"), E2 ("2029-2030"), F2 ("2030-2031").
  - Rows 3–86: 84 sector data records with 4-digit SIC codes as text digits ("1041", "1062", etc.), sector descriptions, and estimated consumption ranges.
- **Dimensions**:
  - Column A: 2.54 cm (`style:column-width="2.54cm"`, ~96 px at 96 DPI, ~14 Excel width units).
  - Column B: 16.91 cm (`style:column-width="16.906875cm"`, ~639 px at 96 DPI, ~70 Excel width units).
  - Columns C–F: ~2.28 cm and 1.905 cm each (~8 cm total / ~302 px, ~10–12 Excel width units).
  - Total table width: 27.43 cm (~1037 px at 96 DPI).
- **LibreOffice headless PDF**:
  - Headless LibreOffice Calc PDF export (`soffice --headless --convert-to pdf`) in portrait orientation (standard A4 with ~0.7in margins, printable width ~17.44 cm).
  - Because Column A (2.54 cm) and Column B (16.91 cm) total 19.45 cm (> 17.44 cm printable width), Calc cannot fit Column B alongside Column A on the first page width.
  - Calc paginates the sheet horizontally across 3 column slices:
    - Pages 1 & 2 (`lo-1.png`, `lo-2.png`): Column A alone (ink width 91 px).
    - Pages 3 & 4 (`lo-3.png`, `lo-4.png`): Column B alone (ink width 517 px).
    - Pages 5 & 6 (`lo-5.png`, `lo-6.png`): Columns C–F (ink width 294 px).
  - Vertically, the 86 rows split across 2 pages (rows 1–50 and rows 51–86), yielding 6 pages total (`lo_pages: 6`).
- **Tables viewport**:
  - Tables renders an on-screen spreadsheet editor with a continuous 2D sheet canvas without page splits (`docs/TABLES-EXPORT-PARITY.md` Section 5 explicitly excludes multi-page spreadsheet pagination).
  - The `test-render-dump` action snapshots the on-screen used-range rect up to drawing area bounds into `A-1.png`: 40 px row header band + 1037 px for columns A–F = 1077 px wide.
- **Scoring mismatch in `compare.py`**:
  - The document disables printed headings (`sheet.print_options.headings = False`), so `lo_cell_box(ref)` returns `None`.
  - Because `lo_cell_box(ref)` is `None`, `align()` skips cropping Tables' on-screen header bands (`ox=40, oy=20`).
  - `pick_sheet_page()` selects candidate page 1 (`lo-1.png`) because text digits in Column A ("1041", "1062", etc.) match `ours`.
  - But `lo-1.png` contains ONLY Column A (width 91 px).
  - `align()` resamples the 1077 px wide 6-column sheet `A-1.png` into `lo-1.png`'s 91 px cropped width: an extreme 11.8× horizontal squeeze (`scale = (1077/91 + 1520/949)/2 = 6.72×`).
  - This horizontal compression obliterates the rendered text into unreadable pixel smears, yielding `words found = 0%` (budget >= 90%, red < 50%) and `SSIM = 0.22` (budget >= 0.75), triggering a `red` verdict.
  - When Column A of `A-1.png` is cropped to its ink and compared directly to `lo-1.png`, the scale is 1.32×, and the rendered text matches LibreOffice.

## Synthetic reduction

`tables/wide-unheaded-consumption-estimates` isolates the horizontal multipage split, 2-tier merged column header, and unprinted headings without external dependencies:

- Single worksheet with a 2-tier header:
  - Row 1: "SIC07 4-digit" (merged A1:A2, bold, vertical center), "Sector Description" (merged B1:B2, bold, vertical center), "Consumption Range (GWh)" (merged C1:F1, bold, horizontal center).
  - Row 2: "2027-2028", "2028-2029", "2029-2030", "2030-2031" across columns C..F (bold).
  - Rows 3–14: 12 synthetic BICS sector rows with 4-digit SIC codes as literal text, sector descriptions, and consumption ranges.
- Column dimensions set to reproduce the horizontal split: Column A width 14 (~2.54 cm), Column B width 70 (~16.9 cm), Columns C..F widths 12, 10, 10, 10 (~8 cm total). Total width ~114 (~27.5 cm).
- Headings unprinted (`sheet.print_options.headings = False`) and gridlines off (`sheet.print_options.gridLines = False`).
- Reproduces the continuous-viewport-vs-paginated-print horizontal squeeze independently of external file dependencies.

## Reproduction

```sh
tools/render-lab/run.sh --app tables
# Inspect tables/wide-unheaded-consumption-estimates in render-lab-out/report.html
```

## Remaining acceptance work

The reduction fixture isolates the defect and allows CI to measure the synthetic case independently. `baseline-real.json` remains unchanged pending measured runs on CI. Issue #1343 remains open until the real document reaches green in Tiers A and B without regressing any other real document.
