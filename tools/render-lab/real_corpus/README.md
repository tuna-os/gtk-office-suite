# Real-document corpus (#1200)

The render lab's single-feature fixtures (`../fixtures.py`) are built to
find bugs one feature at a time. This corpus answers a different
question: would someone opening a real document notice? It holds about
30 published documents per app, rendered and scored exactly like the
fixtures against LibreOffice.

| app | formats | documents | source |
|---|---|---|---|
| Letters | docx, odt | 30 (≤ 8 pages) | GOV.UK publications |
| Tables | xlsx, ods | 30 (≤ 20 pages) | GOV.UK publications |
| Decks | pptx | 30 (≤ 20 slides) | 3 GOV.UK, 27 open.canada.ca (English) |

`manifest.json` lists each document with its download `url`, the
publication `page` it came from, `license`, `publisher` (where the
portal records one), `sha256`, size, the page count LibreOffice renders
(`lo_pages`) and the date it was retrieved. The documents themselves are
**not committed**: the decks alone are 26 MB of pictures.

## Running it

    RENDER_LAB_CORPUS=real tools/render-lab/run.sh --app letters

`fetch_real_corpus.py` downloads each document into the fixtures
directory, caching it by sha256 in `.cache/render-lab-corpus/` (CI caches
the same directory keyed on this manifest). A document whose sha256 no
longer matches was replaced by its publisher and is skipped, because its
verdict would no longer mean the same thing. A skipped or unreachable
document shows as `missing` against its verdict in
`../baseline-real.json`, so link rot is visible instead of silently
shrinking the corpus. To replace a document, pick another from the same
source and update its entry (url, page, sha256, bytes, lo_pages,
retrieved).

The verdicts ratchet against `../baseline-real.json`, separately from
the fixtures' `baseline.json`. `.github/workflows/render-real.yml` runs
the corpus nightly, on demand, and on any PR that changes the corpus or
the lab driver. On main, each document that is not green has one
`render-real` issue (`sync_issues.py --mode real`) that names the
document, its licence and how to reproduce it, and closes itself when the
document turns green. Real documents mix many features, so the way to
move one is to reduce its defect to a single-feature fixture, which gets
its own `render-parity` issue; a regression fails the ratchet instead.

## Licences and attribution

Every document is published under an open government licence that
permits copying, redistribution and adaptation, provided the source is
acknowledged:

- **GOV.UK documents** (63; `"license": "OGL-UK-3.0"`): Contains public
  sector information licensed under the
  [Open Government Licence v3.0](https://www.nationalarchives.gov.uk/doc/open-government-licence/version/3/).
- **open.canada.ca documents** (27; `"license": "OGL-Canada-2.0"`):
  Contains information licensed under the
  [Open Government Licence – Canada](https://open.canada.ca/en/open-government-licence-canada).
  The publishing department is recorded in each entry's `publisher`.

Each entry's `page` is the publication it was taken from. Neither the
UK nor the Canadian Government endorses this project.
