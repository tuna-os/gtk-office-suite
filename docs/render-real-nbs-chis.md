# NBS CHIS KPI submission template: worksheet-image reduction

Tracking: [#1337](https://github.com/tuna-os/gtk-office-suite/issues/1337).

The `render-real-tables` artifact from
[run 40077970330](https://github.com/tuna-os/gtk-office-suite/actions/runs/40077970330)
shows an NHS logo above the first blue title band in `lo-1.png`. The same
area in both `A-1.png` and `B-1.png` is blank. This is a missing drawing,
independent of the text size difference between the screen and print views.

The published workbook's first worksheet references a drawing part. Its
`xl/drawings/drawing1.xml` contains a `twoCellAnchor` with `editAs="oneCell"`,
a `pic`, and an embedded image relationship. Tables currently loads chart
drawings (`tables-core/src/io/charts.rs`), but has no corresponding worksheet
picture model or canvas rendering path.

`tables/worksheet-image` in `tools/render-lab/fixtures.py` reduces this to a
single embedded PNG with the same anchor type. It uses the lab's original
four-colour image rather than copying the NHS logo. The image spans B2 to
D7; labels in A1 and D8 bound the used range so the image is inside the
captured grid even when its drawing is ignored. There are no charts,
merges, rich-text runs or print scaling settings in the reduction.

Generate and compare it with the other Tables fixtures:

```sh
tools/render-lab/run.sh --app tables
```

The render-parity issue synchronizer can track this feature separately from
the real workbook. A renderer fix needs to load the embedded picture through
the worksheet/drawing relationships, retain its anchor and dimensions, and
paint the image at that position on the grid. A round-trip-only test would
not establish that the picture appears.

This reduction does not resolve #1337. The real workbook also has fit-to-page
printing enabled with a stored scale of 57, and its screenshots show
differences in rich-text colour and hyperlink styling. The first report's
scale metric is 1.65, but it compares a screen grid with a printed sheet;
it does not by itself establish an incorrect cell font size. After renderer
changes, rerun both tiers of the real corpus, inspect the images, and update
`tools/render-lab/baseline-real.json` from the measured result. No baseline
verdict is changed by this reduction.
