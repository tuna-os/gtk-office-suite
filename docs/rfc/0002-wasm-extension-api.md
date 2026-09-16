# RFC-0002: GTK Office Suite WASM plugin architecture & IPC extension API

Date: 2026-09-16 · Status: **draft, not accepted** · Tracks: [#443](https://github.com/tuna-os/gtk-office-suite/issues/443)

## Summary

This proposal defines the design for an out-of-process (OOP) IPC and WebAssembly (WASM) extension system for **Letters**, **Tables**, and **Decks**. It outlines how third-party plugins (such as reference managers, custom formula engines, macro runners, and document export filters) can safely extend the office suite without compromising stability, performance, or memory safety.

Key architectural principle: **Extensions run out-of-process or in a sandboxed WASM runtime (`wasmtime`); they NEVER directly touch GTK widgets or mutate document models outside `suite-common-core` transactional command boundaries.**

## Context & Problem Statement

As GTK Office Suite moves towards enterprise fleet readiness and daily-driver dependability, third-party extensibility becomes a key requirement for adoption. Modern enterprise document workflows require integrations with:
1. Reference management tools (e.g., Zotero, Mendeley, EndNote).
2. Specialized domain-specific calculation functions in spreadsheets.
3. Custom export formatters (e.g., ePub, PDF/A validator filters).
4. Automation scripts and macro engines.

However, traditional macro and extension systems (e.g., VBA in MS Office, UNO plugins in LibreOffice) are prone to security vulnerabilities, memory corruption crashes, and UI thread locking. 

## Architectural Constraints

1. **No GTK inside plugin sandboxes**: Following ADR-0001 (`suite-common-core` isolation), plugins interact strictly with pure Rust serialization protocols (`serde_json` / Protocol Buffers / Cap'n Proto over IPC).
2. **Capability-Based Permissions**: Plugins must declare explicit capability requests in their manifest (e.g., `network:fetch`, `fs:read-only`, `document:write-selection`). Users prompt/authorize permissions per plugin.
3. **Deterministic State Synchronization**: Plugin mutations are exposed as transactional Commands handled by `suite-common-core::undo`. Plugins cannot bypass the undo/redo engine or dirty state tracking.
4. **Isolated Memory Space**: WASM plugins execute in `wasmtime` instances with memory limits (e.g., 64MB max memory per instance) and strict instruction execution timeout bounds (fuel-based metering).

## Proposed Architecture

```
+-------------------------------------------------------------------+
|                        GTK Shell Window                           |
|  (Letters / Tables / Decks - window.rs & GTK4 Widget Hierarchy)   |
+-------------------------------------------------------------------+
                                 |
                         Host Extension Bus (suite-common)
                                 |
+--------------------------------+----------------------------------+
|                                                                   |
v                                                                   v
WASM Plugin Host (wasmtime)                           IPC Sidecar Daemon (D-Bus)
- Memory Sandboxing (64MB)                             - Out-of-Process Isolation
- Fuel Metering Execution                              - Native Code Binaries
- Capability Gatekeeper                                - Unix Domain Sockets
```

### 1. WASM Sandbox (`suite-common-wasm`)
- Runs in-process on worker threads, decoupled from the GTK main looper thread.
- Exposes standard ABI for document inspection, text range insertion, cell value computation, and slide shape query.
- Fuel-metered execution prevents infinite loops from freezing the editor host process.

### 2. Out-of-Process IPC Sidecars (`suite-common-ipc`)
- D-Bus or Varlink-based protocol over Unix Domain Sockets for desktop environment integration.
- Enables desktop apps (like Zotero, GNOME Shell extensions, or local AI sidecars) to interact with active documents securely.

### 3. Extension Manifest Spec (`plugin.toml`)

```toml
[plugin]
id = "org.tuna_os.zotero_connector"
name = "Zotero Reference Manager"
version = "0.1.0"
api_version = "1"
type = "wasm" # or "ipc"

[permissions]
capabilities = [
  "document:read-selection",
  "document:write-text",
  "network:http-local"
]
```

## Proof Requirements & Acceptance Criteria

Before any extension API is marked `verified` in `conformance/capabilities.json`:
1. **Core Unit Tests**: 100% test coverage for host-guest ABI serialization and fuel-metering timeout handling in `suite-common-core`.
2. **AT-SPI Journeys**: Deterministic AT-SPI smoke test executing a WASM plugin action (e.g., inserting a formatted citation) in Letters without UI freezes.
3. **Fault Injection**: Fuzzing suite proving that a crashing/panic WASM plugin or severed IPC sidecar socket does not crash the host app or corrupt the open document.

## Open Questions & Non-Goals

- **Non-Goal for Q4 2026**: Centralized plugin store / marketplace backend (focused purely on runtime engine & manifest spec).
- **Open Question**: Performance overhead of IPC vs WASM for large spreadsheet cell range recalculations (requires micro-benchmark suite in `tables-core`).
