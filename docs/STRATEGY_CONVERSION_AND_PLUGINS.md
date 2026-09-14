# Headless Document Conversion & WASM/IPC Plugin Architecture Strategy

This document outlines the strategic roadmap and architecture specification for introducing headless CLI document conversion tools (`suite-convert`) and a memory-safe plugin extensibility model across GTK Office Suite apps (**Letters**, **Tables**, **Decks**).

---

## 1. Overview & Objectives

GTK Office Suite targets Linux desktop productivity powered by Rust, GTK4, and libadwaita. As adoption scales in enterprise and developer workflows, two critical strategic gaps exist:

1. **Headless & Server Automation**: Users and server systems need to perform document format conversions (ODT/ODP/ODS, PDF, DOCX, CSV) and rendering without launching a graphical GTK session or running an X11/Wayland display server.
2. **Safe Extensibility**: Enterprise users and third-party developers require a plugin system to add custom filters, macro utilities, document transformers, and export targets without risking core application stability or memory safety.

---

## 2. Headless CLI Conversion Architecture (`suite-convert`)

### 2.1 Crate Architecture

To maintain compliance with the architecture rule ("No business logic in widget code"), conversion drivers reside exclusively in pure Rust crates (`suite-common-core`, `letters-core`, `tables-core`, `decks-core`).

```
                              ┌────────────────────────┐
                              │  suite-convert (CLI)   │
                              └───────────┬────────────┘
                                          │
                  ┌───────────────────────┼───────────────────────┐
                  ▼                       ▼                       ▼
      ┌───────────────────────┐ ┌───────────────────┐ ┌───────────────────────┐
      │  letters-core (ODT)   │ │ tables-core (ODS) │ │   decks-core (ODP)    │
      └───────────┬───────────┘ └─────────┬─────────┘ └───────────┬───────────┘
                  │                       │                       │
                  └───────────────────────┼───────────────────────┘
                                          ▼
                              ┌────────────────────────┐
                              │   suite-common-core    │
                              └────────────────────────┘
```

### 2.2 CLI Command Interface

The `suite-convert` binary provides a unified CLI interface:

```bash
suite-convert --input document.odt --output document.pdf --format pdf
suite-convert --input spreadsheet.ods --output data.csv --format csv --sheet "Sheet1"
suite-convert --input presentation.odp --output slides/ --format png-pages --dpi 300
```

Key Requirements:
- Zero GTK4/GDK/Cairo X11/Wayland display dependencies.
- Exit code standardisation: `0` (Success), `1` (Format Error), `2` (I/O Error), `3` (Conversion Loss Limit Exceeded).
- Integration with loss-budget reporting flags (`--fail-on-loss-budget`).

---

## 3. WASM / IPC Plugin Architecture

### 3.1 Plugin Sandbox Model

Extensibility must protect document state from plugin crashes or malicious memory access. Two isolation models are defined:

1. **WebAssembly (WASM) Sandbox**:
   - Executes in-process via `wasmtime` runtime.
   - Restricted to explicit Host Function bindings (read document AST, emit transformed AST).
   - Fast invocation overhead for inline document filters and syntax highlighters.

2. **IPC Out-of-Process Sandbox**:
   - Executes external binaries in an isolated child process or Flatpak sandbox sub-container.
   - Communicates via JSON-RPC 2.0 / Varlink over Unix domain sockets.
   - Suitable for heavy external tools (e.g. language servers, specialized format parsers).

### 3.2 Plugin Capability Matrix

| Capability | WASM Sandbox | IPC Sandbox | GTK UI Integration |
|---|---|---|---|
| Document AST Transformation | Allowed | Allowed | N/A (Core layer) |
| File System Access | Restricted / Virtualized | Explicit permissions | N/A |
| Network Socket Access | Disabled by default | Permission-gated | N/A |
| Custom UI Actions / Menus | Exposed via Host API | Exposed via Host API | Registered in `suite-common` |

---

## 4. Phased Implementation Roadmap

### Phase 1: Core Extensibility & Headless Parity (Q4 2026)
- Expose document format serializer/deserializer APIs in `suite-common-core`.
- Create `suite-convert` CLI binary crate supporting ODT->PDF, ODS->CSV, ODP->PDF conversions.
- Add headless conversion integration tests to CI matrix.

### Phase 2: WASM Plugin Runtime Prototype (Q1 2027)
- Integrate `wasmtime` engine into `suite-common-core`.
- Define initial WASM Host ABI (`suite_plugin_v1`) for document filtering.
- Implement sample WASM plugins (e.g. markdown export filter, spellcheck transformer).

### Phase 3: IPC Sandbox & UI Extension Registry (Q2 2027)
- Implement `suite-common` plugin manager UI and GSettings schema for plugin authorization.
- Support out-of-process IPC plugin host protocol over Unix domain sockets.
- Ship developer documentation and SDK templates for Rust/WASM plugins.
