# v1.0 Implementation Queue

> Generated 2026-06-23. Status based on code audit: **Done** (implemented in code),
> **Partial** (structure exists, needs wiring), **Pending** (not started).

## Phase 0 — Foundational (suite-common) [3 open issues]

| Priority | Issue | Area | Status | Effort |
|----------|-------|------|--------|--------|
| P0 | #6 GSettings Backend | suite-common | **Pending** — no gschema XML files exist yet | 2d |
| P0 | #8 Keyboard Shortcuts Dialog | suite-common | **Partial** — app actions registered, no ShortcutsDialog UI | 1d |
| P0 | #9 Responsive Toolbar Breakpoint | suite-common | **Partial** — SuiteToolbar has set_narrow, no AdwBreakpoint wired | 1d |

## Phase 1 — Letters (core editing) [9 open issues]

> **#110 audit (2026-08-14)** — updates the row below and adds detail this
> queue's 06-23 baseline predates. See the dedicated breakdown further down
> ("#110 — structured editing wiring") for what a follow-up PR needs to do.

| Priority | Issue | Area | Status | Effort |
|----------|-------|------|--------|--------|
| P1 | #14 Rich Text Formatting | Letters | **Partial** — TextTags registered, toolbar buttons exist, signal wiring stubs | 1d |
| P1 | #24 Styles Dropdown | Letters | **Pending** — no DropDown wiring, no style detection on cursor | 1d |
| P1 | #25 Text Alignment | Letters | **Pending** — alignment buttons exist, no tag application | 0.5d |
| P1 | #27 Font Size | Letters | **Pending** — toolbar buttons exist, no size change logic | 0.5d |
| P1 | #36 Bullet/Numbered Lists | Letters | **Partial, wrong layer** (corrected 08-14) — `window.rs`'s `toggle_list()` inserts literal `•`/`1.` text prefixes on keystroke; works for typing but is architecture-rule-violating widget-code logic with no nesting/restart and no connection to `structured.rs`'s real `set_list_item()` model | 1d |
| P1 | #34 Undo/Redo + Spell Check | Letters | **Pending** — GtkTextBuffer has built-in undo, needs wiring | 0.5d |

### #110 — structured editing wiring (audited 2026-08-14)

Code-grounded audit for issue #110 ("Daily-driver structured editing: tables,
lists, paragraphs, and sections"). PR #209 added the model layer and claimed
"Closes #110"; it doesn't meet the acceptance criteria (no toolchain here to
build/test, so nothing below was implemented — this is investigation only,
for whoever picks up the wiring with a working `cargo build`/GTK environment).

**What's real (`letters-core/src/structured.rs`, 135 lines):**
`StructuredEditor` wraps a `Document` with cursor/selection/`table_cell`
state and exposes `insert_table_rows`/`insert_table_cols`/
`delete_table_rows`/`delete_table_cols`/`move_table_cell`,
`set_list_item(paragraph, kind, level, start)` (nesting via `level`, restart
via `start`), and `set_paragraph_layout(paragraph, layout)`. This is a real,
GTK-free, unit-testable model — exactly what `CLAUDE.md`'s architecture rule
("no business logic in widget code... belongs in `*-core`") asks for.

**What's fake or parallel in `letters/src/window.rs` (2580 lines):**
- `app.insert-table` (~line 1082): inserts a **literal Markdown-syntax text
  block** (`| Header 1 | ... |` / `| --- |` rows) into the `GtkTextBuffer` as
  plain text. There is no table widget, no cell grid, no row/column
  insert/delete UI — a user gets a wall of pipe characters, not an editable
  table. `structured.rs`'s `insert_table_rows`/`_cols` are never called.
- `app.bullet-list` / `app.numbered-list` (~lines 1668-1679, `toggle_list()`
  at ~2265-2434): a real, working keystroke-driven bullet/number toggler —
  but it works by inserting/stripping literal `•`/`N.` text prefixes and
  measuring leading whitespace for "indent," entirely inside `window.rs`.
  No nesting model, no restart-numbering, and — same as the table case —
  `set_list_item()` is never called. This is also a standing violation of
  this repo's own `CLAUDE.md` architecture rule (business logic living in
  widget code instead of `letters-core`).
- Paragraph spacing/indentation/tabs/styles/page-breaks/headers-footers/
  multi-column sections: `grep` finds no GUI surface for any of these at
  all — `set_paragraph_layout()` has zero callers.

**Why this isn't a quick "wire the button" fix:** `letters/src/bridge.rs`
(654 lines, the actual GTK-buffer↔document bridge) only exposes whole-buffer
batch operations — `capture_from_buffer(&TextBuffer) -> Document` and
`render_to_buffer(&Document, &TextBuffer)`. There is no incremental mapping
from "cursor is inside table N, row R, col C" (what `StructuredEditor`
needs to act on) to a live `GtkTextBuffer` position. Building that mapping —
plus a real table-grid widget to replace the Markdown-text placeholder — is
the actual size of this issue, not a signal-handler stub.

**Suggested phased plan** (effort estimates in this file's usual style, not
independently verified against this repo's velocity):

1. Add incremental cursor↔`Document`-position tracking to `bridge.rs` (or a
   new module) so `StructuredEditor::table_cell()` can be kept in sync with
   real buffer edits, not just a batch capture. **Foundational — everything
   below depends on it.** (~2d)
2. Replace `insert-table`'s Markdown-text dump with a real table widget (grid
   layout, cell navigation, paste) driven by `insert_table_rows`/`_cols`/
   `delete_table_rows`/`_cols`. (~2d)
3. Replace `toggle_list()`'s text-prefix hack with calls to
   `set_list_item()`, adding real nesting/indent and restart-numbering UI.
   (~1d)
4. Add paragraph layout UI (spacing, indent, tabs, styles, page breaks,
   headers/footers, multi-column sections) calling `set_paragraph_layout()`
   — currently doesn't exist at all. (~2d)
5. `tests/gui/test_smoke.py` journeys for table row/col insert, list
   nesting, and paragraph layout editing (per `CLAUDE.md`'s testing
   expectations — AT-SPI-assertable, no VLM in gating tests). (~1d)
6. Unsupported-construct warnings (the acceptance criteria's last bullet) —
   likely lands alongside step 1's bridge work, since that's where a
   round-trip would first notice a construct it can't represent. (~0.5d)

## Phase 2 — Letters (advanced) [6 open issues]

| Priority | Issue | Area | Status | Effort |
|----------|-------|------|--------|--------|
| P2 | #15 DOCX/MD I/O + PDF Export | Letters | **Partial** — engine.rs stubs, export.rs works. Needs file dialogs wired | 2d |
| P2 | #16 Find & Replace Banner | Letters | **Pending** — no banner widget at all | 1.5d |
| P2 | #26 Insert Image/Link/Table/List | Letters | **Pending** — action stubs exist, no dialogs or insertion logic | 2d |
| P2 | #28 Markdown Macros | Letters | **Pending** — needs insert-text signal handler | 1.5d |
| P2 | #38 Auto-Save Timer | Letters | **Pending** — no timer infrastructure | 0.5d |
| P2 | #39 Page Layout & Print View | Letters | **Pending** — page layout visualization, margins, rulers, line spacing | 2.0d |

## Phase 3 — Tables [7 open issues]

| Priority | Issue | Area | Status | Effort |
|----------|-------|------|--------|--------|
| P3 | #22 Cairo Grid Widget | Tables | **Partial** — DrawingArea with draw_func exists, needs full cell rendering | 3d |
| P3 | #10 Formula Bar + Toolbar | Tables | **Partial** — formula bar exists, toolbar buttons need wiring | 1d |
| P3 | #13 Multi-Sheet Workbook | Tables | **Partial** — SheetModel exists, no tab UI | 1.5d |
| P3 | #12 File I/O (CSV/XLSX) | Tables | **Partial** — calamine + xlsxwriter deps exist, no UI wiring | 2d |
| P3 | #11 Sort/Filter/Freeze/Merge | Tables | **Pending** | 2d |
| P3 | #30 Number Formatting + Borders | Tables | **Pending** | 2d |
| P3 | #29 Charts Integration | Tables | **Partial** — charts.rs engine exists, no dialog/toolbar | 1.5d |

## Phase 4 — Decks [6 open issues]

| Priority | Issue | Area | Status | Effort |
|----------|-------|------|--------|--------|
| P4 | #17 Cairo Slide Canvas | Decks | **Partial** — DrawingArea + slide model exists, needs selection/editing | 2d |
| P4 | #18 Slide Sidebar + Management | Decks | **Partial** — OverlaySplitView + ListBox exists, needs add/delete/reorder | 1.5d |
| P4 | #31 Slide Layouts + Shapes | Decks | **Pending** — SlideObjectData enum exists, no insertion UI | 2d |
| P4 | #19 Present Mode | Decks | **Partial** — fullscreen methods exist, needs keyboard nav | 1.5d |
| P4 | #33 Undo/Redo Canvas | Decks | **Pending** | 1d |
| P4 | #20 File I/O (PPTX/ODP) | Decks | **Pending** — engine.rs stubs | 2d |

## Phase 5 — Polish [5 open issues]

| Priority | Issue | Area | Status | Effort |
|----------|-------|------|--------|--------|
| P5 | #7 Empty States | suite-common | **Done** — make_empty_state works in all apps | 0d |
| P5 | #21 Test Infrastructure | Infra | **Partial** — Dogtail stubs exist, need fixing | 2d |
| P5 | #23 justfile | Infra | **Pending** — no justfile yet | 0.5d |
| P5 | #35 Flatpak Polish | Infra | **Partial** — manifests exist, need icons/metainfo/schema | 1d |

## Effort Summary

| Phase | Issues | Est. Effort |
|-------|--------|-------------|
| P0: Foundational (suite-common) | 3 | 4d |
| P1: Letters core | 6 | 4.5d |
| P2: Letters advanced | 6 | 7.5d |
| P3: Tables | 7 | 13d |
| P4: Decks | 6 | 10d |
| P5: Polish | 5 | 5.5d |
| **Total** | **33** | **~44 days** |

> #110's 2026-08-14 breakdown above (~8.5d across 6 steps) is not folded
> into this table — it overlaps #36's existing row rather than being a
> clean addition, and this summary predates the audit. Treat the table
> above as stale for Phase 1 until someone re-totals it.
