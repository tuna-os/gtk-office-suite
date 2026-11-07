# DCMS Business Appointment Rules: Bullet Symbol Font Line Height Reduction

Issue [#1364](https://github.com/tuna-os/gtk-office-suite/issues/1364) tracks the real-document corpus document:
`letters/real-dcms-business-appointment-rules-between-october` (`DCMS_Business_Appointment_Rules_between_October_and_December_2019.docx`, gov.uk, OGL-UK-3.0).
Evidence: `render-real-letters` artifact of [main run 41190814907](https://github.com/tuna-os/gtk-office-suite/actions/runs/41190814907).

## Observed defect analysis

In run 41190814907, `letters/real-dcms-business-appointment-rules-between-october` scored:
- Tier A: **amber** (ink 1.21, words found 99%, Δ position 59.0pt, SSIM 0.54, pages 3/3)
- Tier B: **amber** (ink 1.04, words found 96%, Δ position 36.1pt, SSIM 0.53, pages 1/1)

The green acceptance threshold requires Δ position ≤ 6.0pt and SSIM ≥ 0.75.

### Root cause: bullet marker symbol font line height

In `dcms-business-appointment-rules-between-october.docx`:
- In `word/numbering.xml`, list level 0 (`<w:lvl w:ilvl="0">`) specifies bullet glyph `●` (U+25CF) formatted with font `<w:rFonts w:ascii="Noto Sans Symbols" w:hAnsi="Noto Sans Symbols" .../>`.
- The document body contains 12 bulleted list paragraphs on Page 1 (`p[5]`, `p[7]`, `p[9]`, `p[11]`, `p[13]`, `p[14]`, `p[15]`, `p[16]`, `p[18]`, `p[20]`, `p[22]`, `p[24]`).
- In LibreOffice Writer, the list bullet marker is treated as an inline text run on the first line. Because `Noto Sans Symbols` at 12pt has large vertical metrics (ascent and descent spanning ~24.75pt / 33px), the first line of each bullet list item expands to a line height of 33px. Continuation lines contain only body text (`Arial` 12pt) and wrap with standard line height (~14.25pt / 19–22px).
- In Letters (`letters-core/src/layout/mod.rs` and `pango.rs`), line height calculation (`line_height()`) only inspects the paragraph's body text runs (`para.runs`). The bullet marker is rendered separately as a synthetic vector circle (`bullet_path()`) drawn at the baseline and is omitted from line shaping and height calculation. Consequently, Letters sizes the first line of each list item at only 19px rather than 33px (a 14px shortfall per item).

### Ripple effect on vertical pagination

- Across the 12 bulleted list items on Page 1, this 14px deficit accumulates into ~115px (86.25pt) of vertical shrinkage:
  - In LibreOffice Writer, `p[24]` ends at y = 930px. The remaining printable space on Page 1 accommodates only a few of the 11 trailing empty paragraphs (`p[25]`..`p[35]`), causing 5–6 empty paragraphs to overflow onto the top of Page 2. On Page 2, the section heading (`p[36]`) starts at y = 162px and Table 1 starts at y = 308px; on Page 3, Table 2 starts at y = 194px.
  - In Letters, because Page 1 is underfilled (`p[24]` ends at y = 815px), all 11 empty paragraphs fit onto Page 1 without overflowing. As a result, Page 2 begins with heading `p[36]` at y = 69px (shifted up by 93px / ~70pt), Table 1 starts at y = 215px (shifted up by 93px), and Page 3's Table 2 starts at y = 102px (shifted up by 92px).
- This uniform ~70pt upward shift across Pages 2 and 3 causes the 59.0pt Δ position displacement on Tier A and 36.1pt on Tier B, ambering both tiers.

## Synthetic reduction fixture

`tools/render-lab/fixtures.py` defines `letters/bullet-symbol-font-line-height`:
- An independent DOCX document with an introductory paragraph, two wrapping bullet list items (`style="List Bullet"`), and a following paragraph.
- In `word/numbering.xml`, level 0 specifies bullet character `●` and `<w:rFonts w:ascii="Noto Sans Symbols" w:hAnsi="Noto Sans Symbols"/>`.
- Isolates the symbol font bullet line height expansion and continuation line wrapping from multi-page pagination, empty paragraphs, and tables.

## Testing and verification

- Generated all 74 render fixtures with `python3 tools/render-lab/fixtures.py /tmp/test-fixtures` successfully.
- Verified `bullet-symbol-font-line-height.docx` structure: 4 paragraphs, `List Bullet` styles, and level 0 `Noto Sans Symbols` font in `word/numbering.xml`.
- Added unit test `BulletSymbolFontLineHeightFixtureTest` in `tests/test_render_lab_rules.py`.
- Ran test suite: `python3 -m pytest tests/test_fixture_manifests.py tests/test_render_lab_rules.py tests/test_render_lab_words.py tests/test_render_real_scope.py tests/test_sync_issues.py` (all 73 passed cleanly).
- Validated conformance with `python3 conformance/validate_fixtures.py` and `python3 conformance/validate_capabilities.py`.
- Formatted and linted with `ruff check tools/render-lab/fixtures.py tests/test_render_lab_rules.py --ignore E731`.
- Clean whitespace verified with `git diff --check`.

## Acceptance status

The synthetic fixture isolates the bullet symbol font line height behavior for focused comparison.
As local environments lack the container engine/GUI build runtime to measure the full real corpus,
`tools/render-lab/baseline-real.json` remains untouched until measured on CI.
The real-document acceptance criteria (turning green in both Tier A and Tier B) remain open on #1364.
