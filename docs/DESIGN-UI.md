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

## Ideas taken from iWork (direction set 2026-09-24)

The project owner's direction: **take the best design ideas from Apple's
iWork (Pages, Keynote, Numbers), build them in Rust so they are fast, and
express them through GNOME HIG and libadwaita.** Take the idea, not the
look. Each item below names the iWork pattern, the libadwaita form it
takes here, and what it needs from the engines.

Order matters. These build on the Render Parity Roadmap's Phase 1 (one
renderer per app). A beautiful inspector that edits a property the canvas
doesn't draw is the failure this project already had. So each item lands
only with its render-lab fixture green or improving.

### Shared across the suite

- **The Format inspector (iWork's right-hand "Format" sidebar)**
  - Pattern: one context-sensitive sidebar replaces format dialogs. Its
    tabs follow the selection: Text / Arrange / Style for a shape; Cell /
    Table / Text for cells.
  - Here:
    - `AdwOverlaySplitView` on the right, with an `AdwViewSwitcher` for
      the tabs and `AdwPreferencesGroup` rows (`AdwSpinRow`,
      `AdwComboRow`, `AdwSwitchRow`) for the fields.
    - A header toggle opens it. It collapses to a bottom sheet under the
      narrow `AdwBreakpoint`.
    - Decks' existing inspector becomes this; Letters and Tables gain one.
  - Needs: the Phase 1 style models (Tables `CellStyle`, Decks shape
    style, Letters paragraph and character styles). The inspector edits
    exactly those fields, each through an undoable command.
- **Insert buttons, not menus (iWork's Table · Chart · Text · Shape ·
  Media toolbar)**
  - Here: a short row of labelled icon buttons in the header bar, and a
    `GtkPopover` shape and media library with a search entry.
  - Needs: nothing new for text, table and image. Shapes need the Decks
    shape model.
- **Styles first (paragraph, cell, table and chart styles, previewed)**
  - Pattern: iWork formats through named styles shown as live previews,
    not raw attributes.
  - Here: style pickers render each entry in its own style (a
    `GtkListView` with a custom factory), with "Update style to match
    selection".
  - Needs: named styles in each model; Letters already has them.
- **Templates that look finished (iWork's template chooser)**
  - Here: an `AdwNavigationView` start page with a `GtkGridView` of real
    rendered thumbnails. The thumbnails come from the same renderer as the
    canvas (Phase 1), cached on disk, never hand-drawn.
- **Smart guides and snapping**
  - Pattern: iWork's yellow alignment guides for centre and edges, equal
    spacing, and size matching.
  - Here: guides in the accent colour, computed in the GTK-free core from
    object rectangles (unit-tested), drawn as render nodes.

### Letters (from Pages)

- **Page thumbnails sidebar and a real page view.** Pages' left sidebar of
  page thumbnails depends on the per-page layout engine (Phase 1: render
  tree → pages). Thumbnails are that tree drawn small, not a second
  layout. ✅ (2026-09-25): the sidebar's Pages view beside the Outline
  (Ctrl+Alt+P), drawn by `PageView::draw_page_at` from the page view's
  own typeset; the real page view is ADR 0010's Print Layout.
- **Word processing vs. page layout.** Pages has two document kinds: flowing
  text, and free placement of text boxes on pages. Start with flowing
  text on real pages; free placement reuses the Decks object model later.
- **Distraction-free typing.** The chrome fades while typing and comes back
  on pointer motion (`AdwToolbarView` reveal properties). ✅ (2026-09-25):
  `app.distraction-free` (Ctrl+Alt+D), off by default and remembered;
  shortcuts never hide the bars.

### Decks (from Keynote)

- **Magic Move.**
  - Pattern: a transition that morphs matching objects between
    consecutive slides.
  - Here: objects are matched by id or content in the core. Position,
    size, rotation and opacity are tweened through GSK transform and
    opacity nodes on the GPU renderer: no per-frame re-layout, 60 fps.
    This is where Rust and GSK pay off.
- **Presenter display.** Current slide, next slide, notes and a timer on
  the second monitor, with the audience window fullscreen. The layout uses
  `AdwBreakpoint` so a single-monitor rehearsal mode works too.
- **Object builds (animate in/out, one item at a time)**, driven by the
  same tween engine as Magic Move.
- **Master slides with real placeholders.** A new slide inherits its
  layout's title and body placement and text styles. Geometry inheritance
  landed in #932; text-style inheritance is the Decks shape-style item.

### Tables (from Numbers)

- **Tables as objects on a free canvas.** Numbers' biggest idea is a sheet
  that is a canvas holding several independent tables, charts and text,
  not one infinite grid. It is a large model change, so it gets an ADR
  before any code. The Excel-compatible grid remains the default and the
  interchange format.
- **Header rows and columns that are structural.** They are frozen,
  styled, and used as chart labels and formula names (`=Price × Qty`).
- **Table styles** (banded rows, header emphasis) as named presets, built
  on the cell-style model.
- **Formula editor with range tokens.** References show as coloured chips
  that highlight their range on the grid. The formula-reference colouring
  already exists and is extended into the editor.

## Ideas taken from Google Docs, Sheets and Slides (direction set 2026-09-24)

The project owner also wants the best of Google's editors, and
collaboration if possible. Google's strength is **working together**
rather than layout. So most of the ideas below arrive through RFC-0001
(CRDT collaboration). The rest stand alone and don't wait for it.

### Collaboration (RFC-0001; offline-first, the file stays the document)

- **Live presence**
  - Pattern: collaborators' cursors and selections, each in their own
    colour with a name flag; avatars in the header.
  - Here:
    - an `AdwAvatar` row in the header bar;
    - cursors and selections drawn as render nodes on the canvas;
    - presence state in the core, transport in the shell (RFC-0001
      "Where replication state lives").
- **Share without a cloud account**
  - Here: RFC-0001's LAN or explicit-relay transport. An
    `AdwAlertDialog` "Share" flow shows a join code, off by default. Access
    control waits for RFC-0001's identity work.
- **Comments and threads, @mentions**
  - Letters already has review workflows (`LETTERS-REVIEW-WORKFLOWS.md`);
    Tables and Decks gain the same anchored-comment model.
  - The side panel is the Format inspector's sibling: one
    `AdwOverlaySplitView`, with a tab each.
- **Suggestion mode (Docs)**
  - Edits become tracked suggestions others accept or reject. This builds
    on Letters' tracked changes, and in collaboration each suggestion is
    an operation attributed to its author.
- **Version history with named versions**
  - Here: a timeline sidebar of snapshots, with author colours on changed
    text or cells, "Name this version", and restore as an undoable
    command.
  - It sits on RFC-0001 Phase 4's sidecar history. Before collaboration
    exists, the same UI can show autosave snapshots.
- **Filter views (Sheets)**
  - A filter that is yours alone and doesn't reorder the sheet for
    everyone else. It's local view state that is never replicated, which
    the model has to distinguish from shared state.

### Standalone (no collaboration needed)

- **Docs:**
  - a document outline sidebar built from headings (Letters' structured
    editing already has the tree);
  - "pageless" as a view mode alongside real pages;
  - smart chips (dates, people, links) as inline objects.
- **Sheets:**
  - formula autocomplete with argument hints;
  - dropdown chips for data validation (the model already has
    validations);
  - Explore-style suggested charts and pivots from a selection;
  - smart fill;
  - named and protected ranges surfaced in the inspector.
- **Slides:**
  - "Explore" layout suggestions;
  - a theme builder that edits the master;
  - charts linked to a Tables range, which update when the sheet
    changes (the cross-app clipboard section below is the start).
- **Everywhere:**
  - the command palette (done), in the spirit of Docs' "Search the menus";
  - keyboard-first everything.

### Order

Collaboration features follow RFC-0001's phases: Tables first, then Decks,
then Letters once it has a live GTK-free model. The standalone items follow
the same rule as the iWork ones: they land on the Phase 1 models, each with
its render-lab fixture or GUI journey.

### Rusty and fast (non-negotiable for every item above)

- Canvas drawing goes to GSK render nodes, with Cairo only inside
  well-bounded leaves. Per-page and per-slide render trees are cached and
  invalidated by edit ranges, never rebuilt per frame.
- Layout, hit-testing, snapping, tweening and style resolution live in the
  GTK-free core crates, with unit tests and the existing
  `performance_budgets` tests extended to them (typing latency, slide
  switch, open time for large files).
- No per-frame allocation in draw paths. Scrolling and animation are
  profiled (`GSK_DEBUG`, Sysprof), and regressions fail CI through
  performance budgets.

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

1. ✅ Command palette (2026-07-18; prerequisite discovered and done:
   action-registry unification — Tables/Decks toolbars were closure-wired)
2. ✅ Letters selection popover + style readout (2026-07-18); ✅ paragraph
   style picker drawn in each style by the page's own shaper, and the
   headings outline sidebar, both on the live model (2026-09-25),
   covering body text, Title, Subtitle, six headings, Quote and Code, all
   of which the page draws.
3. Tables: ✅ selection stats + name box + range selection + keyboard nav;
   format-cells sheet still open (only the cycle button exists)
4. Decks: ✅ inspector + presenter pill + status; notes drawer polish and
   slide thumbnails still open
5. Breakpoint/adaptive pass: SuiteWindow collapses the toolbar at 600sp;
   full narrow-width audit still open
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
