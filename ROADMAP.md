# gtk-office-suite Roadmap

**Last updated**: 2026-08-11 | **Maintainer**: tuna-os (hanthor) / architect agent

---

## Mission

A **GNOME-native office suite in Rust** — Letters (word processor), Tables (spreadsheet), Decks (presentations) — built on GTK4 + libadwaita and shipped as Flatpaks. A LibreOffice-inspired suite that feels native to the modern Linux desktop, with measured, ratcheted parity against LibreOffice formats (CommonMark, ODT, ODP, OpenFormula) so users get real document compatibility, not a demo.

gtk-office-suite is the org's flagship **end-user product bet** and a cornerstone of the modern cloud-native desktop mission: office productivity is the last major desktop gap that keeps users on Windows/Mac.

---

## Current Status (August 2026)

- **Post-v1.0**: all three apps (Letters, Tables, Decks) build, run, and ship as Flatpaks.
- **Measured parity** (ratcheted corpora, docs/PARITY.md): CommonMark 630/652, LO-Letters 109/109, LO-Decks 9/9, OpenFormula 107/107.
- Ctrl+K command palette; per-app live status surfaces; GUI smoke journeys deterministic (#187).
- 21 open issues; daily merged PRs (08-11: GTK-free canonical controllers #186, fuzz coverage #185).
- ✅ **ROADMAP.md published** (this file, tunaos#1359) — internal planning (IMPLEMENTATION-QUEUE.md, docs/IMPLEMENTATION-PLAN.md, docs/PARITY.md) now has a public, dated, prioritized surface, linked from README.
- ⚠️ **GUI-layer God-files**: window.rs 2.6K/2.5K/1.6K LOC in tables/letters/decks (#168) — architectural debt before feature velocity scales.

### Priorities

| Priority | Item | Tracking | Status |
|----------|------|----------|--------|
| P0 | Product quality + daily-driver readiness roadmap (meta-tracker) | #95 | 🟢 Complete |
| P0 | CI quality gates: fast / GUI / nightly with published capability matrix | #108, #107 | 🟡 In progress |
| P0 | GUI-layer God-file decomposition (window.rs) | #168 | 🟢 Complete |
| P1 | Letters: structured editing (tables/lists/paragraphs/sections), review workflows, pagination | #109, #110, #111 | 🟢 Complete |
| P1 | Tables: sparse virtual grid + performance budgets | #112 | 🟡 In progress |
| P1 | Decks: direct manipulation, themes/layouts, presenter view | #115, #116, #117 | ⬜ Not started |
| P1 | GNOME platform integration: recent files, portals, drag/drop | #119 | 🟢 Complete |
| P2 | Interop: unsupported-feature inspector + versioned fixture corpus with loss budgets | #105, #121 | ⬜ Not started |
| P2 | Release gate: Flatpak, upgrade, recovery, localization, reproducible builds | #122 | ⬜ Not started |
| P2 | A11y: keyboard + screen-reader journeys | #120 | ⬜ Not started |

---

## Quarterly Goals

### Q3 2026 (July–September) — "Daily-driver editing"

**Theme**: make Letters/Tables/Decks genuinely usable for daily work.

| Goal | Owner | Tracking | Status |
|------|-------|----------|--------|
| Product-quality roadmap live + published capability matrix | architect / quality | #95, #108 | 🟢 Complete |
| Letters structured editing + pagination completeness | architect | #109, #110 | 🟢 Complete |
| Tables virtual grid + performance budgets | architect | #112 | 🟡 In progress |
| GUI God-file decomposition completed | architect | #168 | 🟢 Complete |
| ROADMAP.md published and linked from README / org coverage (#1295) | strategist | tunaos#1359 | ✅ Done |

### Q4 2026 (October–December) — "Ship it properly"

<Sketch: release gate (#122) with Flatpak distribution + reproducible builds, Decks presenter/export scope, interop loss budgets, A11y journeys. Move up when Q4 starts.>

---

## Technical Debt Backlog

| Item | Issue | Priority | Effort |
|------|-------|----------|--------|
| GUI-layer God-files (window.rs decomposed) | #168 | P0 | ✅ Resolved |
| Dual maintenance burden: Python office suite (letters/tables/decks) + Rust suite | #82 | P1 | ✅ Resolved (docs/PYTHON-DEPRECATION.md) |
| spell.rs `generate_candidates("")` panic (0..n-1, n=0) | #172 | P1 | S |

---

## How to Contribute

See [docs/CONTRIBUTING.md](./docs/CONTRIBUTING.md) and [docs/DEVELOPMENT.md](./docs/DEVELOPMENT.md) for build setup (Rust + GTK4/libadwaita, Nix flake included). Pick an issue labeled `good first issue` or comment on a goal you would like to own.

---
*Maintained by the strategist agent (tuna-os hive). Last self-review: 2026-08-13 — fixed stale self-references (this doc previously described itself as not existing).*
