# 2020 - Deep sea stock horizontal multipage reduction (#1340)

The `tables/wide-unheaded-deep-sea-stock` fixture isolates the horizontal multipage
pagination, multi-tier grouped headers with repeated empty cell borders, and unprinted
headings mismatch observed in 2020 - Deep sea stock (`tables/real-2020-deep-sea-stock`).

## Source document and observed defect

- **Source file**: `Deep_Sea_during_2020__as_at_27th_January_2021.ods` (sha256 `4f4ce17f6002140b9946583a812309850838dc98c9ec3e6b40fe45d7613910a9`, OGL-UK-3.0).
- **Structure**: Worksheet 1 (`DSS_summ`) contains 39 rows and 24 active columns (Cols A–X / Col 0..Col 23) in landscape orientation (`pm1` page layout):
  - Rows 1–2: Title and report metadata ("Deep Sea Species Statistics: total landings (tonnes live weight) in 2020", "27-Jan-21", "This weeks report includes swap numbers 1448-1502").
  - Rows 4–7: Multi-tier grouped headers:
    - Row 4: "Landings into" (spanning cols C..G), "Total landings" (cols M..O), "Uptake of quota" (cols Q..U). A continuous top border line runs across Row 4 from Col B to Col X.
    - Rows 5–7: Column sub-headers: "Stock" (merged B5:B7), "Scotland", "England, Wales & N.I.", "Abroad", "by UK vessels", "U.K", "Quota", "2019", "2020", "% change", ending in Col X with "UK", "Quota", "2016".
  - Rows 9–26: Deep Sea species records ("Tusk IV", "Ling IV", "Tusk V,VI,VII", "Ling VI-X,XII,XIV", "Black Scabbardfish V-VII,XII", "Greater Silver Smelt V-VII", "Roundnose/Roughead Grenadier Vb,VI,VII", "Blue Ling Vb,VI,VII") with landings and quota statistics.
  - Rows 28–29: Explanatory footnotes regarding reporting delays.
- **Dimensions & Page Setup**:
  - Page orientation: Landscape (`style:print-orientation="landscape"`, margins 0.354 in / 9 mm left and right).
  - Printable width on landscape A4 (29.70 cm - 2 × 0.90 cm) = 27.89 cm (~1054 px at 96 DPI).
  - The 24 active columns sum to 28.50 cm wide (~1077 px at 96 DPI):
    - Col A: 0.50 cm
    - Col B: 3.84 cm
    - Cols C–W: ~1.22–1.38 cm each (Cols A..W cumulative = 27.04 cm)
    - Col X: 1.46 cm (cumulative = 28.50 cm > 27.89 cm printable width)
- **LibreOffice headless PDF**:
  - Headless LibreOffice Calc PDF export (`soffice --headless --convert-to pdf`) respects the 27.89 cm printable width limit.
  - Because Column X (1.46 cm) pushes the table width past 27.89 cm, Calc splits Column X onto a second horizontal page slice.
  - Page 1 contains Columns A–W (0..22), while Column X (23, "UK Quota 2016") is paginated onto Page 2/3.
- **Tables viewport**:
  - Tables renders an on-screen continuous 2D sheet canvas showing all 24 active columns without artificial page cuts.
  - The `test-render-dump` action snapshots the on-screen used-range rect into `A-1.png`: 40 px row header band + 1077 px for columns A–X = 1117 px wide.
- **Scoring mismatch in `compare.py`**:
  - The document disables printed headings (`sheet.print_options.headings = False`), so `lo_cell_box(ref)` returns `None`.
  - Because `lo_cell_box(ref)` is `None`, `align()` skips cropping Tables' on-screen header bands (`ox=40, oy=20`).
  - `pick_sheet_page()` selects candidate Page 1 (`lo-1.png`), which contains only Columns A–W.
  - `align()` resamples the 1117 px wide 24-column sheet `A-1.png` onto `lo-1.png`'s 23-column width.
  - This horizontal scale mismatch (scale 1.52×) compresses all columns, misaligns text across the entire table, and drops Column X's text, yielding:
    - `ink = 2.59` (threshold < 1.25)
    - `words found = 14%` (threshold >= 90%)
    - `disp_pt = 61.6pt` (threshold <= 6.0pt)
    - `SSIM = 0.09` (threshold >= 0.75)
    - Result: `red` verdict in both Tier A and Tier B.
- **Header border parsing issue in `tables-core`**:
  - In `DSS_summ!F4:G4`, cells are empty but styled with `ce14` specifying `border-top: thin solid #000000` to complete the continuous header top border line.
  - `read_sheet_props_from_ods` in `tables-core/src/io/props.rs` checks `has_value || (crepeat == 1 && repeat == 1)`. Because `crepeat == 2` on empty cells F4:G4, style `ce14` was skipped, leaving a gap in the top border line.

## Synthetic reduction

`tables/wide-unheaded-deep-sea-stock` isolates the 24-column horizontal multipage split, multi-tier grouped headers, repeated empty cell borders, and unprinted headings without external dependencies:

- Landscape A4 worksheet with 24 columns (A..X):
  - Rows 1–2: Document title and metadata.
  - Rows 4–7: Multi-tier grouped headers:
    - Merged categories: "Landings into" (C4:G4), "Total landings" (M4:O4), "Uptake of quota" (Q4:U4).
    - Continuous top border across Row 4 (Cols B..X), explicitly including empty cells F4:G4.
    - Sub-headers: "Stock" (merged B5:B7), column labels across rows 5–7, and bottom border under Row 7.
  - Rows 9–16: 7 representative deep-sea species records with landing figures, percentages, and baseline quotas in Column X.
  - Rows 18–19: Footnotes.
- Column dimensions set to total ~181 Excel width units (~28.5 cm), reproducing the horizontal page slice beyond landscape A4 printable width.
- Headings unprinted (`headings=False`) and gridlines off (`gridLines=False`).
- Page setup configured for landscape A4 (`orientation="landscape"`, `paperSize=A4`).

## Reproduction

```sh
tools/render-lab/run.sh --app tables
# Inspect tables/wide-unheaded-deep-sea-stock in render-lab-out/report.html
```

## Remaining acceptance work

The reduction fixture isolates the defect and allows CI to measure the synthetic case independently. `baseline-real.json` remains unchanged pending measured runs on CI. Issue #1340 remains open until the real document reaches green in Tiers A and B without regressing any other real document.
