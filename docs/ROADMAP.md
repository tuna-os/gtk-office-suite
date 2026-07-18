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

5. ✅ **Tables format-cells sheet** (done 2026-07-18): right-click or
   the palette opens the Format Cells dialog (kind + decimals + currency
   symbol, applied over the selection); number formats now render on the
   canvas and in the a11y cell names — formats were previously invisible
   outside exports. Proven by an I6 smoke test.
6. ✅ **Decks slide thumbnails** (done 2026-07-18): each strip row shows
   an offscreen render of its slide (cairo → GdkMemoryTexture), and the
   current slide's thumbnail refreshes live as content changes. Text on
   the canvas now scales with the slide geometry (18pt at full size,
   matching the pptx writer default; per-run sizes honored) — WYSIWYG
   where a fixed 16pt used to be.
7. ✅ **ODP read/write** (done 2026-07-18): `decks-core/src/odp.rs`,
   same zip+XML pattern as `letters-core/src/odt.rs`. Text boxes with
   styled runs, rects/circles, speaker notes, slide backgrounds, slide
   order. Open/Save dialogs and `open_path` dispatch by extension
   (`decks_core::read_deck`/`write_deck`). Oracle wave (7 tests) caught
   two real reader bugs red-first: LO list-styles clobbering text-style
   names, and Impress's `draw:custom-shape` text (no `draw:text-box`)
   being invisible to the reader. Decks oracle suite: 20 → 27.
8. ✅ **Adaptive/narrow-width audit** (done 2026-07-18): all three apps
   screenshot-audited at 400×700 under Xvfb, start pages and editors with
   demo documents. Letters and Tables adapt cleanly. Decks had two real
   bugs: (a) it registered a second 600sp breakpoint — AdwWindow applies
   at most one, so its editor demanded ~770px and clipped the header
   bar's menu and window controls off-screen; SuiteWindow now exposes
   `narrow_breakpoint` and Decks hangs its setters there (both split
   views collapse, canvas min-width relaxes); (b) the status caption
   collided with the presenter pill (hidden at narrow).
9. ✅ **Remaining advisor polish** (done 2026-07-18): Tables scrollbars
   are now thin overlay indicators (GtkOverlay, no reserved gutters or
   dead corner); the Letters ruler origin is glued to the visible page
   edge (`PageContainer::page_screen_geometry` + a cheap per-frame tick
   that only redraws on change — ticks, margin shading, indent/tab
   markers all land on the page's real screen position at any zoom);
   the command palette ranks recently used actions first
   (`filter_entries_with_recency` in suite-common-core, unit-tested;
   MRU persisted per app in the `palette-recent` GSettings key).

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
