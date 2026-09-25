# gtk-office-suite Roadmap

> **Top priority since 2026-09-24: the [Render Parity Roadmap](docs/RENDER-PARITY-ROADMAP.md).**
> A visual feature is done only when a screenshot of the running app matches
> LibreOffice's rendering of the same file within a recorded budget,
> ratcheted in CI by `tools/render-lab`. Phase 1 (one renderer per app) met its
> exit criterion on 2026-09-24: no fixture is red. Phase 2 (every
> single-feature fixture green) is in progress, and
> [`tools/render-lab/baseline.json`](tools/render-lab/baseline.json) is the live
> scorecard. Until Phase 2 exits, render parity comes before ordinary feature
> work. Collaboration follows [RFC-0001](docs/rfc/0001-crdt-collaboration.md)
> (accepted, Loro) alongside it. The UI direction is in
> [docs/DESIGN-UI.md](docs/DESIGN-UI.md).

The current execution plan is [Roadmap to dependable daily use](docs/readiness-2026-09/README.md),
tracked in [#443](https://github.com/tuna-os/gtk-office-suite/issues/443).
It prioritizes crash reproduction, save/recovery safety and verified user journeys.
The dated ledger below is historical and does not certify present behavior.

**Last updated**: 2026-09-25 | **Maintainer**: tuna-os (hanthor) / architect agent

---

## Mission

A **GNOME-native office suite in Rust** — Letters (word processor), Tables (spreadsheet), Decks (presentations) — built on GTK4 + libadwaita and shipped as Flatpaks. A LibreOffice-inspired suite that feels native to the modern Linux desktop, with measured, ratcheted parity against LibreOffice formats (CommonMark, ODT, ODP, OpenFormula) so users get real document compatibility, not a demo.

gtk-office-suite is the org's flagship **end-user product bet** and a cornerstone of the modern cloud-native desktop mission: office productivity is the last major desktop gap that keeps users on Windows/Mac.

---

## Current Status (September 2026)

- **Post-v1.0**: all three apps (Letters, Tables, Decks) build, run, and ship as Flatpaks.
- **Measured parity** (ratcheted corpora, docs/PARITY.md): CommonMark 630/652, LO-Letters 109/109, LO-Decks 9/9, OpenFormula 107/107.
- Ctrl+K command palette; per-app live status surfaces; GUI smoke journeys deterministic (#187).
- Readiness work is tracked per-item in [docs/readiness-2026-09/](docs/readiness-2026-09/README.md)
  (#443): atomic save (#437), Letters save formats (#436), GUI display/process
  ownership (#241), deterministic GUI infrastructure (#354, 9/10) and CI
  self-tests (#313, 7/8).
- ✅ **ROADMAP.md published** (this file, tunaos#1359) — internal planning (IMPLEMENTATION-QUEUE.md, docs/IMPLEMENTATION-PLAN.md, docs/PARITY.md) now has a public, dated, prioritized surface, linked from README.
- ⚠️ **GUI-layer God-files** (#168) — still architectural debt before feature
  velocity scales, but measured and bounded. Decomposition has started: the chart
  dialog moved out of Tables in #594.

  | file | lines | ceiling |
  |---|---|---|
  | `tables/src/window.rs` | 2213 | 2300 |
  | `decks/src/window.rs` | 1678 | 1800 |
  | `letters/src/window.rs` | 1289 | 1800 |

  The ceilings are enforced by `scripts/release_gate.py`, and
  `tests/test_roadmap_figures.py` checks this table against the files, so the
  numbers cannot drift the way the previous ones did (they claimed 2.5K for
  Letters, which was out by half).

### Priorities

| Priority | Item | Tracking | Status |
|----------|------|----------|--------|
| P0 | Render parity: each app's screen matches LibreOffice, fixture by fixture | [docs/RENDER-PARITY-ROADMAP.md](docs/RENDER-PARITY-ROADMAP.md) | 🟡 Phase 2 in progress |
| P0 | Product quality + daily-driver readiness roadmap (meta-tracker) | #95 | 🟡 In progress |
| P0 | CI quality gates: fast / GUI / nightly with published capability matrix | #108, #107 | 🟡 In progress |
| P0 | GUI-layer God-file decomposition (window.rs) | #168 | 🟡 In progress |
| P1 | Letters: structured editing (tables/lists/paragraphs/sections), review workflows, pagination | #109, #110, #111 | 🟡 In progress |
| P1 | Tables: sparse virtual grid + performance budgets | #112 | 🟡 In progress |
| P1 | Headless CLI document conversion binary (`suite-convert`) | #579 | ⬜ Planned |
| P1 | Decks: direct manipulation, themes/layouts, presenter view | #115, #116, #117 | ⬜ Not started |
| P1 | GNOME platform integration: recent files, portals, drag/drop | #119 | ⬜ Not started |
| P2 | Interop: unsupported-feature inspector + versioned fixture corpus with loss budgets | #105, #121 | ⬜ Not started |
| P2 | Release gate: Flatpak, upgrade, recovery, localization, reproducible builds | #122, #578 | 🟡 In progress |
| P2 | A11y: keyboard + screen-reader journeys | #120 | ⬜ Not started |

---

## Quarterly Goals

### Q3 2026 (July–September) — "Daily-driver editing"

**Theme**: make Letters/Tables/Decks genuinely usable for daily work.

| Goal | Owner | Tracking | Status |
|------|-------|----------|--------|
| Product-quality roadmap live + published capability matrix | architect / quality | #95, #108 | 🟡 In progress |
| Letters structured editing + pagination completeness | architect | #109, #110 | 🟡 In progress |
| Tables virtual grid + performance budgets | architect | #112 | 🟡 In progress |
| GUI God-file decomposition started | architect | #168 | 🟡 In progress |
| ROADMAP.md published and linked from README / org coverage (#1295) | strategist | tunaos#1359 | ✅ Done |

### Q4 2026 (October–December) — "Ship it properly"

**Theme**: production release gating, Flatpak distribution, and headless batch
document processing. Sketch until Q4 starts; the live execution plan remains
[docs/readiness-2026-09/](docs/readiness-2026-09/README.md).

| Goal | Owner | Tracking | Status |
|------|-------|----------|--------|
| Release gate: Flatpak reproducible builds + GSettings migration verification | quality / ops | #122, #578 | 🟡 Planned |
| Headless CLI conversion binary (`suite-convert`) | architect / strategist | #579 | ⬜ Planned |
| Decks presenter view & export rendering parity | architect | #117 | ⬜ Planned |
| Interop loss budgets & unsupported-feature inspector | quality | #105, #121 | ⬜ Planned |
| A11y: keyboard + screen-reader journeys | quality | #120 | ⬜ Planned |

### Proposed, not scheduled

Planning documents that exist but have **not** been accepted and are not
committed to any quarter. They are listed here only so they are discoverable
instead of being rediscovered and rewritten.

| Proposal | Document |
|---|---|
| Extension architecture (sandboxed WASM + out-of-process IPC) | [docs/rfc/0002-extension-architecture.md](docs/rfc/0002-extension-architecture.md) |
| Headless conversion CLI (`suite-convert`) | [docs/HEADLESS-CONVERSION-SPEC.md](docs/HEADLESS-CONVERSION-SPEC.md) |
| Headless layout and rendering | [docs/adr/0009-headless-rendering.md](docs/adr/0009-headless-rendering.md) |
| Enterprise fleet deployment and dconf policy | [docs/ENTERPRISE-DEPLOYMENT.md](docs/ENTERPRISE-DEPLOYMENT.md) |
| Document encryption and digital signatures | [docs/DOCUMENT-SECURITY.md](docs/DOCUMENT-SECURITY.md) |
| Telemetry and crash reporting | [docs/TELEMETRY-STRATEGY.md](docs/TELEMETRY-STRATEGY.md) |
| Template catalog and distribution | [docs/TEMPLATE-CATALOG.md](docs/TEMPLATE-CATALOG.md) |

Each carries its open questions. None precedes the
[Render Parity Roadmap](docs/RENDER-PARITY-ROADMAP.md) or the
[September readiness plan](docs/readiness-2026-09/README.md) (#443).

---

## Technical Debt Backlog

| Item | Issue | Priority | Effort |
|------|-------|----------|--------|
| GUI-layer God-files (window.rs — line counts under Current Status) | #168 | P0 | L |
| ~~Dual maintenance burden: Python office suite (letters/tables/decks) + Rust suite~~ — ✅ the Python repos are archived and feature-frozen; the remaining retirement gates are tracked in [docs/PYTHON-DEPRECATION.md](docs/PYTHON-DEPRECATION.md) | #82 | P1 | L |
| ~~spell.rs `generate_candidates("")` panic (0..n-1, n=0)~~ — ✅ fixed, `saturating_sub(1)` in the transposition loop | #172 | P1 | S |

---

## How to Contribute

See [docs/CONTRIBUTING.md](./docs/CONTRIBUTING.md) and [docs/DEVELOPMENT.md](./docs/DEVELOPMENT.md) for build setup (Rust + GTK4/libadwaita, Nix flake included). Pick an issue labeled `good first issue` or comment on a goal you would like to own.

---
*Maintained by the strategist agent (tuna-os hive). Last self-review: 2026-09-12 —
consolidated the competing Q4 2026 roadmap pull requests into one edit and replaced
the stale window.rs line counts with measured ones. See the note below.*

*Why this edit is one pull request: eighteen `[strategist] planning` pull requests
were open against this file and `docs/ROADMAP.md` at once, five of them variants of
"Q4 2026 roadmap". They conflicted with each other by construction — merging any one
made the rest unmergeable — and several restated figures nothing had re-measured.
#583 was the most accurate and is the basis for this one.*
