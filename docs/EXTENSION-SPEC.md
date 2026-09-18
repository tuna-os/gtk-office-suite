# GTK Office Suite Extension Architecture & WASM Plugin Specification

> **Status**: Strategic Proposal (Q4 2026 / 2027 Horizon)  
> **Tracking Issue**: #762  
> **Target Crate**: `suite-common-core` / `suite-extension`  

---

## 1. Overview

As GTK Office Suite approaches daily-driver production readiness, expanding capability without bloating binary footprints (`letters`, `tables`, `decks`) requires a secure, modular plugin ecosystem.

This specification outlines the hybrid **WASM Sandbox + IPC Extension Architecture** for GTK Office Suite, enabling third-party developers and enterprise fleets to extend editor functionality safely.

---

## 2. Architecture & Isolation

Plugins execute within a sandboxed WebAssembly runtime embedded in `suite-common-core` (or a decoupled host runner).

```
┌─────────────────────────────────────────────────────────────┐
│                   GTK Office Suite App                      │
│             (Letters / Tables / Decks Window)               │
└──────────────────────────────┬──────────────────────────────┘
                               │ IPC / Host Call Bridge
┌──────────────────────────────▼──────────────────────────────┐
│                  suite-extension Runtime                    │
│ ┌─────────────────────────────────────────────────────────┐ │
│ │             WASM Sandbox (wasmi / wamr)                 │ │
│ │  ┌──────────────────────────────────────────────────┐  │ │
│ │  │             Third-Party Plugin Code              │  │ │
│ │  └──────────────────────────────────────────────────┘  │ │
│ └─────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

### Key Principles

1. **Memory Safety & Process Isolation**: WASM plugins operate in linear, bounded host memory. Faulty plugins cannot crash the host GUI application.
2. **Capability-Based Permissions**: Plugins must request explicit permissions in their manifest (`manifest.json`):
   - `read_document`: Inspect document AST / text stream.
   - `modify_document`: Emit structural document diffs.
   - `network_access`: Domain-restricted outbound HTTP calls.
   - `ui_action`: Register action buttons in the GTK headerbar / menu.
3. **Pure Rust Core Integration**: The extension engine relies on GTK-free data structures in `suite-common-core`, executing headless transforms without requiring an active GTK display loop.

---

## 3. Host Function Interfaces (ABI)

The plugin interface exposes standard Host-Guest functions:

```rust
// ABI Definition sketch for Host Function exports
pub trait SuiteExtensionHost {
    fn get_document_ast(&self) -> Result<String, ExtensionError>;
    fn apply_document_diff(&mut self, diff_json: &str) -> Result<(), ExtensionError>;
    fn log_message(&self, level: u32, message: &str);
}
```

---

## 4. Implementation Roadmap

- **Phase 1 (Q4 2026)**: WASM host integration in `suite-common-core` & basic manifest parser.
- **Phase 2 (Q1 2027)**: Document AST query and edit diff ABI stabilization for Letters & Tables.
- **Phase 3 (Q2 2027)**: Enterprise dconf policy enforcement for plugin permissions & signing keys.
