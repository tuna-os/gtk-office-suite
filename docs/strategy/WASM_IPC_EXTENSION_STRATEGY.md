# GTK Office Suite WASM & IPC Extension Architecture Strategy

## Overview

As `gtk-office-suite` matures post-v1.0 across Letters, Tables, and Decks, third-party extensibility must be introduced without compromising safety, UI performance, or GTK boundary principles (zero business logic in GUI code).

This strategy defines a decoupled extension architecture leveraging WebAssembly (WASM) for lightweight in-process plugin sandboxing and JSON-RPC over IPC sidecars for heavy system integrations.

---

## Extension Paradigms

```
+------------------------------------------------------------------+
|                     GTK Office Suite Core                        |
|  (Letters Core / Tables Core / Decks Core / suite-common-core)   |
+------------------------------------------------------------------+
                                  |
            +---------------------+---------------------+
            |                                           |
            v                                           v
  +-------------------+                       +-------------------+
  |  WASM Sandboxes   |                       |   IPC Sidecars    |
  | (Wasmtime Runtime)|                       |  (JSON-RPC 2.0)   |
  +-------------------+                       +-------------------+
  | - Custom Macros   |                       | - Heavy ML / AI   |
  | - Format Filters  |                       | - Database sync   |
  | - Document Lint   |                       | - Cloud connectors|
  +-------------------+                       +-------------------+
```

### 1. WebAssembly (WASM) Plugins
- **Target Use Cases**: Custom document format parsers, text linting/grammar rules, spreadsheet macro functions, and custom document templates.
- **Runtime Sandbox**: Integrated `wasmtime` engine restricted to deterministic memory access. No direct filesystem or network socket access allowed without explicit host capability grants.
- **IPC Protocol**: Memory-mapped binary buffers passing flat memory representations or JSON structures.

### 2. Out-of-Process (IPC) Sidecars
- **Target Use Cases**: Local AI/LLM integration, external database connectors, complex sync engines, and hardware device interfaces.
- **Communication Protocol**: Standardized JSON-RPC 2.0 over Unix domain sockets.
- **Process Isolation**: External binaries execution subject to desktop Flatpak portal permissions (`org.freedesktop.portal.Flatpak`).

---

## Governance & Safety Rules

1. **Strict Core Separation**: Extension APIs map exclusively to `suite-common-core` or application core crates. Extensions NEVER access GTK widget hierarchy or windowing state directly.
2. **Deterministic Capability Gating**: Permissions (network access, local storage, external process invocation) must be defined in plugin manifests (`manifest.json`) and prompted explicitly via Flatpak portal interfaces.
3. **Crashing Isolation**: A panicking WASM plugin or crashing IPC sidecar is trapped gracefully by the core suite runtime without causing host application exit or document data loss.

---

## Strategic Implementation Plan

- **Phase 1 (Q4 2026)**: Finalize plugin manifest schema and host capability boundary specification.
- **Phase 2 (Q1 2027)**: Implement `suite-extension-core` WASM host prototype using `wasmtime`.
- **Phase 3 (Q2 2027)**: Introduce developer SDK, CLI tooling for plugin packaging (`suite-plugin-pack`), and security audit.

---
