# Strategy & Specification: Headless Document Conversion CLI Binaries

**Author**: Strategist Agent  
**Date**: September 2026  
**Status**: Proposal / Strategic Alignment  
**Target Milestone**: Q4 2026 / Q1 2027  

---

## 1. Executive Summary

As `gtk-office-suite` matures its core document engines (`suite-common-core`, `letters-core`, `tables-core`, `decks-core`), there is a high-value ecosystem opportunity to expose GTK-free CLI binaries for document transformation. 

Currently, server-side applications and automated build pipelines rely on heavyweight solutions such as headless LibreOffice (`soffice --headless`) or complex cloud APIs. Exposing lightweight CLI conversion binaries built directly on Rust core crates provides high performance, minimal memory footprints, container friendliness, and seamless scriptability for enterprise productivity workflows.

---

## 2. Strategic Objectives & Value Proposition

1. **Leverage GTK-Free Architecture**:
   `AGENTS.md` and repository standards strictly enforce that business logic must reside in non-GTK core crates (`suite-common-core` and core modules). Headless CLI tools validate and enforce this architectural separation.
2. **Ecosystem & CI/CD Integration**:
   Enables automated batch conversions in Linux enterprise fleet pipelines, serverless functions, document static site generators, and cloud publishing workflows.
3. **Sub-second Batch Execution**:
   Compared to `soffice --headless` (which incurs multi-second startup overhead and large memory allocations), Rust-native conversion binaries start instantaneously and run with minimal overhead.
4. **Deterministic Testing Surface**:
   Headless CLI tools allow headless integration testing of file format round-tripping against the `PARITY.md` test corpus without launching an X11/Wayland display server or GTK runtime.

---

## 3. Architecture & Binary Targets

The proposal introduces a lightweight CLI crate (`suite-convert`) or target binaries:

```
suite-convert/
├── Cargo.toml
└── src/
    ├── main.rs            # Unified CLI parsing (clap/argh)
    ├── letters.rs         # Wiring to letters-core importers/exporters
    ├── tables.rs          # Wiring to tables-core formatters/calculators
    └── decks.rs           # Wiring to decks-core slide parsers
```

### Planned Tool Invocation Interface

```bash
# Convert Markdown to ODT via letters-core
letters-convert input.md --to odt --output document.odt

# Convert CSV to OpenFormula XLSX via tables-core
tables-convert data.csv --to xlsx --output data.xlsx

# Export ODP presentation slides to PDF via decks-core
decks-convert slides.odp --to pdf --output presentation.pdf
```

---

## 4. Implementation Roadmap & Phases

| Phase | Horizon | Deliverables | Success Criteria |
|-------|---------|--------------|------------------|
| **Phase 1: Workspace & Common CLI Framework** | Q4 2026 (Early) | Create `suite-convert` crate, define CLI argument parser, setup error reporting. | Executable crate builds in workspace without GTK dependencies. |
| **Phase 2: Letters Core Integration** | Q4 2026 (Mid) | Wire Markdown / ODT / PDF export engines from `letters-core`. | Round-trip conversions pass against `LO-Letters` corpus. |
| **Phase 3: Tables & Decks Integration** | Q4 2026 (Late) | Wire CSV / OpenFormula / ODP parsing engines. | `tables-convert` and `decks-convert` produce valid documents. |
| **Phase 4: Distribution & Flatpak Integration** | Q1 2027 | Package binaries into Flatpak bundles and enterprise fleet scripts. | Command line tools accessible inside user environment. |

---

## 5. Non-Goals

- GUI or GTK runtime initialization inside conversion binaries.
- Complex interactive editing features.
- Dynamic plugin execution during basic conversions (plugin host hooks reserved for full desktop app runtime).

---

*Related Issue*: #529
