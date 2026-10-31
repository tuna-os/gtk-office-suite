# Schedule of musts 2026 unheaded bordered table reduction (#1323)

The `tables/unheaded-bordered-musts` fixture isolates the `lo_cell_box()` line-detection
false positive, vertical pagination mismatch, and unprinted headings observed in Schedule of musts 2026
(`tables/real-schedule-of-musts-2026`).

## Source document and observed defect

- **Source file**: `ATH_Schedule_of_Musts_2026.xlsx` (sha256 `309bd93a911fbbc572f3fe1db5bf4358844a4b67d131f2f9a36df490025240c1`, OGL-UK-3.0, gov.uk, Department for Education).
- **Structure**:
  - Sheet 1 (`Schedule of Musts 2026`): 132 rows and 6 columns (A..F, primary content in A..D):
    - Row 1: Document title (`Schedule of Musts 2026`, 28pt Arial bold, cell A1) and status formula (`=IF(Sheet1!D2=119,"Fully compliant","Not compliant")`, 28pt Arial bold, cell D1).
    - Rows 2–3: Descriptive guidance text (11pt Arial).
    - Row 4: Column header row (`Paragraph`, `Requirement`, `In Place: ...`, `Comments Action to take`, 11pt/12pt bold Arial, center vertical alignment, text wrap).
    - Row 5: Section header banner (`Top 10 ‘musts’ for chairs and other trustees`, 11pt bold Arial, solid theme fill).
    - Rows 6–15: Top 10 statutory requirements from the Academy Trust Handbook (Col A paragraph refs, Col B requirement text, Col C compliance status, Col D comments).
    - Row 16: Section banner (`Roles and responsibilities`, 11pt bold Arial, solid theme fill).
    - Rows 17–132: Full breakdown of governance, financial, and regulatory obligations.
  - Sheet 2 (`Sheet1`): Hidden calculation/scoring sheet with lookup values.
- **Dimensions & Page Setup**:
  - Orientation: Landscape (`<pageSetup paperSize="9" scale="67" fitToHeight="0" orientation="landscape"/>`).
  - Column widths: Col A = 12.82 units (~90px), Col B = 114.54 units (~802px), Col C = 25.73 units (~180px), Col D = 48.82 units (~342px), totaling ~1,414px across columns A..D.
  - Headings unprinted (`sheet.print_options.headings = False`), gridlines unprinted (`sheet.print_options.gridLines = False`).
  - Borders: Thin solid black borders enclose all cells in columns A..D across all active rows.
- **LibreOffice headless PDF**:
  - Calc converts landscape A4 to PDF at 67% print scaling.
  - Page 1 (`lo-1.png`): Rows 1–16, columns A..D (ink bounding box: left x=67, top y=71, right x=1054, bottom y=697; ink dimensions 988×627px).
  - Pages 2–9: Remaining rows 17–132 paginated across 8 subsequent pages.
  - Headings are not printed: no row numbers (1..16) or column letters (A..D) appear on paper.
- **Tables viewport**:
  - Tables displays the worksheet continuously in its interactive desktop window (`WINDOW = (1100, 1700)` in `capture.py`).
  - The test-render-dump captures the on-screen drawing area (`A-1.png`, 1100×1520px).
  - The capture includes rows 1 through 50+ fitting within the 1520px height at 100% zoom.
- **Scoring and Alignment defect in `compare.py`**:
  - In `compare.py`, `lo_cell_box(ref)` looks for long horizontal and vertical lines to detect heading boxes:
    ```python
    dark = np.asarray(ref.convert("L")) < 128
    rows, cols = long_lines(dark, axis=1), long_lines(dark, axis=0)
    if len(rows) < 3 or len(cols) < 3:
        return None
    return cols[1][1] + 1, rows[1][1] + 1, cols[-1][0], rows[-1][0]
    ```
  - For synthetic fixtures where `headings=True`, `cols[0]` is the left edge of row numbers, `cols[1]` is the divider between row numbers and Column A, `rows[0]` is the top of column letters, and `rows[1]` is the divider between column letters and Row 1.
  - However, in `Schedule of musts 2026`, headings are disabled (`headings=False`). Because the table has complete borders, `rows[0]` is the top line of Row 1 (y=71), `rows[1]` is the bottom line of Row 1 (y=104), `cols[0]` is the left border of Column A (x=68), and `cols[1]` is the divider between Column A and Column B (x=130).
  - `lo_cell_box(ref)` thus returns `(131, 105, 1053, 697)`, which **crops out Column A completely and crops out Row 1 completely from the LibreOffice reference image (`lo-1.png`)**.
  - Meanwhile, Tables' on-screen capture (`A-1.png`) is cropped from `(ox=40, oy=20)` (the GTK header bands), retaining both Row 1 and Column A.
  - When `align()` resamples `A-1.png` onto `ref`, Row 1 of Tables is mapped onto Row 2 of LibreOffice, and Column A of Tables is mapped onto Column B of LibreOffice.
  - This misalignment drops word recall from >98% to 88%, causes displacement $\Delta$position to spike to 148.7pt (budget <= 6.0pt), and degrades SSIM to 0.06 (budget >= 0.75).
  - In addition, `scale = (ours.width / ref.width + ours.height / ref.height) / 2 = 1.77×` due to the 2.42× height ratio between Tables' 1520px viewport capture and LibreOffice's 627px page 1.
- **Conditional formatting defect in `tables-core`**:
  - The workbook specifies conditional formatting with `type="containsText"` for text "Fully compliant" using `<ns0:bgColor theme="9" tint="0.79998168889431442" />` (soft green fill, `RGB(226, 240, 217)`).
  - `tables-core/src/io/condrules.rs` only parses `type="cellIs"` and only extracts direct hex `rgb="..."` strings, ignoring theme colors and `bgColor`.
  - LibreOffice highlights matching cells with soft green, while Tables draws them with default white.

## Synthetic reduction

`tables/unheaded-bordered-musts` isolates the unheaded bordered table cell box mismatch and vertical pagination split without external file dependencies:

- Single worksheet with 16 rows and 4 columns (A..D) in landscape A4 orientation:
  - Row 1: Document title in cell A1 (24pt Arial bold) and status in cell D1 (18pt Arial bold).
  - Rows 2–3: Descriptive notes.
  - Row 4: Column header row across 4 columns (`Paragraph`, `Requirement`, `In Place: ...`, `Comments Action to take`) with text wrap and bold font.
  - Row 5: Section header banner (`Top 10 ‘musts’ for chairs and other trustees`, 11pt bold Arial, solid fill `D9D9D9`).
  - Rows 6–15: Numbered statutory requirements with paragraph references in Col A, multi-line requirements in Col B, and compliance status in Col C.
  - Row 16: Section banner (`Roles and responsibilities`).
- Column dimensions set to 13, 115, 26, and 49 units (total 203 units, ~1,414px).
- Full thin black borders enclose all cells from row 1 to row 16, columns 1 to 4.
- Headings unprinted (`headings=False`) and gridlines off (`gridLines=False`).
- Reproduces the unbroken border pattern that triggers `lo_cell_box()` line detection to crop out Row 1 and Column A on unheaded tables, isolating the defect in a single-feature fixture.

## Reproduction

```sh
tools/render-lab/run.sh --app tables
# Inspect tables/unheaded-bordered-musts in render-lab-out/report.html
```

## Remaining acceptance work

The reduction fixture isolates the defect and allows CI to measure the synthetic
case independently. `baseline-real.json` remains unchanged pending measured
runs on CI. Issue #1323 closes when the real document reaches green in
Tiers A and B without regressing any other real document.
