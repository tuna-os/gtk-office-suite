# WASM and IPC Plugin Extension Architecture Specification

> **Status**: Strategic Specification / Draft  
> **Horizon**: Mid-term (Q4 2026 / Q1 2027)  
> **Target Crate**: `suite-common-core` / `suite-common-plugin`  
> **Related Issue**: [#391](https://github.com/tuna-os/gtk-office-suite/issues/391)

---

## Executive Summary

The GTK Office Suite (*Letters*, *Tables*, *Decks*) requires an extensible, memory-safe, and decoupled plugin framework. To enable third-party developer ecosystems, custom file format conversion filters, custom macro automation, and user interface extensions without risking host application stability or GTK main loop deadlocks, the suite adopts a dual-tier extension architecture:

1. **WASM Sandboxed Plugins** (In-Process WebAssembly via `wasmtime` / `wasmer`)
2. **IPC Out-of-Process Sidecar Plugins** (Unix Domain Sockets / Varlink / D-Bus)

By anchoring host-plugin APIs strictly in `suite-common-core` (GTK-free, pure Rust), extensions operate seamlessly across GUI desktop sessions and headless serverless file-processing binaries.

---

## 1. Architectural Goals & Non-Goals

### Goals
- **Safety & Isolation**: Untrusted third-party plugin crashes or memory violations MUST NOT crash host application processes or corrupt document state.
- **GTK-Free Core Decoupling**: Extension interfaces MUST NOT depend on GTK4 or Libadwaita types. Extension logic operates directly on `suite-common-core` document models, event channels, and AST nodes.
- **Headless & GUI Parity**: Plugins MUST run identically in GUI applications (*Letters*, *Tables*, *Decks*) and headless CLI document converters (`letters-convert`, `tables-convert`).
- **Capability-Based Permissions**: Strict security policies controlling file system access, network access, and document state mutation.

### Non-Goals
- Direct native dynamic library (`.so` / `.dll`) loading without sandboxing.
- Direct GTK4 widget hierarchy manipulation by external plugin code.
- Synchronous blocking calls on the GTK main UI thread.

---

## 2. Core Architecture & Sandboxing Tiers

```
+-----------------------------------------------------------------------------------+
| Host Application / Headless Binary                                                |
|                                                                                   |
|  +-----------------------+     +------------------------+                         |
|  | Letters / Tables /    |     | Headless Converter CLI |                         |
|  | Decks (GTK UI / App)  |     | (Pure Rust CLI Tool)   |                         |
|  +-----------+-----------+     +-----------+------------+                         |
|              |                             |                                      |
|              +--------------+--------------+                                      |
|                             |                                                     |
|                             v                                                     |
|           +----------------------------------+                                    |
|           |       suite-common-core          |                                    |
|           |    Host ABI & Event Dispatch     |                                    |
|           +-----------------+----------------+                                    |
|                             |                                                     |
+-----------------------------|-----------------------------------------------------+
                              |
            +-----------------+-----------------+
            |                                   |
            v                                   v
+-----------------------+           +-----------------------+
|  WASM Runtime Tier    |           |   IPC Sidecar Tier    |
|  (In-Process Sandbox) |           | (Out-of-Process IPC)  |
|                       |           |                       |
|  +-----------------+  |           |  +-----------------+  |
|  |  Wasmtime Engine|  |           |  | Python / Go /   |  |
|  |  (Memory-safe)  |  |           |  | Rust Sidecar    |  |
|  +-----------------+  |           |  +-----------------+  |
+-----------------------+           +-----------------------+
```

### Tier 1: In-Process WASM Sandboxing (`wasmtime`)
- Designed for lightweight macros, document transform filters, linting rules, and format parsers.
- WebAssembly System Interface (WASI) capabilities strictly configured with zero ambient environment access.
- Execution timeout bounds (e.g., 500ms gas limit per event execution) to prevent CPU starvation.

### Tier 2: Out-of-Process IPC Sidecars (Unix Domain Sockets)
- Designed for complex third-party tools, heavy language runtimes (e.g., Python AI plugins, specialized rendering engines), or legacy integrations.
- Communication via async JSON-RPC 2.0 or Protocol Buffers over Unix Domain Sockets (`/tmp/gtk-office-plugin-<pid>.sock`).
- Automatic process supervision and health-checking by host application watcher threads.

---

## 3. Host ABI & Module Contract

Host capabilities are exposed via `suite-common-core` host bindings. Plugins implement standard entrypoints:

```rust
// Contract interface exposed by suite-common-core::plugin

pub trait DocumentPlugin {
    fn metadata(&self) -> PluginMetadata;
    fn on_document_open(&mut self, doc: &mut DocumentContext) -> PluginResult<()>;
    fn on_document_save(&mut self, doc: &DocumentContext) -> PluginResult<()>;
    fn transform_selection(&mut self, text: &str) -> PluginResult<String>;
}
```

### Manifest & Capability Specifications

Every plugin specifies its permissions in a `plugin.toml` manifest:

```toml
[plugin]
id = "org.gtkoffice.markdown-linter"
name = "Markdown Linter & Style Guide"
version = "1.0.0"
author = "Community Contributor"
entrypoint = "linter.wasm"

[capabilities]
read_document = true
modify_selection = true
network_access = false
filesystem_read = ["/usr/share/styles"]
filesystem_write = []
```

---

## 4. Ecosystem & Marketplace Roadmap

- **Phase 1 (Q4 2026)**: Core Host ABI specification, WASM runtime host integration in `suite-common-core`, unit test suite.
- **Phase 2 (Q1 2027)**: Developer SDK (Rust & C templates), plugin manifest validation tooling (`cargo-office-plugin`).
- **Phase 3 (Q2 2027)**: Flathub integration and permission dialogs in Libadwaita preferences window.

---

## 5. Security & Isolation Verification Plan

1. **Memory Bounds Testing**: Verify that out-of-bounds reads/writes inside WASM modules return controlled traps without crashing host binaries.
2. **Gas & Timeout Auditing**: Ensure infinite loops in plugins are safely terminated by `wasmtime` epoch interruption after 500ms.
3. **Fuzzing Host ABI**: Expand `cargo-fuzz` targets against host-plugin serialization interfaces.
