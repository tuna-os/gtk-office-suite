# Performance Benchmarking, Memory Profiling & Cold-Start Optimization Strategy

**Last updated**: 2026-09-05 | **Maintainer**: tuna-os / strategist agent

---

## Executive Summary

As **gtk-office-suite** expands document model features across Letters (word processing), Tables (spreadsheet), and Decks (presentations), maintaining sub-100ms UI responsiveness, under 1.5s cold-start app launch times, and bounded memory footprints (<150 MB base RSS) on Linux desktop surfaces and Flatpak runtimes is critical for end-user adoption and competing with legacy desktop suites.

This document outlines the strategic performance roadmap, establishing key metrics, memory budgeting rules, continuous benchmarking automation in CI, and cold-start optimization milestones.

---

## Performance Targets & Budgets

| App / Subsystem | Benchmark Metric | Target (P95) | Hard Limit / Budget Gate |
|-----------------|------------------|--------------|-------------------------|
| **Cold Startup** | Time to interactive (`window.present()`) | < 800 ms | < 1,500 ms |
| **Letters Core** | Document pagination & layout calculation (100 pgs) | < 50 ms | < 120 ms |
| **Tables Core** | Sparse grid formula evaluation (100k cells) | < 30 ms | < 80 ms |
| **Decks Core** | Slide rendering & transition snapshot frame time | < 16 ms (60 FPS) | < 33 ms (30 FPS) |
| **Memory (Base)**| Idle RSS footprint after launch | < 85 MB | < 150 MB |
| **Memory (Peak)**| RSS footprint with 500-page ODT / 50k-row ODS | < 250 MB | < 450 MB |

---

## Key Pillars

### 1. Pure-Rust Core Microbenchmarks (`criterion`)
- Isolate performance-critical algorithms in `suite-common-core` and app core crates (`letters-core`, `tables-core`, `decks-core`).
- Benchmark document parser engines (ODT, ODS, ODP, OpenFormula) against ratcheted test corpora.
- Automate regression checks in `ci.yml` to prevent performance degradation on PRs.

### 2. Memory Footprint & Allocation Profiling
- Track heap allocations using `dtrace`/`valgrind`/`heaptrack` profiles in nightly CI runs.
- Prevent unneeded UI widget allocations at startup; defer secondary sidebar/dialog construction until requested.
- Enforce strict allocation discipline in custom GTK4 widgets (`allocate` in `size_allocate`, zero allocations inside GTK `snapshot`).

### 3. Flatpak Cold-Start & Portal Optimization
- Pre-compile GSettings schemas into shared runtime paths during Flatpak build.
- Optimize GTK4 icon theme lookup and font cache initialization.
- Utilize async loading for recent files and background autosave indexing.

---

## Implementation Phases

- **Phase 1 (Q3 2026)**: Integrate `criterion` benchmarks in `suite-common-core` and create automated performance regression suite.
- **Phase 2 (Q4 2026)**: Implement cold-start tracing and GTK4 widget allocation profiling in nightly CI gates.
- **Phase 3 (Q1 2027)**: Establish automated memory leak and peak RSS regression detection across full document fixture corpora.
