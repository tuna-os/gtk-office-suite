# ADR 0009: Headless layout and rendering, decoupled from GTK widgets

Date: 2026-09-18 · Status: **proposed**

## Provenance

Consolidates #614 and #642, generated independently on 2026-09-12 as
`docs/PRINT-RENDER-ARCHITECTURE.md` and `docs/adr/0007-headless-print-rendering.md`.
They agree; #642's ADR number 0007 collided with two other drafts, so this takes
0009 (0007 and 0008 are also contested by superseded drafts — see #870 and #873).

## Context

Layout and rendering are currently coupled to GTK widgets — `GtkDrawingArea`
for canvas drawing, `GtkPrintOperation` for print. Three consequences:

1. Paginating or rendering a large document blocks the main loop, because the
   work happens where the widget lives.
2. Thumbnails, visual regression tests and batch export all need a real display
   or Xvfb, which is slow and a reliable source of CI flake.
3. A headless CLI cannot render at all. This is the immediate blocker: the
   `suite-convert` specification lists PDF as an output format for every
   document class and has no way to produce one. That gap is the single
   largest risk to its GTK-free premise.

## Decision

Move layout and rendering into `suite-common-core`, GTK-free:

- **Layout** produces a serializable render tree. Letters pagination, Tables
  grid slicing and Decks slide geometry are pure functions of the document
  model — no widget, no allocation, no main loop.
- **Rendering** targets Cairo surfaces directly: `PdfSurface`, `SvgSurface`,
  `PsSurface`, and PNG buffers. Cairo does not require GTK, which is what makes
  this possible at all. Text goes through Pango, used without widget context.
- **The apps consume it.** `GtkPrintOperation` stays where it belongs — in the
  GUI — but it renders a tree computed on a worker thread rather than doing
  layout inline.

## Consequences

**Good.** Headless PDF export becomes possible, which unblocks `suite-convert`
phase 2. Print preview stops blocking the UI. Visual snapshot tests run in a
plain Rust test with no display, so they get fast and deterministic. And the
same code path renders the preview and the exported file, which is the only way
"what you see is what you get" is actually true rather than approximately true.

**Costly.** The drawing logic currently in `window.rs` and the custom widgets
has to move. That is the real price and it is not small; #642 names it.

**Unresolved.** PDF/A conformance (#614) is a stronger claim than "emits a
PDF" — Cairo's PDF output is not PDF/A without further work, and no draft says
how that gap is closed. Treat archival conformance as out of scope until
someone validates output against a checker.

## Relationship to the readiness plan

Behind [#443]. The part worth doing early is narrow: establish the render-tree
boundary so new drawing code is written on the right side of it, rather than
migrating what exists.

[#443]: https://github.com/tuna-os/gtk-office-suite/issues/443
