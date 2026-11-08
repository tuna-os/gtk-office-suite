# Draft schedule 4 - complaint log horizontal multipage reduction (#1327)

The `tables/wide-unheaded-complaint-log` fixture isolates the horizontal multipage
pagination and unprinted headings mismatch observed in Draft schedule 4 - complaint log
(`tables/real-draft-schedule-4-complaint-log`).

## Source document and observed defect

- **Source file**: `Draft_schedule_4_-_complaint_log.xlsx` (sha256 `4b2a008a9fed94d3ee381ab73bf0045ebb900631f625bf4cc4f7f11f23b1e4be`, OGL-UK-3.0, gov.uk).
- **Structure**: Sheet 1 (`Log of Actionable Complaints`) contains 20 rows and 3 columns (Cols A, B, C) in portrait A4 orientation:
  - Row 1: Document title (`Log of Actionable Complaints`, 14pt Arial bold).
  - Row 3: Header row (`Field`, `Options`, `Notes`, 11pt bold, solid fill `D9E1F2`).
  - Rows 4–20: 17 actionable complaints field definitions:
    - Col A: Field names (11pt bold Aptos Narrow/Calibri).
    - Col B: Options / data type specifications (11pt regular).
    - Col C: Guidance notes and instructions (11pt regular).
  - Sheet 2 (`Lookups`): Reference lookup lists for complaint categories, sub-natures, outcomes, and actions.
- **Dimensions & Page Setup**:
  - Orientation: Portrait (default A4 paper printable width ~17.44 cm ~ 660px at 96 DPI).
  - Column widths: Col A = 59.73 units (~418px), Col B = 101.18 units (~708px), Col C = 180.54 units (~1264px), totaling 341.45 character units (~2389px).
  - Headings unprinted (`sheet.print_options.headings = False`), gridlines unprinted (`sheet.print_options.gridLines = False`).
- **LibreOffice headless PDF**:
  - Calc converts portrait A4 to PDF. Because the 3 columns total 2389px and exceed the ~660px printable width by >3.6×, Calc paginates horizontally across pages:
    - Page 1 (`lo-1.png`): Column A only (ink width 502px).
    - Page 2 (`lo-2.png`): Column B only (ink width 727px).
    - Page 3 (`lo-3.png`): Column C only (ink width 727px).
    - Page 4 (`lo-4.png`): Column D empty slice (422px).
    - Pages 5–8: Sheet 2 (`Lookups`).
- **Tables viewport**:
  - Tables is an interactive desktop spreadsheet editor whose viewport (`capture.py` window width 1100px) displays the grid continuously without artificial page breaks.
  - The test-render-dump captures the on-screen viewport (`A-1.png`, 1100×427px), showing both Column A (418px) and Column B (partially, ~682px of 708px) side-by-side.
  - All cell contents, fonts, theme fills, and text wrap render faithfully (100% colors, 96% word recall, 0 lost lines).
- **Scoring mismatch in `compare.py`**:
  - The document disables printed headings (`sheet.print_options.headings = False`), so `lo_cell_box(ref)` returns `None`.
  - Because `lo_cell_box(ref)` is `None`, `align()` skips cropping Tables' on-screen header bands (`ox=40, oy=20`).
  - `pick_sheet_page()` selects candidate page 1 (`lo-1.png`, Col A only, width 502px) and discards subsequent horizontal page slices.
  - `align()` resamples `A-1.png` (Cols A + B across 1100px) down onto `lo-1.png` (Col A only, width 502px).
  - This horizontal squeeze (`scale = 1.66×`, width ratio 2.19×) causes `scale = 1.66×`, `Δ position = 25.7pt` (budget <= 6.0pt), and `SSIM = 0.13` (budget >= 0.75), yielding an `amber` verdict in both Tier A and Tier B.

## Synthetic reduction

`tables/wide-unheaded-complaint-log` isolates the 3-column horizontal multipage split and unprinted headings without external file dependencies:

- Single worksheet with 20 rows and 3 columns (A..C):
  - Row 1: Title in 14pt Arial bold.
  - Row 3: Header row across 3 columns (`Field`, `Options`, `Notes`) with solid fill and 11pt bold font.
  - Rows 4–20: 17 synthetic complaints log specification rows with bold field names in Col A, options in Col B, and notes in Col C.
- Column dimensions set to 60, 101, and 180 units (total 341 units, ~2389px), reproducing the horizontal pagination split across pages in headless LibreOffice Calc.
- Headings unprinted (`headings=False`) and gridlines off (`gridLines=False`).
- Clean text with standard column widths, Calibri fonts, and solid fill on header row.
- Crossing the horizontal page boundary reproduces the continuous-viewport-vs-paginated-print mismatch independently of external file dependencies.

## Reproduction

```sh
tools/render-lab/run.sh --app tables
# Inspect tables/wide-unheaded-complaint-log in render-lab-out/report.html
```

## Remaining acceptance work

The reduction fixture isolates the defect and allows CI to measure the synthetic
case independently. `baseline-real.json` remains unchanged pending measured
runs on CI. Issue #1327 closes when the real document reaches green in
Tiers A and B without regressing any other real document.
