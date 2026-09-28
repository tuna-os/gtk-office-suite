# Measured by CI, driven by contributors: how gtk-office-suite scales through structured regressions

*TunaOS project, September 2026 — draft for community review*

Gtk-office-suite is a GNOME-native office suite in Rust — **Letters**, **Tables**, **Decks** — built with GTK4/libadwaita and shipped as Flatpaks. We're at a inflection point: Q3's "Daily-driver editing" priority is wrapping, Q4's "Ship it properly" (release gates, accessibility, reproducible Flatpak builds) is starting, and the work ahead is the kind that scales through *structured contribution*, not heroic commits.

This post explains why the project's architecture makes it uniquely contributor-friendly right now, and where we need hands.

## The regressions are the guardrails

Traditional office suites are feature-gated. Ours is *regression-gated*: every visual feature is done when a screenshot of the running app matches LibreOffice's rendering of the same file within a measured budget. That constraint lives in CI (`tools/render-lab/`), and it's both a burden and a gift.

The burden: you can't hide quality behind alpha labels. Any rendered character that misaligns, any spacing that drifts, any formula that calculates wrong is visible on the test report. Screenshots don't lie the way assertions can.

The gift: *the work is predictable*. A contributor doesn't need to guess whether their paragraph-breaking change is "good enough." CI will render before and after, measure the delta, and report exactly which fixtures improved, regressed, or stayed stable. That feedback loop — fixture → implement → render → verify — is deterministic and incremental. You can land one fixture at a time.

## Where the work is: four entry points

### 1. Render parity fixtures (phase 2 in progress)

**Effort**: S–M | **Domain**: XML, LibreOffice automation, image diffing

The render-lab pipeline measures three corpora: LO-authored HTML scenarios converted to .docx at test time; round-trip cycle tests; CommonMark spec examples. Phase 1 (every single-feature fixture green) is the 2026-09-24 target. Phase 2 (edge cases, combined features, stress tests) is running now, and the backlog is open.

Each fixture is a small document: "a 12pt paragraph with 1.5 line spacing and a bold run at position 3–8." A contributor writes the scenario in HTML, the test harness converts it through LibreOffice, the suite reads it, and CI diffs the rendering. Fixtures don't require GTK knowledge, deep format expertise, or merge conflict risk — they're data, and they're reviewable as examples.

**To contribute**: pick a capability from the feature matrix, write 2–3 HTML scenarios that exercise it, and open a PR. The render-lab harness runs them, uploads the results, and the diff is transparent. See [docs/RENDER-PARITY-ROADMAP.md](../RENDER-PARITY-ROADMAP.md).

### 2. GUI decomposition: shrinking window.rs (architectural growth management)

**Effort**: M–L | **Domain**: GTK4 state management, Rust module organization

The flagship God-file ceiling is enforced: `tables/src/window.rs` has a 2300-line ceiling, `decks/src/window.rs` 1800 lines, `letters/src/window.rs` 1800 lines. The measure is in CI (`scripts/release_gate.py`), and the ceiling is ratcheted up incrementally only when architectural decomposition justifies it.

This creates real module extraction work: separating chart-dialog code from Tables (already done in #594), disentangling clipboard from window state, moving format-specific dialogs into separate modules. Each extraction is a PR: old code → new module → update imports → verify tests pass.

**To contribute**: file an issue for a dialog or feature you want to extract, implement the module boundary, and open the PR with test coverage. See [ROADMAP.md](../../ROADMAP.md) and issue #168.

### 3. Accessibility compliance: AT-SPI screen reader audit gates

**Effort**: M–L | **Domain**: GTK4 accessibility, Orca screen reader, AT-SPI

Accessibility is a mandatory blocker for downstream Linux desktop distribution adoption. The work spans three layers: GTK4 accessible nodes (already exposed for Tables grid and Decks canvas), widget-relative spatial extents translation, and Orca screen reader navigation verification (tracked in ROADMAP issue #120).

Currently, there is no standardized audit gate or CI testing suite. This is foundational — a contributor who builds the infrastructure (Orca navigation test harness, CI fixture validation, high-contrast theme verification) unlocks adoption for downstream distributions.

**To contribute**: start with [docs/ACCESSIBILITY-COMPLIANCE-ROADMAP.md](../ACCESSIBILITY-COMPLIANCE-ROADMAP.md) (being drafted); propose AT-SPI inspection protocols or Orca test patterns. This work requires accessible technology (screen reader, AT-SPI inspection tools), so contributors with a11y domain expertise are especially welcome.

### 4. Release gates and Flatpak reproducibility (Q4 2026 production readiness)

**Effort**: L | **Domain**: Flatpak reproducible builds, GSettings, recovery/rollback verification

Q4 is "Ship it properly": Flatpak reproducible builds (#122, #578), GSettings migration verification, atomic save recovery (#437). These are necessary but not yet tracked with concrete sub-milestones.

Work here includes: verifying that a Flatpak built in two different environments produces byte-identical bundles, documenting and testing the GSettings upgrade path from legacy Python app to Rust app, implementing recovery workflows so a crash doesn't leave the document in an inconsistent state.

**To contribute**: pick one of the Q4 milestones, propose a testing/verification strategy (as a bead or issue), and open a PR with the implementation. See [docs/readiness-2026-09/README.md](../readiness-2026-09/README.md) and ROADMAP.md.

## Contributor scaffold already in place

We've built the onboarding structure so you don't have to reverse-engineer it:

- **CONTRIBUTING.md**: clone, build (Rust 1.80+, GTK 4.14+, libadwaita 1.5+), run tests under Xvfb, push.
- **AGENTS.md**: the exact CI commands and pitfalls. Written for automation, but invaluable for humans.
- **Nix flake**: reproducible dev environment if you use Nix.
- **good first issue** label: curated entry points for first-time contributors.
- **[docs/DEVELOPMENT.md](../DEVELOPMENT.md)** and **[docs/TESTING.md](../TESTING.md)**: detailed build and test lane documentation.

## Join us

If you're interested in any of these areas — render parity, GUI architecture, accessibility compliance, release engineering — open an issue, comment on an existing one, or drop by and introduce yourself. We're at a phase where structured contribution has the most leverage: the roadmap is public, the quarterly goals are clear, and the regression gates mean every PR is an auditable step forward.

The office suite that feels native to the modern Linux desktop starts with people like you.

---

## Related reading

- [ROADMAP.md](../../ROADMAP.md) — current quarterly milestones and priorities
- [docs/RENDER-PARITY-ROADMAP.md](../RENDER-PARITY-ROADMAP.md) — the render-lab pipeline and fixture corpus
- [docs/CONTRIBUTING.md](../CONTRIBUTING.md) — how to get started
- [docs/DEVELOPMENT.md](../DEVELOPMENT.md) — build and development environment setup
- [#168: GUI-layer God-file decomposition](https://github.com/tuna-os/gtk-office-suite/issues/168)
- [#120: Accessibility: keyboard + screen-reader journeys](https://github.com/tuna-os/gtk-office-suite/issues/120)
- [#443: Readiness 2026-09 — daily-driver editing completion tracker](https://github.com/tuna-os/gtk-office-suite/issues/443)
