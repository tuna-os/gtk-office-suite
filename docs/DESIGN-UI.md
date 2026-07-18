# Interface Design Plan — modern, sleek, powerful

The engines are measured; the interface is what people judge in the first
ten seconds. This plan commits the suite to a design position and a
sequenced build-out. Companion docs: GNOME-GUIDELINES.md (HIG rules),
PARITY.md (feature truth), ADR-0002 (what's testable).

## Design position

**Sleek by default, powerful on demand.** GNOME HIG is the baseline, not
the ceiling: clean AdwToolbarView chrome, flat white canvas, generous
whitespace — but every capability the engines prove must be *reachable in
two interactions or fewer* from where the user is working. The failure
mode to avoid is the classic libadwaita trap: beautiful, empty, and
feature-starved. The engines now do more than the UI shows; closing that
gap IS the design task.

Three surfacing mechanisms, shared across all three apps:

1. **Context reveals capability.** Selection is intent: selecting text
   raises a floating format popover (b/i/u/s, highlight, size, color,
   link — all engine-proven); selecting a slide object raises
   arrange/style controls; selecting cells raises format/sum affordances.
   Nothing shown until relevant, everything shown when it is.
2. **The command palette is the power ceiling.** Ctrl+K opens a
   GtkSearchEntry-driven palette listing *every* action with its shortcut
   (GioAction registry already exists — the palette enumerates it).
   Searchable verbs beat buried menus; this is how "powerful" coexists
   with "sleek". Also the cheapest feature-discoverability instrument we
   can build.
3. **The status bar is live, not decorative.** Letters: word count (done)
   + cursor style readout ("Bold · 12pt · List"). Tables: selection
   sum/avg/count (spreadsheet table stakes). Decks: slide x/y + object
   count (a11y descriptions already compute this — same source).

## Per-app layouts

### Letters
- AdwToolbarView: flat header (tabs inline via AdwTabBar), single slim
  format bar that adapts — collapses to icon groups under AdwBreakpoint,
  overflowing into a "More" popover rather than disappearing.
- Page canvas stays the centerpiece: shadowed white page on neutral
  desktop (PageContainer, done), rulers optional (View menu), margins
  visible on hover only.
- Selection popover replaces reaching for the toolbar in the common case.
- Typography dialog (font family/size/color/spacing) — the engine
  round-trips these; the UI must offer them.

### Tables
- Formula bar merges into the header area (Calc-style): name box (cell
  ref, editable → jump), fx entry (focus flow done).
- Grid: crisp 1px hairlines, subtle header shading, selection as filled
  accent-color overlay with autofill handle. Frozen header row/col
  shading on scroll.
- Selection statistics in the status bar; right-click → format cells
  sheet (number formats are engine-proven now).

### Decks
- Left slide strip (done) + canvas + collapsible right inspector
  (AdwOverlaySplitView): position/size/style of the selected object —
  inspector fields are the visible twin of the a11y descriptions.
- Presenter affordances: bottom-center pill (prev/next/present), speaker
  notes drawer under the canvas (engine round-trips notes; UI must edit
  them).

## Feature-surfacing audit (the checklist)

Every ✅ row in PARITY.md gets a "reachable in ≤2 interactions" audit
entry; a feature the engine proves but the UI hides is a design red.
Current known reds: number formats (no UI), links (no insert flow beyond
Ctrl+K plan), font size/color (menu only), table insertion (none).
Resolved 2026-07-18: speaker-notes editing (Decks notes expander, wired
to the model). Audit note: the palette plan assumed a complete GioAction
registry — true only for Letters; Tables/Decks toolbars were closure-
wired, so action-registry unification is the palette's prerequisite.

## Verification

- **I7 (VLM tier)** judges the visual language nightly against this doc's
  vocabulary (flat, spacious, accent-color selection, no double chrome).
- **HIG audit doc** (GNOME-GUIDELINES.md) stays the hard-rule list.
- **Palette coverage test** (deterministic): every registered GioAction
  appears in the palette with a label — an I6 smoke assertion, and
  incidentally a forcing function to name actions properly.
- Screenshot set regenerated per release for the metainfo (Flathub is a
  design surface too).

## Sequencing

1. Command palette (shared widget in suite-common; biggest power-per-line)
2. Letters selection popover + status readout
3. Tables selection stats + name box + format-cells sheet
4. Decks inspector + notes drawer
5. Breakpoint/adaptive pass on all three (narrow-width usable)
6. Feature-surfacing audit sweep against PARITY.md, then screenshot set

## Cross-app clipboard (styling and data must survive)

Copying between Letters, Tables, and Decks is where suite-ness becomes
real — and our shared model makes it cheap to do *properly*:

- **Suite fragment format** (`application/x-tunaos-suite+json`): a serde
  serialization of model fragments — styled runs/paragraphs from Letters
  (letters-core types), a cell grid with *formulas, values, and number
  formats* from Tables, SlideObjects from Decks (which already carry
  letters-core Run/RunStyle). Because Letters and Decks literally share
  the run types, styled text pastes losslessly in both directions by
  construction.
- **Standard formats alongside**: every copy also offers `text/html`
  (styled interchange with external apps; tables as `<table>`) and
  `text/plain` (TSV for cell grids). Paste prefers suite JSON → HTML →
  plain.
- **Paste mapping matrix** (each cell of this matrix is a pure function
  in the core crates, unit-testable without a clipboard):
  - Tables grid → Letters: a real cell-tagged table (the table model
    exists); formulas degrade to values, formats to formatted strings.
  - Tables grid → Decks: a text box per row, or a table-styled box (v2).
  - Letters styled text → Decks text box: runs carried verbatim.
  - Decks text box → Letters: runs carried verbatim.
  - External HTML → all three: through the existing HTML-ish readers.
- **Testing**: `to_fragment`/`from_fragment` round-trips live in the core
  crates (I1); a GUI smoke test covers one real end-to-end copy/paste per
  direction once the GDK plumbing lands (I6).

Implementation order: serde feature on letters-core model types → fragment
module in suite-common-core → GDK ContentProvider glue per app.
