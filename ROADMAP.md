# gtk-office-suite Roadmap

**Last updated**: 2026-09-10 | **Maintainer**: tuna-os

---

## Mission

A **GNOME-native office suite in Rust** — Letters (word processor), Tables (spreadsheet), Decks (presentations) — built on GTK4 + libadwaita and shipped as Flatpaks. A LibreOffice-inspired suite that feels native to the modern Linux desktop, with measured, ratcheted parity against LibreOffice formats (CommonMark, ODT, ODP, OpenFormula) so users get real document compatibility, not a demo.

gtk-office-suite is the org's flagship **end-user product bet** and a cornerstone of the modern cloud-native desktop mission: office productivity is the last major desktop gap that keeps users on Windows/Mac.

---

## Current Status (September 2026)

- **Post-v1.0 Production Readiness**: All three applications (Letters, Tables, Decks) build cleanly, pass unit test suites (~90 tests), and ship as Flatpaks.
- **Measured Format Parity** (`docs/PARITY.md`): LO-Letters 109/109, LO-Decks 9/9 (soffice oracle 28), CommonMark 630/652, OpenFormula 107/107.
- **Dependency & Platform Modernization**: Upgraded `quick-xml` to 0.42 and `rdocx-oxml` to 0.12.0. Renovate org presets adopted without code-owner paging.
- **Testing & Release Safety**: Enforcement of strict non-gating/gating split in `ci.yml` and `nightly.yml` (e.g. `REQUIRE_SOFFICE=1` explicitly required for full ODF validation).

---

## Strategic Roadmap Priorities (Q4 2026)

| Priority | Strategic Item | Target Horizon | Status |
|----------|----------------|----------------|--------|
| P0 | Q4 2026 Flatpak Distribution & Release Quality Gate | Q4 2026 (Near-term) | 🟡 In progress |
| P0 | Screen Reader (AT-SPI) Automated Accessibility Audit Gate | Q4 2026 (Near-term) | 🟡 In progress |
| P1 | Headless Document Conversion CLI Binaries (`letters-cli`, `tables-cli`, `decks-cli`) | Q4 2026 (Near-term) | 🟡 In progress |
| P1 | Document Interoperability Loss-Budget & Unsupported Feature Inspector Framework | Q4 2026 / Q1 2027 | ⬜ Planned |
| P1 | Client-Side Document Encryption, Digital Signatures & PDF/A Archiving | Q4 2026 / Q1 2027 | ⬜ Planned |

---

## Quarterly Goals

### Q3 2026 (July–September) — "Daily-Driver Core & Dependency Modernization"

**Theme**: Solidify GTK-free core architecture and dependency stack.

| Goal | Tracking | Status |
|------|----------|--------|
| `quick-xml` 0.42 & `rdocx-oxml` 0.12.0 migration | #298, #331 | ✅ Completed |
| Shared Renovate org presets & non-paging code ownership | #402 | ✅ Completed |
| GTK-free crate separation enforcement (`suite-common-core`) | `AGENTS.md` | ✅ Completed |

### Q4 2026 (October–December) — "Distribution, Accessibility & Enterprise Readiness"

**Theme**: Harden release gates, accessibility standards, headless workflows, and document safety.

| Goal | Focus Area | Status |
|------|------------|--------|
| **Release Gate & Reproducible Builds** | Automated Flatpak bundle validation, delta updates, and atomic recovery | 🟡 In progress |
| **Automated Accessibility Gate** | Dogtail AT-SPI screen reader journey validation in CI | 🟡 In progress |
| **Headless Conversion CLI** | Serverless/CLI document rendering without GTK display dependency | 🟡 In progress |
| **Document Security & Archiving** | Client-side encryption, PGP/X.509 signatures, and PDF/A export | ⬜ Planned |

---

## Technical Debt & Architecture Refactoring

| Item | Priority | Scope |
|------|----------|-------|
| GUI-layer God-file decomposition (`window.rs` across letters/tables/decks) | P0 | Refactor window signal wiring into GTK-free controllers |
| Legacy Python office suite deprecation & sunset | P1 | Remove leftover python suite references and scripts |
| Headless CLI binary isolation | P1 | Extract CLI conversion targets without GTK dependency |

---

## How to Contribute

See [docs/CONTRIBUTING.md](./docs/CONTRIBUTING.md) and [docs/DEVELOPMENT.md](./docs/DEVELOPMENT.md) for build setup (Rust + GTK4/libadwaita, Nix flake included). Pick an issue labeled `good first issue` or comment on a goal you would like to own.
