# gtk-office-suite Roadmap

> **Top priority since 2026-09-24: the [Render Parity Roadmap](docs/RENDER-PARITY-ROADMAP.md).**
> A visual feature is done only when a screenshot of the running app matches
> LibreOffice's rendering of the same file within a recorded budget,
> ratcheted in CI by `tools/render-lab`. Phase 1 (one renderer per app) met its
> exit criterion on 2026-09-24: no fixture is red. Phase 2 exited on
> 2026-10-01 (#1198): every single-feature fixture is green in both tiers
> except the ambers accepted as metric artifacts, recorded with their
> measurements. [`tools/render-lab/baseline.json`](tools/render-lab/baseline.json)
> is the live scorecard; the README's per-app table is generated from it,
> and `docs/PARITY.md`'s Render column is checked against it, both in CI. Phase 3 (real documents) is next; render evidence still
> comes before ordinary feature work. Collaboration follows [RFC-0001](docs/rfc/0001-crdt-collaboration.md)
> (accepted, Loro) alongside it. The UI direction is in
> [docs/DESIGN-UI.md](docs/DESIGN-UI.md).

**The plan of record to completion is [#1190](https://github.com/tuna-os/gtk-office-suite/issues/1190)
(since 2026-10-01): v3.0 on Flathub.** Its sub-issues are phased (0: make every
CI signal true; 1: render parity Phase 2 and real-document evidence; 2: interop
and recovery tails; 3: release) and each states its exit as something CI
reports. The per-area evidence stays in the
[readiness ledger](docs/readiness-2026-09/README.md) (#443), which #1190 drives
to done. Long-tail ideas are parked in
[#1216](https://github.com/tuna-os/gtk-office-suite/issues/1216), not in
separate issues.
The dated ledger below is historical and does not certify present behavior.

This file states no status of its own. Whether an item is open or done is
its issue's state, and progress is the
[readiness ledger](docs/readiness-2026-09/README.md)'s. The only figures
here are the ones `tests/test_roadmap_figures.py` checks against the
repository, and that test fails on any other `n/m` figure added to this
file, README.md, `docs/ROADMAP.md` or `docs/TESTING.md`.

**Last updated**: 2026-10-02 | **Maintainer**: tuna-os (hanthor) / architect agent

---

## Mission

A **GNOME-native office suite in Rust** — Letters (word processor), Tables (spreadsheet), Decks (presentations) — built on GTK4 + libadwaita and shipped as Flatpaks. A LibreOffice-inspired suite that feels native to the modern Linux desktop, with measured, ratcheted parity against LibreOffice formats (CommonMark, ODT, ODP, OpenFormula) so users get real document compatibility, not a demo.

gtk-office-suite is the org's flagship **end-user product bet** and a cornerstone of the modern cloud-native desktop mission: office productivity is the last major desktop gap that keeps users on Windows/Mac.

---

## Current Status

- **Post-v1.0**: all three apps (Letters, Tables, Decks) build, run, and ship as Flatpaks.
- **Measured parity** (ratcheted corpora, docs/PARITY.md; each figure is the
  corpus baseline over the corpus size): CommonMark 652/652, LO-Letters 109/109,
  LO-Decks 9/9, OpenFormula 107/107.
- Ctrl+K command palette; per-app live status surfaces; GUI smoke journeys deterministic (#187).
- Readiness work is tracked per-item in [docs/readiness-2026-09/](docs/readiness-2026-09/README.md)
  (#443): atomic save (#437), Letters save formats (#436), GUI display/process
  ownership (#241), deterministic GUI infrastructure (#354, 9/10) and CI
  self-tests (#313, 7/8).
- The June planning documents this file replaced (the v1.0 queue, plan and handoff) are in [docs/archive/](docs/archive/README.md); they were wrong about the project and are kept for history only.
- **GUI-layer God-files** (#168) — still architectural debt before feature
  velocity scales, but measured and bounded. Decomposition has started: the chart
  dialog moved out of Tables in #594.

  | file | lines | ceiling |
  |---|---|---|
  | `tables/src/window.rs` | 2233 | 2300 |
  | `decks/src/window.rs` | 1548 | 1800 |
  | `letters/src/window.rs` | 1134 | 1300 |

  The ceilings are enforced by `scripts/release_gate.py`, and
  `tests/test_roadmap_figures.py` checks this table against the files, so the
  numbers cannot drift the way the previous ones did (they claimed 2.5K for
  Letters, which was out by half).

### Priorities

Each item's state is its tracking issue's; the table no longer repeats it,
because a hand-typed status column is the kind of claim that drifted here
before: nothing re-read it when the issue moved.

| Priority | Item | Tracking |
|----------|------|----------|
| P0 | Render parity: each app's screen matches LibreOffice, fixture by fixture | [docs/RENDER-PARITY-ROADMAP.md](docs/RENDER-PARITY-ROADMAP.md) |
| P0 | Product quality + daily-driver readiness roadmap (meta-tracker) | #95 |
| P0 | CI quality gates: fast / GUI / nightly with published capability matrix | #108, #107 |
| P0 | GUI-layer God-file decomposition (window.rs) | #168 |
| P1 | Letters: structured editing (tables/lists/paragraphs/sections), review workflows, pagination | #109, #110, #111 |
| P1 | Tables: sparse virtual grid + performance budgets | #112 |
| P1 | Headless CLI document conversion binary (`suite-convert`) | #579 |
| P1 | Decks: direct manipulation, themes/layouts, presenter view | #115, #116, #117 |
| P1 | GNOME platform integration: recent files, portals, drag/drop | #119 |
| P2 | Interop: unsupported-feature inspector + versioned fixture corpus with loss budgets | #105, #121 |
| P2 | Release gate: Flatpak, upgrade, recovery, localization, reproducible builds | #122, #578 |
| P2 | A11y: keyboard + screen-reader journeys | #120 |

---

## Quarterly Goals

### Q3 2026 (July–September) — "Daily-driver editing"

**Theme**: make Letters/Tables/Decks genuinely usable for daily work. The
quarter has ended; this is what it set out to do, and each goal's outcome is
its issue's state.

| Goal | Owner | Tracking |
|------|-------|----------|
| Product-quality roadmap live + published capability matrix | architect / quality | #95, #108 |
| Letters structured editing + pagination completeness | architect | #109, #110 |
| Tables virtual grid + performance budgets | architect | #112 |
| GUI God-file decomposition started | architect | #168 |
| ROADMAP.md published and linked from README / org coverage | strategist | tunaos#1359 |

### Q4 2026 (October–December) — "Ship it properly"

**Theme**: production release gating, Flatpak distribution, and headless batch
document processing. The goals below are older than the plan of record; where
they differ, [#1190](https://github.com/tuna-os/gtk-office-suite/issues/1190)
wins (release gate #1209, Flathub #1210, performance and accessibility #1208,
interop #1206).

| Goal | Owner | Tracking |
|------|-------|----------|
| Release gate: Flatpak reproducible builds + GSettings migration verification | quality / ops | #122, #578 |
| Headless CLI conversion binary (`suite-convert`) | architect / strategist | #579 |
| Decks presenter view & export rendering parity | architect | #117 |
| Interop loss budgets & unsupported-feature inspector | quality | #105, #121 |
| A11y: keyboard + screen-reader journeys | quality | #120 |

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
| Retiring the Python office suite: the repos are archived and feature-frozen, and the remaining gates are in [docs/PYTHON-DEPRECATION.md](docs/PYTHON-DEPRECATION.md) | #82 | P1 | L |

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
