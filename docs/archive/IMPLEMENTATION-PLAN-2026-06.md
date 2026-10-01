> **Archived 2026-10-01 — historical, not current.** This file describes
> the project as of June 2026 and is wrong about it today (it lists the
> Tables grid, Decks file I/O and Letters formatting as pending, and calls
> the suite functionally complete; the README calls it pre-alpha). Nothing
> checks it against the code, so it drifted silently. The live plan is
> [#1190](https://github.com/tuna-os/gtk-office-suite/issues/1190) and
> [docs/readiness-2026-09/](../readiness-2026-09/README.md). Kept only so
> its history and links stay reachable.

# Implementation Plan — v1.0 Remaining Features

> 10 remaining issues after v1.0 feature completion. All are v1 polishing — the office suite is functionally complete.
> Generated 2026-06-24 from AGENT-REFERENCE-LIBRARY.md analysis + LibreOffice source audit.

---

## Current State

- **52 issues closed** across all phases
- **10 issues open** — all v1 polish, no blockers
- **3 apps** compile clean (`cargo check --workspace`)
- **106 tests** exist, runnable only on build machine with GTK4 libs

---

## Phase 1: Quick Wins (1.3d)

| # | Feature | Effort | LO Reference | GNOME HIG |
|---|---------|--------|-------------|-----------|
| #66 | Change hints | 0.3d | `svl/hint.hxx` — SfxHintId enum | Used with `AdwToast` for undo/selection feedback |
| #64 | String pool | 0.5d | `svl/sharedstringpool.hxx` | No UI — pure data structure |
| #59 | Unit conversion | 0.5d | `svl/converter.hxx` — mm↔twips↔pixels | Used in ruler, page margins |

**#66 Change hints:** Extend `suite-common/src/events.rs` `Hint` enum with DocumentModified, ViewChanged. Wire into Decks/Tables undo managers.

**#64 String pool:** `StringPool::intern(String) -> u32` with case-insensitive lookup. Used by Tables for repeated cell values.

**#59 Unit conversion:** `mm_to_px()`, `twips_to_px()`, `px_to_mm()` in suite-common. Used by Letters ruler and Tables column widths.

---

## Phase 2: Style & Property System (2.5d)

| # | Feature | Effort | LO Reference | GNOME HIG |
|---|---------|--------|-------------|-----------|
| #65 | Property pool | 1.5d | `svl/poolitem.hxx` + `itemset.hxx` | Unified formatting panel |
| #58 | Style trait | 1d | `svl/style.hxx` + `stylepool.hxx` | AdwPreferencesDialog for styles |

**#65 Property pool:** `PropertyValue` enum (Bool, Int, Float, String, Color, Enum, Point, Rect) + `PropertySet` HashMap. Replaces scattered `font_size: f64`, `bold: bool` fields.

**#58 Style system:** `Style` struct (name, parent, PropertySet) + `StylePool` (HashMap). Default style sets per app. Resolve via parent chain.

---

## Phase 3: Decks Polish (2d)

| # | Feature | Effort | LO Reference | GNOME HIG |
|---|---------|--------|-------------|-----------|
| #3 | Markdown in text boxes | 1d | `sd/` slide text rendering | Reuse Letters markdown engine |
| #54 | Master slides | 2d | `sd/source/ui/slidesorter/` | Edit Master as Stack page |

**#3 Markdown:** Reuse Letters' `pulldown-cmark` → `GtkTextTag` in Decks inline editing.

**#54 Master slides:** `MasterSlide` struct, `Slide.layout_idx` ref. Master view via "View → Edit Master" toggle.

---

## Phase 4: Shared Infrastructure (2.5d)

| # | Feature | Effort | LO Reference | GNOME HIG |
|---|---------|--------|-------------|-----------|
| #60 | Search/Find | 1d | `svl/srchitem.hxx` | GtkRevealer bar (matches GNOME Text Editor) |
| #63 | Print infrastructure | 1.5d | `svl/gridprinter.hxx` | GtkPrintOperation, print preview |

**#60 Search:** `SearchQuery` struct + generic `search()` function. Migrate Letters find to shared. Tables gets find-in-sheet. Decks gets find-across-slides.

**#63 Print:** `PageSetup` + `render_page()` + `print_document()`. Used by all three apps.

---

## Phase 5: Final Polish (2d)

| # | Feature | Effort | LO Reference | GNOME HIG |
|---|---------|--------|-------------|-----------|
| #21 | Test infrastructure | 2d | LO `qa/` framework | 80% coverage on engine code |

**Expanded tests per app:** suite-common (15+), Tables (12+), Decks (8+), Letters (6+).

---

## Dependency Graph

```
Phase 1 (#66, #64, #59) —— independent, can parallelize
    ↓
Phase 2 (#65 → #58) — property pool needed before styles
    ↓
Phases 3-4 can run in parallel once Phase 2 foundations exist
    ↓
Phase 5 (#21) — test everything together
```

---

## Total: ~11d across 5 phases

| Phase | Issues | Days |
|-------|--------|------|
| 1: Quick wins | #66, #64, #59 | 1.3d |
| 2: Style system | #65, #58 | 2.5d |
| 3: Decks polish | #3, #54 | 2d |
| 4: Shared infra | #60, #63 | 2.5d |
| 5: Tests | #21 | 2d |
| **Total** | **10 issues** | **~11d** |
