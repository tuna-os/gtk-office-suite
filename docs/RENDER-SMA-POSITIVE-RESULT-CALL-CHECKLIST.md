# SMA Positive Result Call Checklist: Table Cell List Indent Reduction

Issue [#1365](https://github.com/tuna-os/gtk-office-suite/issues/1365) tracks the published checklist document:
`letters/real-sma-positive-result-call-checklist` (`20260929_SMA_positive_result_call_checklist_FINAL.docx`).
Evidence: `render-real-letters` artifact of [main run 41190814907](https://github.com/tuna-os/gtk-office-suite/actions/runs/41190814907).

## Observed defect analysis

In run 41190814907, `letters/real-sma-positive-result-call-checklist` scored:
- Tier A: **amber** (ink 1.06, words 99%, Δ position 11.5pt, colours 100%, SSIM 0.85, pages 2/2)
- Tier B: **green** (ink 1.04, words 99%, Δ position 5.2pt, SSIM 0.85, pages 1/1)

The green threshold for word position displacement (`GREEN_DISP_PT`) is ≤ 6.0pt.
Tier B only scores Page 1 (viewport), where displacement is 5.2pt. Tier A scores both Page 1 and Page 2,
with Page 2 displacement exceeding 17pt, pulling the overall Tier A mean to 11.5pt.

### Grid and bounding comparison

On Page 2, LibreOffice Writer and Letters render the exact same 2-column table geometry:
- Left table border: x = 76px
- Column divider: x = 642px
- Right table border: x = 677px
- Column 0 width: 566px (424.5pt)
- Column 1 width: 35px (26.25pt)

However, horizontal row boundaries on Page 2 diverge:
- LibreOffice: Row 6 cont. y = 28..60; Row 7 y = 60..169 (h = 109px); Row 8 y = 169..245 (h = 76px); Table bottom y = 245; Signed line y = 312.
- Letters: Row 6 cont. y = 28..59; Row 7 y = 59..185 (h = 126px); Row 8 y = 185..277 (h = 92px); Table bottom y = 277; Signed line y = 343.

In Letters, Row 7 is 17px taller (+1 line of text) and Row 8 is 16px taller (+1 line of text).
The accumulated +33px (+24.75pt) height pushes the bottom of the table and the following "Signed..." paragraph down by 31px (23.25pt).

### Root cause: table cell list item indent

In `sma-positive-result-call-checklist.docx`, table cells in Rows 7 and 8 contain bulleted list items styled as `List Paragraph` with `w:numPr`:
- `word/styles.xml` defines `<w:style w:styleId="ListParagraph"><w:pPr><w:ind w:left="720"/></w:pPr></w:style>` (36pt left indent).
- `word/numbering.xml` defines `<w:lvl w:ilvl="0"><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl>` (36pt text indent, 18pt hanging marker).

In LibreOffice Writer:
- For a list item inside a table cell, the bullet hangs in front at x = 103..108px (18pt from padding edge).
- Both the first line and wrapped continuation lines start at x = 128px (36pt from padding edge).
- The full available cell text width is 566px − 52px = 514px (385.5pt), allowing the long items in Rows 7 and 8 to fit within 2 and 3 lines respectively.

In Letters (`letters-core/src/docx.rs`):
- `list_indent_from_declared(&doc, p, &mut para.style)` adjusts body paragraphs (line 1179) so `left_indent_pt = declared - text_indent(level)`.
- However, `list_indent_from_declared` was omitted for table cell paragraphs in `for_each_nested_paragraph` / `CellItemRef::Paragraph` (line 1279).
- Consequently, table cell list items retain the style's declared `left_indent_pt = 36.0pt`, and `layout::paragraph_box_x` adds `lists::text_indent_pt(0) = 18.0pt` on top, totaling 54.0pt (72px).
- This extra 18.0pt (24px) indent shifts wrapped text to x = 152px, narrowing the wrapping width in the cell by 24px.
- The reduced width forces wrapped lines to wrap one line early in Row 7 and Row 8, causing the vertical displacement on Page 2.

## Synthetic reduction fixture

`tools/render-lab/fixtures.py` defines `letters/table-cell-list-indent`:
- An independent DOCX document with a 2-row, 1-column `Table Grid` table.
- Row 0 contains an unindented header paragraph followed by two wrapping bullet list items (`List Bullet`).
- Row 1 contains a following paragraph to verify row height and subsequent line positioning.
- Isolated from multi-page pagination, footers, headers, hyperlinks, or font substitutions.

## Testing and verification

- Generated fixtures using `python3 tools/render-lab/fixtures.py /tmp/fixtures`.
- Added unit test in `tests/test_render_lab_rules.py` verifying `TableCellListIndentFixtureTest`.
- Ran test suite: `python3 -m pytest tests/test_fixture_manifests.py tests/test_render_lab_rules.py tests/test_render_lab_words.py tests/test_render_real_scope.py tests/test_sync_issues.py` (all passed).
- Verified conformance with `python3 conformance/validate_fixtures.py` and `python3 conformance/validate_capabilities.py`.
- Formatted and linted with `ruff check tools/render-lab/fixtures.py tests/test_render_lab_rules.py --ignore E731`.

## Acceptance status

The synthetic fixture isolates the table cell list indent mismatch for focused comparison.
As local environments lack the container engine/GUI build runtime to measure the full real corpus,
`tools/render-lab/baseline-real.json` remains untouched until measured on CI.
The real-document acceptance criteria (turning green in both Tier A and Tier B) remain open on #1365.
