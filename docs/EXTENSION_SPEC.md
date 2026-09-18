# GTK Office Suite WASM & IPC Extension Architecture Specification

## Overview

This specification establishes the architectural roadmap for third-party extensions across Letters, Tables, and Decks in `gtk-office-suite`. The extension model leverages WebAssembly (WASM) for sandboxed custom logic and DBus/IPC capabilities for deep desktop environment integration.

## Goals

- **Sandboxed Security**: Run third-party extensions within a memory-isolated WASM environment without compromising core application stability or security.
- **Cross-App Host API**: Expose standard host bindings via standard ABI contracts for document inspection, custom formula evaluation (Tables), and custom export filters (Letters & Decks).
- **Extension UI Slots**: Define controlled UI insertion points (e.g. sidebar panels, dialog extensions, command palette actions) driven declaratively.

## Architecture

```
+-------------------------------------------------------------+
|                     GTK Office App Host                     |
|  (Letters / Tables / Decks - GTK4 / libadwaita UI Loop)     |
+------------------------------+------------------------------+
                               |
              Host API / CABI / DBus IPC Interface
                               |
+------------------------------v------------------------------+
|                    WASM Plugin Runtime                       |
|   (Wasmtime / Memory-isolated Sandbox / Permission Gates)   |
+-------------------------------------------------------------+
```

## Plugin Types & Interfaces

1. **Formula Plugins (Tables)**:
   - Register custom scalar or array functions into the OpenFormula engine.
   - Deterministic execution guarantees with execution timeout budgets.

2. **Format & Export Filters (Letters/Tables/Decks)**:
   - Provide custom import/export transformations (e.g., custom XML/JSON schema converters).

3. **Automation & Macro Plugins**:
   - Event-driven document manipulation triggered via user shortcuts or palette actions.

## Governance & Phase Rollout

- **Phase 1 (Q4 2026)**: Core Rust WASM runtime abstraction in `suite-common-core`.
- **Phase 2 (Q1 2027)**: Tables formula extension ABI and CLI test harness.
- **Phase 3 (Q2 2027)**: Letters/Decks UI extension slot integration.
