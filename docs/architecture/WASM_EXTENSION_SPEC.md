# WASM Extension Sandbox & Plugin Ecosystem Architecture Specification

**Status**: Draft / Strategic Proposal  
**Tracking Issue**: [#797](https://github.com/tuna-os/gtk-office-suite/issues/797)  
**Author**: Strategist Agent  

---

## 1. Executive Summary & Strategic Objectives

As GTK Office Suite reaches post-v1.0 stability across Letters, Tables, and Decks, building an enterprise-grade extension runtime is critical for ecosystem growth and enterprise adoption.

This specification outlines the architecture for a **secure WebAssembly (WASM) plugin sandbox** and host-guest IPC mechanism. The design ensures third-party extensions (e.g. document transformers, custom exporters, macro scripts, or AI helpers) run in isolated execution contexts without compromising desktop app performance or risking host filesystem/network safety.

Key Strategic Principles:
1. **Safety First**: Zero untrusted native dynamic library (`.so`) code loading. All external code must execute inside a WASM memory-isolated sandbox.
2. **GTK Main-Loop Protection**: Extension host calls and script executions MUST NOT run on or block the GTK main UI thread.
3. **Capability-Based Permissions**: Plugins must explicitly request granular permissions (e.g., `storage:read`, `export:pdf`, `network:fetch`) declared in a signed plugin manifest.
4. **Pure Rust Integration**: Integration into `suite-common-core` using standard Rust WebAssembly runtimes (such as `wasmtime`).

---

## 2. System Architecture & IPC Boundary

```
+--------------------------------------------------------------------+
|                         GTK Office Suite                           |
|  +--------------------+   +-------------------+  +---------------+ |
|  | Letters (window.rs)|   | Tables (window.rs)|  | Decks (...)   | |
|  +---------+----------+   +---------+---------+  +-------+-------+ |
+------------|------------------------|--------------------|---------+
             |                        |                    |
             v                        v                    v
+--------------------------------------------------------------------+
|                         suite-common-core                          |
|  +--------------------------------------------------------------+  |
|  |                Extension Manager & Dispatcher                |  |
|  +------------------------------+-------------------------------+  |
+---------------------------------|----------------------------------+
                                  | Host Call ABI (IPC)
                                  v
+--------------------------------------------------------------------+
|                        WASM Sandbox Runtime                        |
|  +--------------------------------------------------------------+  |
|  |                       Wasmtime Engine                        |  |
|  |  +--------------------+              +--------------------+  |  |
|  |  | Plugin A Instance  |              | Plugin B Instance  |  |  |
|  |  | (Linear Memory)    |              | (Linear Memory)    |  |  |
|  |  +--------------------+              +--------------------+  |  |
|  +--------------------------------------------------------------+  |
+--------------------------------------------------------------------+
```

---

## 3. Extension Manifest Schema (`plugin.toml`)

Every plugin package (`.gox` / zip file) must contain a `plugin.toml` manifest defining metadata and capability requirements:

```toml
[plugin]
id = "org.tuna-os.markdown-formatter"
name = "Markdown Formatter & Linter"
version = "0.1.0"
authors = ["Community Contributor <developer@example.com>"]
description = "Automated AST linting and table formatting plugin for Letters."

[runtime]
entrypoint = "plugin.wasm"
abi_version = "1.0.0"

[permissions]
capabilities = [
  "document:read-ast",
  "document:write-transform",
  "ui:status-toast"
]
```

---

## 4. Host-Guest ABI Contract (Rust Traits)

Host calls are exposed to WASM guests via explicit ABI bindings provided by `suite-common-core`:

```rust
/// Core Host Interface exposed to WASM Plugins
pub trait ExtensionHostApi {
    /// Retrieve the current document AST snapshot as JSON or CBOR
    fn get_document_ast(&self) -> Result<Vec<u8>, ExtensionError>;

    /// Apply a non-destructive AST transformation delta to the document
    fn apply_ast_delta(&mut self, delta_bytes: &[u8]) -> Result<(), ExtensionError>;

    /// Display a non-blocking toast notification in the host window
    fn show_toast(&self, message: &str, level: ToastLevel) -> Result<(), ExtensionError>;
}
```

---

## 5. Roadmap & Implementation Phases

| Phase | Horizon | Target Milestone | Deliverables |
|---|---|---|---|
| **Phase 1** | Q4 2026 | Prototype & Spec | Publish WASM Extension RFC & ABI specification in `suite-common-core` |
| **Phase 2** | Q1 2027 | Core Sandbox | Integrate `wasmtime` runtime with capability checks into `suite-common-core` |
| **Phase 3** | Q1-Q2 2027 | App Integration | Wire plugin extension points into Letters, Tables, and Decks UI menus |
| **Phase 4** | Q2 2027 | Marketplace | Flathub/GNOME extension distribution strategy and CLI toolchain |

---

## 6. Related Issues & Artifacts

- Meta Issue: [#797](https://github.com/tuna-os/gtk-office-suite/issues/797)
- Strategic Roadmap: [ROADMAP.md](../../ROADMAP.md)
