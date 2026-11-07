# Electricity markets workbook reduction (#1336)

The `render-real-tables` artifact from run `40077970330` shows two distinct
differences on the cover sheet of
`real-competition-in-uk-electricity-markets-2025-data`:

- LibreOffice prints the cover at a reduced size. The source's
  `xl/worksheets/sheet1.xml` has `<pageSetup paperSize="9" scale="46" …>`.
  Tables captures the editing grid at screen size. The reported 2.33× scale
  is consistent with this reduction (100/46 is approximately 2.17, with
  headings and ink cropping also contributing to the measured ratio).
- Column A has width `158.7109375`. The Tables capture cuts off the long
  description in row 2 at its right edge; LibreOffice's printed cover shows
  the complete description. This is a separate viewport/capture limitation.

`tables/print-scale` in `tools/render-lab/fixtures.py` isolates the first
difference. It repeats the existing `tables/values` grid, changing only
`page_setup.scale` to 46. It fits in the capture, avoiding the cover sheet's
wide column, long text, font changes, hyperlinks and multiple worksheets.
Both fixtures use the same print-heading and gridline settings.

Generate the corpus and compare the two fixtures with:

```sh
python3 tools/render-lab/fixtures.py /tmp/render-fixtures
tools/render-lab/run.sh --app tables
```

This reduction does not resolve #1336. The screen-versus-print scale
comparison and the clipped wide-column capture still need fixing and
measurement. In particular, reducing the editor's zoom to a document's
print percentage would change normal editing behavior merely to match the
PDF oracle. No comparison thresholds or real-document baselines are changed
by this reduction; update `baseline-real.json` only after measuring both
tiers and checking the real-document ratchet.
