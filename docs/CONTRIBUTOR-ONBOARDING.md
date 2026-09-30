# Contributor Onboarding Guide

Welcome to gtk-office-suite. This guide highlights high-leverage entry points for contributors in Q4 2026.

## Why Contribute Now?

gtk-office-suite is targeting production readiness in Q4 2026. The project has:

- **Measured quarterly milestones** — daily-driver editing by Q3, production gates by Q4
- **Deterministic, scoped work** — render-parity fixtures, GUI decomposition, accessibility audits
- **Published roadmap** — [ROADMAP.md](../ROADMAP.md) and per-item tracking in [docs/readiness-2026-09/](readiness-2026-09/README.md)
- **Clear architecture** — [docs/ARCHITECTURE.md](ARCHITECTURE.md) and [AGENTS.md](../AGENTS.md) explain module layout and CI expectations

## Four High-Leverage Entry Points

### 1. Render-Parity Fixtures (P0 — Render Matching)

**What it is**: Screenshot-based visual regression testing against LibreOffice. Every fixture is a document that must render identically in gtk-office-suite and LibreOffice.

**Why it matters**: We measure document fidelity fixture-by-fixture, not hand-waving "compatibility claims."

**How to contribute**:
- Review [docs/RENDER-PARITY-ROADMAP.md](RENDER-PARITY-ROADMAP.md) — each app has a Phase 2 scorecard
- Pick a fixture marked **amber** (renders in our app, differs from LibreOffice, or missing logic)
- Run `tools/render-lab/run.sh --app letters` to generate before/after screenshots
- Fix the rendering logic in the app or document model
- Commit with test evidence: `git commit -s -m "fix(letters): <fixture name> render parity"`

**Entry difficulty**: Medium. Requires understanding document models + Cairo rendering, but fixtures are isolated and well-scoped.

**Related issues**: [#95](https://github.com/tuna-os/gtk-office-suite/issues/95), docs/RENDER-PARITY-ROADMAP.md

### 2. GUI Decomposition (P0 — God-File Cleanup)

**What it is**: Breaking down large window.rs files that have grown beyond 1,500 lines each.

**Why it matters**: Large files are hard to review, test, and maintain. We enforce ceilings: Letters 1,800 lines, Tables 2,300 lines, Decks 1,800 lines.

**How to contribute**:
- Pick a cohesive feature set in `letters/src/window.rs`, `tables/src/window.rs`, or `decks/src/window.rs` (e.g., find dialog, export menu, search bar)
- Extract it to a new module under `src/` (e.g., `src/find.rs`)
- Wire the module's signals back into window.rs
- Verify line count with `scripts/release_gate.py` — your PR must keep the file under ceiling

**Entry difficulty**: Easy to medium. Requires understanding GTK signal flow, but the boundary is clear (extract → wire signals → verify count).

**Related issues**: [#168](https://github.com/tuna-os/gtk-office-suite/issues/168)

### 3. Accessibility Audit (P2 — AT-SPI Screen Reader)

**What it is**: Verifying that AT-SPI nodes expose correct screen reader semantics for Tables grids, Decks canvases, and dialog boxes.

**Why it matters**: Linux desktop adoption depends on accessible interfaces. We measure with Orca screen reader and AT-SPI inspection tools.

**How to contribute**:
- Install Orca and AT-SPI tools: `sudo apt install orca at-spi2-core`
- Run an app: `cargo run -p letters`
- Open Orca: `orca --help` (or Ctrl+Alt+O in many desktops)
- Navigate with keyboard, verify Orca announces correct text/roles
- File accessibility bugs with reproduction steps and Orca output
- Or: add AT-SPI assertions to `tests/gui/test_smoke.py` for specific journeys

**Entry difficulty**: Low. No coding required for bug reports; moderate Rust if writing test cases.

**Related issues**: [#120](https://github.com/tuna-os/gtk-office-suite/issues/120), [#306](https://github.com/tuna-os/gtk-office-suite/issues/306)

### 4. Flatpak & Release Gates (P2 — Distribution)

**What it is**: Ensuring the Flatpak build is reproducible, settings migrate cleanly, and the app recovers from crashes.

**Why it matters**: Users expect a bulletproof release candidate. Crashes, lost settings, and corrupted saves are release blockers.

**How to contribute**:
- Test the Flatpak build locally: `flatpak run org.tunaos.letters`
- Stress-test: create documents, force-quit, reopen — verify recovery works
- Test settings migration: change preferences, kill the app, restart — verify prefs persist
- Run the smoke tests: `just test-gui-all` or `tests/gui/run_gui_tests.sh test_smoke.py`
- File bugs with reproduction steps and logs (`~/.var/app/org.tunaos.letters/`)

**Entry difficulty**: Low to medium. Requires testing discipline, not deep coding.

**Related issues**: [#122](https://github.com/tuna-os/gtk-office-suite/issues/122), [#578](https://github.com/tuna-os/gtk-office-suite/issues/578)

## How to Get Started

1. **Pick an entry point above** and review its related issues
2. **Read [CONTRIBUTING.md](../CONTRIBUTING.md)** for commit style, pre-commit gates, and code review expectations
3. **Read [AGENTS.md](../AGENTS.md)** to understand how CI gates work (test layers, render-lab, oracle suites)
4. **Check [ARCHITECTURE.md](ARCHITECTURE.md)** to understand module boundaries (core crates are GTK-free; business logic goes there)
5. **Start small**: pick a scoped fixture, a 200-line GUI extraction, or a single accessibility bug
6. **Open a draft PR early** — we review incrementally and give feedback before the work is done

## Good-First-Issue Labels

We tag low-friction contributor tasks with labels:

- `good-first-issue` — isolated, well-scoped, well-documented
- `help-wanted` — needs external contribution
- `documentation` — docs-only (no code changes)

Search by label in [GitHub Issues](https://github.com/tuna-os/gtk-office-suite/issues).

## Questions?

- **#contributors** on the community Zulip (if exists)
- **Discussions** tab on GitHub for design questions
- **Issues** for bugs or feature requests

---

**Last updated**: 2026-09-30  
**For**: Q4 2026 release readiness and community scaling
