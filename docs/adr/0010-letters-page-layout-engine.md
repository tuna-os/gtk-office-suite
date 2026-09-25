# ADR 0010: The Letters page layout engine

Date: 2026-09-24 · Status: **accepted** (Phase 1 of `docs/RENDER-PARITY-ROADMAP.md`)

## Context

Letters draws grey page rectangles and lays **one `GtkTextView` across every
page and every gap** (`letters/src/page_container.rs`). Text does not reflow
per page: it runs over the gaps and the bottom margins. The page count comes
from a second, unstyled Pango pass (`letters/src/layout.rs`), so headings,
spacing, lists and page breaks do not move page breaks. Print preview is a
third layout and PDF export (Typst) a fourth. No two agree, and none agrees
with LibreOffice: the render lab's `letters/pagination` fixture shows 6 pages
where LibreOffice shows 4.

"100%" zoom is not a size either: the page is fitted to the window width at
up to 1.5 px/pt.

ADR 0009 (proposed) argues for headless layout in general. This ADR decides
the Letters part concretely.

## Decision

1. **A page layout engine in `letters-core::layout`.** Input: the
   `Document` model and `LayoutOptions` (default font, page geometry,
   widow/orphan counts). Output: a serialisable `RenderTree` of pages; each
   page holds positioned items in points from its top-left corner: body,
   header and footer lines; list markers; table cell boxes.
   - It places paragraphs in columns and pages: space before/after, line
     spacing, left/right/first-line indents, list hanging indents, page
     breaks, keep-heading-with-next, orphan and widow control (2/2), tables
     (equal columns, rows never split), headers and footers with `{page}` and
     `{total}`, multiple columns.
   - List markers and numbering come from `letters-core::lists`, shared with
     the editor buffer, so "3." cannot be "3." on screen and "1." in print.
2. **Shaping behind a trait.** The engine never touches glyphs: a `Shaper`
   breaks one paragraph into `LineBox`es (char range, x offset, width,
   ascent, descent) at a given width. The engine is arithmetic over those
   and is unit-tested with `MonoShaper`, a font-free shaper with exact
   metrics, so layout tests are deterministic and need no fonts or display.
3. **Pango and Cairo, not GTK, behind the `render` feature.**
   `layout::pango::PangoShaper` shapes with a pangocairo font map at 72 dpi
   (one Pango unit is one point) with metrics hinting off, so line breaks do
   not depend on zoom. `layout::pango::Typeset::draw_page(cr, page)` is the
   one drawing routine for a page: the page view, print and PDF call it with
   a Cairo context scaled to their device. Pango and Cairo are not GTK; the
   crate stays free of `gtk4`, and without the feature it has no C
   dependencies at all.
4. **Staged adoption in the app.**
   1. A read-only **Print Layout** page view in `letters` draws the tree.
      It is a view toggle beside the editable "Draft" view (the current
      TextView), and it is what the render lab captures. Its zoom is
      physical: 100% is 96/72 px per point; fit-to-width is a separate mode.
   2. Print, Print Preview and PDF export draw with `Typeset::draw_page`
      (done: `letters/src/printing.rs`; the old unstyled pagination in
      `letters/src/layout.rs` is gone and the Draft view takes its page count
      from the engine too). The render lab rasterises the PDF export and
      compares it with the on-screen pages (`print_agreement`). Typst stays as
      "Export as PDF with Typst".
   3. Editing moves onto the page view (caret, selection and hit-testing
      through the tree, `GtkIMContext`, `GtkAccessibleText`), then the
      TextView path is removed. That step also gives Letters the live
      GTK-free model that RFC-0001 Phase 0 asks for. In sub-stages, each
      leaving the app working, with Draft as the fallback throughout:
      - **3a (done): the page view edits the buffer.** `Typeset::hit_test`,
        `caret` and `selection_rects` (letters-core, tested) map points on
        the page to document positions; `bridge::capture_with_starts`,
        `buffer_offset` and `paragraph_offset` map document positions to
        buffer offsets through list markers, table pipes and footnote
        markers. `letters/src/page_edit.rs` places the buffer cursor on
        click, selects on drag and double/triple click, sends typing
        through a `GtkIMMulticontext`, and handles editing and movement
        keys; formatting, undo, find and save keep acting on the buffer.
        Print Layout lays out again when the main loop is idle after each
        edit. The GtkTextBuffer is still the live state.
      - 3b: `GtkAccessibleText` on the page view; list continuation,
        Markdown shortcuts and the suite clipboard format on the page view.
      - 3c: the live model. The page view edits a `letters_core::Document`
        through `StructuredEditor`-style operations and relays out only the
        paragraphs an edit touched; the buffer becomes Draft's view of the
        model instead of the other way round. This is RFC-0001 Phase 0.
      - 3d: Print Layout becomes the default view; the TextView path is
        removed once nothing depends on it.

## Consequences

- Page count, page breaks and margins follow the document's styles, and can
  be compared with LibreOffice page by page in the render lab.
- Until stage 3, what the user edits (Draft) and what they see in Print
  Layout are two renderings; the tree is the one that is right. The toggle
  makes that explicit instead of pretending one view is both.
- A renderer that re-shapes a paragraph to draw it must use
  `layout::paragraph_request`, or it can break lines differently from the
  engine. `Typeset` does; nothing else should shape paragraphs.
- Not yet in the tree: images (they need their size in the model), footnote
  bodies, floating objects, table column widths and merged cells from the
  file. Each lands with its render-lab fixture.
- Typst export stays, as an alternative "typeset" export; it no longer
  defines what a page looks like.
