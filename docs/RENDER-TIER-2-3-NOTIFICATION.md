# Tier 2/3 Notification of Dealing with a Non-UK Tier 1: Table Cell Content Control Reduction

Issue [#1359](https://github.com/tuna-os/gtk-office-suite/issues/1359) tracks the published UK Office of Financial Sanctions Implementation (OFSI) document:
`letters/real-tier-2-3-notification-of-dealing-with-a-non-uk-t` (`Tier_2_Tier_3_Notification_on_transacting_with_non-UK_Tier_1.docx`).
Document: "Tier 2/3 notification of dealing with a non-UK Tier 1 under INT/2022/2469656 or INT/2024/4423849"
URL: https://www.gov.uk/government/publications/russian-oil-services-ban (file https://assets.publishing.service.gov.uk/media/65d4cd13188d77001603876a/Tier_2_Tier_3_Notification_on_transacting_with_non-UK_Tier_1.docx).
Evidence: `render-real-letters` artifact of [main run 41190814907](https://github.com/tuna-os/gtk-office-suite/actions/runs/41190814907).

## Observed defect analysis

In run 41190814907, `letters/real-tier-2-3-notification-of-dealing-with-a-non-uk-t` scored:
- Tier A: **amber** (ink 1.07, words 96%, Δ position 3.3pt, colours –, SSIM 0.91, pages 6/6, lost_lines: 2)
- Tier B: **green** (ink 1.03, words 99%, Δ position 1.5pt, colours –, SSIM 0.87, pages 1/1, lost_lines: 0)

In Tier B, broadway capture captures only the first viewport (Page 1), which renders the intro section cleanly and achieves green on all metrics.
In Tier A, all 6 pages are rendered and evaluated:
- Words found is 95.7% (well above `GREEN_WORDS = 0.90`).
- Median position displacement is 3.3pt (well within `GREEN_DISP_PT = 6.0pt`).
- Structural similarity is 0.906 (well above `GREEN_SSIM = 0.75`).
- Ink ratio is 1.07 (well within valid range).
- However, `lost_lines: 2` is reported.
Because `verdict()` in `tools/render-lab/compare.py` requires `m["lost_lines"] == 0` for a green verdict, these 2 lost lines force the entire document to amber.

### Document and table layout analysis

The document spans 6 pages organized across 5 Parts:
- Pages 1–2: Introduction and guidance paragraphs (Paragraphs 0–27 in `word/document.xml`).
- Page 3: PART 1 – NOTIFIER’S DETAILS (Paragraphs 28–31 and Table 30: 10 rows).
- Page 4: PART 2 – YOUR DETAILS (Paragraphs 32–37 and Table 35: 6 rows).
- Page 5: PART 3 – DETAILS OF TIER 1 PROVIDER (Paragraphs 38–42 and Table 41: 3 rows) and PART 4 – NOTIFICATION OVERVIEW (Paragraphs 43–48 and Table 46: 4 rows).
- Page 6: PART 5 – DOCUMENTATION (Paragraphs 49–53 and Table 53: 4 rows) and trailing address/contact paragraphs (54–59).

Comparing per-page pixel metrics from `pixel-letters-A.json`:
- Page 1: ink mismatch 0.610, ink miss 1px 0.163
- Page 2: ink mismatch 0.627, ink miss 1px 0.089
- Page 3: ink mismatch 0.667, ink miss 1px 0.034
- Page 4: ink mismatch 0.438, ink miss 1px 0.001 (near-perfect match)
- Page 5: ink mismatch 0.822, ink miss 1px 0.537 (worst page across document)
- Page 6: ink mismatch 0.450, ink miss 1px 0.047

### Root cause: cell-level content controls (Structured Document Tags `w:sdt` in `w:tr`)

In WordprocessingML / OOXML, Structured Document Tags (`<w:sdt>`) can appear at run level (inside `<w:p>`), block level (inside `<w:body>`), or row/cell level (inside `<w:tr>`, wrapping `<w:tc>`).

In `word/document.xml` of `Tier_2_Tier_3_Notification_on_transacting_with_non-UK_Tier_1.docx`, Table 46 (PART 4 table) has a 2-column grid (`w:gridCol` widths: 2990 and 5885 dxa):
- **Row 0**: Column 0 is a standard `<w:tc>` containing `"Date of activity (of the contracted goods or services)"`.
  Column 1 is wrapped in a cell-level content control `<w:sdt>` as a direct child of `<w:tr>`, containing `<w:sdtContent><w:tc><w:tcPr><w:tcW w:w="5885" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>Click or tap to enter a date.</w:t></w:r></w:p></w:tc></w:sdtContent></w:sdt>`.
- **Row 1**: Column 0 is a standard `<w:tc>` containing `"Product(s) activity relates to"`.
  Column 1 is a standard `<w:tc>` (its dropdown `<w:sdt>` is nested inside the paragraph `<w:p>`).
- **Row 2**: Column 0 is a standard `<w:tc>` containing `"General Licence"`.
  Column 1 is wrapped in a cell-level content control `<w:sdt>` as a direct child of `<w:tr>`, containing `<w:sdtContent><w:tc><w:tcPr><w:tcW w:w="5885" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>Choose an item.</w:t></w:r></w:p></w:tc></w:sdtContent></w:sdt>`.
- **Row 3**: Column 0 is a standard `<w:tc>` containing `"Description of the activity..."`.
  Column 1 is an empty `<w:tc>`.

In LibreOffice Writer:
- Cell-level `<w:sdt>` elements inside `<w:tr>` are properly unpacked into their contained table cells `<w:tc>`.
- The table renders with uniform 2-column geometry across all 4 rows.
- Column 1 displays the placeholder text `"Click or tap to enter a date."` on Row 0 and `"Choose an item."` on Row 2.

In Letters (`letters-core/src/docx.rs`):
- Table parsing uses `rdocx` (`table.row(ri).cell(ci)`), which only inspects direct `<w:tc>` children of `<w:tr>`. Any sibling `<w:sdt>` elements inside `<w:tr>` are ignored and skipped.
- While `docx_chips::unwrap` strips inline controls and `docx_toc::unwrap` lifts block-level TOC controls, neither unwraps cell-level content controls inside `<w:tr>`.
- Consequently, Letters drops the second cell of Row 0 and Row 2 entirely:
  - Row 0 has only 1 cell, and the 7-word line `"Click or tap to enter a date."` is completely lost.
  - Row 2 has only 1 cell, and the 3-word line `"Choose an item."` is completely lost.
- In `compare.py`, `match_words` identifies both lost lines (`total >= LOST_LINE_MIN_WORDS and 2 * hits <= total`), yielding `lost_lines: 2`.
- This causes the amber verdict in Tier A.

## Synthetic reduction fixture

`tools/render-lab/fixtures.py` defines `letters/table-cell-content-control`:
- An independent DOCX document with a 3-row `Table Grid` table.
- Row 0 has Column 0 as a regular `<w:tc>` and Column 1 wrapped in `<w:sdt><w:sdtContent><w:tc>...<w:t>Click or tap to enter a date.</w:t>...</w:tc></w:sdtContent></w:sdt>`.
- Row 1 has Column 0 as a regular `<w:tc>` and Column 1 wrapped in `<w:sdt><w:sdtContent><w:tc>...<w:t>Choose an item.</w:t>...</w:tc></w:sdtContent></w:sdt>`.
- Row 2 has both columns as standard `<w:tc>` cells to verify following row geometry and alignment.
- Isolated from multi-page pagination, footers, headers, hyperlinks, or font substitutions.

## Testing and verification

- Generated fixtures using `python3 tools/render-lab/fixtures.py /tmp/fixtures`.
- Added unit test in `tests/test_render_lab_rules.py` verifying `TableCellContentControlFixtureTest`.
- Ran test suite: `python3 -m pytest tests/test_fixture_manifests.py tests/test_render_lab_rules.py tests/test_render_lab_words.py tests/test_render_real_scope.py tests/test_sync_issues.py` (all passed).
- Verified conformance with `python3 conformance/validate_fixtures.py` and `python3 conformance/validate_capabilities.py`.
- Formatted and linted with `ruff check tools/render-lab/fixtures.py tests/test_render_lab_rules.py --ignore E731`.

## Acceptance status

The synthetic fixture isolates the cell-level content control drop for focused implementation and verification.
As local environments lack the container engine/GUI build runtime to measure the full real corpus,
`tools/render-lab/baseline-real.json` remains untouched until measured on CI.
The real-document acceptance criteria (turning green in both Tier A and Tier B) remain open on #1359.
