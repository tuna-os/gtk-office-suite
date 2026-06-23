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

| Priority | Issue | Area | Status | Effort |
|----------|-------|------|--------|--------|
| P1 | #14 Rich Text Formatting | Letters | **Partial** — TextTags registered, toolbar buttons exist, signal wiring stubs | 1d |
| P1 | #24 Styles Dropdown | Letters | **Pending** — no DropDown wiring, no style detection on cursor | 1d |
| P1 | #25 Text Alignment | Letters | **Pending** — alignment buttons exist, no tag application | 0.5d |
| P1 | #27 Font Size | Letters | **Pending** — toolbar buttons exist, no size change logic | 0.5d |
| P1 | #36 Bullet/Numbered Lists | Letters | **Pending** — no list support at all | 1d |
| P1 | #34 Undo/Redo + Spell Check | Letters | **Pending** — GtkTextBuffer has built-in undo, needs wiring | 0.5d |

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
