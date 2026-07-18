# CONTEXT — domain glossary and load-bearing rules

Three GNOME-native office apps in Rust/GTK4: **Letters** (word processor),
**Tables** (spreadsheet), **Decks** (presentations). Roadmap: PLAN.md.
Feature tiers + test mapping: docs/PARITY.md. Dev setup: docs/DEVELOPMENT.md.
Testing: docs/TESTING.md. Decisions: docs/adr/.

## Glossary

- **Core crate** — GTK-free library owning an app's model and I/O
  (`letters-core`, `tables-core`, `decks-core`, shared `suite-common-core`).
- **Shell** — the app binary: widgets, rendering, signal wiring. No logic.
- **Bridge** — the single module translating model ⇄ widget state
  (`letters/src/bridge.rs`: Document ⇄ GtkTextBuffer tags).
- **Document / Paragraph / Run** — letters-core model: paragraphs of styled
  runs; offsets identical to GtkTextBuffer's (paragraph break = one char).
- **Ratchet** — a corpus test whose pass count may never drop below its
  baseline file; raising the baseline is how parity progress is recorded.
- **Oracle** — headless LibreOffice in CI: our files must open there and
  round-trip content; it also *authors* the parity corpus at test time.
- **Parity corpus** — LO-generated scenario documents our engine must read
  correctly (`letters-core/tests/lo_parity.rs`, 104 scenarios).

## Rules

1. **No business logic in shells.** If a function doesn't need a GTK type,
   it goes in a core crate with tests. (ADR-0001)
2. **Never `|| true` a test step.** (docs/TESTING.md has the war story.)
3. **Oracle, not port** — measure against LibreOffice behavior; never vendor
   its code, tests, or data files. (ADR-0001)
4. **Core crates build clippy-clean** (`-D warnings`, enforced in CI).
5. **Engine choices are settled** (IronCalc, rdocx, pulldown-cmark,
   zip+quick-xml, zspell, Typst). rdocx gaps are fixed upstream in
   hanthor/rdocx, not worked around here.
