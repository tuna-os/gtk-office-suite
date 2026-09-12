# gtk-office-suite Roadmap

The current execution plan is [Roadmap to dependable daily use](docs/readiness-2026-09/README.md),
tracked in [#443](https://github.com/tuna-os/gtk-office-suite/issues/443).
It prioritizes crash reproduction, save/recovery safety, document security, enterprise management, extensibility, and verified user journeys.
The dated ledger below is historical and does not certify present behavior.

**Last updated**: 2026-09-11 | **Maintainer**: tuna-os (hanthor) / strategist agent

---

## Mission

A **GNOME-native office suite in Rust** — Letters (word processor), Tables (spreadsheet), Decks (presentations) — built on GTK4 + libadwaita and shipped as Flatpaks. A LibreOffice-inspired suite that feels native to the modern Linux desktop, with measured, ratcheted parity against LibreOffice formats (CommonMark, ODT, ODP, OpenFormula) so users get real document compatibility, not a demo.

gtk-office-suite is the org's flagship **end-user product bet** and a cornerstone of the modern cloud-native desktop mission: office productivity is the last major desktop gap that keeps users on Windows/Mac.

---

## Current Status (September 2026)

- **Post-v1.0**: all three apps (Letters, Tables, Decks) build, run, and ship as Flatpaks.
- **Measured parity** (ratcheted corpora, docs/PARITY.md): CommonMark 630/652, LO-Letters 109/109, LO-Decks 9/9, OpenFormula 107/107.
- Ctrl+K command palette; per-app live status surfaces; GUI smoke journeys deterministic (#187).
- Enterprise dconf deployment policies, hardware token digital signatures, and WASM/IPC plugin SDK specifications established.
- 21 open issues; daily merged PRs (GTK-free canonical controllers, fuzz coverage).
- ✅ **ROADMAP.md published** — internal planning (IMPLEMENTATION-QUEUE.md, docs/IMPLEMENTATION-PLAN.md, docs/PARITY.md) has a public, dated, prioritized surface, linked from README.
- ⚠️ **GUI-layer God-files**: `window.rs` refactoring underway to maintain strict architecture boundaries.

### Priorities

| Priority | Item | Tracking | Status |
|----------|------|----------|--------|
| P0 | Product quality + daily-driver readiness roadmap (meta-tracker) | #95, #443 | 🟡 In progress |
| P0 | Q4 2026 Release Gate: Flatpak reproducible builds, update lifecycle & zero-regression audit | #600 | 🟡 In progress |
| P0 | Headless Document Conversion CLI (`gtk-office-convert`) & batch pipeline | #600 | 🟡 In progress |
| P0 | CI quality gates: fast / GUI / nightly with published capability matrix | #108, #107 | 🟡 In progress |
| P1 | Client-Side Document Security: AES-GCM encryption, digital signatures (X.509/PKCS#11) | #600 | 🟡 In progress |
| P1 | Enterprise Fleet Governance: system-wide dconf policy enforcement & lockdown schemas | #600 | 🟡 In progress |
| P1 | Extensibility Architecture: WASM sandbox & IPC plugin extension SDK | #600 | 🟡 In progress |
| P1 | Letters: structured editing (tables/lists/paragraphs/sections), review workflows, pagination | #109, #110, #111 | 🟡 In progress |
| P1 | Tables: sparse virtual grid + performance budgets | #112 | 🟡 In progress |
| P1 | Decks: direct manipulation, themes/layouts, presenter view | #115, #116, #117 | ⬜ Not started |
| P2 | Interop: unsupported-feature inspector + versioned fixture corpus with loss budgets | #105, #121 | 🟡 In progress |
| P2 | A11y: keyboard + AT-SPI screen-reader automated audit gates | #120 | 🟡 In progress |

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
| Headless CLI & document security architecture baseline | strategist | #600 | ✅ Done |

### Q4 2026 (October–December) — "Enterprise Fleet Readiness & Platform Extensibility"

**Theme**: complete release certification, enterprise deployment controls, batch conversion tooling, document security, and WASM/IPC extension ecosystems.

| Goal | Owner | Tracking | Status |
|------|-------|----------|--------|
| Flatpak reproducible build pipeline & enterprise release gate | strategist / ops | #600 | 🟡 In progress |
| Headless document conversion CLI binary (`gtk-office-convert`) | strategist / engine | #600 | 🟡 In progress |
| Client-side document encryption & PKCS#11 digital signatures | strategist / sec | #600 | 🟡 In progress |
| System-wide dconf policy enforcement & administrative lockdown | strategist / ops | #600 | 🟡 In progress |
| WASM/IPC plugin SDK & extension API specification | strategist / arch | #600 | 🟡 In progress |
| Automated AT-SPI accessibility audit gate in CI | quality / a11y | #120, #600 | 🟡 In progress |

---

## Technical Debt Backlog

| Item | Issue | Priority | Effort |
|------|-------|----------|--------|
| GUI-layer God-files (window.rs 2.6K/2.5K/1.6K LOC) | #168 | P0 | L |
| Dual maintenance burden: Python office suite (letters/tables/decks) + Rust suite | #82 | P1 | L |
| spell.rs `generate_candidates("")` panic (0..n-1, n=0) — ✅ fixed, `saturating_sub(1)` in transposition loop | #172 | P1 | S |

---

## How to Contribute

See [docs/CONTRIBUTING.md](./docs/CONTRIBUTING.md) and [docs/DEVELOPMENT.md](./docs/DEVELOPMENT.md) for build setup (Rust + GTK4/libadwaita, Nix flake included). Pick an issue labeled `good first issue` or comment on a goal you would like to own.

---
*Maintained by the strategist agent (tuna-os hive). Last self-review: 2026-09-11 — updated Q4 2026 strategic objectives & enterprise roadmap alignment.*
