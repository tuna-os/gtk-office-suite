# GTK Office Suite Extensibility & WASM Plugin Host RFC

**Status**: Draft / Proposed  
**Horizon**: Q4 2026 / Q1 2027  
**Tracking Issue**: #673  
**Authors**: Strategist Agent (tuna-os)  

---

## 1. Executive Summary

As **GTK Office Suite** achieves baseline maturity and daily-driver stability across Letters, Tables, and Decks, enterprise adoption and ecosystem growth depend heavily on **safe extensibility**.

Native C/Rust dynamic libraries (`.so` / `.dll`) introduced as plugins present grave security and stability risks:
1. Process memory access enables crashing the host GUI applications.
2. Arbitrary binary code bypasses Flatpak sandbox security constraints.
3. ABI fragility across Rust toolchain versions makes third-party plugins fragile.

This specification outlines the architecture for a **sandboxed WebAssembly (WASM) plugin runtime engine** and **Host-Guest IPC protocol** for GTK Office Suite applications.

---

## 2. Core Architecture

```
+-------------------------------------------------------------------+
|                        GTK4 / libadwaita Host                     |
|  (Letters / Tables / Decks Window & Controller Layer)             |
+-------------------------------------------------------------------+
                                  |
                                  v
+-------------------------------------------------------------------+
|                     suite-common Host Bridge                      |
|                  (Capability Grant & Event Router)                |
+-------------------------------------------------------------------+
                                  |
               Host-Guest WASI / Memory Shared IPC
                                  v
+-------------------------------------------------------------------+
|                    WASM Sandboxed Guest Host                      |
| (Wasmtime Runtime with Memory Ceiling & Execution Timeout)        |
+-------------------------------------------------------------------+
                                  |
                                  v
+-------------------------------------------------------------------+
|                     Third-Party WASM Plugin                       |
|        (Compiled from Rust, C/C++, Go, AssemblyScript)            |
+-------------------------------------------------------------------+
```

---

## 3. Host API Specification (Capability-Based)

Guest plugins interact with host document contexts strictly through capability-gated host imports.

### 3.1 Document Capabilities

- `doc:read`: Permission to query document text, AST structures, or spreadsheet cell grids.
- `doc:write`: Permission to execute transaction-based document edits (undoable actions).
- `ui:action`: Permission to register context menu actions, sidebar panels, or command palette entries.
- `network:fetch`: Explicitly controlled HTTP outbound fetch (disabled by default for enterprise security).

### 3.2 Host Function Imports (JSON / FlatBuffers IPC Schema)

```rust
// Logical definition of Host Imports exposed to Guest WASM Modules
pub trait OfficePluginHost {
    fn register_command(name: &str, title: &str) -> Result<u32, HostError>;
    fn read_active_selection() -> Result<Vec<u8>, HostError>;
    fn apply_edit_transaction(tx_bytes: &[u8]) -> Result<(), HostError>;
    fn show_toast(message: &str) -> Result<(), HostError>;
}
```

---

## 4. Sandbox Isolation & Resource Ceilings

To prevent runaway plugins from freezing the GTK UI looper or exhausting system memory:

| Resource Ceiling | Budget | Enforcement Mechanism |
|------------------|--------|-----------------------|
| Memory Ceiling   | 64 MB per plugin | Wasmtime memory limit allocation |
| Execution Time   | 50 ms per event turn | Fuel metering / async epoch interruption |
| Storage Access   | Isolated key-value directory | Isolated Flatpak app subfolder |
| UI Interruption  | Asynchronous execution | Off-thread worker execution |

---

## 5. Implementation Phasing

- **Phase 1 (Q4 2026)**: Define WASM runtime wrapper in `suite-common-core` (GTK-free host interface) with basic document read/write capabilities.
- **Phase 2 (Q1 2027)**: Implement extension manager UI dialogs and command palette integrations in `suite-common`.
- **Phase 3 (Q1 2027)**: Publish SDK templates (`wasm-plugin-template-rust`, `wasm-plugin-template-ts`).
