# ADR-0007: Suite-wide print architecture and PDF/A archiving strategy

Date: 2026-09-05 · Status: proposed

## Context

Linux desktop and enterprise deployments require high-fidelity printing and long-term archival document exports. Currently, Letters, Tables, and Decks have fragmented export and pagination approaches:
- Letters relies on Cairo canvas layouts with Typst pipelines for PDF export.
- Tables renders sheet grids directly to screen Cairo contexts with partial multipage splitting logic.
- Decks renders slides individually without unified page setup and printer dialog abstractions.

To achieve enterprise and public-sector readiness, the office suite requires:
1. Native GTK4/Libadwaita `GtkPrintOperation` integration with CUPS printer discovery, page-setup persistence, and background spooling.
2. Compliance with ISO 19005 (PDF/A-1b, PDF/A-2b, and PDF/A-3) standards for digital archiving, including embedded fonts, color device profiles, and standardized metadata.
3. Decoupling of document pagination and print rendering logic from widget code into core crates.

## Decision

1. **Print Driver Abstraction in `suite-common`**:
   - Introduce a shared `SuitePrintOperation` wrapper around `gtk4::PrintOperation` to standardize print settings, page ranges, duplexing, and margins.
   - Decouple print preparation (pagination, layout calculations) into pure models within `letters-core`, `tables-core`, and `decks-core`.

2. **PDF/A Compliance Engine**:
   - Enforce PDF/A-2b as the primary archival target for export pipelines.
   - Require full font embedding and subsetting (TrueType/OpenType) via FreeType/Cairo or Typst backend engines.
   - Embed standardized XMP metadata (title, author, creation date, PDF/A conformance identifier) and sRGB ICC color profiles.

3. **Application-Specific Printing Pipelines**:
   - **Letters**: Model-driven pagination calculating line breaking, header/footer zones, page numbers, and footnote placement prior to paint operations.
   - **Tables**: Print area boundaries, grid line toggling, page breaks across sparse columns/rows, and repeat header rows across pages.
   - **Decks**: Slide-to-page scaling, handout layouts (1, 2, 3, 4, 6, 9 slides per page), and speaker notes print modes.

4. **Testing and Verification**:
   - Add unit tests in core crates verifying pagination calculation invariants.
   - Introduce automated headless PDF validation tests using `pdfinfo` and `qpdf` / `verapdf` to assert conformance markers and font embedding.

## Consequences

- **Strengths**: Unlocks public-sector, legal, and enterprise compliance requirements for document preservation; guarantees consistent printing dialogs across all three suite applications.
- **Constraints**: PDF/A export paths must validate that external raster images contain valid color spaces and reject un-embeddable proprietary font references.
