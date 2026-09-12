# GTK Office Suite Extension & WASM Plugin Host Architecture

This document defines the strategic roadmap and architecture specification for extending the **GTK Office Suite** (Letters, Tables, Decks) via out-of-process and sandboxed WASM plugins.

---

## 1. Executive Summary & Goals

As GTK Office Suite approaches production release readiness, supporting custom enterprise workflows, third-party template generators, specialized formula libraries, and custom export filters becomes critical.

### Key Strategic Objectives
1. **Host Stability & Isolation**: Plugin execution must never crash or block the main GTK application UI thread (`window.rs`).
2. **Memory & Access Security**: Plugins run inside a WASM sandbox (`wasmtime` or `wasmer`) or over isolated IPC (`gdbus` / Unix domain sockets) with granular capability permissions.
3. **Cross-Language Support**: Support extensions written in Rust, C/C++, WebAssembly, or Python.

---

## 2. Architecture Overview

```
+-------------------------------------------------------------+
|                 GTK Office Suite App Host                   |
|  (Letters / Tables / Decks - GTK4 / libadwaita UI Thread)    |
+------------------------------+------------------------------+
                               |
                               | Extension Event Bus (suite-common-core)
                               v
+-------------------------------------------------------------+
|                   suite-extension-api                       |
|           Host API Bindings & Capability Guard              |
+------------------------------+------------------------------+
                               |
          +--------------------+--------------------+
          |                                         |
          v                                         v
+-------------------+                     +-------------------+
|  WASM Plugin Host |                     | IPC Plugin Host   |
|  (Sandboxed)      |                     | (Out-of-Process)  |
+-------------------+                     +-------------------+
```

---

## 3. Capability Permissions Model

Plugins must declare requested capability permissions in their manifest (`extension.toml`):

```toml
[extension]
id = "org.tuna_os.tables.finance_pack"
name = "Enterprise Financial Functions"
version = "1.0.0"

[permissions]
read_document = true
write_cells = true
network_access = false
file_system = ["/tmp/exports"]
```

---

## 4. Implementation Phasing

1. **Phase 1 (Q4 2026)**: Publish `suite-extension-api` core specification and trait definitions.
2. **Phase 2 (Q1 2027)**: Integrate sandboxed WASM plugin host engine for Tables custom functions.
3. **Phase 3 (Q2 2027)**: Enable GUI widget extensions (sidebars and status bar indicators) via libadwaita plugin surfaces.

---
