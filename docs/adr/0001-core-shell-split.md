# ADR-0001: Core/shell split, oracle-based parity, settled engines

Date: 2026-07-18 · Status: accepted

## Context

Until July 2026 the apps' logic lived inside GTK widget code and the GUI
test workflow silently ran zero tests (`pytest || true` with pytest never
installed). Three launch-blocking bugs shipped unnoticed, Letters had no
document model (formatting existed only as GtkTextBuffer tags and was
dropped on save), and nothing measured progress toward being a credible
office suite.

## Decision

1. **Core/shell split.** Each app has a GTK-free core crate owning its
   model, logic, and file I/O (`letters-core`, `tables-core`, `decks-core`,
   shared `suite-common-core`). App binaries are shells: widgets, rendering,
   signal wiring. A single bridge module per app translates model ⇄ widget
   state. Core crates build with `clippy -D warnings` and must be testable
   without GTK headers.

2. **Oracle-based parity, not porting.** Parity with established suites is
   measured behaviorally against headless LibreOffice in CI (our files must
   open there; LO authors our parity corpus at test time). We never vendor
   LibreOffice/OpenOffice code, tests, or data files (MPL/GPL, and thirty
   years of compatibility archaeology we explicitly do not want). Vendored
   corpora are permissive-licensed only (CommonMark spec CC-BY-SA, MIT
   fixture generators). Progress is recorded as ratcheted pass counts that
   CI refuses to let regress.

3. **Engines are settled.** IronCalc (spreadsheet), rdocx (OOXML
   persistence — our own crate; gaps get upstream fixes, not workarounds),
   pulldown-cmark (Markdown), zip+quick-xml (PPTX), zspell (spelling),
   Typst (PDF export; moving from CLI to library, phase 4). The document
   *semantics* layer (letters-core's model) is our own — no mature Rust
   crate exists for it. Text layout belongs to Pango, never to us.

## Consequences

- Feature work starts with a failing test in a core crate (TDD); widget
  code cannot accumulate untested logic without failing review.
- Parity claims are numbers (LO corpus 104/104, CommonMark 594/652, oracle
  green) rather than assertions.
- The Writer long-tail (fields, macros, change tracking, frames) is out of
  scope until an ADR admits a specific item (see docs/PARITY.md tier 3).
- CI cost: the oracle job installs LibreOffice headless (~1 min) on every
  run — accepted as the price of honest interop signal.
