# Forecast Evaluation Report: Wide Unprinted Column Alignment Reduction

Issue [#1333](https://github.com/tuna-os/gtk-office-suite/issues/1333) tracks the published economic model database:
`tables/real-june-2026-forecast-evaluation-report-economic-mo` (`Forecast-evaluation-report-June-2026-economic-model-development-database.xlsx`).
Evidence: `render-real-tables` artifact of [main run 42211548166](https://github.com/tuna-os/gtk-office-suite/actions/runs/42211548166).

## Observed defect analysis

In run 42211548166, `tables/real-june-2026-forecast-evaluation-report-economic-mo` scored:
- Tier A: **amber** (ink 0.98, words 91%, Δ position 89.6pt, colours 100%, SSIM 0.15, scale 1.22×, pages 1/1)
- Tier B: **amber** (ink 0.99, words 91%, Δ position 89.6pt, colours 100%, SSIM 0.15, scale 1.22×, pages 1/1)

The green threshold requires scale within ±10% (`abs(scale - 1.0) <= 0.10`), displacement (`GREEN_DISP_PT`) ≤ 6.0pt, and `SSIM` ≥ 0.75.
While word recognition (91%) and color presence (100%) meet green thresholds, scale (1.22×), displacement (89.6pt), and SSIM (0.15) fail.

### Geometry and print clipping comparison

The workbook's first sheet (`Contents`) defines:
- Column A: width = 8.78 characters (~64px / 48pt)
- Column B: width = 119 characters (~838px / 628.5pt)
- Row 2: Right-aligned, top-aligned multiline heading ("June 2026 Forecast evaluation report\n2 June 2026", Calibri 20, ht=72pt, `customHeight="1"`)
- Row 3: Centered title in an orange/tan fill box ("Economic model  development database", Calibri 16, ht=21.95pt, `customHeight="1"`)
- Rows 4 and 5: Left-aligned wrapped explanatory paragraphs (Calibri 12, ht=69pt and ht=47.25pt)
- Rows 7, 8, and 9: Section heading and hyperlinked contents list with cell indentation (`indent="2"`)
- Print options: `headings=False` (unprinted row/column headings)
- Page setup: A4 portrait (`paperSize="9"`, margins 0.7in left/right)

In LibreOffice Calc's headless PDF conversion (`lo_render.py`):
1. Printable page width between 0.7in margins on A4 portrait is ~494.5pt (~659.3px at 96 DPI).
2. The total width of Columns A and B is ~676.5pt (~902px). Because Column B exceeds the printable page width, Calc clips Column B at the right page margin (~660px).
3. In Row 2, the multiline title is right-aligned in Column B (sitting at x ≈ 550..902px). Calc clips the text at the page margin, leaving only the first letters ("June 2026 F...") visible on Page 1 before the edge.
4. In Tables on screen (`A-1.png` and `B-1.png`), the viewport renders the full 838px width of Column B, showing the full right-aligned heading.

### Scorer alignment gap with unprinted headings

Because `headings=False`, LibreOffice Calc prints no black-bordered row/column heading boxes.
In `tools/render-lab/compare.py`:
- `lo_cell_box(ref)` requires at least 3 horizontal and 3 vertical frame lines, and returns `None` for unprinted sheets.
- When `lo_cell_box` is `None`, `compare.py:align` cannot determine heading box coordinates and does not crop screen header bands (`ox, oy` for row headers `1..9` and column headers `A, B`).
- `compare.align` crops `ref` to its ink (728×369 px, clipped at the right margin) and resizes the entire unstripped 934×429 screen capture onto it.
- This creates an artificial scale distortion of 1.22× (934 / 728 = 1.28× horizontal ratio), misaligns the row and column furniture with cell data, and inflates word displacement to 89.6pt.

## Synthetic reduction fixture

`tools/render-lab/fixtures.py` defines `tables/wide-column-right-align`:
- A single-sheet XLSX document with `headings=False`.
- Column A (width 8.78) and Column B (width 119) matching the unprinted wide-column layout.
- Row 2: Right-aligned, top-aligned multiline heading (`Alignment(horizontal="right", vertical="top", wrap_text=True)`).
- Row 3: Centered title with solid fill.
- Row 4: Indented entry with `Alignment(horizontal="left", indent=2)`.
- Isolated from multi-sheet workbooks, complex external hyperlinks, or printer-specific binary settings.

## Acceptance status

The synthetic fixture isolates wide-column right-alignment, centered title layout, and cell indentation under unprinted headings for targeted evaluation.
As local environments lack the container engine/GUI build runtime to measure the full real corpus, `tools/render-lab/baseline-real.json` remains untouched until measured on CI.
The real-document acceptance criteria (turning green in both Tier A and Tier B) remain open on #1333.
