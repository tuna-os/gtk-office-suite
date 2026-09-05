# Suite Performance & Benchmarking Strategy

## Overview

As GTK Office Suite (Letters, Tables, Decks) scales to handle enterprise documents, large spreadsheets, and media-rich presentations, maintaining responsiveness and efficiency is a core requirement. This document outlines the suite-wide performance benchmarking targets, continuous memory profiling standards, and Flatpak cold-start optimization roadmap for Q4 2026 and 2027.

---

## 1. Performance Goals & SLA Budgets

| Metric | Target SLA | Measurement Tool | Scope |
| :--- | :--- | :--- | :--- |
| **Cold Start Latency** | $< 500\text{ ms}$ (Flatpak launch to first frame render) | `hyperfine`, systemd trace | Letters, Tables, Decks |
| **Viewport Scrolling** | $60\text{ FPS}$ (16.6ms frame limit) / $120\text{ FPS}$ optional | GTK4 Inspector / Cairo frame profiler | Canvas, Sheet grid, Buffer text |
| **Large ODT Import** | $< 1.5\text{ s}$ for 200-page document | `criterion` benchmark suite | `letters-core` |
| **Spreadsheet Recalculation** | $< 100\text{ ms}$ for 100k formulas | `criterion` benchmark suite | `tables-core` |
| **Memory Ceiling** | $< 120\text{ MB}$ RSS baseline idle | Valgrind / Heaptrack / Massif | All binary processes |

---

## 2. Architecture & Benchmark Guidelines

### GTK4 Redraw & Allocations
- **No Heap Allocations in `snapshot()`**: Custom GTK4 widgets MUST allocate children and compute geometry layouts inside `size_allocate` or event handlers, never during Cairo/GSK snapshot drawing.
- **Surface Texture Caching**: Slide thumbnails in Decks and formatted cell overlays in Tables must utilize cached `GdkMemoryTexture` objects, invalidating only region bounds on model mutations.

### Pure Core Benchmarking
- Core parsing, formula evaluation, and document serialization logic must reside in pure Rust crates (`suite-common-core`, `letters-core`, `tables-core`, `decks-core`).
- Micro-benchmarks are implemented using `criterion` and executed in non-graphical environments.

---

## 3. Continuous Integration & Profiling Automation

1. **Nightly Performance Regression Gate**:
   - Run `cargo bench` on standard hardware baselines to detect regressions $> 5\%$ in parsing or layout.
2. **Massif Memory Leak Checks**:
   - Integrated valgrind/massif runs on smoke test fixtures to detect heap accumulation across repeated document open/close cycles.
3. **Flatpak Sandbox Overhead Audits**:
   - Measure GSettings compilation overhead, portal IPC latency, and fontconfig cache initialization times.

---

## 4. Implementation Roadmap

- **Phase 1 (Q4 2026)**: Integrate `criterion` benchmarks into `suite-common-core` and app core crates. Establish memory floor metrics.
- **Phase 2 (Q1 2027)**: Implement dirty-rectangle viewport culling in `Tables` scroll containers and `Decks` canvas area.
- **Phase 3 (Q2 2027)**: Optimize Flatpak GSettings pre-compilation and font cache warming for sub-400ms startup.
