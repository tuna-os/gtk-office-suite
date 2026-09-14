# GTK Office Suite Extension API & WebAssembly (WASM) Plugin Ecosystem Architecture

**Status**: Draft Specification  
**Tracking Issue**: #675  
**Author**: Strategist Agent  
**Target Release**: Q4 2026 / Q1 2027  

---

## 1. Executive Summary

As GTK Office Suite scales core capabilities across **Letters** (word processor), **Tables** (spreadsheet), and **Decks** (presentations), safe third-party extensibility becomes a key strategic lever for ecosystem growth and enterprise adoption.

This specification defines the runtime sandbox, Host-Guest Inter-Process Communication (IPC) protocol, and capability security model for GTK Office Suite WebAssembly (WASM) plugins.

---

## 2. Strategic Objectives & Security Boundaries

### Objectives
1. **Safety & Sandboxing**: Execute untrusted third-party code in fine-grained sandboxes without risking application crashes, GTK main loop freezes, or Flatpak container security breaches.
2. **Language Agnosticism**: Enable plugin development in Rust, C/C++, Go, AssemblyScript, Python (via WASI), and JavaScript.
3. **Enterprise Fleet Compliance**: Allow organization administrators to enforce signed plugin verification and control plugin permissions via `dconf` / GSettings policies.

### Non-Goals
- Native shared-library (`.so` / `.dll`) binary plugins running in-process (rejected due to memory safety and crash isolation concerns).
- Unrestricted disk or network access from plugin code without explicit capability grants.

---

## 3. Architecture & Execution Model

```
+-------------------------------------------------------------------+
|                        GTK4 Application Process                   |
|  (Letters / Tables / Decks - Main Thread + Event Loop)            |
+-------------------------------------------------------------------+
                                  |
                                  | Shared Memory / Async Channel
                                  v
+-------------------------------------------------------------------+
|                     suite-plugin-host (Wasmtime)                  |
|  - Capabilities Guard & Permission Enforcer                       |
|  - Host API Export Bindings                                       |
|  - Resource Limits (Memory Ceiling, CPU Instruction Metering)     |
+-------------------------------------------------------------------+
                                  |
                           WASM Sandbox Boundary
                                  v
+-------------------------------------------------------------------+
|                        WASM Guest Module                          |
|  (Custom Function / Export Filter / Macro / Document Inspector)   |
+-------------------------------------------------------------------+
```

---

## 4. Host API & Capability Grants

Plugins must explicitly request required capabilities in their `plugin.toml` manifest:

```toml
[plugin]
id = "org.tunaos.gtk-office.plugin.markdown-table-exporter"
name = "Markdown Table Exporter"
version = "0.1.0"
api_version = "1.0.0"

[capabilities]
read_document = true
write_document = false
network_access = false
file_system = []
```

### Core Host Functions Exported to WASM Guest

- `suite_doc_get_text(offset: u32, length: u32) -> *mut u8`
- `suite_doc_insert_text(offset: u32, text_ptr: *const u8, len: u32) -> i32`
- `suite_cell_get_value(row: u32, col: u32) -> *mut u8`
- `suite_cell_set_value(row: u32, col: u32, val_ptr: *const u8, len: u32) -> i32`
- `suite_log_info(msg_ptr: *const u8, len: u32)`

---

## 5. Enterprise Administration & Fleet Policy

Enterprise fleet deployments can restrict plugin execution through `dconf` schema controls (`org.tunaos.gtk-office-suite.plugins`):

- `allowed-plugins`: Whitelist of permitted plugin IDs.
- `require-signatures`: Require Ed25519 signatures from trusted enterprise keys.
- `enable-wasm-runtime`: Global toggle for plugin execution.

---

## 6. Implementation Roadmap

- **Phase 1 (Q4 2026)**: Finalize `suite-common-core` WASM host interfaces and manifest schema validator.
- **Phase 2 (Q1 2027)**: Implement `Tables` custom cell function WASM extensions.
- **Phase 3 (Q1 2027)**: Implement `Letters` / `Decks` import/export document filter WASM plugins.
