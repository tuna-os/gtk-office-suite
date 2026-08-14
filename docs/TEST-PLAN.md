# Test Verification Plan — Issue #21

> All tests exist but can ONLY run on the build machine which has GTK4 dev libraries.
> This document tracks what we need to verify before claiming v1.0 is truly complete.

---

## Build Machine

Per handoff document:
- SSH accessible
- `toolbox run --container finupdate` — Fedora 43 toolbox with GTK4 dev deps
- `flatpak run org.flatpak.Builder` for Flatpak builds
- Rust toolchain via `rustup` inside toolbox
- Workspace: the gtk-office-suite checkout (and its sibling repos)

**Sync command (from a local checkout to the build machine):**
```bash
rsync -a <local-checkout>/ <host>:/var/home/<user>/dev/hanthor/hanthor-rust/ --exclude target --exclude .git --exclude .flatpak-builder*
```

**Note:** The workspace was renamed to `gtk-office-suite`. Need to update the build machine path or sync to the new name.

---

## Test Inventory

### suite-common (7 tests — compiles and links here, can run now)

| File | Test | What it verifies |
|------|------|-----------------|
| `format.rs` | `test_general` | General format passthrough |
| `format.rs` | `test_number` | Number format with thousands separator |
| `format.rs` | `test_currency` | Currency symbol + format |
| `format.rs` | `test_percent` | Percentage conversion |
| `format.rs` | `test_date` | Excel serial date + ISO string |
| `format.rs` | `test_excel_serial_epoch` | Excel serial conversion correctness |
| `format.rs` | `test_scientific` | Scientific notation |

**Status:** Can run locally now — no GTK deps needed.

### Tables (6 tests — needs GTK4 libs to link)

| File | Test | What it verifies |
|------|------|-----------------|
| `engine.rs` | `test_engine_creation` | TablesEngine initializes |
| `engine.rs` | `test_set_and_get_cell` | Cell value roundtrip |
| `engine.rs` | `test_formula_sum` | SUM formula evaluation |
| `engine.rs` | `test_formula_concat` | CONCAT formula evaluation |
| `engine.rs` | `test_grid_export` | Grid export to Vec<Vec<String>> |
| `charts.rs` | `test_bar` | Bar chart surface creation |
| `charts.rs` | `test_pie` | Pie chart surface creation |

**Status:** Cannot run locally (missing `libadwaita-1`, `libgtk-4`). Must run on the build machine.

### Letters (3 tests — needs GTK4 libs to link)

| File | Test | What it verifies |
|------|------|-----------------|
| `engine.rs` | `test_docx_roundtrip` | DOCX write→read preserves content |
| `engine.rs` | `test_markdown_to_typst` | Markdown→Typst conversion |
| `engine.rs` | *(third test)* | Document model operations |

**Status:** Cannot run locally. Must run on the build machine.

### Decks (1 test — needs GTK4 libs to link)

| File | Test | What it verifies |
|------|------|-----------------|
| `engine.rs` | `test_pptx_roundtrip` | PPTX write→read preserves slides, objects, positions |

**Status:** Cannot run locally. Must run on the build machine.

---

## Verification Commands (run on the build machine)

```bash
# In toolbox:
toolbox run --container finupdate

# Sync code first:
rsync -a <local-checkout>/ <host>:/var/home/<user>/dev/tuna-os/gtk-office-suite/ --exclude target --exclude .git --exclude .flatpak-builder*

# On the build machine, inside toolbox:
cd ~/dev/tuna-os/gtk-office-suite
cargo test --workspace

# Expected:
# suite-common: 7 tests pass
# tables: 7 tests pass
# letters: 3 tests pass
# decks: 1 test pass
# Total: 18 tests pass

# Flatpak build verification:
flatpak run org.flatpak.Builder --state-dir=.flatpak-builder build-dir flatpak/org.tunaos.tables.json
flatpak run org.flatpak.Builder --state-dir=.flatpak-builder build-dir flatpak/org.tunaos.decks.json
flatpak run org.flatpak.Builder --state-dir=.flatpak-builder build-dir flatpak/org.tunaos.letters.json
```

---

## Missing Tests (to add for comprehensive coverage)

### Tables
- [ ] Sort preserves data/formats/borders integrity
- [ ] Sort with empty cells, numeric cells, mixed types
- [ ] Freeze panes: frozen row/col counts stored correctly
- [ ] Merge: merge/unmerge cycle preserves cell content
- [ ] Number format: all 7 format kinds applied correctly
- [ ] Cell borders: all 5 border styles rendered
- [ ] Column resize: width clamped between 30-500
- [ ] Data validation: each rule type validates correctly
- [ ] Undo/redo: cell edit, sort, format, border, merge, freeze commands
- [ ] ODS import: parses all calamine Data variants correctly

### Decks
- [ ] Presenter release journey: current/next slide, notes, timer, display
  selection, and external-display fallback (ADR-0004)
- [ ] Missing-media journey names the slide and path before presentation or
  export (`decks-core/src/presenter.rs`)
- [ ] PDF export verifies 16:9 geometry, backgrounds, masters, and fonts
- [ ] Undo/redo: AddObject → Undo → Redo → object restored
- [ ] Undo/redo: DeleteSlide → Undo → slide restored
- [ ] Undo/redo: ReorderSlides swap and unswap
- [ ] PPTX roundtrip with images
- [ ] Slide transitions: surfaces created, animation completes
- [ ] Object drag: snapped positions match grid

### Letters
- [ ] Markdown macro conversion (**bold**, *italic*, # heading)
- [ ] Find/replace: forward search finds text
- [ ] DOCX roundtrip with formatted text
- [ ] Spell check: known word passes, misspelling flagged

### suite-common
- [ ] UndoManager: undo/redo stack ordering
- [ ] UndoManager: redo stack cleared on new execute
- [ ] Broadcaster: listeners receive hints
- [ ] Excel serial date: Lotus 1-2-3 leap year bug handled
- [ ] Excel serial date: known dates verified (2026-01-01 = 46101)
