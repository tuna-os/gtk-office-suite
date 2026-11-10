# Clipboard between the suite's apps (INTEGRATION-PLAN A10)

The plan item says "audit the formats each app offers and accepts, then fix the
gaps". This is the audit, read from the code on 2026-11-10. It separates what
the code shows from what nothing has been run to confirm. **Nothing is fixed
here.**

## What already exists

The suite has a designed clipboard, not an ad hoc one (ADR 0012, DESIGN-UI):

- A neutral fragment model, `suite_common_core::document` (runs of styled text in
  paragraphs, grid cells), carried on the clipboard as
  `application/x-tunaos-suite+json` (`suite_common_core::document::MIME`). Each
  app converts to and from it at its own boundary (`letters-core`, `tables-core`
  and `decks-core` each have a `fragment` module re-exporting `MIME` and
  `GridCell`).
- `suite_common::clipboard` (`suite-common/src/lib.rs`) builds one provider that
  offers the suite JSON **beside HTML and plain text**, and has `offers` and
  `read_string` for the paste side.

## Copy, per app

| App | Offers on Ctrl+C | Where |
|---|---|---|
| Letters | suite JSON, HTML, plain text, plus `sections::CLIPBOARD_MIME` (section breaks) when the selection has any | `letters/src/doc_tab.rs` `copy_selection`; Cut is a copy then a model edit, one undo restores it |
| Tables | suite JSON (`copy_range` of the selection), HTML, plain text | `tables/src/window.rs` grid key handler |
| Decks | suite JSON of the selected object (`copy_object`), HTML, plain text | `decks/src/canvas_keys.rs`; only when no text session or text widget has focus |
| Lifafa | not audited: the composer is a WebKit editor, so copy and paste are WebKit's own (HTML and plain text) | `lifafa/src/ui/rich_editor/` |
| Mandelbrot | not audited | |

## Paste, per app

| App | Accepts | Falls back to |
|---|---|---|
| Letters | the suite JSON (with section breaks if offered), inserted at the caret | the text view's own plain-text paste (`Propagation::Proceed`) |
| Tables | the suite JSON, applied at the selected cell through the same path as typing | plain text read as a TSV grid |
| Decks | the suite JSON, pasted as a text box (`paste_as_text_box`, 240 x 200) on the current slide | nothing on the canvas: with no suite JSON on the clipboard the key is passed on |

So a range copied in Tables arrives in Letters and Decks as a fragment, and a
Letters selection arrives in Tables as a grid and in Decks as a text box. That
is the plan's "a Tables range pastes into Letters, Decks and the composer",
for the three office apps, by code.

## Gaps the code shows

1. **Outside HTML does not appear to be read by the office apps.** Searching
   `letters-core`, `tables-core` and `decks-core` for `text/html`, `from_html` and
   `parse_html` finds no clipboard reader (the only `text/html` is Letters' *save*
   format list); a search for other names could still turn one up. A table or formatted
   text copied from a browser or LibreOffice lands as plain text in Letters,
   as a TSV grid in Tables if it is tab-separated, and not at all on the Decks
   canvas.
2. **Decks has no plain-text paste on the canvas.** With no suite JSON offered,
   Ctrl+V does nothing unless a text box is being edited.
3. **Tables pastes plain text as strings only**: no number or date detection on
   that path is visible in the handler (the fragment path carries typed cells).
4. **Drag and drop is a separate path**: this audit covers Ctrl+C/V only.
   `suite-common/src/file_drop.rs` handles files; dragging a range or a text
   selection between apps was not audited.

## Not confirmed by any test

The suite JSON round trip is covered by core unit tests, but no GUI journey
copies in one app and pastes in another. Two things in particular are
unverified: that Lifafa's composer pastes what the office apps offer as HTML
with its formatting intact, and that a Tables range pasted into Letters keeps
its cell structure rather than flattening to tab-separated text.

## Suggested order

1. A journey that copies in Tables and pastes in Letters and Decks (and the
   reverse), so the working path is a claim with evidence.
2. HTML paste into Letters (a `text/html` to `Fragment` reader in
   `letters-core`, GTK-free and testable against fixtures), then Tables' table
   paste from HTML, which is the common case (browser, LibreOffice).
3. Plain-text paste on the Decks canvas.
