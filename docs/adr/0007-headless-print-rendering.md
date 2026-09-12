# ADR 0007: Headless PDF/ODF document rendering & print preview architecture

## Status

Proposed.

## Context

GTK Office Suite (Letters, Tables, Decks) currently executes document rendering and print preview generation directly coupled to GTK4 main looper widgets (`GtkDrawingArea`, `GtkPrintOperation`). While suitable for interactive desktop editing, this direct widget dependency creates significant bottlenecks:

1. **GUI Main Thread Blocking**: Rendering complex multi-page documents or spreadsheets for print previews locks the GTK main event loop.
2. **Headless & CI Limitations**: Visual testing, thumbnail generation, and batch export operations currently require running a full GTK/Xvfb display environment.
3. **Enterprise Batch Pipelines**: Automated workflows (e.g. headless conversion to PDF/ODF or background print generation) cannot run efficiently as pure CLI processes.

## Decision

We decouple document layout calculation, vector rendering, and print preview serialization into GTK-free modules within `suite-common-core`:

1. **GTK-Free Rendering Core (`suite-common-core::render`)**:
   - Define abstract traits for page layout, text placement (via PangoCairo/Pango font handling decoupled from GTK widgets), and canvas drawing primitives.
   - All document layout calculations (Letters pagination, Tables grid slicing, Decks slide geometry) must execute in pure Rust, generating serializable `RenderTree` nodes.

2. **Headless Pipeline & Target Exporters**:
   - Provide direct rendering backends for Cairo surface outputs (`cairo::PdfSurface`, `cairo::PsSurface`, `cairo::SvgSurface`, PNG memory buffers) without instantiating any GTK `GtkWidget`.
   - Enable `suite-common-core` to render pages, slides, and sheets headlessly for CLI batch conversions and automated visual regression checks.

3. **Asynchronous GTK Print Operation Integration**:
   - Desktop applications (`SuiteWindow`) interface with `GtkPrintOperation` using background worker threads that consume `suite-common-core::render` outputs.
   - Print previews render asynchronously off the GTK main thread to maintain smooth UI responsiveness during multi-page print jobs.

## Consequences

- **Pros**:
  - Unblocks headless document printing, PDF export, and thumbnail generation in server/CLI environments without X11/Wayland dependencies.
  - Improves desktop application responsiveness by moving CPU-heavy layout and vector rendering off the GTK main thread.
  - Enhances testability by enabling fast, deterministic visual snapshot tests in pure Rust CI suites.
- **Cons**:
  - Requires migrating existing app-specific drawing logic in `window.rs` / custom widgets to use `suite-common-core` layout primitives.
