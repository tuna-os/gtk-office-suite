# Headless Document Rendering & Print Preview Architecture (Q4 2026)

## Overview

GTK Office Suite requires a headless document layout and rendering architecture to support high-fidelity print preview generation, export to vector PDF/A format, and zero-display batch processing across **Letters**, **Tables**, and **Decks**.

Currently, print operations and preview snapshots rely on GTK widget mapping and interactive main loop execution (`window.rs` / `snapshot()`). This architecture decouples document geometry calculation and canvas rendering into GTK-free core crates.

---

## Strategic Objectives

1. **Zero-Display Server Rendering**: Perform page pagination, document layout calculations, and PDF export without requiring an active X11/Wayland display server or GTK widget allocation.
2. **Deterministic Print Preview**: Ensure document pagination and rendering are bit-for-bit identical between headless PDF generation and desktop print preview dialogs.
3. **PDF/A Standard Compliance**: Support vector output certified against PDF/A standards for long-term document archiving and enterprise compliance.
4. **Headless Batch Pipeline Integration**: Provide rust API bindings for `letters-cli`, `tables-cli`, and `decks-cli` to perform batch PDF conversions.

---

## Architectural Decomposition

```
+-------------------------------------------------------------------+
|                        App Binaries                               |
|        (letters / tables / decks GTK4 / libadwaita UI)            |
+--------------------------------+----------------------------------+
                                 |
                                 v
+-------------------------------------------------------------------+
|                     Print & Preview Service                       |
|           (GtkPrintOperation bridge / Preview Modal UI)           |
+--------------------------------+----------------------------------+
                                 |
                                 v
+-------------------------------------------------------------------+
|                      suite-common-core                            |
|  - Layout Engine (pagination, paragraph breaking, grid pages)    |
|  - Render Canvas Abstraction (Cairo surface / vector PDF)          |
|  - Headless Pipeline Engine (PDF/A metadata, font embedding)       |
+--------------------------------+----------------------------------+
```

### Module Responsibilities

- **`suite-common-core::render`**: Defines display-agnostic canvas traits (`CanvasDevice`, `PageGeometry`, `FontMetrics`).
- **`letters-core::layout`**: Implements continuous-flow line breaking, block pagination, header/footer placement, and page numbering.
- **`tables-core::print`**: Implements grid tiling, column fit-to-page logic, print area scoping, and header row repeating across page splits.
- **`decks-core::render`**: Handles slide frame rasterization, presenter note inclusion, and handout 2-up/4-up layout calculations.

---

## Performance & Quality Targets

- **Pagination Speed**: Under 150ms for a 100-page document in `letters-core`.
- **Memory Footprint**: Peak memory consumption during multi-page PDF rendering constrained under 128MB.
- **Visual Parity**: 100% layout parity between GTK surface snapshots and headless Cairo PDF surface exports.

---

## Roadmap Milestones (Q4 2026)

1. **M1 (Oct 2026)**: Abstract `CanvasDevice` traits into `suite-common-core` and implement GTK-free page geometry layout engines.
2. **M2 (Nov 2026)**: Integrate Cairo vector PDF/A export backend into `letters-core` and `tables-core`.
3. **M3 (Dec 2026)**: Expose CLI `--export-pdf` flags in headless conversion binaries and enforce automated visual parity test assertions in CI.
