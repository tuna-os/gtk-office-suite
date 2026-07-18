# Roadmap

State as of 2026-07-18: the surfacing wave, format-parity stragglers, and
the oracle expansion are done (see PARITY.md for the row-by-row truth;
TESTING.md for the coverage bar: 65 oracle tests, CommonMark 630/652,
13 smoke, all gates green). What follows is what's left, ordered by
leverage. Every item names its proving instrument — nothing lands
without a test that fails first.

## Now — next few sessions

1. ✅ **Cross-app clipboard, second half** (task #25, done 2026-07-18).
   GDK glue in all three apps: copy offers suite-JSON + `text/html` +
   plain/TSV; paste prefers the suite fragment (Tables formulas stay
   live, Letters styled runs keep their tags, Decks pastes text boxes).
   Proven by I6 copy/paste round trips in Letters and Tables. Still
   open from the original spec: Letters pasting a grid as a real
   cell-tagged table (blocked on the buffer-table bridge gap, item 3)
   and true two-app smoke tests (need a WM in the harness).
2. ✅ **Virtual a11y children** (task #23, done 2026-07-18). The Tables
   grid exposes each used cell (role cell, name "B2: 20", selection
   state) and the Decks canvas exposes each object (role list item,
   "Text box: …"/"Rectangle") as real AT-SPI nodes — GridArea/CanvasArea
   DrawingArea subclasses with persistent GObject accessibles linked via
   set_accessible_parent/update_next_accessible_sibling. Proven by two
   I6 smoke tests. Remaining: on-screen extents still report position
   (0,0) (size is correct) — the widget-relative→screen translation
   needs the same treatment; keep driving tests by name/keyboard.
3. ✅ **Letters bridge list gap** — already closed: the bridge renders
   ListKind as visible markers and captures them back losslessly
   (bridge round-trip test green); the PARITY red was stale. The
   remaining aesthetic step (native list rendering instead of literal
   markers in the buffer) folds into the grid-paste-as-table work.
4. **rdocx upstream follow-up** — fork-side done 2026-07-18: the
   ParagraphRef::line_spacing_multiple getter landed on hanthor/rdocx
   (rev 6f91a40, pin bumped) and docx line spacing round-trips with an
   LO oracle test. Still upstream-gated: merge the fork's getters into
   tensorbee/rdocx#6, then publish letters-core/decks-core to crates.io
   and drop the git pin.

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
