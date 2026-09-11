# Headless Document Conversion CLI Strategy

## Overview

As `gtk-office-suite` matures, server-side document automation, CI/CD document validation, and headless format conversion represent key enterprise adoption opportunities.

This document outlines the architecture and phased roadmap for exposing non-GUI, headless conversion capabilities via CLI binaries (`suite-convert`, `letters-cli`, `tables-cli`, `decks-cli`).

---

## Architectural Principles

1. **GTK-Free Engine Layer**:
   All document parsing, formatting, calculation, layout math, and export logic MUST reside in GTK-free core crates (`suite-common-core`, `letters-core`, `tables-core`, `decks-core`).
2. **Headless Execution**:
   CLI conversion tools must run in minimal server environments (e.g., standard Linux containers) without requiring `DISPLAY`, Wayland compositors, Xvfb, or `libgtk-4` initialization.
3. **Deterministic Output & Parity**:
   Headless conversions produce identical rendered outputs (PDF, HTML, Plaintext, Markdown) as the desktop GUI export flows, sharing 100% of the underlying format engines.

---

## Deliverables & Milestones

### Phase 1: Core CLI Conversion Harness (Q4 2026)

- `suite-convert`: Single entry-point CLI tool for multi-format conversion:
  ```bash
  suite-convert --from odt --to pdf document.odt -o document.pdf
  suite-convert --from xlsx --to csv sheet.xlsx -o sheet.csv
  suite-convert --from odp --to pdf presentation.odp -o presentation.pdf
  ```
- Streaming stdin/stdout pipeline support for Unix shell compositions.

### Phase 2: Microservice & Container Packaging

- Publish lightweight OCI container image (`ghcr.io/tuna-os/suite-convert:latest`).
- Add GitHub Action (`tuna-os/suite-convert-action`) for continuous document compliance testing in CI pipelines.

### Phase 3: Performance Budgets & Loss Auditing

- Benchmark conversion throughput (pages/sec, memory ceiling < 64MB per worker).
- Emit structured JSON loss reports when document features outside current import/export support are encountered.

---

## Technical Dependencies

- Rust core crates: `suite-common-core`, `letters-core`, `tables-core`, `decks-core`.
- PDF rendering backend: Cairo / Skia / pdf-writer integration.
