# suite-convert: Headless CLI Document Conversion & Batch Processing Architecture

## Overview

`suite-convert` is a planned pure-Rust command-line tool and library crate designed for GTK-free document format conversion, batch rendering, and document inspection across Letters, Tables, and Decks documents.

By separating document parsing, AST manipulation, and format export into `suite-common-core` (and document-specific core crates), `suite-convert` enables fast, headless operations without needing a display server (`Xvfb`, `Wayland`), GTK runtime initialization, or GUI dependencies.

---

## Core Objectives

1. **GTK-Free Execution**: Zero GTK/libadwaita/Cairo dependencies required for CLI invocation.
2. **Multi-Format Interoperability**:
   - **Letters**: ODT, Markdown (CommonMark), Plain Text, HTML export, PDF rendering.
   - **Tables**: ODS, CSV, TSV, OpenFormula evaluation, XLSX/HTML summary export.
   - **Decks**: ODP, PDF slide export, Markdown slide decks.
3. **High-Performance Batch Processing**: Multi-threaded document rendering and metadata extraction.
4. **CI & Automation Integration**: Headless conversion verification in CI without display requirements.

---

## Architecture & Crate Structure

```
                  ┌──────────────────────┐
                  │    suite-convert     │ (Pure CLI Binary)
                  └──────────┬───────────┘
                             │
     ┌───────────────────────┼───────────────────────┐
     ▼                       ▼                       ▼
┌──────────────┐      ┌──────────────┐      ┌──────────────┐
│ letters-core │      │  tables-core │      │  decks-core  │
└──────┬───────┘      └──────┬───────┘      └──────┬───────┘
       │                     │                     │
       └─────────────────────┼─────────────────────┘
                             ▼
                  ┌──────────────────────┐
                  │  suite-common-core   │ (Formats, AST, OpenFormula)
                  └──────────────────────┘
```

### Module Responsibilities

- **`suite-common-core`**: Defines baseline `Document`, `Span`, `Block`, `Formula`, and `Style` primitives, AST parsers, and serialization logic.
- **`letters-core` / `tables-core` / `decks-core`**: Contain domain-specific AST transforms, pagination algorithms, formula recalculation engines, and format codecs.
- **`suite-convert`**: Exposes argument parsing (via `clap`), batch file walking, and output stream formatting.

---

## CLI Interface Specification

### Command Signature

```bash
suite-convert [OPTIONS] <INPUT_PATH> --to <FORMAT> [--output <OUTPUT_PATH>]
```

### Options

| Flag | Description |
|---|---|
| `-t, --to <FORMAT>` | Target format (`pdf`, `odt`, `ods`, `odp`, `md`, `html`, `txt`, `csv`) |
| `-o, --output <PATH>` | Output file path or directory (defaults to stdout or input basename) |
| `-b, --batch` | Enable batch directory conversion |
| `-j, --threads <N>` | Concurrent worker thread count (defaults to CPU cores) |
| `--extract-metadata` | Emit document properties and statistics as JSON |
| `--evaluate-formulas` | Force full re-evaluation of OpenFormula expressions (Tables) |
| `-v, --verbose` | Verbose execution logging |

---

## Implementation Phases

### Phase 1: CLI Shell & Text/Markdown Codecs
- Crate initialization under `crates/suite-convert` or top-level binary.
- Basic CLI structure using `clap`.
- Support Markdown and Plain Text conversions leveraging `suite-common-core`.

### Phase 2: ODF Importers/Exporters Integration
- Integration of ODT, ODS, and ODP parsers from core crates.
- Fast stream conversion to HTML and Markdown.

### Phase 3: Headless PDF & Vector Rendering
- Headless PDF generation via `pdf-writer` or pure-Rust rendering backends without Cairo/GTK widget binding.

### Phase 4: CI & Enterprise Integration
- Integration into CI test harnesses.
- Performance benchmarking and ratchet checks against LibreOffice conversion output.
