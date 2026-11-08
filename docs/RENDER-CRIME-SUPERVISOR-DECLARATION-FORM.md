# Crime Supervisor Declaration Form: Table Row Minimum Height Pagination Reduction

Issue [#1353](https://github.com/tuna-os/gtk-office-suite/issues/1353) tracks the real-document corpus document:
`letters/real-crime-supervisor-declaration-form` (`Crime_Supervisor_Declaration_Form_.docx`, gov.uk, OGL-UK-3.0).
Publication: [Standard Crime Contract 2025](https://www.gov.uk/government/publications/standard-crime-contract-2025).
Source URL: `https://assets.publishing.service.gov.uk/media/68dd0139c487360cc70c9fab/Crime_Supervisor_Declaration_Form_.docx`
Evidence: `render-real-letters` artifact of [main run 41190814907](https://github.com/tuna-os/gtk-office-suite/actions/runs/41190814907).

## Observed defect analysis

In run 41190814907, `letters/real-crime-supervisor-declaration-form` scored:
- Tier A: **amber** (ink 0.97, words found 89%, Δ position 55.4pt, SSIM 0.60, pages 6/6, lost_lines: 12)
- Tier B: **amber** (ink 1.02, words found 100%, Δ position 3.5pt, SSIM 0.72, pages 1/1, lost_lines: 0)

The green acceptance thresholds require:
- Words found ≥ 90% (`GREEN_WORDS = 0.90`)
- Median displacement ≤ 6.0pt (`GREEN_DISP_PT = 6.0`)
- Structural similarity ≥ 0.75 (`GREEN_SSIM = 0.75`)
- Lost lines == 0 (`lost_lines == 0`)

### Root cause 1: table row minimum height pagination (`w:trHeight`)

In `Crime_Supervisor_Declaration_Form_.docx`:
- Page 2 contains Section 3: Legal Competence Standard for Supervisors (Table 2 in `word/document.xml`, containing 13 rows).
- Rows 0 through 5 are laid down on Page 2:
  - Row 0: `3. Legal Competence Standard for Supervisors`
  - Row 1: `i) Areas of Knowledge`
  - Row 2: `a) Has held a current non-conditional practising certificate...`
  - Row 3: `b) Has achieved the Criminal Litigation Accreditation Scheme (CLAS)...`
  - Row 4: `ii) Skills/ Procedure/ Knowledge...`
  - Row 5: `a) Has undertaken a minimum of 6 Police Station Advice and Assistance cases...` (with items 1 through 6)
- In LibreOffice Writer:
  - Row 5 bottom edge ends at y = 905px (678.75pt).
  - Page height is 841.9pt (1123px) with top and bottom margins of 1440 dxa = 72pt (96px), providing a printable content area ending at y = 1027px (769.9pt).
  - Available space on Page 2 after Row 5 is 1027 - 905 = 122px (91.15pt).
  - Row 6 is an internal section heading:
    `Skills/ Procedure/ Knowledge | Examples from last 12 months... | UFN/ Client Name | Offence Description... | Date last worked on`
    Row 6 declares `<w:trPr><w:trHeight w:val="1813"/></w:trPr>` (1813 dxa = 90.65pt = 120.87px).
    With cell padding (top/bottom) and border rules, Row 6 requires 93.75pt (125px).
  - Because 93.75pt > 91.15pt, Row 6 does not fit in the space remaining on Page 2.
  - LibreOffice honors the row's required minimum height, breaks the table after Row 5, and starts Row 6 at the top of Page 3.
- In Letters (`letters-core/src/layout/mod.rs`):
  - `layout_table()` only checks if a row fits whole when `doc.table_whole_rows.get(&first.table).contains(&row)` (the `w:cantSplit` property).
  - Because Row 6 does not carry `w:cantSplit`, Letters attempts to split Row 6 across pages.
  - It computes the cell slices: the 3 short text lines inside Row 6 require only ~35pt (46px).
  - Since 35pt fits within the 91.15pt remaining on Page 2, all slices are deemed to fit (`done == true`).
  - Line 1543 then clamps the row height to the remaining page space (`min_left.min(flow.bottom() - flow.y)`), placing Row 6 at the bottom of Page 2 (y = 903..1028px) and even overflowing slightly into the bottom margin.
  - As a result, Row 6 is orphaned at the bottom of Page 2 without any of its corresponding data rows.

### Domino effect across Pages 2–6

Because Row 6 was placed on Page 2 instead of starting Page 3, every subsequent page in Letters shifts by exactly one row:
- **Page 3**: In LibreOffice, Page 3 holds Row 6 (heading) and Row 7 (`bi) Has undertaken 20 examples of magistrates' court Representation...`, items 1..20). In Letters, Page 3 begins directly with Row 7 and has extra room at the foot, pulling Row 9 (`Skills/ Procedure/ Knowledge`) onto Page 3.
- **Page 4**: In Letters, Page 4 starts with Row 10 (1..10) and Row 11 (1..5) and Row 12, leaving space at the bottom for Table 3 (Section 4: Crime Case Involvement) to start. In LibreOffice, Section 4 begins at the top of Page 5.
- **Page 5**: In Letters, Table 3 finishes on Page 5, and Table 4 (Section 5: Declaration) starts at the bottom of Page 5, splitting Row 0 onto Page 5. In LibreOffice, Section 5 is placed entirely on Page 6.
- **Page 6**: In Letters, Page 6 has only Row 1 of Table 4.
This 1-row offset across 5 pages causes:
- 55.4pt median word displacement (threshold ≤ 6.0pt)
- 12 lost lines (threshold == 0)
- SSIM degradation to 0.60 (threshold ≥ 0.75)
- Words found drop to 89% (threshold ≥ 90%)

### Root cause 2: form field checkbox drop (`FORMCHECKBOX`)

In `Crime_Supervisor_Declaration_Form_.docx`:
- Section 2 (Generic Supervisor Requirements, Page 1) contains two form field checkboxes:
  `<w:fldChar w:fldCharType="begin"><w:ffData><w:checkBox><w:sizeAuto/><w:default w:val="0"/></w:checkBox></w:ffData></w:fldChar>`
  `<w:instrText xml:space="preserve"> FORMCHECKBOX </w:instrText>`
  `<w:fldChar w:fldCharType="separate"/><w:fldChar w:fldCharType="end"/>`
- Section 5 (Declaration, Page 6) contains two additional form field checkboxes (`Tick box to confirm [FORMCHECKBOX]`).
- LibreOffice renders `FORMCHECKBOX` fields as open square box glyphs `☐`.
- In Letters, `rdocx` parses text runs and omits field character markers (`fldChar` / `instrText`), completely dropping the checkbox glyphs.
- On Page 1, this missing glyph content contributes to Tier B SSIM degradation to 0.72 (threshold ≥ 0.75).

### Root cause 3: list numbering continuation across unnumbered paragraphs

In Section 5:
- Paragraphs 11, 13, 15, and 19 share `numId="6"`, separated by empty paragraphs and unnumbered text ("OR").
- In LibreOffice, numbering continues across the unnumbered paragraphs as items 1), 2), 3), and 4).
- In Letters, unnumbered paragraphs reset the list ordinal, causing items to render as `1.`, `1.`, `1.`, `1.`.
- In addition, `numId="7"` (paragraphs 5, 6, 7) uses lower-alpha numbering `a)`, `b)`, `c)` in LibreOffice, but defaults to decimal `1.`, `2.`, `3.` in Letters.

## Synthetic reduction fixture

`tools/render-lab/fixtures.py` defines `letters/table-row-min-height-page-break`:
- An independent DOCX document with 1-inch margins on all sides.
- A 2-row table with `Table Grid` style:
  - Row 0: height 7.5in (540pt = 10800 dxa), filling the upper portion of Page 1 and leaving ~108pt of remaining vertical space.
  - Row 1: contains short text (~15pt), but specifies `<w:trPr><w:trHeight w:val="3600"/></w:trPr>` (2.5in / 180pt / 3600 dxa).
- In LibreOffice Writer, because 180pt > 108pt remaining, Row 1 starts on Page 2 (2 pages total).
- In Letters (prior to fix), because Row 1 lacks `w:cantSplit` and its text fits in 108pt, Letters incorrectly clamps Row 1 onto Page 1 (1 page total).
- Isolates the table row minimum height pagination behavior into a single, clean test case.

## Testing and verification

- Generated all 74 render fixtures with `python3 tools/render-lab/fixtures.py /tmp/test-fixtures` successfully.
- Verified `table-row-min-height-page-break.docx` structure: 2 rows, 1 table, `Table Grid` style, `w:trHeight` elements with values `10800` and `3600`.
- Added unit test `TableRowMinHeightPageBreakFixtureTest` in `tests/test_render_lab_rules.py`.
- Ran test suite: `python3 -m pytest tests/test_fixture_manifests.py tests/test_render_lab_rules.py tests/test_render_lab_words.py tests/test_render_real_scope.py tests/test_sync_issues.py` (all 60 passed cleanly).
- Validated conformance with `python3 conformance/validate_fixtures.py` and `python3 conformance/validate_capabilities.py`.
- Formatted and linted with `ruff check tools/render-lab/fixtures.py tests/test_render_lab_rules.py --ignore E731`.
- Clean whitespace verified with `git diff --check`.

## Acceptance status

The synthetic fixture isolates the table row minimum height pagination defect for focused implementation and benchmarking.
As local environments lack the container engine/GUI build runtime to measure the full real corpus,
`tools/render-lab/baseline-real.json` remains untouched until measured on CI.
The real-document acceptance criteria (turning green in both Tier A and Tier B) remain tracked on #1353.
