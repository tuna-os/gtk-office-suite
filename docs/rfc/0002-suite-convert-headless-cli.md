# RFC 0002 — `suite-convert` Headless Document Conversion Binary Architecture

**Status**: Draft  
**Tracking Issue**: #579  
**Target Milestone**: Q4 2026  

---

## 1. Context & Rationale

`gtk-office-suite` (Letters, Tables, Decks) currently relies on GTK4/libadwaita applications for document editing, rendering, and file export. However, enterprise workflows, CI/CD pipelines, and cloud document automation require a lightweight, headless command-line binary to convert documents between formats (ODT, OOXML, PDF, CSV, CommonMark, OpenFormula) without launching a GUI or requiring an X11/Wayland display server (`Xvfb`).

The `suite-convert` CLI binary addresses issue #579 by providing a headless batch conversion utility powered directly by `suite-common-core`, `letters-core`, `tables-core`, and `decks-core`.

---

## 2. Goals & Key Requirements

1. **GTK-Free Execution**: Must link only against pure Rust core crates (`suite-common-core` and core domain models). Zero dependency on GTK, GDK, libadwaita, or display servers.
2. **Deterministic Batch Processing**: Fast, reproducible conversion with explicit error handling and exit codes (0 = success, 1 = conversion error, 2 = format loss budget exceeded, 3 = I/O or permission failure).
3. **Format Parity & Loss Diagnostics**: Integrated format loss reporting (`--report-loss`) outputting structured JSON metrics for unsupported elements.
4. **Piping & Standard Streams**: Support for `stdin` / `stdout` streaming where applicable (e.g. Markdown to HTML/ODT, CSV to ODS).
5. **Resource Ceilings**: Strict memory allocation limits and timeout safeguards to prevent memory exhaustion during batch server processing.

---

## 3. Command-Line Specification

```bash
# Basic file conversion
suite-convert --to pdf report.odt output.pdf

# Convert spreadsheet to CSV with explicit loss checking
suite-convert --to csv --check-loss data.xlsx output.csv

# Stream input from stdin to stdout
cat document.md | suite-convert --from markdown --to odt > document.odt

# Output JSON conversion loss report
suite-convert --to docx draft.odt --report-loss loss-metrics.json
```

### Supported Format Matrix

| Input Format | Supported Output Formats | Core Engine |
|--------------|-------------------------|-------------|
| ODT / DOCX / MD | PDF, ODT, DOCX, CommonMark, HTML, TXT | `letters-core` |
| ODS / XLSX / CSV | ODS, XLSX, CSV, TSV, PDF | `tables-core` |
| ODP / PPTX | ODP, PPTX, PDF, PNG (slide images) | `decks-core` |

---

## 4. Architecture & Crate Structure

```
                             [ suite-convert (CLI binary) ]
                                           │
         ┌─────────────────────────────────┼─────────────────────────────────┐
         ▼                                 ▼                                 ▼
[ letters-core ]                   [ tables-core ]                   [ decks-core ]
         │                                 │                                 │
         └─────────────────────────────────┼─────────────────────────────────┘
                                           ▼
                                 [ suite-common-core ]
```

### Core Decoupling Strategy
- All document parsing, document model state, and serialization engines reside in `*-core` crates.
- `suite-convert` handles CLI arg parsing (via `clap`), input validation, progress reporting, and output stream routing.
- No GTK widgets or GSettings schemas are initialized during conversion.

---

## 5. Security & Sandbox Constraints

- **Flatpak Compatibility**: Ship `suite-convert` as an embedded command within the Flatpak bundle (`org.tuna_os.Office.Convert`) or standalone package.
- **Malformed Input Protection**: Integrate malformed document fuzz testing to ensure invalid or corrupt input files return clean error codes without panicking or hanging.

---

## 6. Verification & Test Plan

1. **Unit Tests**: Test CLI parsing and format conversion round-trips in `suite-convert` unit tests (`cargo test -p suite-convert`).
2. **Headless Integration Tests**: CI pipeline step in `ci.yml` verifying `suite-convert` execution without `Xvfb`.
3. **Format Parity Ledger**: Audit loss reporting against existing format parity benchmarks (`docs/PARITY.md`).

---

## 7. Next Steps

1. Create initial crate structure `crates/suite-convert` in workspace.
2. Implement basic ODT/DOCX/PDF CLI pipeline.
3. Wire format loss budget reporter and integration tests.
