# ADR 0003 — Later-tier scope decisions

Date: 2026-07-18 · Status: accepted

The roadmap's "Later" tier required a scope decision per item before any
of it counts as red-first work. These are those decisions.

## 1. CommonMark last 22 — IN, up to the model's ceiling

Chase serializer escaping, entities, and autolinks until the ratchet
stops moving. **Ceiling accepted:** cases that require an
entity-preserving text model (storing `&amp;` vs `&` distinctly) are
out; we normalize to resolved text. Expected landing zone ~645/652.
The ratchet only moves up.

## 2. Track changes / comments / footnotes — FOOTNOTES ONLY

- **Footnotes: IN.** rdocx already has a footnotes module; the model
  grows a per-run footnote reference and a document footnote list.
  Round-trip docx + oracle wave + an insert action in Letters.
- **Comments: OUT** for now — needs anchored-range UI (margin layer)
  that PageContainer doesn't have; revisit after footnotes ship.
- **Track changes: OUT** — requires a revision model across every edit
  path (undo, clipboard, IO). Its own project; not entered piecemeal.

## 3. Charts persisted into xlsx — IN, minimal chart part

Write a real DrawingML chart part (bar, line, pie — the three kinds the
Cairo dialog already draws) with cached + formula-referenced series,
anchored via a drawing part. Read back what we write. Oracle: Calc
opens the workbook and the chart survives a Calc rewrite structurally.
**Out:** styling fidelity (colors/fonts beyond defaults), other chart
kinds, chart editing of foreign charts (preserved as-is if present).

## 4. Conditional formatting / pivots / array formulas — CF BASICS ONLY

- **Conditional formatting: IN**, cell-value rules only (greater/less/
  between/equal + solid fill), persisted to xlsx `<conditionalFormatting>`
  and rendered on the canvas. Managed via a small dialog.
- **Pivot tables: OUT** (large, separate model).
- **Array formulas: OUT** (tracks IronCalc upstream).

## 5. Master slides — IN: read + render + Impress import

`read_pptx` gains slideLayout/slideMaster parsing: master background
color and non-placeholder decoration shapes, slide→layout→master
mapping. Canvas already renders masters. ODP master read comes along
only if it falls out of the same pass cheaply. **Out:** master editing
UI (masters are read-only styling context), placeholder inheritance
semantics beyond background + decorations.

## 6. i18n — IN, wiring only

gettext-rs + `po/` scaffolding + all user-facing strings wrapped in the
three apps and suite-common, POT extraction scripted. **Out:** actual
translations (community work), locale-aware number/date formats in
Tables (separate decision, interacts with formulas).

## 7. Flathub — PREP ONLY

Everything up to the submission PR: release tag, metainfo with
screenshot URLs, flathub-layout manifests validated. The outward PR to
flathub/flathub is a human action (James submits or explicitly asks).

## Execution order

5 → 1 → 3 → 2(footnotes) → 4(CF) → 6 → 7 — dependency-light first,
outward-facing last.
