# Air safety course application form: print-scale reduction

Issue #1338 concerns `tables/real-air-safety-course-application-form-dhasc-fsc-fla`.
The corpus manifest pins the source workbook to SHA-256
`6449121b1eb6cc35299d792462d1fad94617b340400f111a5d6686302f3e4de5`.

The first worksheet's XML specifies A4 portrait paper and an explicit
`pageSetup scale="53"`. The workbook also defines a print area of
`'CoAST App Form'!$A$2:$K$42` and a manual row break after row 26.
Its saved screen zoom is 90%, which is a separate setting from print scale.

`tables/air-safety-reduced-print-scale` isolates the explicit 53% print
scale on the same 5-by-4 values block as `tables/values`. It fits on one
page at 100%, so pagination and fit-to-page cannot explain a size change.
It omits the real document's merges, pictures, print area and manual break.
LibreOffice should print the block and headings at 53% of their normal size.

This is a reproduction fixture, not a completed rendering fix. Tables'
render-dump action captures the screen grid, while LibreOffice's reference
comes from PDF printing. The new fixture makes that scale difference
independently measurable; it does not establish that scale is the only
cause of the real document's red verdict. The real document still needs
the print area, page breaks, drawings and clipping investigated against
rendered evidence. Keep its baseline red until both tiers are measured
green, and check the rest of the real corpus before updating it.
