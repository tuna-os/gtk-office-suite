# Insolvency Service Transactions horizontal multipage reduction (#1331)

The `tables/wide-unheaded-transactions` fixture isolates the horizontal multipage
pagination and unprinted headings mismatch observed in Insolvency Service Transactions
April 2026 (`tables/real-insolvency-service-transactions-april-2026`).

## Source document and observed defect

- **Source file**: `Insolvency-Finance-Report_-_Apr_2026.xlsx` (sha256 `198020582dd78b324c7fc52a610d627596a221018500f4e331248a39afd93e7f`, OGL-UK-3.0).
- **Structure**: A single worksheet (`Insolvency-Finance-Report-A (R)`) containing 21 rows and 11 columns (A through K):
  - Row 1: Header row (`Department Family`, `Entity`, `Date`, `Expense Type`, `Expense Area`, `Supplier`, `Transaction  Number`, `Amount`, `Description`, `Supplier Type`, `VAT RegNo`).
  - Rows 2–21: 20 monthly expenditure transaction records over £25,000 published pursuant to UK government transparency guidelines.
- **Dimensions**:
  - Columns A through K widths: A=13.0, B=20.78, C=10.55, D=42.89, E=55.55, F=54.66, G=19.0, H=11.0, I=45.22, J=12.89, K=13.78.
  - Total column widths sum to 299.32 units (~79 cm wide, ~2,250 px at 96 DPI).
  - All cells use standard Calibri 11pt, default borders/fills, date formatting in Column C (`mm-dd-yy`), and general numbers in Columns G, H, K.
- **LibreOffice headless PDF**:
  - Headless LibreOffice Calc PDF export (`soffice --headless --convert-to pdf`) in portrait orientation (standard A4 with 0.7in margins, printable width ~17.44 cm).
  - The 79 cm wide table exceeds printable width by >4.5×.
  - Calc paginates the sheet horizontally across 4 pages (`lo_pages: 4`), where `lo-1.png` holds only columns A through C (and left edge of D).
  - Vertically, the 21 rows fit easily within a single A4 page height (~25 cm printable).
- **Tables viewport**:
  - Tables renders a continuous spreadsheet grid on screen without page boundaries or pagination (`docs/TABLES-EXPORT-PARITY.md` Section 5 explicitly excludes multi-page spreadsheet pagination).
  - The `test-render-dump` action snapshots the on-screen used-range rect up to drawing area bounds (`A-1.png` captures all 11 columns plus on-screen header bands, width ~1117 px).
- **Scoring mismatch in `compare.py`**:
  - The document disables printed headings (`sheet.print_options.headings = False`), so `lo_cell_box(ref)` returns `None`.
  - Because `lo_cell_box(ref)` is `None`, `align()` skips cropping Tables' on-screen header bands (`ox=40, oy=20`).
  - `pick_sheet_page()` selects candidate page 1 (`lo-1.png`, columns A..C, width ~600 px) and discards subsequent horizontal page slices.
  - `align()` resamples `A-1.png` (all 11 columns, width ~1117 px) down onto `lo-1.png`'s 3-column width (~600 px).
  - This horizontal squeeze (`scale = 1.69×`) compresses columns A through K into the width of columns A through C, displacing text horizontally by 124.2 pt (`disp_pt = 124.2pt` vs budget <= 6.0pt), dropping columns D through K (`words found = 34%` vs budget >= 90%, red < 50%), and reducing structural similarity to `SSIM = 0.12` (vs budget >= 0.75), yielding a `red` verdict in both Tier A and Tier B.

## Synthetic reduction

`tables/wide-unheaded-transactions` isolates the horizontal multipage split and unprinted headings without external file dependencies:

- Single worksheet with 21 rows:
  - Row 1: Header row across 11 columns (`Department Family`, `Entity`, `Date`, `Expense Type`, `Expense Area`, `Supplier`, `Transaction Number`, `Amount`, `Description`, `Supplier Type`, `VAT RegNo`).
  - Rows 2–21: 20 synthetic government spend transaction records with representative agency names, supplier names, categories, amounts, dates, and reference numbers.
- Total column width exceeds 300 units (~79 cm), reproducing the horizontal pagination split across 4 pages in headless LibreOffice Calc.
- Headings unprinted (`sheet.print_options.headings = False`) and gridlines off (`sheet.print_options.gridLines = False`).
- Clean text with standard column widths, Calibri 11pt font, and date formatting.
- Crossing the horizontal page boundary reproduces the continuous-viewport-vs-paginated-print mismatch independently of external file dependencies.

## Reproduction

```sh
tools/render-lab/run.sh --app tables
# Inspect tables/wide-unheaded-transactions in render-lab-out/report.html
```

## Remaining acceptance work

The reduction fixture isolates the defect and allows CI to measure the synthetic
case independently. `baseline-real.json` remains unchanged pending measured
runs on CI. Issue #1331 remains open until the real document reaches green in
Tiers A and B without regressing any other real document.
