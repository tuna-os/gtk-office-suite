# RFC-0002: WASM and IPC extension architecture for GTK Office Suite

Date: 2026-09-12 · Status: **draft, not accepted** · Tracks: [#701](https://github.com/tuna-os/gtk-office-suite/issues/701)

## Summary

This RFC outlines the architectural blueprint for third-party extensibility and plugin support across **Letters**, **Tables**, and **Decks**. As GTK Office Suite reaches daily-driver maturity post-v1.0, enabling domain-specific automation, custom formula functions, and non-standard format converters is vital for broader desktop and enterprise adoption.

This document proposes a dual-tier extension engine:
1. **In-process sandboxed WASM runtime** for data transformers, formula extensions, and document validators (GTK-free, safe, sub-millisecond invocation).
2. **Out-of-process IPC host** via D-Bus / Varlink for complex extensions requiring native desktop capabilities (external UI windows, network integrations, system services).

---

## Why extensibility belongs in planning now

The current execution plan remains centered on [Roadmap to dependable daily use](../readiness-2026-09/README.md) ([#443](https://github.com/tuna-os/gtk-office-suite/issues/443)). Core stability, crash safety, and ODF/OOXML parity remain top priorities. 

However, architecture decisions made in `suite-common-core` and application shells today must avoid locking out extension boundaries. Establishing a clean plugin host contract prevents future core re-architecting when ecosystem plugins are introduced.

---

## Technical Constraints & Design Principles

1. **No GTK in core WASM contracts**: Extension hooks that operate on data models (e.g., custom spreadsheet functions or text filters) must run strictly against pure Rust data models in `suite-common-core` without GTK or libadwaita dependencies (ADR-0001).
2. **Memory Safety & Sandbox Isolation**: Untrusted third-party plugins must run inside a memory-safe WebAssembly sandbox (`wasmi` or `wasmtime` with memory budgets) so a bug or crash in a plugin cannot crash the main editor process.
3. **Flatpak Sandbox Compatibility**: Extension manifests and binaries must respect Flatpak isolation. External IPC processes must communicate over explicit D-Bus sockets configured in the Flatpak manifest (`--talk-name=...` or session bus filters).
4. **Atomic Save Safety**: Extension hooks triggered on document save (`document:pre_save` / `document:post_save`) must operate strictly within the `suite-common-core::atomic_save` transactional pipeline ([#437](https://github.com/tuna-os/gtk-office-suite/issues/437)) without corrupting primary ODF/OOXML files.

---

## Proposed Architecture

### Tier 1: In-Process Sandboxed WASM Plugins

For high-performance, synchronous operations on document models:
- **Runtime**: WebAssembly Component Model target (`wasm32-wasip1` / WIT interface definitions).
- **Core Host Crate**: `suite-plugin-core` (GTK-free).
- **Capability Boundaries**:
  - `formula:register_func(name, fn)` for Tables custom spreadsheet formulas.
  - `transform:filter_text(buffer)` for Letters document formatting or text transformations.
  - `export:custom_exporter(doc, format)` for custom document serializations.

```
+-------------------------------------------------------+
|                GTK Application Shell                  |
|          (Letters / Tables / Decks Window)           |
+-------------------------------------------------------+
                           |
                           v
+-------------------------------------------------------+
|                 suite-plugin-core                     |
|           WASM Sandbox & Capability Host             |
+-------------------------------------------------------+
  |                   |                   |
  v                   v                   v
[Plugin A: WASM]   [Plugin B: WASM]   [Plugin C: WASM]
 (Pure Logic)       (Text Filter)      (Formula Pack)
```

### Tier 2: Out-of-Process D-Bus IPC Extensions

For plugins requiring native UI dialogs, network sync, or system service access:
- **Protocol**: D-Bus interface `org.gnome.GtkOfficeSuite.Extension` / Varlink IPC.
- **Process Isolation**: Separate process invocation managed by Flatpak portal or systemd transient scopes.
- **Capabilities**:
  - Asynchronous background task execution.
  - Integration with external cloud backends or localized database connectors.

---

## Phased Implementation Roadmap

| Phase | Horizon | Focus Areas | Primary Deliverables |
|---|---|---|---|
| **Phase 1** | Q4 2026 | Interface Definitions (WIT) | WIT contracts for `suite-common-core`, WASM component model validation |
| **Phase 2** | Q1 2027 | Sandboxed WASM Host | `suite-plugin-core` runtime, security isolation & memory limits |
| **Phase 3** | Q2 2027 | App Shell Hooks | Integration of custom formulas in Tables & text filters in Letters |
| **Phase 4** | Q3 2027 | IPC Extension Host & Portal | D-Bus extension manager & Flatpak manifest portal extensions |

---

## Open Questions & Risk Mitigation

1. **WASM Runtime Overhead**: `wasmi` provides zero-JIT security suitable for Flatpaks on embedded or minimal environments, while `wasmtime` provides JIT compilation speed. Benchmarks on startup time and memory footprint will settle the engine selection.
2. **UI Extension Points**: GTK4 libadwaita UI components (e.g. adding custom header bar buttons or sidebar panels) require a declarative JSON/YAML action definition schema so plugins cannot inject arbitrary C/GTK pointers.
