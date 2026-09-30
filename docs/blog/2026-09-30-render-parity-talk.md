# Talk Proposal: Measuring Document Fidelity — How gtk-office-suite Ratchets Parity Against LibreOffice on Every Commit

**For**: FOSDEM 2027, GUADEC 2027, GNOME.Asia 2027  
**Duration**: 30–45 minutes  
**Audience**: Desktop developers, Rust + GTK4 practitioners, office software maintainers

## Abstract

Building an office suite in 2026 means proving document fidelity, not claiming it. gtk-office-suite has developed a measurable approach: screenshot-based visual regression testing against LibreOffice, ratcheted in CI. This talk covers:

1. **Why screenshot CI matters** — file format compatibility (CommonMark, OOXML, ODF) doesn't prove visual fidelity; a feature can round-trip perfectly but render wrong on screen
2. **The render-lab instrument** — how we capture reference screenshots from LibreOffice headless, compare app output at multiple tiers (offscreen render, GTK Broadway, Xvfb, Flatpak VM), and ratchet pass counts
3. **Making it workflow-friendly** — fixture-driven development (one .docx = one visual feature), per-fixture pass budgets, deterministic failures that unblock contributors
4. **Reusable patterns** — the same approach works for any multi-format system: web rendering, CAD, data visualization, or PDF tooling

## Why Now?

- **Phase 1 complete** (2026-09-24): render-lab framework is live; all fixtures are green or amber (no red). Phase 2 (every fixture green) is in progress
- **Novel in the ecosystem** — most office suites claim compatibility; gtk-office-suite measures it visually
- **High-leverage for contributors** — deterministic, well-scoped fixtures reduce onboarding friction and make rendering work tractable
- **Desktop adoption blocker solved** — visual parity is what users see; this talk shows how to build it into shipping software

## Key Talking Points

### The Problem

- Existing office suite compatibility claims are vague ("compatible with Office 2019") and unverifiable
- Test suites measure file content (text, formatting, formulas) but not visual rendering
- Features in the document model can be invisible on screen; CI has no way to catch this
- Contributors don't know what "done" looks like for a visual feature

### The Solution: Screenshot-Driven Development

- **Reference standard**: LibreOffice headless rendering (the oracle)
- **Ratcheted corpora**: fixture-by-fixture pass counts that CI refuses to let regress
- **Multi-tier rendering**: capture output at multiple abstraction levels (offscreen GTK, GTK Broadway, Xvfb, Flatpak VM in QEMU)
- **Deterministic failures**: if a fixture is red or amber, the failure is reproducible and scoped to one feature

### Evidence & Metrics

- **Letters parity**: 109/109 LibreOffice-authored corpus (verified ratchet in `letters-core/tests/lo_parity.rs`)
- **Decks parity**: 9/9 LibreOffice oracle (pptx round-trip through Impress)
- **CommonMark fidelity**: 630/652 round-trip idempotence (target met; 22 remaining are escape/entity edge cases)
- **OpenFormula conformance**: 107/107 (IronCalc upstream-main corpus)

All numbers printed to CI job summary on every push; none allowed to regress.

### Workflow for Contributors

1. Pick a fixture from the render-parity scorecard (e.g., "tables/bold-text" or "decks/master-backgrounds")
2. Run `tools/render-lab/run.sh --app letters` to generate before/after screenshots
3. Fix the rendering logic in the app or document model
4. Commit with evidence: `git commit -s -m "fix(letters): bold-text render parity"`
5. Next push: fixture moves from amber to green, pass count increments

This makes visual feature work deterministic and measurable—exactly what drawing-heavy software needs.

### Reusable Patterns

The approach works for any system that reads/writes other formats:

- **Web rendering engines** (WebKit, Blink): render corpus against reference (Chrome, Firefox)
- **PDF tooling**: compare rendered output against source PDFs
- **CAD & data visualization**: fixture-driven regression testing
- **Game engines**: visual QA at multiple render backends

The infrastructure is ~200 lines of Python + per-format harness (~100–200 LOC). The payoff is deterministic, ratcheted quality that scales.

## Slide Outline

1. **Opening**: "Every office suite claims compatibility. We measure ours."
2. **Problem**: Why file format parity alone isn't enough
3. **Solution**: The render-lab instrument (diagram)
4. **Demo**: Running `tools/render-lab/run.sh` live, showing before/after
5. **Results**: The scoreboard (Letters 109/109, Decks 9/9, CommonMark 630/652)
6. **Workflow**: How contributors use render-parity to build features
7. **Generalization**: Applying the pattern to other systems
8. **Closing**: Invitation to contribute / adopt the pattern

## Demo Ideas

- Live run of `tools/render-lab/run.sh --app letters` on a fixture
- Show a before/after screenshot comparison
- Show the CI job summary printing ratcheted pass counts
- Show a contributor workflow: pick a fixture → fix rendering → commit → fixture moves green

## Why This Talk Matters

- **Desktop adoption**: GNOME users want a native office suite; this talk explains how to build one correctly
- **Developer community**: Rust + GTK4 is a powerful combination; this talk shows a real, shipping example with real problems solved
- **Ecosystem pattern**: Other projects (PDF tools, web engines, CAD software) can adopt this measurement framework
- **Q4 2026 momentum**: gtk-office-suite is entering production-readiness phase; a conference talk is high-visibility validation

## Related Work

- gtk-office-suite repository: https://github.com/tuna-os/gtk-office-suite
- Render-parity roadmap: [docs/RENDER-PARITY-ROADMAP.md](../RENDER-PARITY-ROADMAP.md)
- Format parity scorecard: [docs/PARITY.md](../PARITY.md)
- Contributor onboarding: [docs/CONTRIBUTOR-ONBOARDING.md](CONTRIBUTOR-ONBOARDING.md)
- LibreOffice oracle tests: `letters-core/tests/lo_parity.rs`, `decks-core/tests/lo_parity.rs`

## About the Speaker

[Fill in based on who will deliver this talk. Maintainer or core contributor from tuna-os.]

- Deep knowledge of gtk-office-suite architecture and render-parity framework
- Hands-on experience with LibreOffice oracle testing and screenshot CI
- Familiar with GTK4, Rust document models, and format compatibility challenges

---

**Submitted**: 2026-09-30  
**Venues**: FOSDEM 2027 (CFP open Q1 2027), GUADEC 2027 (CFP typically Q3 2026), GNOME.Asia 2027
