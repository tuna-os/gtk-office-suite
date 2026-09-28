# Scaled by structure: how gtk-office-suite grows through measured contribution

*TunaOS project, September 2026*

gtk-office-suite is a GNOME-native office suite in Rust — Letters (writing), Tables (spreadsheets), Decks (presentations) — built with GTK4/libadwaita and shipped as Flatpaks. We're at the inflection point where execution speed matters more than feature parity: Q3's "Daily-driver editing" priority is wrapping, Q4's "Ship it properly" (release gates, accessibility, reproducible Flatpak builds) is starting. This is when a project either scales through structure or stalls on heroics.

This post maps where we need hands, why the work is predictable, and what makes contribution here different from "pick an issue and ship code."

## The render-parity funnel: from fixtures to shipped features

The most visible constraint in gtk-office-suite is the render-parity pipeline. Every visual feature is done when a screenshot of the running app matches LibreOffice's rendering of the same file within a measured budget. That's not a shipping gate — it's a *contribution funnel*.

Here's how it works:

1. **Write a fixture** — a small test document in HTML. Example: "a 12pt paragraph with 1.5 line spacing and a bold run at position 3–8."
2. **The harness converts it** — headless LibreOffice Writer renders your HTML to .docx at test time (fresh, not vendored).
3. **Our engine reads it** — Letters extracts the text, styles, and formatting.
4. **CI renders both** — side-by-side screenshots, pixel diffs on the report.
5. **You see instantly** — did this work? Did anything regress? The diff is transparent.

This is not "submit code and hope for approval." It's "write a test scenario, run render-lab locally (takes ~3 minutes), and the rendering is your code review." A fixture passes or fails in isolation. You don't need to understand the entire codebase to contribute one.

**To start**: pick a feature from the capability matrix in [docs/RENDER-PARITY-ROADMAP.md](../RENDER-PARITY-ROADMAP.md). Write 2–3 HTML scenarios. Run `tools/render-lab/run.sh --app letters`. Open a PR with the results. CI will render on every push, and the diff is the proof.

## GUI decomposition: architectural entry points with enforced boundaries

The second constraint is architectural growth. Three core files — `letters/src/window.rs`, `tables/src/window.rs`, `decks/src/window.rs` — have enforced line-count ceilings:

| File | Ceiling |
|---|---|
| letters/src/window.rs | 1800 |
| tables/src/window.rs | 2300 |
| decks/src/window.rs | 1800 |

These limits are not aspirational — they're enforced by `scripts/release_gate.py` in CI and refuse to let the line count drift. When a file approaches its ceiling, the only way forward is module extraction: disentangle a dialog, separate a feature, move it into a dedicated module.

This creates predictable, scoped work: "extract chart-dialog from Tables into a separate module" is a bounded refactoring. You don't need to understand the whole app. You need to understand one dialog's state, move it, update imports, and verify tests pass.

**Example**: issue #594 (chart dialog extraction) is this pattern. A self-contained feature moved out of the God-file, tests updated, merged. That clears ceiling space for the next feature.

**To start**: when a window.rs is within 200 lines of its ceiling, look for a dialog or feature that could be a separate module. Propose the extraction as an issue. Implement it as a module, add tests, PR it. See issue #168 for the full decomposition plan.

## Accessibility compliance: infrastructure-first, then features

The third entry point is less about code and more about infrastructure: accessibility compliance. Issue #120 (keyboard + screen-reader journeys) is planned but not started. This isn't about adding features — it's about building the test harness that *proves* features work for users with assistive technology.

What's needed:

- **AT-SPI inspection protocols** — documenting how to verify that widget trees expose the right accessible nodes
- **Orca screen reader test cases** — automated navigation tests that verify screen reader users can reach every interactive element
- **High-contrast validation** — theme tests that verify the UI is usable under system high-contrast modes

This work doesn't require a deep understanding of office-suite semantics. It requires accessibility domain expertise (screen reader workflows, AT-SPI node structure, keyboard navigation patterns). If you've done this work before — even for a different GNOME app — it's portable.

**To start**: read [docs/TESTING.md](../TESTING.md) to understand the CI lane structure. Propose AT-SPI inspection test cases as an issue. We have the widgets; we need the harness.

## Release gates: Flatpak reproducibility and recovery

The fourth entry point is release engineering: Flatpak reproducible builds (#122, #578), GSettings migration verification (#436), atomic save recovery (#437). These are necessary but not yet tracked with concrete sub-milestones.

Work here includes:

- **Verifying Flatpak builds** — two identical builds of the Flatpak in different environments should produce byte-identical bundles. Building the CI harness to prove it.
- **Settings migration** — documenting and testing the upgrade path from legacy Python app settings to Rust app settings. Does user configuration survive the transition?
- **Recovery workflows** — a crash shouldn't leave the document in an inconsistent state. Implement the verification test.

This is infrastructure and validation, not feature work. It's high-leverage — every user hitting production depends on these being right.

**To start**: pick one sub-goal from the Q4 readiness list. Propose a testing or verification strategy as an issue. Implement it with tests. See [docs/readiness-2026-09/README.md](../readiness-2026-09/README.md) for the detailed execution plan.

## Why this scales

The pattern across all four entry points is the same: **the work is scoped, the success criterion is measurable, and the feedback is fast.**

- **Render-parity fixtures** fail or pass; regressions are caught in pixels, not prose.
- **GUI decomposition** has a line-count ceiling; done means tests pass and the module is separate.
- **Accessibility harness** has explicit test cases; done means screen readers can navigate it.
- **Release gates** have binary outcomes; the build reproduces or it doesn't.

There's no "let's argue about this design" — the artifacts are concrete. Fixture rendering is deterministic. Line counts don't move. Tests pass or fail. That removes friction and scales contribution beyond maintainers' opinions.

## Contributor scaffold

The infrastructure is already in place:

- **[CONTRIBUTING.md](../CONTRIBUTING.md)**: clone, build (Rust 1.80+, GTK 4.14+, libadwaita 1.5+), run tests, push.
- **[AGENTS.md](../AGENTS.md)**: the exact CI commands and gotchas — written for automation, invaluable for humans.
- **[docs/DEVELOPMENT.md](../DEVELOPMENT.md)**: build and environment setup.
- **[docs/TESTING.md](../TESTING.md)**: test lanes, what each proves, how to run them locally.
- **Nix flake**: reproducible dev environment if you use Nix.
- **`good first issue` label**: curated entry points for first-time contributors.

## Join us

If any of these four areas speak to you — render parity, GUI architecture, accessibility compliance, or release engineering — open an issue, comment on an existing one, or show up in our community space.

The office suite that feels native to the modern Linux desktop starts with people like you. And right now, the structure is in place to make that contribution predictable and visible.

---

## Related reading

- [ROADMAP.md](../../ROADMAP.md) — current quarterly milestones and priorities
- [docs/RENDER-PARITY-ROADMAP.md](../RENDER-PARITY-ROADMAP.md) — the render-lab pipeline and fixture corpus
- [docs/readiness-2026-09/README.md](../readiness-2026-09/README.md) — Q4 2026 release readiness tracker (#443)
- [#168: GUI-layer God-file decomposition](https://github.com/tuna-os/gtk-office-suite/issues/168)
- [#120: Accessibility: keyboard + screen-reader journeys](https://github.com/tuna-os/gtk-office-suite/issues/120)
- [#122, #578: Release gate and Flatpak reproducibility](https://github.com/tuna-os/gtk-office-suite/issues/122)
- [CONTRIBUTING.md](../CONTRIBUTING.md) — how to get started
