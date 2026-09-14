# Headless CLI Document Conversion Binary (`suite-convert`) Specification

**Target Release**: Q4 2026 | **Tracking Issue**: #662

## Overview

`suite-convert` is a headless command-line document conversion utility for `gtk-office-suite`. It leverages `suite-common-core` format engine backends (Letters, Tables, Decks export/import engines) to convert office documents without launching a GTK main loop or requiring an X11/Wayland display server.

---

## Architecture & Design Goals

1. **GTK-Free Headless Processing**: Document parsing, conversion, and rendering pipeline execute entirely on non-GUI worker threads, isolating UI display logic.
2. **Deterministic Output**: Converts input formats (ODT, DOCX, CommonMark, ODS, XLSX, ODP, PPTX) to standard exchange formats (PDF, HTML, Plain Text, CSV) with identical results across server environments.
3. **Format Compatibility Loss Budgeting**: Calculates loss budgets and outputs structured JSON audit reports when converting between formats with feature asymmetry.
4. **Fast Execution & Low Footprint**: Operates with minimal memory overhead suitable for batch scripts, server side conversion pipelines, and CI validation.

---

## CLI Interface Specification

```bash
suite-convert [OPTIONS] --input <INPUT_FILE> --output <OUTPUT_FILE>
```

### Options

| Parameter | Short | Type | Description |
|---|---|---|---|
| `--input <PATH>` | `-i` | Path | Source document path (`.odt`, `.docx`, `.ods`, `.xlsx`, `.odp`, `.pptx`, `.md`) |
| `--output <PATH>` | `-o` | Path | Destination document path (`.pdf`, `.html`, `.txt`, `.csv`) |
| `--format <FMT>` | `-f` | String | Explicit target format override (`pdf`, `html`, `txt`, `csv`) |
| `--loss-budget <PATH>` | `-l` | Path | Optional file path to output structured JSON loss budget report |
| `--strict` | `-s` | Flag | Abort conversion with error code if format feature loss exceeds threshold |
| `--quiet` | `-q` | Flag | Suppress standard output logs, outputting only errors |

---

## Exit Codes

- `0`: Success (conversion completed within acceptable loss budget)
- `1`: Invalid arguments or input file not found
- `2`: Document parsing / deserialization failure
- `3`: Conversion engine failure
- `4`: Strict mode violation (unsupported features lost exceeding threshold)

---

## Integration Plan & Verification

1. Unit tests in `suite-common-core` covering format conversion without GTK runtime dependencies.
2. Integration smoke tests validating batch conversion across sample fixture documents in `tests/test_suite_convert.py`.
3. Performance benchmarks tracking conversion latency and peak resident memory.
