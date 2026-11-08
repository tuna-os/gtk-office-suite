# Fruit and Vegetable Aid Scheme Annual Stats 2025 reduction (#1334)

The `render-real-tables` artifact from run `40077970330` shows why
`tables/real-fruit-and-vegetable-aid-scheme-annual-stats-2025` is amber
in Tiers A and B (scale 2.27×, word recall 61–63%, Δ position 96.6 pt, SSIM 0.12,
lost lines 5–6).

## Root cause analysis

The source document (`F___V_Annual_Stats_2025.xlsx`, OGL-UK-3.0, retrieved
2026-10-01, SHA-256 `3aa893761bbf708e49ebdb6c44f561726cbfb846747c3a1a6132bec7c5713352`)
has dimension `A1:D156` across a single sheet. Two distinct defects cause the
amber verdict:

### 1. Multi-page column pagination ("Down, then across" vs screen viewport)

- The sheet defines wide custom columns in `xl/worksheets/sheet1.xml`:
  - Column A: width `44.81640625` (~377 px ink width)
  - Column B: width `42.0` (~354 px ink width)
  - Column C: width `59.453125` (~657 px ink width)
  - Column D: width `18.54296875`
- The combined width of the used columns (~1400 px) far exceeds the standard
  A4 portrait printable width (~660 px between default 0.7" margins).
- When LibreOffice Calc converts the sheet to PDF in headless mode without
  explicit page setup, it prints in the default "Down, then across" sequence
  over 16 pages:
  - Pages 1–4: Column A across 4 vertical page spans (rows 1–33, 34–66, 67–99, 100–156).
  - Pages 5–8: Column B across 4 vertical page spans.
  - Pages 9–12: Column C across 4 vertical page spans.
  - Pages 13–16: Column D across 4 vertical page spans.
- In Tables (`A-1.png` and `B-1.png`), the application captures the continuous
  on-screen editing grid (rows 1–57, columns A, B, and C all visible side-by-side
  in the viewport).
- The render-lab comparison harness matches `A-1.png` against `lo-1.png` (which
  contains only Column A for rows 1–33).
- During `align()`, our on-screen grid (~1080 px wide across columns A, B, and C)
  is resampled into `lo-1.png`'s single column bounding box (~377 px wide). This
  causes:
  - `scale = 2.27×` (reported ratio of our grid width to the reference column width);
  - Column A being compressed into the left ~150 px of the reference, causing
    massive displacement (Δ position 96.6 pt) and SSIM collapse (0.12).

### 2. External workbook formulas vs cached precalculated values

- Cells in Column C (rows 7–10, 14–18, 37–57) contain formulas referencing an
  external workbook (e.g., `=SUM('[1]1:34'!E4)`).
- The XLSX package stores precalculated values in `<v>` tags (such as `1051`,
  `756`, `174`, `1041`, `1149753193.29`, `39887548.52`).
- In LibreOffice Calc (and Excel) headless rendering, when external workbooks
  cannot be resolved, the cached `<v>` values are displayed.
- Tables (`tables-core/src/io/load.rs`) submits the raw formula string to IronCalc.
  Because external workbook references cannot be resolved, IronCalc evaluates
  them to `#REF!`, overwriting the precalculated values and dropping word
  recall to 61–63%.

## Reduction fixture

`tables/paginated-columns` in `tools/render-lab/fixtures.py` isolates the first
defect (down-then-across column pagination):

- It defines a 3-column, 40-row sheet with column widths 45, 42, and 60.
- Exceeds A4 portrait printable width and spans 2 vertical pages.
- LibreOffice Calc paginates it into 6 pages:
  - Pages 1–2: Column A (rows 1–33, 34–40)
  - Pages 3–4: Column B (rows 1–33, 34–40)
  - Pages 5–6: Column C (rows 1–33, 34–40)
- Tables renders all three columns side-by-side in its on-screen grid.
- This isolates column pagination from external formula evaluation, merged
  cells, font variations, and other real-corpus complexities.

Generate the corpus with:

```sh
python3 tools/render-lab/fixtures.py /tmp/fixtures
tools/render-lab/run.sh --app tables
```

## Next steps

1. In `tools/render-lab/compare.py`, support horizontal tile stitching for
   workbooks with multi-page vertical spans (down-then-across pagination),
   matching column stripes against the on-screen viewport.
2. In `tables-core/src/io/load.rs`, preserve cached formula `<v>` values when
   external workbook reference formulas evaluate to `#REF!`.
3. Update `tools/render-lab/baseline-real.json` once green verdicts are measured
   in CI.
