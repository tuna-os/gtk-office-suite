# Handoff — gtk-office-suite (formerly hanthor-rust)

**Date:** 2026-06-23
**Repo:** https://github.com/tuna-os/gtk-office-suite
**Milestone:** v1.0 shipped. Tag: `v1.0`. Release: https://github.com/tuna-os/gtk-office-suite/releases/tag/v1.0

---

## Current State

Three Rust GTK4/libadwaita apps: **Letters** (word processor), **Tables** (spreadsheet), **Decks** (presentations). Built as Flatpaks targeting GNOME 50 runtime.

### Recent work (this session)

| Phase | What was done |
|-------|---------------|
| P0 | GSettings schemas, keyboard shortcuts dialog, responsive toolbar infrastructure |
| P1 | Letters: B/I/U/S/H formatting, H1-H6/Code/Blockquote styles, alignment, font size, undo/redo, bullet/numbered lists |
| P2 | Letters: Find/Replace overlay, Markdown/DOCX/HTML/PDF file I/O, insert image/link/table, Markdown macros (`**bold**`/`*italic*`/`# heading`/`> quote`), auto-save timer, drag-and-drop images |
| P3 | Tables: IronCalc formula engine (`ironcalc_base` v0.7.1), multi-sheet workbook, XLSX/ODS/CSV I/O (calamine + rust_xlsxwriter), copy/paste TSV clipboard, column auto-fit |
| P4 | Decks: Cairo slide canvas, shapes (rect, circle), image insertion, fullscreen present mode, slide sidebar, PPTX I/O (zip + OpenXML) |
| P5 | Flatpak icons, metainfo, CHANGELOG, v1.0 tag, GitHub release |

### Test results

```
Letters: 2 tests pass (document roundtrip, markdown→typst)
Tables:  7 tests pass (engine CRUD, SUM/CONCAT formulas, bar/pie charts)
suite-common: 1 test passes (empty state creation)
```

### Build status

All three apps compile in workspace (`cargo check --workspace`). Flatpaks build on Bluefin host via `flatpak run org.flatpak.Builder`. No CI yet for the new repo name.

### Key architectural decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Formula engine | IronCalc (not Formualizer) | MIT/Apache 2.0 license, 83 functions, simpler API |
| Grid rendering | Cairo DrawingArea (not GtkColumnView) | Full cell-level control, freeze/merge support |
| Rich text | GtkTextView + TextTags (not WebKit) | Native GTK, no JS dependency, lighter Flatpak |
| File I/O | calamine + rust_xlsxwriter (Tables), pulldown-cmark + pandoc CLI (Letters), zip + OpenXML (Decks) | Picked most mature Rust libraries per format |
| Document format | Markdown (Letters canonical) | Simple text storage, renders via pulldown-cmark |

---

## Remaining Work (Post-v1.0)

### Letters
- Page layout visualization (margins as gray side panels) — partially stubbed
- Spell-check `set_enable_spell_checking()` — toggle exists, needs GTK4 verification
- Better heading style rendering (right-click on heading text may be broken)
- Paragraph spacing / line spacing

### Tables
- Column resize via drag (divider cursor exists, resize not wired)
- Sort/filter on column headers
- Merge cells
- Freeze panes
- Number formatting (currency, date, percent)
- Cell borders UI
- Data validation
- Charts dialog integration (charts.rs engine exists, no UI)

### Decks
- Slide transitions (fade, slide, zoom)
- Speaker notes
- Undo/redo on canvas operations
- Grid/guides for alignment
- Master slides / templates

### Infrastructure
- Flatpak publishing CI pipeline (OCI image push to GHCR)
- GUI tests (Dogtail stubs exist but target wrong app name)
- justfile for dev workflows
- GtkSourceView5 dependency consideration for syntax highlighting

---

## Reference Implementations (cloned locally)

| Project | Path | Useful for |
|---------|------|------------|
| **Loupe** (GNOME Image Viewer) | `/tmp/loupe/` | Cairo DrawingArea, gesture handling, fullscreen, subclass pattern |
| **Papers** (GNOME Document Viewer) | `/tmp/papers/` | Find sidebar, search box with case-sensitive/whole-word toggles |
| **GNOME Tour** | `/tmp/gnome-tour/` | AdwCarousel, adaptive layout |
| **Marko Editor** | `/tmp/marko-editor/` | Markdown↔GtkTextTag conversion, pulldown-cmark event-stream pattern |
| **Velotype** | `/tmp/velotype/` | Block-based markdown editor (gpui, not GTK4 — architecture reference) |
| **gnome-gui-spec** | `/tmp/gnome-gui-spec/` | GNOME HIG patterns, Blueprint/XML examples, 34 app audits |
| **gtk4-rs examples** | `/tmp/gtk4-rs/examples/` | `column_view_datagrid/` — GtkColumnView data grid pattern |

### Files to read for context
- `/home/james/dev/hanthor/hanthor-rust/IMPLEMENTATION-QUEUE.md` — Prioritized issue list with effort estimates
- `/home/james/dev/hanthor/hanthor-rust/AGENT-REFERENCE-LIBRARY.md` — Pattern catalog with code examples
- `/home/james/dev/hanthor/hanthor-rust/AGENT-GNOME-REFERENCE.md` — GNOME Rust reference app documentation

### GitHub Issues (36 open in v1.0 milestone)

Key unaddressed issues:
- **#9** Responsive breakpoints (SuiteToolbar has `set_narrow()`, not wired to window resize)
- **#11** Sort/filter/freeze/merge (data model stubbed, no UI)
- **#21** Test infrastructure (Dogtail stubs target wrong app name)
- **#23** justfile for build/test/lint
- **#29** Charts dialog (charts.rs engine exists, no toolbar/dialog)
- **#30** Number formatting + cell borders
- **#33** Decks undo/redo for canvas
- **#35** Flatpak publishing CI

### Build machine

**himachal** — remote build/test machine. SSH accessible. Has:
- `toolbox run --container finupdate` — Fedora 43 toolbox with GTK4 dev deps
- `flatpak run org.flatpak.Builder` for Flatpak builds
- Rust toolchain via `rustup` inside toolbox
- Workspace at `/var/home/james/dev/hanthor/hanthor-rust/`
- To sync: `rsync -a /home/james/dev/hanthor/hanthor-rust/ himachal:/var/home/james/dev/hanthor/hanthor-rust/ --exclude target --exclude .git --exclude .flatpak-builder*`

---

## Suggested Skills

| Skill | When to use |
|-------|-------------|
| `searxng-search` | Researching implementation patterns, finding crates, debugging obscure GTK4/Rust issues |
| `design-an-interface` | Designing dialogs (chart dialog, insert table, preferences) |
| `codebase-design` | Deep module design for Tables engine, Decks rendering pipeline |
| `diagnosing-bugs` | Runtime crashes, GTK init order issues (like the StyleManager bug fixed this session) |
| `tdd` | Writing tests for new engine features (formula evaluation, file I/O round-trips) |
| `obsidian-vault` | Documenting architectural decisions in the hanthor Obsidian vault |
| `pi-subagents` | Parallel work on independent features (e.g., spreadsheet grid + file I/O in parallel) |

---

## Redacted

- `search.manatee-basking.ts.net` — internal SearXNG instance (Tailscale)
- `github.com/tuna-os` — org where repo lives
- `/var/home/james/` — home dir on himachal (build machine)
- himachal hostname and IP redacted
