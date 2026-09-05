# Headless Document Conversion CLI and Serverless API Architecture Strategy

**Target Milestone**: Q4 2026 / v1.2 Release  
**Status**: Proposal & Architecture Strategy  
**Crate Alignment**: `suite-common-core`, `letters-core`, `tables-core`, `decks-core`

---

## Strategic Objective

The GTK Office Suite core format engines (`letters-core`, `tables-core`, `decks-core`) provide high-speed, GTK-free pure Rust parsing, rendering, and conversion capabilities across OpenDocument (ODT/ODS/ODP), Office OpenXML (DOCX/XLSX/PPTX), CommonMark, PDF, and HTML.

Currently, these capabilities are exposed primarily through the GTK4 GUI applications (`letters`, `tables`, `decks`). Expanding the delivery model to expose lightweight, GTK-free standalone CLI binaries and a C/WASM API enables headless serverless processing, CI document validation pipelines, server-side document format conversion, and containerized microservice deployment without GTK/Display server overhead.

---

## System Architecture & Component Mapping

```
+-----------------------------------------------------------------------------------+
|                               Headless Entry Points                              |
|  +------------------------+  +------------------------+  +---------------------+  |
|  | letters-convert (CLI)  |  | tables-convert (CLI)   |  | decks-convert (CLI) |  |
|  +------------------------+  +------------------------+  +---------------------+  |
|  +-----------------------------------------------------------------------------+  |
|  |                 suite-convert (Unified Multi-Format CLI)                     |  |
|  +-----------------------------------------------------------------------------+  |
+------------------------------------------+----------------------------------------+
                                           |
                                           v
+-----------------------------------------------------------------------------------+
|                            Pure-Rust Core Layer (GTK-Free)                         |
|  +-------------------+      +-------------------+      +----------------------+  |
|  |   letters-core    |      |    tables-core    |      |      decks-core      |  |
|  | ODT / DOCX / MD   |      | ODS / XLSX / CSV  |      |  ODP / PPTX / Canvas |  |
|  +-------------------+      +-------------------+      +----------------------+  |
|  +-----------------------------------------------------------------------------+  |
|  |                             suite-common-core                              |  |
|  | Document Model, Format Registry, Loss Budget Inspector, PDF/SVG Exporters   |  |
|  +-----------------------------------------------------------------------------+  |
+-----------------------------------------------------------------------------------+
```

---

## Core Capabilities & CLI Interface Design

### 1. `letters-convert` CLI Binary
- **Input Formats**: ODT, DOCX, CommonMark (`.md`), Plain Text (`.txt`).
- **Output Formats**: PDF, HTML, CommonMark (`.md`), ODT, DOCX.
- **Key Flags**:
  - `--input <PATH>` / `-i`: Path to input document.
  - `--output <PATH>` / `-o`: Path to target output file.
  - `--format <FMT>` / `-f`: Output format override (`pdf`, `html`, `md`, `odt`, `docx`).
  - `--loss-budget-check`: Inspect unsupported feature loss budget before conversion.
  - `--quiet` / `-q`: Suppress progress output, emit JSON result on stdout.

### 2. `tables-convert` CLI Binary
- **Input Formats**: ODS, XLSX, CSV, TSV.
- **Output Formats**: CSV, TSV, HTML, ODS, XLSX, PDF.
- **Key Flags**:
  - `--sheet <NAME_OR_INDEX>`: Target specific sheet for export (default: active/first sheet).
  - `--recalculate`: Trigger IronCalc formula evaluation before export.
  - `--loss-budget-check`: Audit formula and formatting fidelity during lossy conversion.

### 3. `decks-convert` CLI Binary
- **Input Formats**: ODP, PPTX.
- **Output Formats**: PDF, SVG (per-slide export), PNG/JPEG thumbnail array, ODP, PPTX.
- **Key Flags**:
  - `--slide <INDEX_RANGE>`: Export specific slide numbers (e.g. `1-3,5`).
  - `--dpi <DPI>`: Target rendering resolution for image/raster exports (default: 150 DPI).

---

## CI/CD and Serverless Deployment Modes

1. **Standalone Microservice Container**: Minimal Scratch/Alpine Docker image (< 25MB) containing static `suite-convert` binaries for serverless AWS Lambda or Google Cloud Run invocation.
2. **CI Document Validation Gate**: Execute headless conversion in GitHub Actions workflows to verify PR document changes don't break baseline visual layout or lose semantic markers.
3. **Rust C API / WASM Bindings**: Expose C-ABI static libraries (`libsuite_convert.a`) and WebAssembly modules (`suite_convert_wasm.wasm`) for edge document processing in browsers or Node.js runtimes.

---

## Implementation Milestones & Roadmap

- **Phase 1 (Q4 2026)**: Modularize core conversion pipelines out of app crates into `suite-common-core` adapters. Add CLI argument parsing with `clap`.
- **Phase 2 (Q4 2026)**: Implement headless PDF/SVG/HTML export drivers using Cairo/Pango headlessly under `suite-common-core`.
- **Phase 3 (Q1 2027)**: Add loss-budget assertion flags (`--fail-on-loss`) and automated benchmark performance suites for sub-100ms conversions.
