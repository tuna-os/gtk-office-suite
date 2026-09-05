# ADR-0007: Suite-wide print architecture and ISO PDF/A archiving strategy

Date: 2026-09-05 · Status: proposed

## Context

The office suite components (`Letters`, `Tables`, and `Decks`) require robust printing and archival export capabilities to serve enterprise, institutional, and legal use cases. Modern Linux desktop environments interface with printing subsystems via `GtkPrintOperation` and CUPS, while digital record retention mandates compliance with ISO 19005 (PDF/A-1b and PDF/A-2b standards).

Previously, print and PDF export paths lacked a unified architecture across the suite:
- Print logic risked accumulating inside widget-level shell code, violating the core/shell architectural boundary (ADR-0001).
- PDF generation varied across formats without standardized metadata validation or archival conformance profiles.
- Headless testing of pagination and print layout lacked deterministic test fixtures.

## Decision

1. **GTK-Free Pagination and Layout in Core Crates**:
   - `letters-core`, `tables-core`, and `decks-core` shall define deterministic pagination engines implementing a shared `PaginatedDocument` trait.
   - Page geometry, margins, header/footer zones, column breaks, and page break algorithms remain entirely GTK-free and unit-testable.
   - Core crates produce vector draw command sequences or headless Cairo surface layouts decoupled from display scales.

2. **Unified `SuitePrintOperation` in `suite-common`**:
   - `suite-common` provides `SuitePrintOperation` wrapping `gtk::PrintOperation` and `adw::PrintDialog`.
   - Manages asynchronous print job lifecycle, custom print options (page ranges, duplex, paper orientation), print preview rendering, and system printer communication.
   - Connects to core pagination interfaces via thin bridge adapters.

3. **Standardized ISO PDF/A Archival Export**:
   - Standardize PDF export pipelines on PDF/A-1b and PDF/A-2b compliance:
     - Enforce embedded fonts and complete font subsetting.
     - Mandate sRGB / standard ICC output intent color profiles.
     - Embed standardized XMP metadata (`pdfaid:part`, `pdfaid:conformance`, document title, author, creation timestamp).
     - Prohibit device-dependent color spaces without color profiles, embedded Javascript, and encrypted streams in archival mode.
   - Provide automated verification harnesses using headless CLI oracle tools (`verapdf`, `pdfinfo`, `cairosvg`).

## Consequences

- **Architecture Integrity**: Widget layers (`window.rs`) only instantiate `SuitePrintOperation` and pass document handles; pagination algorithms and vector layout remain pure Rust in core crates.
- **Enterprise & Regulatory Readiness**: Documents exported under the PDF/A profile satisfy long-term document retention requirements for legal and government environments.
- **Testing & Verification**: Print pagination and PDF/A metadata validation can be tested in headless CI environments without requiring physical printer hardware or GUI display servers.
