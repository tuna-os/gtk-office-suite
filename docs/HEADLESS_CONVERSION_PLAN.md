# Headless Document Conversion CLI (`suite-convert`) Planning Specification

This document details the architectural specification and implementation roadmap for `suite-convert`, a headless CLI tool for high-performance batch document format conversion across GTK Office Suite formats (CommonMark, ODT, ODP, PDF, OpenFormula).

## Executive Summary

To support cloud services, CI/CD document validation pipelines, and enterprise automation without requiring X11/Wayland dependencies or GTK display contexts, `suite-convert` provides a headless command-line entry point leveraging pure Rust core parsing and rendering modules (`suite-common-core`, `letters-core`, `tables-core`, `decks-core`).

---

## Architectural Principles

1. **Zero GTK Dependencies**: `suite-convert` depends strictly on core non-UI crates.
2. **Deterministic Output**: Conversions produce consistent document AST and binary encodings.
3. **Batch Processing Capabilities**: Supports directory traversals and parallel conversion pipelines.
4. **Rich Interop Error Reporting**: Detailed error diagnostics for unsupported format elements.

---

## Command Line Interface Specification

```bash
suite-convert [OPTIONS] --input <INPUT_FILE> --output <OUTPUT_FILE>
```

### Options

- `-i, --input <PATH>`: Path to input document (`.md`, `.odt`, `.odp`, `.ots`, etc.)
- `-o, --output <PATH>`: Path to output document (`.pdf`, `.html`, `.odt`, `.md`)
- `-f, --from <FORMAT>`: Override input format detection (e.g. `odt`, `markdown`, `openformula`)
- `-t, --to <FORMAT>`: Target conversion format (e.g. `pdf`, `odt`, `markdown`, `json-ast`)
- `--strict`: Fail conversion on lossy feature drops
- `--quiet`: Suppress non-error diagnostics

---

## Core Component Integration

```
                 ┌──────────────────┐
                 │  suite-convert   │
                 └────────┬─────────┘
                          │
         ┌────────────────┼────────────────┐
         ▼                ▼                ▼
 ┌───────────────┐┌───────────────┐┌───────────────┐
 │ letters-core  ││  tables-core  ││  decks-core   │
 └───────┬───────┘└───────┬───────┘└───────┬───────┘
         │                │                │
         └────────────────┼────────────────┘
                          ▼
              ┌───────────────────────┐
              │   suite-common-core   │
              └───────────────────────┘
```

---

## Phased Rollout Plan

- **Phase 1**: CommonMark <-> ODT format conversion via `letters-core`.
- **Phase 2**: OpenFormula / OTS evaluation & export via `tables-core`.
- **Phase 3**: ODP presentation rendering engine integration via `decks-core`.
- **Phase 4**: Direct PDF/A archival export streaming.
