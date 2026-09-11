# Architecture — GTK Office Suite

> Detailed architecture, module layout, dependency inventory, and the
> LibreOffice patterns this codebase borrows from. For a newcomer's
> orientation start with the [README](../README.md); for GNOME HIG
> compliance rules see [GNOME-GUIDELINES.md](GNOME-GUIDELINES.md); for
> the conventions you are expected to follow when writing code see
> [CONTRIBUTING.md](CONTRIBUTING.md).

---

## The core/GTK split

The organising principle of the whole workspace: **every app is two
crates.** A `*-core` crate holds the document model, the file formats and
the undo commands and has no GTK dependency at all; a binary crate on top
holds the windows, widgets and rendering.

That boundary is load-bearing, not cosmetic:

- The document models are testable without a display, which is why the
  property tests, round-trip tests, seeded command campaigns and oracle
  comparisons all live in the core crates.
- The core crates can be published to crates.io and used without GTK.
- A bug is either in the model or in the view, and the crate boundary
  usually says which before you start reading.

The rule that keeps it honest: **a GTK window never owns authoritative
document state.** See [Canonical Document Controllers](#canonical-document-controllers--threading-rules)
below.

> **Which `suite-common`?** Two other repositories in the organisation
> carry this name — a standalone Python/meson library, and
> `suite-common-rust`, a smaller separately-maintained Rust crate.
> **This workspace's `suite-common/` is the canonical, actively-developed
> shared Rust crate.** See
> [tunaos#517](https://github.com/tuna-os/tunaos/issues/517) for the
> collision this note resolves.

---

## Workspace Structure

Nine crates. Line counts are indicative of weight, not targets.

| Crate | GTK? | Published | Holds |
|---|---|---|---|
| `suite-common-core` | no | yes | Undo, number formats, styles, property pool, search, print, units, atomic saves, autosave, ZIP bounds, interop reports |
| `suite-common` | yes | no | Command palette, shortcuts dialog, file dialogs, toasts, GTK test helpers |
| `suite-export` | no | yes | PDF export |
| `letters-core` | no | no | Document model, DOCX/ODT/Markdown I/O, structured editing, sessions |
| `letters` | yes | no | Window, tabs, GtkTextView bridge, ruler, styles |
| `tables-core` | no | yes | Sheet model, IronCalc engine wrapper, XLSX/ODS/CSV I/O, workbook controller |
| `tables` | yes | no | Window, Cairo grid, charts, dialogs |
| `decks-core` | no | no | Deck model, PPTX/ODP I/O, object commands, decks controller |
| `decks` | yes | no | Window, slide canvas, sidebar, transitions, present mode |

```
gtk-office-suite/
├── suite-common-core/src/   # atomic_save.rs  autosave.rs  zip_guard.rs
│                            # undo.rs  format.rs  style.rs  props.rs
│                            # search.rs  print.rs  units.rs  interop.rs
│                            # session.rs  templates.rs  recent.rs  palette.rs
├── suite-common/src/        # lib.rs  file_dialogs.rs  toast_manager.rs
│                            # gtk_test.rs
├── suite-export/src/
│
├── letters-core/src/        # model.rs  structured.rs  docx.rs  odt.rs
│                            # markdown.rs  session.rs  fragment.rs
├── letters/src/             # window.rs  bridge.rs  actions.rs  doc_tab.rs
│                            # styles.rs  ruler.rs
│
├── tables-core/src/         # sheet.rs  engine.rs  controller/  io/
├── tables/src/              # window.rs  grid_area.rs  charts.rs
│
├── decks-core/src/          # engine/  odp.rs  undo.rs  controller.rs
├── decks/src/               # window.rs  canvas.rs  canvas_area.rs
│                            # sidebar.rs  toolbar.rs  transition.rs
│
├── tests/                   # gui/ (AT-SPI journeys, container, stress)
│                            # Python harness tests
├── interop/                 # Reviewable interop corpus (unzipped package XML)
├── conformance/             # Capability ledger and validators
├── fuzz/                    # cargo-fuzz targets (separate workspace)
├── flatpak/                 # Manifests, metainfo, desktop files, schemas, icons
├── flathub/                 # Flathub-layout manifests (submission prep)
├── po/                      # Translations
├── runbooks/  scripts/  skills/
└── docs/                    # This directory
```

### Module size guidance

| File | Soft limit | Split into |
|---|---|---|
| `window.rs` | 600 | `canvas.rs`, `toolbar.rs`, `sidebar.rs` |
| `engine.rs` | 500 | `read.rs` + `write.rs` for format I/O |
| `undo.rs` | 250 | By command category (object, slide, format) |

Over the limit today, and known to be (measured, not estimated):

| File | Lines |
|---|---|
| `tables/src/window.rs` | 2,329 |
| `decks/src/window.rs` | 1,783 |
| `letters/src/window.rs` | 1,278 |
| `tables-core/src/sheet.rs` | 1,222 |
| `suite-common/src/lib.rs` | 1,219 |
| `letters/src/bridge.rs` | 1,199 |

These are real debt, not a style preference: the reentrancy crashes fixed
in v2.1.0 all lived in files this size.

---

## Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Formula engine | IronCalc (not Formualizer) | MIT/Apache 2.0 license, 83 functions, simpler API |
| Grid rendering | Cairo DrawingArea (not GtkColumnView) | Full cell-level control, freeze/merge support |
| Rich text | GtkTextView + TextTags (not WebKit) | Native GTK, no JS dependency, lighter Flatpak |
| File I/O | calamine + rust_xlsxwriter (Tables), pulldown-cmark + rdocx (Letters), zip + quick-xml (Decks) | Most mature Rust libraries per format |
| Document format | Markdown (Letters canonical) | Simple text storage, renders via pulldown-cmark |
| Undo architecture | Generic `Command<T>` trait in `suite-common-core` | Matches LO's SfxUndoAction, reusable across all apps, and GTK-free so it is testable headlessly |
| Number formatting | `suite-common-core` NumberFormat, num-format crate | Matches LO's SvNumberFormatter, locale-aware |

---

## Canonical Document Controllers & Threading Rules

To allow headless testing, dirty state tracking, and consistent undo history without GTK runtime dependencies:

1. **Controller Ownership Boundary**:
   - `tables_core::controller::WorkbookController` owns the canonical workbook state (`WorkbookState`), engine calculation context, undo history, file path identity, and dirty flag.
   - `decks_core::controller::DecksController` owns the slide list, master slide list, object commands, undo history, file path identity, and dirty flag.
   - `letters_core::session::DocumentSession` owns per-tab session identity (file path, autosave slot, closing-after-save state).
   - GTK application windows (`window.rs`) do not duplicate authoritative document state; they act strictly as view/input adapters over the shared core controllers.
   - Core format crates (`tables-core`, `decks-core`, `letters-core`) act as persistence adapters for format I/O (XLSX/ODS, PPTX/ODP, DOCX/ODT/MD).

2. **Threading and Mutability Rules**:
   - Controllers run on the main UI/application thread (using `Rc<RefCell<...>>` / `Rc<Cell<...>>`) to match GTK4 single-threaded event dispatching.
   - Asynchronous background tasks (e.g. file loading or rendering workers) must pass immutable snapshots or owned data across threads, communicating results back to the main thread via glib channels or idle callbacks before updating the canonical controller.
   - All state mutations (cell edits, slide modifications, undo/redo operations) MUST route through controller methods to guarantee recalculation, dirty flag synchronization, and undo history lockstep.

---

## LibreOffice Architecture Mapping

Our architecture deliberately mirrors LibreOffice's shared-layer pattern:

```
LibreOffice                    GTK Office Suite
═══════════                    ════════════════
svl/ (shared library)    →     suite-common/
  undo.hxx                     undo.rs       (SfxUndoAction → Command<T>)
  numformat.hxx                format.rs     (SvNumberFormatter → NumberFormat)
  SfxBroadcaster.hxx           events.rs     (SfxBroadcaster → Broadcaster<H>)
  style.hxx                    (v2: style.rs)
  poolitem.hxx                 (v2: props.rs)

editeng/ (editing engine) →     per-app modules
  borderline.hxx               CellBorder in tables/src/window.rs

sc/ (Calc)                →     tables/
  cell, document, formula       engine.rs, window.rs
  sort, filter, merge           window.rs (SheetModel methods)

sd/ (Draw/Impress)        →     decks/
  slidesorter                  sidebar.rs
  undoobjects                  undo.rs
  presenter                    transition.rs
```

Where the shared-layer equivalents actually live today (the tree above
predates the `*-core` split):

| LibreOffice | Our equivalent | File |
|---|---|---|
| `svl/undo.hxx` — SfxUndoAction, SfxUndoManager | `Command<T>`, `UndoManager<T>` | `suite-common-core/src/undo.rs` |
| `svl/numformat.hxx` — SvNumberFormatter | `NumberFormat`, `NumberFormatKind` | `suite-common-core/src/format.rs` |
| `svl/SfxBroadcaster.hxx` — SfxBroadcaster, SfxListener | `Broadcaster<H>`, `Listener<H>` | `suite-common-core/src/events.rs` |
| `svl/style.hxx` — SfxStyleSheet | style inheritance | `suite-common-core/src/style.rs` |
| `svl/poolitem.hxx` — SfxItemPool | property pool | `suite-common-core/src/props.rs` |
| `editeng/borderline.hxx` — SvxBorderLineStyle | `CellBorder`, `BorderStyle` | `tables/src/window.rs` |

**Reference:** LibreOffice source at `~/dev/libreoffice-core/` (sparse checkout: `sc/`, `sd/`, `svl/`, `editeng/`).

A note on how LibreOffice is used: as an **oracle**, not a porting target.
The parity corpora run real Writer and Impress and compare against what
they produce, rather than against a hand-written belief about what they
would produce. See ["An oracle, not a port"](blog/2026-07-18-oracle-not-port.md).

---

## Data Flow — Tables

```
User clicks cell
  → GestureClick → xy_to_cell(scroll_x, sheet)
  → sheet.selected_row/col = (row, col)
  → da.queue_draw()

User types in formula bar
  → fx_entry.connect_activate()
  → engine.set_cell_text(row, col, val)
  → engine.evaluate()
  → sheet.sync_from_engine()
  → da.queue_draw()

Draw cycle:
  → da.set_draw_func(draw_grid)
  → draw_grid() → draw_grid_region() (if freeze panes)
  → renders headers, cells with formatting, borders, selection highlight
```

---

## Data Flow — Decks

```
User clicks object on canvas
  → GestureClick → hit_test_object(slide.objects, x, y)
  → selected_object.set(Some(index))
  → da.queue_draw()

User drags object
  → GestureDrag → drag_begin: hit_test + capture position
  → drag_update: set_obj_position() with snap_to_grid
  → drag_end: MoveObjectCmd → undo.execute()

User double-clicks text box
  → GestureClick(n=2) → hit_test_object → inline GtkTextView
  → Enter key: ChangeTextCmd → undo.execute()

Slide navigation (arrows in present mode)
  → key_pressed → TransitionState.start(from, to)
  → glib::timeout 16ms → draw_transition() via Cairo surfaces
```

---

## Dependency inventory

External crates, by what they are relied on for. The rule is in
[Crate Selection Rules](#crate-selection-rules) below: never re-implement
a format parser, formula engine, or graphics library.

| Crate | Used by | Relied on for |
|---|---|---|
| `ironcalc_base` | tables-core | Formula engine and evaluation |
| `calamine` | tables-core | XLSX, XLS, ODS reading |
| `rust_xlsxwriter` | tables-core | XLSX writing |
| `rdocx`, `rdocx-oxml` | letters-core, letters | DOCX read/write |
| `pulldown-cmark` | letters-core, letters, decks | Markdown parsing |
| `quick-xml` | letters-core, decks-core, decks | ODF/OOXML XML — a pull parser, which is why deep nesting and entity expansion are not a hazard |
| `zip` | every core crate | ODF/OOXML package containers (bounded by `suite_common_core::zip_guard`) |
| `typst-as-lib`, `typst-pdf` | suite-export | PDF generation |
| `num-format`, `chrono` | suite-common-core | Locale-aware numbers, date parsing |
| `regex` | suite-common-core, tables-core | Search, data validation |
| `serde`, `serde_json` | several | Settings, clipboard fragments, reports |
| `tempfile` | several | Atomic saves, test fixtures |
| `zspell` | letters | Spell checking |
| `cairo-rs`, `pango`, `pangocairo` | letters, tables, decks | Text measurement and rendering |
| `image` | decks | JPEG/WebP/GIF decoding |
| `gettext-rs` | suite-common | Translations |

Development-only: `proptest` (round-trip properties), `libfuzzer-sys` (in
the separate `fuzz/` workspace), plus `dogtail`, `pytest` and `ffmpeg` for
the GUI journeys.

---

## Reference implementations

Consulted for patterns; none of this is vendored.

| Project | Why it is useful here |
|---|---|
| [LibreOffice core](https://git.libreoffice.org/core) | Feature catalogs, shared-layer architecture, undo/number-format/border patterns. Also the parity oracle. |
| [IronCalc](https://github.com/ironcalc/ironcalc) | The formula engine Tables uses |
| [Rnote](https://github.com/flxzt/rnote) | Rust/GTK4 canvas drawing, undo/redo, selection |
| [Loupe](https://gitlab.gnome.org/GNOME/loupe) | Cairo, DrawingArea, fullscreen, gestures |
| [Papers](https://gitlab.gnome.org/GNOME/papers) | Find sidebar, search box |
| [GNOME HIG](https://developer.gnome.org/hig/) | Widget and layout patterns |

---

## Crate Selection Rules

1. **Prefer existing Rust crates** — never re-implement a format parser, formula engine, or graphics library
2. **Check license compatibility** — MIT/Apache 2.0 preferred; GPL-3.0 matches our license
3. **Minimize dependency tree** — prefer crates with few transitive deps
4. **Active maintenance** — check last commit date, open issues
5. **Create new crates only when** — no existing crate exists for the job AND the code would be reused across multiple apps

---

## Keyboard Shortcuts

| Shortcut | Letters | Tables | Decks |
|----------|---------|--------|-------|
| Ctrl+Z | Undo | Undo | Undo |
| Ctrl+Shift+Z | Redo | Redo | Redo |
| Ctrl+S | Save | Save | Save |
| Ctrl+O | Open | Open | Open |
| Ctrl+N | New document | New sheet | New presentation |
| Delete | Delete text | Clear cell | Delete object |
| Escape | — | — | Exit fullscreen |
| Arrows | Navigate text | — | Navigate slides |
| F5 | — | — | Present |
