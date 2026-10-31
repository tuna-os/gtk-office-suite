# Supervising panel engineers contact list: fit-to-width reduction (#1349)

Issue #1349 compares the UK Government publication [Supervising panel engineers: contact list](https://www.gov.uk/government/publications/contact-details-of-engineers-on-the-supervising-engineer-panel)
(file `Supervising_panel_engineer__-__October_2026.ods`, OGL-UK-3.0) with LibreOffice.
In [CI run 40077970330](https://github.com/tuna-os/gtk-office-suite/actions/runs/40077970330),
artifact `render-real-tables`, `tables/real-supervising-panel-engineers-contact-list` failed
with red verdicts in both Tiers A and B (scale 1.58×, words found 5%, ink 7.00, Δ position 218.0 pt, SSIM 0.37, lost lines 20).

## Root causes and evidence

1. **ODF fit-to-width print scaling (`style:scale-to-X="1"`) vs 100% on-screen zoom**:
   - The document's `styles.xml` defines `<style:page-layout-properties style:print-orientation="landscape" style:scale-to-X="1" .../>`.
   - The table contains 17 columns totaling ~81.5 cm in width, which exceeds the printable width of landscape A4 (~26 cm).
   - In LibreOffice Calc's headless PDF export (`lo_render.py`), Calc dynamically scales down the entire sheet to ~32% so that all 17 columns fit across 1 page width. Because row heights and fonts are scaled down by 32%, LibreOffice prints 140 rows on Page 1 (`lo-1.png`), and the remaining 20 rows on Page 2 (`lo-2.png`).
   - Tables renders the sheet at 100% on-screen zoom (the model's stored column widths and row heights). In the 1100×1700 test window, Tables' `DrawingArea` viewport captures only 38 rows into `A-1.png`.

2. **Scorer page matching inversion (`pick_sheet_page`)**:
   - In `tools/render-lab/compare.py`, `pick_sheet_page` attempts to find the printed page starting the active sheet using `shared / n_lo >= SHEET_CONTAINED` (0.5).
   - Because `lo-1.png` has 140 rows (3387 words) while `A-1.png` only captured 38 rows (946 words), `shared / n_lo` for `lo-1.png` was only 27.9%, failing the threshold.
   - `lo-2.png` only holds the tail 20 rows of the sheet (473 words). Because common table words ("Mr", "Ms", "Ltd", "Road", "Consulting", "Water", etc.) occur in both `A-1.png` and `lo-2.png`, `shared / n_lo` for `lo-2.png` reached 53.5%, causing `pick_sheet_page` to mistakenly pick `lo-2.png` instead of `lo-1.png`.
   - `compare.py` therefore compared `A-1.png` (rows 1–38) against `lo-2.png` (rows 141–160), yielding 5% word recall and an ink ratio of 7.00.
   - Even when scored against `lo-1.png`, the 32% fit-to-page print scaling in LibreOffice vs 100% on-screen zoom causes severe vertical misalignment (scale 1.61, SSIM 0.13, and >70% of rows missing from the 38-row capture).

## Reduction fixture

`tables/fit-to-width` in `tools/render-lab/fixtures.py` isolates fit-to-width print scaling from the real document's other features (110 `text:a` hyperlinks, 160 real contact entries, and complex multi-line addresses).

- **Grid**: 12 columns (A–L, width 18 each) and 20 rows of simple data (`Header <col>`, `R<row>C<col>`).
- **Page setup**: Landscape A4, `fitToWidth = 1`, `fitToHeight = 0` (unconstrained height), `fitToPage = True`.
- **Purpose**: The 12 columns exceed landscape A4 printable width at 100% scale, requiring Calc to scale them horizontally to fit 1 page width while allowing rows to paginate vertically.

## Reproduction

```sh
python3 tools/render-lab/fixtures.py /tmp/fixtures
tools/render-lab/run.sh --app tables
```

## Next steps

- Reconcile on-screen editor zoom and viewport capture with fit-to-page / fit-to-width print scaling references, or enhance `compare.py` tile/scale alignment.
- Correct `pick_sheet_page` in `tools/render-lab/compare.py` to prevent short trailing pages from outbidding the actual opening page when `ours` covers the top of a long sheet.
- Measure both tiers in CI and update `tools/render-lab/baseline-real.json` once green.
