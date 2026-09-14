# GTK Office Suite Extension API & Sandboxed WASM Plugin Ecosystem Specification

**Status**: Draft / Strategic Planning  
**Target Milestone**: Q4 2026 / Q1 2027  
**Tracking Issue**: #675  
**Maintainer**: Strategist Agent (ACMM L6)

---

## 1. Executive Summary & Strategic Objectives

As GTK Office Suite (Letters, Tables, Decks) reaches daily-driver editing stability and ratcheted format parity, establishing a modular extensibility architecture is essential for long-term ecosystem growth, community adoption, and enterprise customization.

This specification details the design for the **GTK Office Suite Extension API** powered by a capability-sandboxed **WebAssembly (WASM)** plugin runtime.

### Key Goals
1. **Language-Agnostic Extensibility**: Allow developers to build plugins in Rust, C/C++, Go (TinyGo), TypeScript (AssemblyScript), or Python (via Component Model) targeting a standardized WebAssembly interface (WASI / WIT).
2. **Strict Process & Capability Isolation**: Enforce zero-trust sandboxing so third-party plugins cannot compromise host system stability, access unauthorized file paths, or inspect sensitive document data without explicit permission grants.
3. **Flatpak-Native Integration**: Operate seamlessly within Flatpak sandbox boundaries without requiring elevated D-Bus permissions or host filesystem access.
4. **Decoupled Architecture**: Maintain full compliance with the core suite architecture rule (*No business logic in widget code*) by executing plugin logic against GTK-free DOM representations in `suite-common-core`.

---

## 2. Architecture & Sandbox Model

```
 ┌─────────────────────────────────────────────────────────────────┐
 │                   GTK4 Application Host                         │
 │  ┌───────────────────┐               ┌───────────────────────┐  │
 │  │   UI / Window     │               │   suite-common-core   │  │
 │  │ (window.rs / GTK) │               │   (Document DOM Model)│  │
 │  └─────────┬─────────┘               └───────────┬───────────┘  │
 └────────────┼─────────────────────────────────────┼──────────────┘
              │ Event Dispatch                      │ Host Callbacks
              ▼                                     ▼
 ┌─────────────────────────────────────────────────────────────────┐
 │                 WASM Plugin Host Engine (wasmtime)              │
 │  ┌───────────────────────────────────────────────────────────┐  │
 │  │ Capability Guard & Fine-grained Permission Manager         │  │
 │  └─────────────────────────────┬─────────────────────────────┘  │
 └────────────────────────────────┼────────────────────────────────┘
                                  │ Sandboxed IPC Boundary
                                  ▼
 ┌─────────────────────────────────────────────────────────────────┐
 │                  Sandboxed WASM Guest Plugin                    │
 │  ┌───────────────────────────────────────────────────────────┐  │
 │  │  Guest Logic (Rust / C / Go / AssemblyScript / WIT ABI)   │  │
 │  └───────────────────────────────────────────────────────────┘  │
 └─────────────────────────────────────────────────────────────────┘
```

### 2.1 Host Engine Runtime
The host runtime uses `wasmtime` embedded within Rust core services. Execution occurs on background worker threads without blocking the main GTK GUI thread loop.

### 2.2 Capability Permission Granularity
Plugins explicitly declare requested capabilities in their `plugin.toml` manifest:

```toml
[plugin]
id = "org.gtkoffice.extensions.markdown-exporter"
name = "Advanced Markdown Exporter"
version = "0.1.0"
api_version = "1.0"

[capabilities]
read_document = true
write_document = false
network_access = false
filesystem_read = ["/tmp/export-previews"]
```

---

## 3. Host-Guest ABI & WIT Interface

The interface between host and plugins is defined using Wasm Interface Type (WIT) specifications:

```wit
package gtkoffice:extension@1.0.0;

interface document-dom {
  record node-info {
    id: string,
    kind: string,
    content: string,
  }

  get-root-nodes: func() -> list<node-info>;
  replace-node-content: func(node-id: string, new-content: string) -> result<_, string>;
}

world plugin-extension {
  import document-dom;
  export initialize: func() -> result<_, string>;
  export execute-action: func(action-id: string, payload: string) -> result<string, string>;
}
```

---

## 4. Security, Enterprise & dconf Controls

1. **System Administrator Policy Enforcement**: Enterprise administrators can disable plugin loading entirely via dconf/GSettings keys (`/org/gtkoffice/suite/plugins/enabled`).
2. **Signature Verification**: Production plugins must be signed using ed25519 repository keys.
3. **Execution Resource Budgets**: Host engine limits max heap memory allocation (e.g. 128MB per plugin instance) and CPU execution time per call (e.g. 5000ms max execution budget).

---

## 5. Roadmap & Implementation Phases

- **Phase 1 (Q4 2026)**: Core WIT ABI definitions and `suite-common-core` WASM execution bridge.
- **Phase 2 (Q1 2027)**: Letters & Tables document manipulation bindings and manifest verification.
- **Phase 3 (Q1/Q2 2027)**: Extension Manager UI dialog in `suite-common` and signed plugin distribution format.
