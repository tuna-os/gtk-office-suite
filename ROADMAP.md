# gtk-office-suite Roadmap

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
- Daily merged PRs and continuous GTK-free core refactoring.
- ✅ **ROADMAP.md published** — internal planning now has a public, dated, prioritized surface, linked from README.
- ⚠️ **Enterprise Security & Distribution Gap**: document-level encryption, XML digital signatures, and standardized fleet management policies (dconf lockdown) are key priorities for enterprise readiness.

### Priorities

| Priority | Item | Tracking | Status |
|----------|------|----------|--------|
| P0 | Product quality + daily-driver readiness roadmap (meta-tracker) | #95 | 🟡 In progress |
| P0 | CI quality gates: fast / GUI / nightly with published capability matrix | #108, #107 | 🟡 In progress |
| P0 | Document Security: client-side encryption (PBKDF2/AES-GCM) & XML signatures | — | 📋 Planned |
| P1 | Letters: structured editing (tables/lists/paragraphs/sections), review workflows, pagination | #109, #110, #111 | 🟡 In progress |
| P1 | Tables: sparse virtual grid + performance budgets | #112 | 🟡 In progress |
| P1 | Decks: direct manipulation, themes/layouts, presenter view | #115, #116, #117 | ⬜ Not started |
| P1 | Headless CLI Conversion Binaries (`suite-convert`) for server/automation workflows | — | 📋 Planned |
| P2 | GNOME platform integration: recent files, portals, drag/drop | #119 | ⬜ Not started |
| P2 | Interop: unsupported-feature inspector + versioned fixture corpus with loss budgets | #105, #121 | ⬜ Not started |
| P2 | Release gate: Flatpak, upgrade, recovery, localization, reproducible builds | #122 | ⬜ Not started |

---

## Quarterly Goals

### Q3 2026 (July–September) — "Daily-driver editing"

**Theme**: make Letters/Tables/Decks genuinely usable for daily work.

| Goal | Owner | Tracking | Status |
|------|-------|----------|--------|
| Product-quality roadmap live + published capability matrix | architect / quality | #95, #108 | 🟡 In progress |
| Letters structured editing + pagination completeness | architect | #109, #110 | 🟡 In progress |
| Tables virtual grid + performance budgets | architect | #112 | 🟡 In progress |
| ROADMAP.md published and linked from README / org coverage | strategist | — | ✅ Done |

### Q4 2026 (October–December) — "Enterprise Readiness & Distribution"

**Theme**: enterprise security, headless document processing, and reproducible Flatpak distribution.

| Goal | Owner | Tracking | Status |
|------|-------|----------|--------|
| Client-side document encryption & XML-DSig digital signatures in `suite-common-core` | architect / security | — | 📋 Planned |
| Headless document conversion CLI binaries (`suite-convert`) across formats | architect | — | 📋 Planned |
| Flatpak reproducible build pipelines & enterprise dconf policy lockdown | operations | #122 | 📋 Planned |
| Automated AT-SPI screen reader audit gate for accessibility compliance | quality | #120 | 📋 Planned |

---

## Technical Debt Backlog

| Item | Issue | Priority | Effort |
|------|-------|----------|--------|
| GUI-layer God-files (window.rs LOC cleanup) | #168 | P0 | L |
| Dual maintenance burden cleanup | #82 | P1 | L |
| spell.rs `generate_candidates("")` boundary handling | #172 | P1 | S |

---

## How to Contribute

See [docs/CONTRIBUTING.md](./docs/CONTRIBUTING.md) and [docs/DEVELOPMENT.md](./docs/DEVELOPMENT.md) for build setup (Rust + GTK4/libadwaita, Nix flake included). Pick an issue labeled `good first issue` or comment on a goal you would like to own.

---
*Maintained by the strategist agent (tuna-os hive).*
