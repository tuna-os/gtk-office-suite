# Product and Quality Roadmap

Date: 2026-07-21

Execution is tracked in the [scoped issue backlog](ISSUE-BACKLOG-2026-07.md)
and coordinating [GitHub issue #95](https://github.com/tuna-os/gtk-office-suite/issues/95).

This roadmap resets the definition of "working" around complete user journeys.
The format engines are a strong foundation, but a passing round-trip test does
not prove that the GUI edits the same state, that undo restores it, or that the
application protects unsaved work.

## Product direction

Build three focused, GNOME-native applications that share a small office core:

- **Letters**: an excellent everyday document editor, not a partial clone of
  every Writer feature.
- **Tables**: a dependable spreadsheet for real workbooks, with a scalable
  grid, trustworthy recalculation, and complete multi-sheet behavior.
- **Decks**: a fast presentation editor with polished direct manipulation,
  themes/layouts, and reliable PPTX/ODP exchange.

Use existing standards and mature engines wherever possible. OOXML,
OpenDocument, OpenFormula, CommonMark, PDF, GTK, Pango, Cairo, IronCalc,
calamine, rust_xlsxwriter, and rdocx are integration boundaries, not features
to reimplement. New custom code should concentrate on the document model,
GNOME interaction design, state management, and adapters around those tools.

## Current assessment

The core format test suite is healthy: `cargo test --workspace --all-targets`
passes, including the installed LibreOffice oracle tests. The run on 2026-07-21
took about four minutes from a cold dependency build. It also emitted a large
warning set, including dead GUI fields and deprecated GTK APIs.

Product confidence is lower than the green core suite suggests:

1. Tables' live workbook and undo manager own different `SheetState` values.
   GUI edits never execute the table undo commands, so the registered undo and
   redo actions operate on a detached snapshot.
2. Tables presents multiple sheets but uses one calculation engine. Switching
   sheets synchronizes the selected sheet from that shared engine, allowing
   one sheet's data to overwrite another.
3. Tables does not track dirty state or confirm before closing. Decks has the
   same data-loss risk. Letters has the only close guard.
4. Decks installs imported slides on open but does not install the imported
   master collection, losing master content in the live editor and later save.
5. Most preferences are inert controls. Tables and Decks ignore their settings
   argument; in Letters, only spell checking is persisted. Auto-save controls
   exist without an auto-save implementation.
6. Letters' pagination measures the buffer as one unstyled Pango layout,
   estimates a uniform line height, and splits pages using line-derived byte
   offsets. It is not yet a WYSIWYG pagination/layout engine.
7. Tables starts at 100 rows by 26 columns, truncates displayed cell text to 14
   characters, uses hard-coded light colors, and computes its first visible
   column using the default width even after columns are resized.
8. The large window modules (`letters` 2,181 lines, `tables` 1,467, `decks`
   1,312) mix state, actions, dialogs, rendering, and persistence. This makes
   integration defects easy to introduce and difficult to test without a GUI.

## Definition of done

A capability is complete only when all applicable layers are green:

| Layer | Required evidence |
|---|---|
| Model | Deterministic unit/property tests for edits and invariants |
| Persistence | Read, write, and loss-budget tests on native fixtures |
| Interoperability | Our write -> LibreOffice read/rewrite -> our read, plus LibreOffice-authored input |
| Integration | The real app action mutates the canonical model and undo/redo restores it |
| Journey | GUI test covers create/open, edit, save, reopen, and close behavior |
| Visual | Stable screenshots at wide/narrow sizes, light/dark, and 100/200% scale |
| Accessibility | Keyboard-only completion and AT-SPI role/name/state checks |
| Performance | A budgeted benchmark for document open, edit latency, scrolling, and save |

Tests that only exercise an isolated command type may not be used to mark its
GUI feature complete. The parity scorecard should report the layers separately
instead of collapsing them into one check mark.

## Test framework expansion

### 1. Add a canonical app-state integration layer

Move each application's mutable document state behind a GTK-free controller.
Actions call the controller; GTK observes controller events. Test the same
controller entry points used by the GUI.

Minimum regression cases:

- Tables: edit A1, undo, redo, save, reopen; repeat on two sheets and assert no
  cross-sheet mutation.
- Decks: open a deck with a decorated master, edit a slide, undo, save, reopen,
  and assert the master and mapping remain.
- Letters: apply mixed styles across Unicode text, paginate, save, reopen, and
  assert layout-affecting properties and offsets remain valid.

### 2. Extend the GUI harness from smoke checks to journeys

Keep AT-SPI for discoverability and keyboard input. Add test-only diagnostic
actions, enabled only in test builds, that serialize a normalized state
snapshot. A journey should assert both visible behavior and final model state;
screenshots alone are insufficient.

Required shared helpers:

- temporary XDG data/config directories and isolated GSettings;
- deterministic locale, fonts, theme, scale, clock, and animation settings;
- file-dialog handling through the document portal or an injectable chooser;
- structured app logs and automatic artifact capture on failure;
- wait-for-condition helpers based on GTK/AT-SPI state, never fixed sleeps;
- a reusable unsaved-close, save-as, reopen, clipboard, and crash-recovery
  journey.

### 3. Build an interoperability corpus with loss budgets

Store small, reviewable fixtures authored by LibreOffice and, where licensing
allows, Microsoft Office. Record the authoring application/version and the
expected normalized semantics in a sidecar manifest.

Compare semantics rather than ZIP bytes. Each format needs a declared loss
budget:

- **Must preserve**: text/values, formulas, sheet/slide order, styles exposed
  by the UI, images, links, notes, page geometry, and accessibility-relevant
  structure.
- **May preserve opaquely**: unsupported OOXML/ODF parts that can safely be
  copied through without interpretation.
- **May lose with warning**: explicitly unsupported constructs when saving to
  a less capable format.
- **Must never silently lose**: supported content, unknown content promised as
  pass-through, or edits in another open sheet/document.

Add package-level XML assertions for relationships and content types, then
retain LibreOffice as the behavioral oracle. When available in CI, add an
optional OnlyOffice command-line/render comparison lane; do not make a closed
Office installation a required developer dependency.

### 4. Add differential and property testing

- Generate bounded documents/workbooks/decks with `proptest`, round-trip them,
  and assert normalized equivalence.
- Fuzz ZIP/XML and import entry points with `cargo-fuzz`; malformed external
  files must return errors without panic or excessive allocation.
- Differentially evaluate supported spreadsheet formulas against LibreOffice
  with stable numeric tolerances and explicit date/locale cases.
- Test Unicode grapheme, bidi, combining-mark, emoji, and CJK input throughout
  cursor movement, selection, pagination, and persistence.

### 5. Make visual tests deterministic and useful

Capture scenario screenshots, not only launch screens. Establish a small set
of golden images after the state bugs are fixed. Use pixel/perceptual diffs for
geometry and color regressions; use a VLM only for non-deterministic semantic
review. Always retain the baseline, actual, diff, app log, and state snapshot.

Matrix:

| Dimension | Required values |
|---|---|
| Width | 400, 800, 1280 |
| Theme | light, dark, high contrast |
| Scale | 1x, 2x |
| State | empty, populated, selection, dialog, error |

### 6. CI gates

Create three lanes:

- **Fast PR gate (under 8 minutes warm):** format, clippy with warnings denied
  for changed crates, unit/property tests, package validation, headless
  controller journeys.
- **GUI PR gate:** focused AT-SPI journeys and deterministic screenshots for
  changed applications.
- **Nightly/release gate:** full LibreOffice corpus, fuzz smoke, large-document
  performance, complete visual matrix, Flatpak build/install/launch, and file
  association/portal tests.

Publish JUnit plus a machine-readable capability matrix. A release is blocked
by a regression in data safety, a Tier 1 journey, or the interoperability loss
budget—not by an aspirational feature that has never been admitted to scope.

## Delivery roadmap

### Phase 0 — Truth and data safety

1. Replace the single check-mark parity table with layered status.
2. Add failing controller/GUI regressions for Tables undo, two-sheet isolation,
   Decks master loading, and close-with-unsaved-work in all three apps.
3. Wire dirty state, atomic save (temporary file plus rename), save failure
   reporting, auto-save/recovery, and close confirmation consistently.
4. Turn inert preferences into bindings or remove them until implemented.
5. Make warnings actionable and deny new warnings in changed code.

Exit criterion: no known silent data-loss path; all existing advertised
actions are connected to live state.

### Phase 1 — Canonical document controllers

1. Give Tables one canonical workbook model with a calculation context per
   workbook/sheet as required by IronCalc; remove mirrored GUI and undo state.
2. Give Decks one `Deck` state object, including masters, and make every edit a
   command against it.
3. Move Letters from buffer-authoritative state toward model-authoritative edit
   operations while retaining GtkTextView as the editing surface.
4. Split window modules into controller, actions, persistence, dialogs, and
   views.

Exit criterion: headless tests exercise the exact commands invoked by GTK.

### Phase 2 — Credible daily-driver depth

**Letters**

- WYSIWYG pagination based on styled runs and paragraphs; reliable Unicode
  offsets; editable tables; list indentation/nesting; paragraph spacing;
  headers/footers and page breaks visible in the editor.
- Then comments, change tracking, TOC, bidi/RTL editing, and section layouts.

**Tables**

- Sparse/virtualized grid that grows on demand; row/column insertion/deletion;
  sheet rename/delete/reorder; fill handle; formula-reference editing;
  filters; named ranges; print areas and page setup.
- Then richer charts, pivot tables, array/dynamic formulas, protection, and
  large-workbook performance budgets.

**Decks**

- Selection handles, resize/rotate, multi-select, grouping, arrange/align,
  shape fill/stroke/text properties, image crop/fit, duplicate slide, and
  slide layout/theme selection.
- Then presenter view, export/print, animations, media, comments, and template
  workflows.

Exit criterion: each app completes its create-edit-save-reopen daily-driver
journey using both its preferred open format and the common Microsoft format.

### Phase 3 — GNOME-native polish

Use [hanthor/gnome-gui-spec](https://github.com/hanthor/gnome-gui-spec) skills,
intent maps, and application audits for design review and scaffolding, together
with this repository's `vision-check` and `broadway-inspect` verification
skills. Treat current upstream GNOME HIG and libadwaita documentation as the
final authority.

1. Replace dense undifferentiated toolbars with a small primary toolbar,
   contextual controls, popovers, and discoverable menus.
2. Use theme colors and Pango everywhere; remove hard-coded light canvases and
   Cairo toy text rendering.
3. Provide consistent recent files, templates, drag/drop, portal-backed file
   access, error recovery, adaptive layouts, touch targets, and help.
4. Complete keyboard and screen-reader journeys, including spatial bounds for
   virtual canvas/grid children.

Exit criterion: light/dark/high-contrast and wide/narrow visual matrices pass,
with all Tier 1 journeys keyboard-completable.

### Phase 4 — Ecosystem and release confidence

1. Grow compatibility fixtures from real-world, sanitized documents.
2. Add migration warnings and a document-inspector report for unsupported
   features before save.
3. Validate Flatpak portals, MIME associations, thumbnails, translations,
   crash reporting policy, reproducible builds, and upgrade/recovery behavior.
4. Publish the capability matrix with every release.

## Near-term implementation queue

The next engineering sessions should be kept in this order:

1. Tables canonical workbook plus live undo wiring.
2. Shared dirty/save/close/recovery controller and cross-app journey tests.
3. Decks master-state open regression and fix.
4. Real preference bindings and theme-correct custom rendering.
5. State-snapshot support in the GUI harness and two-sheet/master/unsaved
   end-to-end tests.
6. Refactor the three window modules only along the seams introduced above.
7. Begin the daily-driver depth work, starting with scalable Tables storage,
   Letters layout correctness, and Decks direct-manipulation tools.
