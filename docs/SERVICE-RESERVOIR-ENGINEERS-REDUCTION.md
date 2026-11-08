# Service reservoir panel engineers horizontal multipage reduction (#1346)

The `tables/wide-unheaded-contact-list` fixture isolates the horizontal multipage
pagination and unprinted headings mismatch observed in Service reservoir panel
engineers: contact list (`tables/real-service-reservoir-panel-engineers-contact-list`).

## Source document and observed defect

- **Source file**: `Service_reservoirs_panel_engineers_-_October_2026.ods` (sha256 `3364dfb1b6adf02a614077ba9ad9ddfe6c0321061dc9fa0fc77a5885ba65a9c2`, OGL-UK-3.0).
- **Structure**: A single worksheet (`Service_reservoir_panel_list`) containing 8 rows (rows 1–3 title and guidance prose, row 4 empty, row 5 table header, rows 6–7 engineer contact records, row 8 empty) and 15 columns (A through O: `Surname`, `Forename`, `Title`, `Company name`, `Address line 2`, `Address line 3`, `Town`, `Post Town`, `County`, `Post Code`, `Phone Number`, `Mobile Number`, `Email Address`, `Existing panel`, `Joint corresponding panel`).
- **Dimensions**: Column widths total 74.3 cm (~29.3 inches, ~2,808px at 96 DPI). Cell text, standard fonts (Arial 12pt/14pt), and hyperlink styling are handled accurately by `tables-core`.
- **LibreOffice headless PDF**: Headless LibreOffice Calc PDF export in portrait orientation (A4 with ~0.7in margins, printable width ~6.87in / ~17.4cm) cannot fit a 74.3 cm wide table onto a single page width. Calc therefore paginates the sheet horizontally across 6 pages (`lo_pages: 6`), where `lo-1.png` holds only columns A through D.
- **Tables viewport**: Tables renders a continuous grid on screen without page boundaries or pagination (`docs/TABLES-EXPORT-PARITY.md` Section 5 explicitly excludes multi-page spreadsheet pagination). The `test-render-dump` action snapshots the on-screen used-range rect up to drawing area bounds (`A-1.png` captures columns A through G, width ~1040px in Broadway `WINDOW = (1100, 1700)`).
- **Scoring mismatch in `compare.py`**:
  - The document disables printed headings (`sheet.print_options.headings = False`), so `lo_cell_box(ref)` returns `None`.
  - Because `lo_cell_box(ref)` is `None`, `align()` does not crop Tables' on-screen column (`A`..`G`) and row (`1`..`7`) header bands (`ox=40, oy=20`).
  - `pick_sheet_page()` selects candidate page 1 (`lo-1.png`, columns A..D, width ~600px) and discards subsequent horizontal pages.
  - `align()` resizes `A-1.png` (columns A..G, width ~1040px) down to `lo-1.png`'s width (~600px).
  - This horizontally compresses columns A through G into the width of columns A through D (a 1.45× horizontal squeeze, `scale = 1.45×`), displacing text horizontally by 79.5pt (`disp_pt = 79.5pt` vs budget <= 6.0pt) and reducing structural similarity to `SSIM = 0.22` (vs budget >= 0.75), yielding amber despite 95% word recall.

## Synthetic reduction

`tables/wide-unheaded-contact-list` isolates the horizontal multipage split and unprinted headings without proprietary or personal data:

- Single worksheet with 9 rows: title and explanatory notes in rows 1–3, header row at row 5 across 15 columns (`Surname` through `Joint corresponding panel`), and 4 rows of synthetic UK engineer contact records with typical names, companies, addresses, postcodes, and contact numbers.
- Total column width exceeds 280 units (~74 cm), forcing headless LibreOffice Calc to paginate horizontally across multiple pages.
- Headings unprinted (`sheet.print_options.headings = False`) and gridlines off (`sheet.print_options.gridLines = False`).
- Clean text with standard column widths and Arial fonts.
- Crossing the horizontal page boundary reproduces the continuous-viewport-vs-paginated-print mismatch independently of external file dependencies.

## Reproduction

```sh
tools/render-lab/run.sh --app tables
# Inspect tables/wide-unheaded-contact-list in render-lab-out/report.html
```

## Remaining acceptance work

The reduction fixture isolates the defect and allows CI to measure the synthetic
case independently. `baseline-real.json` remains unchanged pending measured
runs on CI. Issue #1346 remains open until the real document reaches green in
Tiers A and B without regressing any other real document.
