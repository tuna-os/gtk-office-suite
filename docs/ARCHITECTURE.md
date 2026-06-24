# Architecture — GTK Office Suite

> Detailed architecture, module layout, and LibreOffice pattern mappings.
> For GNOME HIG compliance rules, see [GNOME-GUIDELINES.md](GNOME-GUIDELINES.md).

---

## Workspace Structure

```
gtk-office-suite/
├── suite-common/       # Shared library crate
│   └── src/
│       ├── lib.rs      # Module declarations + re-exports
│       ├── undo.rs     # Generic Command<T> + UndoManager<T>
│       ├── format.rs   # NumberFormat engine
│       ├── events.rs   # Broadcaster<H> + Listener<H>
│       ├── file_dialogs.rs
│       └── toast_manager.rs
│
├── letters/            # Word processor binary
│   └── src/
│       ├── main.rs
│       ├── window.rs   # LettersWindow (1,820 lines — large, needs refactor)
│       ├── engine.rs   # Document model, format conversion
│       ├── export.rs   # Typst/PDF export
│       ├── styles.rs   # GtkTextTag management
│       └── ruler.rs    # Horizontal ruler widget
│
├── tables/             # Spreadsheet binary
│   └── src/
│       ├── main.rs
│       ├── window.rs   # TablesWindow + SheetModel + draw_grid (refactored: ~1,200)
│       ├── engine.rs   # TablesEngine wrapping IronCalc
│       ├── charts.rs   # Cairo chart rendering (Bar, Line, Pie)
│       ├── export.rs   # Typst/PDF export
│       └── undo.rs     # Table-specific undo commands
│
├── decks/              # Presentations binary
│   └── src/
│       ├── main.rs
│       ├── window.rs   # DecksWindow (refactored: ~650 lines)
│       ├── engine.rs   # PPTX I/O, Slide/SlideObject model
│       ├── canvas.rs   # Slide rendering + image loading + hit testing
│       ├── toolbar.rs  # Toolbar builder
│       ├── sidebar.rs  # Slide list rebuild
│       ├── undo.rs     # Deck-specific undo commands
│       ├── transition.rs  # Slide transition animations
│       └── export.rs   # Typst/PDF export
│
├── flatpak/            # Flatpak packaging
│   ├── org.tunaos.letters-rust.json
│   ├── org.tunaos.letters-rust.metainfo.xml
│   ├── org.tunaos.letters-rust.desktop
│   ├── org.tunaos.tables-rust.json
│   ├── org.tunaos.tables-rust.metainfo.xml
│   ├── org.tunaos.tables-rust.desktop
│   ├── org.tunaos.decks-rust.json
│   ├── org.tunaos.decks-rust.metainfo.xml
│   ├── org.tunaos.decks-rust.desktop
│   └── icons/
│       ├── org.tunaos.letters-rust.svg
│       ├── org.tunaos.tables-rust.svg
│       └── org.tunaos.decks-rust.svg
│
├── tests/              # Integration tests
├── docs/               # Documentation
│   ├── research/       # Architecture research notes
│   ├── audit-phase3.md
│   └── audit-phase4.md
│
├── Cargo.toml          # Workspace root
├── justfile            # Build/test/lint shortcuts
├── README.md
├── CHANGELOG.md
├── AGENT-REFERENCE-LIBRARY.md    # Pattern catalog for AI agents
├── AGENT-GNOME-REFERENCE.md      # GNOME app reference doc
└── IMPLEMENTATION-QUEUE.md       # Status tracking
```

---

## Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Formula engine | IronCalc (not Formualizer) | MIT/Apache 2.0 license, 83 functions, simpler API |
| Grid rendering | Cairo DrawingArea (not GtkColumnView) | Full cell-level control, freeze/merge support |
| Rich text | GtkTextView + TextTags (not WebKit) | Native GTK, no JS dependency, lighter Flatpak |
| File I/O | calamine + rust_xlsxwriter (Tables), pulldown-cmark + rdocx (Letters), zip + quick-xml (Decks) | Most mature Rust libraries per format |
| Document format | Markdown (Letters canonical) | Simple text storage, renders via pulldown-cmark |
| Undo architecture | Generic Command<T> trait in suite-common | Matches LO's SfxUndoAction, reusable across all apps |
| Number formatting | suite-common NumberFormat, num-format crate | Matches LO's SvNumberFormatter, locale-aware |

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

**Reference:** LibreOffice source at `/var/home/james/dev/libreoffice-core/` (sparse checkout: `sc/`, `sd/`, `svl/`, `editeng/`).

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

## Module Size Guidelines

| File | Max Lines | Action if exceeds |
|------|-----------|-------------------|
| `window.rs` | 600 | Split into `canvas.rs`, `toolbar.rs`, `sidebar.rs` |
| `engine.rs` | 500 | Split into `read.rs` + `write.rs` for format I/O |
| `undo.rs` | 250 | Split by command category (object, slide, format) |

**Current status:**
- Decks `window.rs`: 650 lines → acceptable (recently refactored from 946)
- Tables `window.rs`: 1,420 lines → needs refactor (target: split SheetModel + rendering)
- Letters `window.rs`: 1,820 lines → largest, needs refactor

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
