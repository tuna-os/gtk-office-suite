# Roadmap

State as of 2026-07-18: the surfacing wave, format-parity stragglers, and
the oracle expansion are done (see PARITY.md for the row-by-row truth;
TESTING.md for the coverage bar: 65 oracle tests, CommonMark 630/652,
13 smoke, all gates green). What follows is what's left, ordered by
leverage. Every item names its proving instrument — nothing lands
without a test that fails first.

## Now — next few sessions

1. **Cross-app clipboard, second half** (task #25). The pure paste
   matrix (`*/src/fragment.rs`) is done and unit-tested; the GDK
   `ContentProvider`/clipboard glue per app is not. Copy in Tables must
   offer suite-JSON + `text/html` + TSV; paste in Letters must build a
   real cell-tagged table. *Prove:* one I6 smoke per direction
   (Tables→Letters, Letters→Decks, Decks→Letters, external HTML→each).
2. **Virtual a11y children** (task #23, deep half of issue #87).
   Per-cell / per-object AT-SPI nodes for the Tables grid and Decks
   canvas. This also unblocks honest GUI testing — today the canvases
   are opaque (bogus extents, description-string assertions). *Prove:*
   smoke tests navigate to a cell/object accessible directly.
3. **Letters bridge list gap** (the one red I5 in PARITY). The buffer
   keeps literal "- " for lists; model↔buffer translation should carry
   ListKind. *Prove:* existing red bridge test goes green.
4. **rdocx upstream follow-up**: land the ParagraphRef line-spacing
   getter (and numbering/highlight getters, hyperlink write) in
   tensorbee/rdocx#6, then close docx line-spacing read (PARITY red),
   publish letters-core/decks-core to crates.io (metadata ready), and
   drop the git pin.

## Near — rounds out the product

5. **Tables format-cells sheet** (DESIGN-UI §Tables): right-click →
   number-format UI over the engine-proven formats; formats currently
   reachable only via the cycle button. *Prove:* smoke drives the sheet;
   I2 xlsx round-trip already covers persistence.
6. **Decks slide thumbnails** in the sidebar (advisor finding): render
   the canvas snapshot into each row. *Prove:* I7 visual; smoke asserts
   rows carry image children once a11y children exist.
7. **ODP read/write** for Decks (its LO-native format, like ODT for
   Letters). *Prove:* I2 round-trip + oracle wave, same pattern as
   `letters-core/src/odt.rs`.
8. **Adaptive/narrow-width audit** (DESIGN-UI §5): drive all three apps
   at 400sp under Xvfb, fix what breaks; Tables fx bar and Decks
   inspector are the likely offenders. *Prove:* smoke at narrow
   geometry + screenshot set.
9. **Remaining advisor polish**: Tables overlay scrollbars (replace the
   corner-grid Scrollbars), ruler origin aligned to the page edge,
   palette recency ranking (persist last-used actions in GSettings).

## Later — each needs a scope decision (ADR) before it's "red"

- **CommonMark last 22** (escapes, entities, autolinks — mostly
  serializer escaping subtleties; ratchet is at 630, ceiling ~645
  without an entity-preserving model).
- **Track changes / comments / footnotes** (PARITY Tier 3; rdocx has a
  footnotes module already).
- **Charts persisted into xlsx** (Cairo dialogs exist; the chart part
  is not written).
- **Conditional formatting, pivot tables, array formulas** (Tables
  Tier 3; array formulas depend on IronCalc roadmap).
- **Master slides applied on render + Impress template import**
  (Decks Tier 2/3).
- **i18n**: gettext wiring — no user-facing string is translatable
  today.
- **Flathub submission**: manifests and metainfo exist and validate;
  needs screenshots (now auto-generated), release tags, and the
  submission PR.

## Standing infrastructure notes

- Builds: himachal `gtkbuild` distrobox (18 cores, ~10s warm) is the
  app-binary builder; binaries rsync back and pass smoke locally.
- Screenshots: the `Screenshots` workflow regenerates the README
  walkthrough weekly from `tests/gui/walkthrough.py` + the demo docs.
- Testing bar: see TESTING.md — oracle floor 25/20/20, red-first waves,
  corpus growth beyond ~70 hand-written oracle tests.
