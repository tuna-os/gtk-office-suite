# Extensibility & Plugin Architecture Strategy

**Document Version**: 1.0.0  
**Target Release**: Q4 2026 / 1.1 Milestone  
**Status**: Strategic Proposal  
**Related Issue**: #391  

---

## Executive Summary

As **GTK Office Suite** (**Letters**, **Tables**, **Decks**) advances toward widespread desktop adoption on Linux and Flathub, providing a secure, high-performance plugin and extensibility architecture is essential for supporting custom document filters, enterprise scripting, macro engines, and UI widget extensions. 

This document outlines the strategic roadmap for embedding **WebAssembly (WASM)** and **IPC Sidecar process** extensibility directly within `suite-common-core` (GTK-free), preventing main-loop blocking while enforcing absolute sandboxing and memory safety.

---

## Strategic Goals & Motivations

1. **Safety & Sandboxing**: Avoid legacy dynamic plugin models (`dlopen`) which expose main application binaries to segmentation faults, security exploits, and memory leaks.
2. **Ecosystem & Macro Support**: Enable third-party developers to write macros and filters in Rust, C/C++, TypeScript, Python, or Go compiled to WASM.
3. **Decoupled Architecture**: Keep plugin logic in `suite-common-core` so that headless conversion scripts, GUI windows, and CI test pipelines share identical extensibility models.
4. **UI Extension Safety**: Allow declarative sidebar and toolbar additions without permitting direct, unchecked GTK object mutations that crash main threads.

---

## Architectural Topology

```
+-----------------------------------------------------------------------+
|                         GTK Office Suite App                          |
|                  (Letters / Tables / Decks Window)                    |
+-----------------------------------+-----------------------------------+
                                    |
                            (Declarative UI)
                                    v
+-----------------------------------------------------------------------+
|                           suite-common                                |
|             (GTK Action Maps, Sidebar Panes, Dialogs)                 |
+-----------------------------------+-----------------------------------+
                                    |
                             (Host API Call)
                                    v
+-----------------------------------------------------------------------+
|                         suite-common-core                             |
|               (Plugin Manager, Host Runtime, Safety Sandbox)          |
+-----------------+-----------------------------------+-----------------+
                  |                                   |
           (WASM ABI / Wasmtime)              (Async IPC / stdio)
                  v                                   v
+-----------------------------------+   +-------------------------------+
|        WASM Plugin Sandbox        |   |      IPC Sidecar Process      |
|  - Custom Document Import/Export  |   |  - External Scripting Engine  |
|  - Macro Calculations & Transforms|   |  - Heavy Data Processing      |
+-----------------------------------+   +-------------------------------+
```

---

## Core Technical Components

### 1. WebAssembly (WASM) Host Engine
- **Runtime**: Powered by `wasmtime` embedded within `suite-common-core`.
- **Capability-Based Permissions**: Plugins must request explicit permissions in `plugin.toml` (e.g., `network_access = false`, `fs_read = ["/tmp/exports"]`).
- **Memory Safety**: Strict page allocation limits (e.g., 64MB max memory per plugin execution sandbox).

### 2. IPC Sidecar Protocol
- **Transport**: Stdio or Unix domain sockets using JSON-RPC 2.0 or Protocol Buffers.
- **Isolation**: Executed as separate unprivileged processes wrapped in Linux cgroups or Flatpak sub-sandboxes.

### 3. Declarative UI Extensions
- Extensions cannot instantiate GTK widgets directly.
- Plugins return structured GMenu definitions and AdwPropertyRow schema descriptions, rendered safely by `suite-common`.

---

## Implementation Milestones & Roadmap

| Phase | Horizon | Target Deliverables | Success Metrics |
| :--- | :--- | :--- | :--- |
| **Phase 1** | Q4 2026 | WASM Host runtime prototype in `suite-common-core`, plugin manifest validator (`plugin.toml`) | Zero main-thread blocking, unit tests passing |
| **Phase 2** | Q1 2027 | Document transform API & custom filter registration for `letters-core` & `tables-core` | Ability to load custom export filters without app recompilation |
| **Phase 3** | Q2 2027 | Declarative UI menu & panel extensions in `suite-common` | Third-party sidebar extensions rendering deterministically |
| **Phase 4** | Q3 2027 | Open Plugin Marketplace & Flathub extension bundle specification | Secure distribution pipeline for verified plugins |

---

## Verification & Compliance Standards

- **Unit Testing**: All WASM host bindings must be unit-tested in `suite-common-core` without GTK dependencies.
- **Security Audit**: Sandbox violation test suite enforcing file system isolation and memory limit caps.
- **AT-SPI Compliance**: Any UI elements registered by plugins must adhere to GNOME accessibility guidelines and AT-SPI tree integrity.
