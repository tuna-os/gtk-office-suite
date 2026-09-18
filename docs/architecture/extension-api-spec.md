# Architectural Specification: GTK Office Suite WASM & D-Bus IPC Extension API

**Tracking Issue**: #804  
**Author**: Strategist Agent (tuna-os hive)  
**Status**: Draft / Strategic Proposal  
**Target Horizon**: Q4 2026 / 2027  

---

## 1. Executive Summary

As Letters, Tables, and Decks achieve core feature parity and daily-driver stability (post-v1.0), extending functionality via third-party plugins becomes essential for ecosystem growth and enterprise adoption.

This specification outlines the architecture for third-party extensibility in `gtk-office-suite`. It establishes a dual-tier extension model:
1. **In-Process WebAssembly (WASM) Plugins**: High-performance, sandboxed logic (custom formula functions, document linting, headless format filters).
2. **Out-of-Process D-Bus IPC Extensions**: Isolated desktop integrations and custom UI panels running as separate user processes.

---

## 2. Core Architecture & Isolation Rules

To maintain the project's primary mission — daily-driver reliability and crash resilience — all extension execution must follow strict isolation rules:

1. **Memory & Thread Safety**: Plugins MUST NOT execute arbitrary native binary code inside the main application process.
2. **UI & Business Logic Separation**: Extensions MUST NOT directly modify GTK widget state or internal memory structures. All document mutations pass through the canonical `suite-common-core` command and undo pipeline.
3. **Fault Tolerance**: A crashing plugin or out-of-bounds WASM memory access MUST NOT crash the host application (`letters`, `tables`, or `decks`).

---

## 3. WebAssembly (WASM) Plugin Runtime

WASM plugins are executed using an embedded, lightweight WASM runtime (such as `wasmtime` or `wasmer`) managed by `suite-common-core`.

### Capabilities & Capabilities Matrix
Plugins declare required permissions in an `extension.toml` manifest:

```toml
[extension]
id = "org.tuna_os.letters.markdown_linter"
name = "Markdown Linter"
version = "0.1.0"
api_version = "1.0"

[capabilities]
read_document = true
mutate_document = true
network_access = false
filesystem_access = false
```

### Supported WASM Interfaces
- **Custom Functions (Tables)**: Register pure custom calculation functions into the IronCalc engine.
- **Document Transformers (Letters/Decks)**: Pure functions mapping `DocumentAST -> DocumentAST` or string content transformations.
- **Import/Export Filters**: Custom file decoders/encoders running in isolated linear memory.

---

## 4. D-Bus IPC Extension Interface

For extensions requiring complex UI additions, native system integration, or external service connections, communication occurs over session D-Bus.

### Bus Interfaces
- `org.gnome.GtkOfficeSuite.ExtensionManager`: Hosts register and activate extensions.
- `org.gnome.GtkOfficeSuite.DocumentBridge`: Provides asynchronous document inspection, event notifications (e.g. `DocumentSaved`, `SelectionChanged`), and transactional edit dispatches.

---

## 5. Security & Enterprise Governance

- **Sandboxing**: WASM plugins have zero system call access by default. Filesystem and network access are restricted unless explicitly granted and confirmed by user prompt.
- **Enterprise Policy**: Deployment configurations via `dconf` / GSettings can enforce plugin whitelists or disable third-party WASM execution across enterprise fleets.

---

## 6. Implementation Milestones

- **Phase 1 (Q4 2026)**: Define WASM host binding traits in `suite-common-core`.
- **Phase 2 (Q1 2027)**: Implement IronCalc custom function WASM loader in `tables`.
- **Phase 3 (Q2 2027)**: Expose D-Bus `DocumentBridge` interface for out-of-process sidebar integration.
