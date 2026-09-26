# Export parity: our PDF vs LibreOffice PDF (docx/pptx)

Owner direction: pixel comparisons on the PDF (or other rendered-format)
output of identical ppt and docx files. xlsx is explicitly out of scope —
spreadsheets are not an important published rendered artifact.

## Background: what already exists

- **Reference side, done.** `tools/render-lab/lo_render.py` converts each
  fixture docx/pptx to PDF via soffice, then rasterizes to `lo-<n>.png` at
  96 DPI. The reference PDFs already exist as pipeline intermediates.
- **Letters.** Has an "Export PDF" menu entry, and
  `letters/src/printing.rs` draws the laid-out pages (`Typeset::draw_page`,
  ADR 0010) onto any Cairo surface — paper, preview, and PDF show the same
  pages. The faithful path exists; it needs a headless trigger.
- **Decks.** `decks/src/export.rs` has `to_typst` plus PDF via the
  in-process Typst engine, but it is not wired to any action
  (`#![allow(dead_code)]`) and it is not faithful: text boxes become plain
  text, shapes become generic rects. **Not usable for pixel parity.**
- **Tables / suite-export.** Typst → PDF in-process with embedded fonts.
  Out of scope with xlsx.

## Design

1. **Headless export hooks, one per app**, mirroring the existing
   `test-render-dump` actions: `--export-pdf <out>` opens the fixture
   docx/pptx off-screen and writes a PDF with no dialogs.
   - Letters: reuse the `printing.rs` Cairo path headless.
   - Decks: **new** Cairo-surface PDF drawn from the canvas code —
     explicitly not the Typst path. This is the largest item.
2. **Rasterize ours identically**: the same `pdftoppm -r 96` invocation as
   `lo_render.py` → `ours-<n>.png`. Same DPI, same naming, no new tools.
3. **Compare with the existing metrics** (words / displacement / SSIM),
   ours-PNG vs lo-PNG. Same thresholds and CI gates as screenshot parity.
4. **Separate baseline file** (`tools/render-lab/baseline-export.json`):
   keeps screenshot baselines untouched and lets the two parities move
   independently.
5. **Manifest + CI**: fixtures opt in via the manifest (`export: true`,
   docx/pptx only); `render-parity.yml` gains an export job with the same
   sticky-comment and issue-sync behavior as the screenshot jobs.
6. **Paper fidelity rule**: the PDF page size must be the document's size.
   Letters' `page_setup` already uses the document paper, not the system
   default; the Decks Cairo PDF must use the real slide size (not the
   16×9cm Typst default in `decks/src/export.rs`).

## Known risks (recorded, not solved here)

- **Font substitution.** Our PDFs may embed different fonts than LO's
  (especially near the Typst embedded set). Expect early diffs to be
  font-driven; record them as such before tuning anything.
- **Shared-tooling sequencing.** `compare.py` is owned by the concurrent
  OCR-metric stream. The export-compare wiring lands after it, or in that
  stream's hands; the app-side hooks (item 1) proceed independently.

## Out of scope

- xlsx entirely.
- Typst-path comparison (it can never be pixel-faithful).
- Changing screenshot-parity thresholds.
- `tables/wrap-text` (#920): still blocked on the Excel-vs-Calc decision.

## Acceptance

Per-fixture `ours-<n>.png` vs `lo-<n>.png` verdicts in CI for every
`export: true` docx/pptx fixture, tracked in `baseline-export.json`, with
honesty rules applying unchanged: look at the images, fix the exporter or
the metric — never the thresholds.
