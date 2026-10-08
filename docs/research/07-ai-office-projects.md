# Newer office projects: what we can take

> Survey of 2026's agent-oriented office projects for code we can reuse.
> Date: 2026-10-08. Each project was cloned and read, never built or run.

## Licences

| Project | Licence | Usable by us (GPL-3.0) |
|---|---|---|
| [BetterOffice](https://github.com/xhayankhan/betteroffice) (Rust) | Apache-2.0; NOTICE "Copyright 2026 Elia Hilse, The OpenOOXML Project" | Yes. Leave out the OFL fonts in `packages/fonts*`, `xlsx-raster/assets`. Its CLA allows relicensing later releases, so pin exact versions. |
| [GenOffice](https://github.com/genspark-ai/genoffice) (TS + Rust sidecar) | Apache-2.0; NOTICE "Mainfunc, Inc." | Yes, except `ee/` (Enterprise licence, no distribution) and `packages/pptx-engine/src/vendor/mtx` (MPL-2.0). |
| [HermesOffice](https://github.com/criptogus/HermesOffice) | Apache-2.0 | It is a fork of an older GenOffice snapshot that adds only agent integration. Use GenOffice instead. |
| [OfficeCLI](https://github.com/iOfficeAI/OfficeCLI) (C#) | Apache-2.0; NOTICE "OfficeCLI … goworm" | Yes. Its 146 example Office files have no stated provenance, so don't use them as fixtures. |
| [OfficeCLI-rust](https://github.com/RainLib/OfficeCLI-rust) | Apache-2.0, no NOTICE | Yes. Carry OfficeCLI's NOTICE with anything taken from it. |
| [EigenPal docx-editor](https://github.com/eigenpal/docx-editor) (TS) | Mixed | Only `packages/core` (minus `export/export-color-font.ts` and its tests). **Not** `packages/editor-api`, `packages/pro`, `packages/docx-to-pdf`, `api/convert*` or `examples/docx-to-pdf`: those are under the Pro Evaluation licence. |
| [CasualOffice/docs](https://github.com/CasualOffice/docs) | Apache-2.0; its `docx-editor/` is an MIT fork of older EigenPal re-stamped "All rights reserved" | Take ideas from EigenPal core instead. |
| DocQuill | — | No longer public. |

Apache-2.0 code may go into GPL-3.0 code (one way only). Each ported file
needs:

- its origin and copyright line;
- a note that we changed it;
- the upstream NOTICE in [`THIRD-PARTY-NOTICES.md`](../../THIRD-PARTY-NOTICES.md).

## Adopted wholesale

**`betteroffice-drawingml` =0.3.0** draws Decks' preset shapes. Before this,
Decks drew 5 presets itself plus about 20 hand-written polygons, and
everything else came out as its bounding box. The crate:

- is self-contained (only indexmap and serde), contains no `unsafe` code, has 210 tests, and doesn't need wasm;
- adds about 30 presets that match LibreOffice side by side:
  - stars 4–32, heptagon, decagon, dodecagon;
  - left-right and bent arrows, corner, folded corner;
  - donut, no-smoking sign, cube;
  - flowchart terminator;
  - rounded-rectangle, ellipse and cloud callouts;
  - math multiply.

We keep our own outlines where we have them. Three presets the crate draws
wrongly (`upDownArrow`, `flowChartManualInput`, `flowChartOffpageConnector`)
stay as boxes. Ribbons come out as their outline only, without the folds.

Nothing else is worth adopting wholesale:

- BetterOffice `xlsx-calc` has 175 functions against IronCalc's 494.
- Its `docx-layout` would replace our model and Pango text stack, depends on wasm-bindgen, and is tuned to Word rather than LibreOffice.
- GenOffice's xlsx sidecar pins an older IronCalc.
- OfficeCLI-rust has no layout engine (pagination happens in the browser) and brings in very old XML and zip crates.
- EigenPal is TypeScript and about 40× our size.

## Ports, ranked

### Decks

1. **Preset adjustments.** We only read `roundRect`'s `adj`; every other preset is drawn at its defaults.
   - Read all of `a:avLst` and pass it to `preset_geometry_layers`.
   - Needs the adjustments stored in the model (`ShapeKind::Other`).
2. **Custom geometry (`a:custGeom`).** We read none at all.
   - Source: BetterOffice `pptx-parse/src/custom_geometry.rs` (314 lines; `arcTo` becomes cubics).
   - Destination: `decks-core/src/engine`.
3. **PowerPoint table styles.** We draw every table as Medium Style 2 – Accent 1.
   - Sources: GenOffice `pptx-engine/src/table-style.ts` and OfficeCLI `Core/TableStyles/` (74 style GUIDs).
   - Check the tints against LibreOffice.
4. **Charts.**
   - BetterOffice's `chart` feature has a 6.8k-line plot geometry covering 12 chart families, emitting drawing operations a Cairo sink can replay.
   - Ours is 5 kinds; doughnut is drawn as pie (`chart.rs:483`).
   - GenOffice `build-chart.ts` axis ticks (negative values, log axes) fit `charts.rs::nice_axis`.
5. **EMF/WMF pictures.** BetterOffice `pptx-render/src/metafile.rs` (2.3k lines, fuzzed) replays vectors; it draws no text.
6. **Symbol-font bullets.** BetterOffice `ooxml-text/src/symbol_font.rs` maps Wingdings and Webdings bullets to Unicode; today they print as Latin letters.

### Letters

The source for all of these is EigenPal `packages/core/src/layout`, the Apache part.

1. **Line rules.** `exact` and `atLeast` are read as single spacing (`docx.rs` `styled_line_multiple`). See `paragraph-style.ts` `applyLineSpacing`. In Word:
   - `exact` puts the baseline at 80% of the line box;
   - `atLeast` grows the box upward.
2. **Autospacing.** Before and after spacing is 14pt, and 0 between items of one list (`adjacent-paragraph-spacing.ts`, `list-auto-spacing.ts`). Documents made from HTML come out about 9pt tight per paragraph.
3. **Keeps.**
   - keepLines and turning widow control off: `pagination-keeps.ts`.
   - keepNext chains: we look one paragraph ahead.
4. **Header and footer insets.**
   - The body starts at `max(margin, header distance + header height)`.
   - First-page and even-page header variants: `page-furniture-insets.ts`.
5. **Tabs.** A 0.5" default tab interval and right, centre and decimal stops (`paragraph-tabs.ts`). Pango's 8-space default is wrong for every document.
6. **List markers.**
   - Use the declared hanging indent instead of a fixed 18pt (`list-marker-geometry.ts`).
   - More number formats: OfficeCLI `Core/WordNumFmtRenderer.cs` has about 60, against our 5.
7. **Table rows.**
   - Move `cantSplit` rows whole.
   - Repeat `tblHeader` rows.
   - TableNormal cell margins are 108 twips left and right, 0 top and bottom.
8. **Compatibility mode.** Mode 15 and above against legacy, and the `w:compat` options; EigenPal's `compatibility/*.ts` lists about 30 rules. Becomes a new `docx_compat.rs`.
9. **Font metrics.** Put the font's line gap above the baseline (`shaped-line-metrics.ts`). Measure against LibreOffice first: the sources disagree.

**Caution.** EigenPal and OfficeCLI calibrate against Word, but the render lab
compares against LibreOffice. Land each rule behind a fixture that shows
LibreOffice agrees.

### Tables

1. **Keep Excel's saved values for formulas we can't evaluate.** `io/load.rs:65-100` throws away the cached value of every formula cell. Source: GenOffice sidecar `recalc.rs` (`pin_unparsable_formulas`, `requote_bare_sheet_names`).
2. **Number formats.** Port these to `format_code.rs`:
   - Excel's General rule: 11 significant digits, scientific outside 1e-4..1e11. Ours is `{:.10}`.
   - `[ColorN]`;
   - elapsed `[h]`;
   - fractional seconds.

   Sources: BetterOffice `xlsx-model/src/numfmt.rs` and GenOffice `numfmt-fix.ts`, which also does `####` overflow and `*` repeat fill.
3. **Text spill.** Right-aligned text spills left and centred text spills both ways (BetterOffice `xlsx-render::spill_clip`); ours spills only left-aligned text (`sheet.rs:1363`).
4. **Excel tables, sparklines and table styles.** From the GenOffice sidecar's `table_styles.rs` and `sparklines.rs`. Today we report tables only as lost and drop sparklines.

### Render lab

- **Per-document census of unsupported features**, shown next to each verdict, to explain red rows and rank ports. It would list:
  - presets drawn as boxes;
  - `custGeom` shapes;
  - list formats mapped to decimal;
  - charts drawn as a different kind;
  - unknown table-style GUIDs.

  The idea is from OfficeCLI-rust's FidelityReport.
- **Tag-count round-trip audit.** Open each corpus document, save it, and list elements that vanish. From CasualOffice `roundtrip-audit.mjs`.
- **`lo-probe` metrics fixtures.** One-font documents that measure LibreOffice's line pitch, to settle font-metric questions.
- **Bounded vertical registration.** Align pages within about half a line before scoring.
- **Pagination corpus with Word reference data.** GenOffice `apps/docs/tests/pagination-corpus` has 23 Word files with page counts from Word 16.106 and LibreOffice 24.2. This is the Word reference we lack, but check the files' provenance before copying them in.
