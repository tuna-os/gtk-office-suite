# ADR-0002: GUI testing strategy and file-open architecture

Date: 2026-07-18 · Status: accepted

## Context

With the model layers proven by corpora and oracles (ADR-0001), the open
question was how deep GUI-level testing can and should go. Probing GTK
4.14's AT-SPI surface established hard facts:

- Synthetic input is real: XTest keystrokes and AT-SPI button actions
  traverse GTK's genuine input path.
- Text content, labels, focus, and styled-run *boundaries* are assertable
  over AT-SPI; font-weight attributes and toggle `checked` state are not
  (GTK 4.14 limitations).
- DrawingArea-based UIs (Tables grid, Decks canvas) are invisible to
  AT-SPI until they implement accessible children (issue #87).

## Decisions

1. **The gating GUI tier asserts behavior, not pixels**: launch, input
   reaching the buffer, state labels, and — the strongest form — the
   **file round-trip journey**: launch with a document argument, edit via
   synthetic input, Ctrl+S, assert the saved bytes. Visual/formatting
   *appearance* stays with the non-gating VLM tier; formatting *fidelity*
   stays with the model corpora.
2. **Apps accept file arguments** (`GApplicationFlags::HANDLES_OPEN` in
   SuiteApp, `connect_open` per app). This is simultaneously a user
   feature ("Open with…" from Files) and the enabler for journey tests.
   Letters implements it; Tables/Decks follow the same pattern.
3. **Widget lookups in app code must be structural searches, not
   fixed-depth chains.** A three-level `first_child()` chain in
   `get_textview` silently returned None after a widget-tree change,
   making Ctrl+S a no-op for every tab — found only when the journey test
   existed. Depth-first search by type is the required pattern.
4. **GTK tests share one thread.** GTK refuses initialization from a
   second thread; buffer/bridge unit tests therefore live in a single
   `#[test]` running all cases, skipping cleanly when no display exists.

## Consequences

- Every fidelity property proven at model level can graduate to a journey
  test once its interaction surface exists.
- Journey tests caught two shipping bugs on day one (save no-op; notes
  loss in Decks came from the corpus analog). This tier stays gating.
- Issue #87 (canvas a11y) is the ceiling for Tables/Decks journey depth
  and is therefore prioritized as testability work, not just accessibility
  compliance.
