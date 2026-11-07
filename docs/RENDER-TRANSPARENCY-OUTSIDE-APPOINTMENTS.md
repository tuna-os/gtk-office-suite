# Transparency Information About Outside Appointments: Table Cell Numbered List Indent Reduction

Issue [#1358](https://github.com/tuna-os/gtk-office-suite/issues/1358) tracks the published transparency document:
`letters/real-transparency-information-about-outside-appointme` (`20180321_SA_BAR.docx`).
Document: "Transparency information about outside appointments and employment taken up by former members of the Cabinet Office: October to December 2017"
URL: https://www.gov.uk/government/publications/cabinet-office-business-appointment-rules-advice (file https://assets.publishing.service.gov.uk/media/5ab3fcde40f0b67d6747962c/20180321_SA_BAR.docx).
Evidence: `render-real-letters` artifact of [main run 41190814907](https://github.com/tuna-os/gtk-office-suite/actions/runs/41190814907).

## Observed defect analysis

In run 41190814907, `letters/real-transparency-information-about-outside-appointme` scored:
- Tier A: **amber** (ink 1.05, words 100%, Δ position 17.8pt, colours –, SSIM 0.92, pages 1/1)
- Tier B: **amber** (ink 1.35, words 100%, Δ position 17.3pt, colours –, SSIM 0.90, pages 1/1)

The green threshold for word position displacement (`GREEN_DISP_PT`) is ≤ 6.0pt.
Both Tier A (17.8pt) and Tier B (17.3pt) substantially exceed the threshold, while words found is 100% and SSIM is ≥ 0.90.

### Grid and bounding comparison

The document consists of a single 2-column, 6-row table:
- Left table border: x = 112px
- Column divider: x = 371px
- Right table border: x = 666px
- Column 0 width: 259px (194.25pt)
- Column 1 width: 295px (221.25pt)

Rows 0 through 4 have identical horizontal boundaries in LibreOffice Writer and Letters:
- Row 0: y = 96..147 (h = 51px)
- Row 1: y = 147..235 (h = 88px)
- Row 2: y = 235..304 (h = 69px)
- Row 3: y = 304..355 (h = 51px)
- Row 4: y = 355..425 (h = 70px)

Only Row 5 diverges:
- LibreOffice: Row 5 y = 425..699 (h = 274px, table bottom at y = 699px)
- Letters: Row 5 y = 425..717 (h = 292px, table bottom at y = 717px)

In Letters, Row 5 is 18px taller (+13.5pt / +1 line of text).

### Root cause: table cell numbered list item indent

In `20180321_SA_BAR.docx`, Row 5 Column 1 contains two numbered list items styled as `NormalWeb` with `w:numPr`:
- `word/numbering.xml` defines `abstractNumId="0"` with `<w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/><w:ind w:left="720" w:hanging="360"/></w:lvl>` (36pt text indent, 18pt hanging marker).
- The paragraphs in Row 5 Column 1 specify an explicit left indent: `<w:ind w:left="405"/>` (405 twips = 20.25pt = 27px at 96 DPI).

In LibreOffice Writer:
- For a numbered list item inside a table cell with declared indent `w:ind w:left="405"`, the numeric marker ("1.", "2.") hangs 18pt in front at `x = 382px` (2.25pt from cell padding edge).
- Both the first line and wrapped continuation lines start at the declared text indent `x = 406px` (20.25pt from cell padding edge).
- The full available cell text width is 260px (195pt), allowing item 2 to fit within 10 lines, ending at `y = 678px`.

In Letters (`letters-core/src/docx.rs`):
- `list_indent_from_declared(&doc, p, &mut para.style)` adjusts body paragraphs (line 1179) so `left_indent_pt = declared - text_indent(level)`.
- However, `list_indent_from_declared` was omitted for table cell paragraphs in `for_each_nested_paragraph` / `CellItemRef::Paragraph` (line 1279).
- Consequently, table cell list items retain the declared `left_indent_pt = 20.25pt` (27px), and `layout::paragraph_box_x` adds `lists::text_indent_pt(0) = 18.0pt` (24px) on top, totaling 38.25pt (51px).
- This extra 18.0pt (24px) indent shifts the marker from `x = 382px` to `x = 406px` (onto the text line) and shifts wrapped text from `x = 406px` to `x = 430px`.
- The narrowed cell text width (236px instead of 260px) forces wrapped lines to wrap early, spilling onto an extra line (+1 line, ending at `y = 696px`).
- The expanded row height (+18px / +13.5pt) and horizontal text shift produce a median word displacement of 17.8pt across the row, causing the amber verdict in Tier A and Tier B.

## Synthetic reduction fixture

`tools/render-lab/fixtures.py` defines `letters/table-cell-numbered-list-indent`:
- An independent DOCX document with a 2-row, 1-column `Table Grid` table.
- Row 0 contains an unindented header paragraph followed by two wrapping numbered list items (`List Number`) with declared `left_indent = Pt(20.25)`.
- Row 1 contains a following paragraph to verify row height and subsequent line positioning.
- Isolated from multi-page pagination, footers, headers, hyperlinks, or font substitutions.

## Testing and verification

- Generated fixtures using `python3 tools/render-lab/fixtures.py /tmp/fixtures`.
- Added unit test in `tests/test_render_lab_rules.py` verifying `TableCellNumberedListIndentFixtureTest`.
- Ran test suite: `python3 -m pytest tests/test_fixture_manifests.py tests/test_render_lab_rules.py tests/test_render_lab_words.py tests/test_render_real_scope.py tests/test_sync_issues.py` (all passed).
- Verified conformance with `python3 conformance/validate_fixtures.py` and `python3 conformance/validate_capabilities.py`.
- Formatted and linted with `ruff check tools/render-lab/fixtures.py tests/test_render_lab_rules.py --ignore E731`.

## Acceptance status

The synthetic fixture isolates the table cell numbered list indent mismatch for focused comparison.
As local environments lack the container engine/GUI build runtime to measure the full real corpus,
`tools/render-lab/baseline-real.json` remains untouched until measured on CI.
The real-document acceptance criteria (turning green in both Tier A and Tier B) remain open on #1358.
