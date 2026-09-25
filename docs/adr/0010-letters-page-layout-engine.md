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
      - **3b (done):** `GtkAccessibleText` on the page view (a "line" is the
        laid-out line); list continuation shared by both views
        (`bridge::enter_in_list`); the suite clipboard format on both views.
      - 3c: the live model (RFC-0001 Phase 0), in two steps:
        - **3c-1 (done):** `letters_core::edit` — the operations a live
          `Document` changes by (`Insert`, `Delete`, `Mark`, `SetParaStyle`),
          each returning its exact undo; and incremental relayout: a
          `ShapeCache` keyed by `request_key` means an edit re-shapes only
          the paragraphs it changed (`Typeset::update`, used by Print
          Layout after every edit).
        - **3c-2 (done):** each tab holds a `LiveModel` (letters/src/live.rs):
          a `Document` that follows the buffer edit by edit. Typing and
          deleting inside a paragraph become `edit` ops; paragraph breaks,
          list markers, table pipes, objects and formatting-tag changes mark
          it stale, and the next read re-captures. It is always exactly the
          captured document (tested after every edit, including 1,500
          seeded random edits). Print Layout reads it instead of capturing
          the buffer on every keystroke.
        - **3c-3 (done): the model is the source of truth.** Save, copy,
          the page view and undo read the `LiveModel`; the buffer's own
          undo is off and Undo/Redo apply the model's `edit::History`
          (inverse ops; typed words merge into one step). The page view
          edits the model first (typing, Delete/Backspace, Enter — a list
          item continues its list, an empty one ends it) and the buffer is
          re-rendered from the model for just the changed paragraphs
          (`project`). Buffer-side edits (Draft, formatting actions) become
          ops by re-reading only the lines they touched and diffing them
          against the model (`bridge::capture_span`, `edit::diff`):
          typing, Enter, Backspace across a break, formatting and list
          markers never read the whole buffer; nor do table edits (the
          span widens to the whole contiguous table block) or inline
          images (followed like typed text). Structured commands (table,
          list, page break) run on the model document and are diffed into
          ops. Undo/Redo sensitivity follows the `History`. A
          200-paragraph keystroke-relayout budget runs in CI.
      - **3d (done):** Print Layout is the default view (`print-layout`
        defaults to true) and takes focus when shown; the selection popover
        and Markdown shortcuts work on it. Draft stays as an option, now a
        pageless sheet: one continuous white column at the page's text
        width, no page boxes, gaps or page count — pages are the page
        view's job alone, so the old paged-TextView drawing path is gone.
        The render dump always draws the page view. The `GtkTextBuffer`
        remains as Draft's view of the model.

## Design constraints from the CRDT spike

RFC-0001's Phase 1 spike ([results](../rfc/0001-spike-results.md)) recommends
Loro (not adopted: the owner decides). Whatever library is chosen, the live
model must be replicable without a rewrite, so `letters_core::edit` holds to
these rules:

1. **One sequence.** The document is one sequence of chars: paragraph text,
   one `OBJECT` char per inline object (image, footnote reference — an embed
   is a placeholder char carrying a mark, as the spike found Loro and
   Automerge need), one break char between paragraphs. Every op addresses it
   by offset (`edit::locate`, `paragraph_start`).
2. **Formatting is marks with per-key expand rules.** `MarkKey::expand`:
   bold, italic, size, colour… expand `After` (typing at their end
   continues them); link, inline code and raw HTML expand `None`. This is
   Loro's `config_text_style` / Automerge's `ExpandMark`; yrs cannot express
   it natively, which is one reason it was not recommended.
3. **Ops carry their style explicitly.** `Insert` holds the runs it
   inserts, styled; `typing_style` applies the expand rule once, where the
   user types. Replaying an op never consults the rule, so two peers (or an
   undo) agree even when their rules would differ.
4. **Paragraph formatting is a paragraph attribute** (`SetParaStyle`), which
   maps to an attribute of the paragraph's break char or a map keyed by a
   stable paragraph id.
5. **Every op has an exact inverse** (`apply` returns it), property-tested:
   the sequence text changes exactly as a string edit, and undoing any
   sequence of ops restores the document.
6. **Table structure is not a sequence edit.** Ops that would join or split
   table cells are refused (`EditError::TableStructure`); table rows and
   columns change through their own structural commands, which will need a
   tree or map in a CRDT, not the text sequence.
7. **No CRDT dependency** until the owner chooses one.

Text rendering: the page view, print and PDF draw with `Typeset`'s own Pango
context — greyscale antialiasing, no hinting, no metric hinting — so a page
is laid out once in points and only scaled. That is stricter than
`suite_common::use_ui_font_rendering` (slight hinting, for UI text that is
not a printed page) and already greyscale, so the page view does not call
it.

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
