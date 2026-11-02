# Reservoir contact list: horizontal pagination reduction

Issue #1352 compares the public [All reservoir panel engineers contact list](https://www.gov.uk/government/publications/contact-details-of-engineers-on-the-all-reservoirs-panel)
with LibreOffice. In [run 40077970330](https://github.com/tuna-os/gtk-office-suite/actions/runs/40077970330),
artifact `render-real-tables`, inspect
`tables/real-all-reservoir-panel-engineers-contact-list/lo-1.png`,
`A-1.png` and `B-1.png`.

The reference's first page contains Surname, Title and Forename. Tables
also shows Company name and address columns in the first capture. The
reference has five pages; comparing only its first page with a continuous
editor viewport therefore includes text from different horizontal pages.
This is one concrete mismatch, not proof that pagination explains every
red metric. The screenshots also differ in page margins and headings.

The source ODS `styles.xml` specifies portrait orientation and
`style:scale-to="100%"`. Its first column widths in `content.xml` are
3.3514, 2.1167, 5.8914, 6.1560, 5.0271 and 5.3975 cm: the first six
columns cannot fit across a portrait page at their stored scale.

`tables/horizontal-pagination` in `tools/render-lab/fixtures.py` reduces
this to six equal-width columns, two rows, portrait A4 and 100% scale,
with fit-to-page disabled. It uses synthetic text, no personal contact
information, no merges, no hyperlinks and no special cell styles. The
explicit print area includes all six columns; the reference must paginate
rather than shrink them. The existing `col-row-size` fixture tests cell
geometry, not the boundary between printed pages.

Generate and compare with:

```sh
python3 tools/render-lab/fixtures.py /tmp/reservoir-fixtures
tools/render-lab/run.sh --app tables
```

This reduction is a partial contribution. It does not fix rendering or
establish green tiers. Follow-up work must align the page and viewport
comparison, measure both tiers, check the other real documents, and run
`RENDER_LAB_CORPUS=real tools/render-lab/run.sh --app tables --update-baseline`
before changing `baseline-real.json` or closing #1352. The new fixture can
receive its own render-parity issue through the lab's usual issue sync.
