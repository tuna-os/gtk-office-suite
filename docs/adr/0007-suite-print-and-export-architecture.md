# ADR-0007: Unified Suite-Wide Print Architecture and Typst PDF Export Pipeline

Date: 2026-09-04 · Status: accepted

## Context

Desktop productivity applications require high-fidelity printing and deterministic document export (specifically PDF). Historically, the suite had fragmented export and print capabilities:
1. **Letters** relied on ad-hoc regex/string substitutions over HTML markup to generate Typst source, dropping headers, footers, margins, and custom page geometry.
2. **Tables** lacked direct export from the IronCalc grid model to printable paginated sheets.
3. **Decks** implemented slide-to-Typst serialization in `decks/src/export.rs`, but the code remained disconnected from GTK action handlers and keyboard shortcuts.
4. **Print UI Integration**: None of the applications implemented GNOME-standard `GtkPrintOperation` lifecycles or Flatpak printing portal (`org.freedesktop.portal.Print`) integration.

## Decision

1. **Shared Page Geometry & Configuration in `suite-common-core::print`**:
   - `PageSetup`, `PageSize`, `Orientation`, and margin definitions in `suite-common-core::print` act as the authoritative data model for print dimensions, visual rulers, and page breaks.
   - All document models (Letters document pages, Tables print ranges/sheet tiling, Decks slide aspect ratios) map into standard `PageSetup` structs before export or rendering.

2. **Unified In-Process Typst Compilation via `suite-export`**:
   - PDF compilation across all applications is standardized on `suite_export::compile_pdf` and `suite_export::compile_pdf_to_file` powered by `typst-as-lib`.
   - External CLI invocations (`typst compile`) are prohibited to maintain zero-runtime dependency guarantees inside Flatpak sandboxes.
   - Domain-specific model-to-Typst serializations (`to_typst`) reside in their respective `-core` crates and are verified with deterministic headless unit tests.

3. **GNOME-Native Print Dialogs & `GtkPrintOperation`**:
   - User-facing print flows utilize GTK4's `GtkPrintOperation` connected to libadwaita windows.
   - When running in a sandboxed Flatpak environment, print operations dispatch transparently through XDG Desktop Portals (`org.freedesktop.portal.Print`).
   - Standard actions `app.print` (`Ctrl+P`) and `app.export-pdf` (`Ctrl+Shift+E` / command palette) are registered in each app's GioAction registry and exposed via the Ctrl+K palette.

4. **Automated Conformance & Visual Regression Gate**:
   - The headless test harness verifies that all model-to-Typst generators produce valid, non-empty PDFs matching expected page counts and font allocations.
   - Headless oracle tests validate that generated PDFs render correctly under standard PDF rendering engines (`pdftoppm` / `poppler-utils`).

## Consequences

- **Pros**:
  - Predictable, uniform export behavior and print shortcuts across Letters, Tables, and Decks.
  - Zero external process spawning inside sandboxed Flatpak containers.
  - High-fidelity PDF output matching LibreOffice visual baselines.
- **Cons**:
  - Requires maintaining Typst markup generation templates in `-core` crates as document features evolve (e.g. nested tables, vector shapes).
