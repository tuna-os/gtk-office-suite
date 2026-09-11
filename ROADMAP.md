# gtk-office-suite Roadmap

The current execution plan is [Roadmap to dependable daily use](docs/readiness-2026-09/README.md),
tracked in [#443](https://github.com/tuna-os/gtk-office-suite/issues/443).
It prioritizes crash reproduction, save/recovery safety and verified user journeys.
The dated ledger below is historical and does not certify present behavior.

**Last updated**: 2026-08-11 | **Maintainer**: tuna-os (hanthor) / architect agent

---

## Mission

A **GNOME-native office suite in Rust** — Letters (word processor), Tables (spreadsheet), Decks (presentations) — built on GTK4 + libadwaita and shipped as Flatpaks. A LibreOffice-inspired suite that feels native to the modern Linux desktop, with measured, ratcheted parity against LibreOffice formats (CommonMark, ODT, ODP, OpenFormula) so users get real document compatibility, not a demo.

gtk-office-suite is the org's flagship **end-user product bet** and a cornerstone of the modern cloud-native desktop mission: office productivity is the last major desktop gap that keeps users on Windows/Mac.

---

# gtk-office-suite Roadmap

**Last updated**: 2026-09-11 | **Maintainer**: tuna-os (hanthor) / architect agent

---

## Mission

A **GNOME-native office suite in Rust** — Letters (word processor), Tables (spreadsheet), Decks (presentations) — built on GTK4 + libadwaita and shipped as Flatpaks. A LibreOffice-inspired suite that feels native to the modern Linux desktop, with measured, ratcheted parity against LibreOffice formats (CommonMark, ODT, ODP, OpenFormula) so users get real document compatibility, not a demo.

gtk-office-suite is the org's flagship **end-user product bet** and a cornerstone of the modern cloud-native desktop mission: office productivity is the last major desktop gap that keeps users on Windows/Mac.

---

## Current Status (September 2026)

- **Post-v1.0**: all three apps (Letters, Tables, Decks) build, run, and ship as Flatpaks.
- **Measured parity** (ratcheted corpora, docs/PARITY.md): CommonMark 630/652, LO-Letters 109/109, LO-Decks 9/9, OpenFormula 107/107.
- Ctrl+K command palette; per-app live status surfaces; GUI smoke journeys deterministic (#187).
- GTK-free domain controllers extracted into `suite-common-core` (GTK-free architecture rule enforced).
- ✅ **Q3 2026 Daily-Driver Editing**: core document models, structured editing, sparse grid rendering, and ODF interop baselines established.
- 🟡 **Q4 2026 Transition**: decompose `window.rs` God-files, establish enterprise deployment schemas (`dconf`), headless document conversion CLI (`suite-convert`), and privacy-first local LLM integration.

### Priorities

| Priority | Item | Tracking | Status |
|----------|------|----------|--------|
| P0 | GUI-layer God-file decomposition (`window.rs` -> domain controllers) | #168 | 🟡 In progress |
| P0 | Enterprise fleet deployment & dconf policy lockdown framework | #534, #563 | 🟡 In progress |
| P0 | Headless document conversion CLI binary (`suite-convert`) | #541, #546 | 🟡 In progress |
| P1 | Privacy-first AI assistant protocol & local LLM context extraction | #548 | 🟡 In progress |
| P1 | Peer-to-peer offline-first CRDT document sync architecture | #545 | 🟡 In progress |
| P1 | Decks: direct manipulation, themes/layouts, presenter view | #115, #116, #117 | ⬜ Not started |
| P1 | GNOME platform integration: recent files, portals, drag/drop | #119 | ⬜ Not started |
| P2 | Interop: unsupported-feature inspector + versioned fixture corpus with loss budgets | #105, #121 | ⬜ Not started |
| P2 | Release gate: Flatpak, upgrade, recovery, localization, reproducible builds | #122 | 🟡 In progress |
| P2 | A11y: keyboard + screen-reader journeys | #120 | ⬜ Not started |

---

## Quarterly Goals

### Q3 2026 (July–September) — "Daily-driver editing"

**Theme**: make Letters/Tables/Decks genuinely usable for daily work.

| Goal | Owner | Tracking | Status |
|------|-------|----------|--------|
| Product-quality roadmap live + published capability matrix | architect / quality | #95, #108 | ✅ Done |
| Letters structured editing + pagination completeness | architect | #109, #110 | ✅ Done |
| Tables virtual grid + performance budgets | architect | #112 | ✅ Done |
| GUI God-file decomposition started | architect | #168 | 🟡 In progress |
| ROADMAP.md published and linked from README / org coverage | strategist | tunaos#1359 | ✅ Done |

### Q4 2026 (October–December) — "Ship it properly & Enterprise Readiness"

**Theme**: production deployment, enterprise fleet management, CLI integration, and offline-first collaboration.

| Goal | Owner | Tracking | Status |
|------|-------|----------|--------|
| Decompose `window.rs` God-files into GTK-free domain controllers | architect | #168 | 🟡 In progress |
| Headless CLI conversion binary (`suite-convert`) & WASM/IPC plugin SDK | strategist / architect | #541, #546 | 🟡 In progress |
| Enterprise fleet dconf policy management & admin lockdown | strategist | #534 | 🟡 In progress |
| Privacy-first desktop AI assistant & local LLM context engine | strategist | #548 | 🟡 In progress |
| Offline-first CRDT peer session synchronization protocol | strategist | #545 | 🟡 In progress |
| Release gate (#122): Flatpak distribution, crash recovery, reproducible builds | quality / ops | #122 | 🟡 In progress |

---

## Technical Debt Backlog

| Item | Issue | Priority | Effort |
|------|-------|----------|--------|
| GUI-layer God-files (window.rs 2.6K/2.5K/1.6K LOC) | #168 | P0 | L |
| Dual maintenance burden: Python office suite (letters/tables/decks) + Rust suite | #82 | P1 | L |
| ~~spell.rs `generate_candidates("")` panic (0..n-1, n=0)~~ — ✅ fixed, `saturating_sub(1)` in the transposition loop | #172 | P1 | S |

---

## How to Contribute

See [docs/CONTRIBUTING.md](./docs/CONTRIBUTING.md) and [docs/DEVELOPMENT.md](./docs/DEVELOPMENT.md) for build setup (Rust + GTK4/libadwaita, Nix flake included). Pick an issue labeled `good first issue` or comment on a goal you would like to own.

---
*Maintained by the strategist agent (tuna-os hive). Last self-review: 2026-09-11 — aligned Q4 2026 release goals, enterprise CLI requirements, and strategic priorities.*
