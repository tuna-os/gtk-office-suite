# ADR 0007: WASM & IPC Plugin Extension Architecture

## Status

Proposed

## Context

`gtk-office-suite` requires a safe, high-performance extensibility framework to enable user-contributed plugins (custom import/export filters, formula functions, document automation scripts, and UI sidebars) without risking GTK main thread panics or compromising memory safety.

## Decision

We adopt a hybrid **WebAssembly (Wasmtime) + IPC Sidecar** architecture for all application plugins in Letters, Tables, and Decks.

### 1. WebAssembly (WASM) In-Process Host
- Plugins that process data (document filters, text transforms, custom formula evaluators) execute inside a guest Wasmtime sandbox hosted by `suite-common-core`.
- Guest WASM modules have **zero direct access** to host filesystem, network, or GTK UI components.
- Capability-based imports strictly limit memory consumption (max 64MB per plugin execution) and CPU time (fuel-metered execution).

### 2. IPC Sidecar Services
- Complex or language-native extensions (e.g. Python/C++ integration) execute as external subprocesses communicating via Varlink or GDBus over Unix Domain Sockets.
- UI extensions register actions via declarative JSON manifest schemas; `suite-common` renders menu/toolbar buttons on behalf of the plugin.

---

## Consequences

### Positive
- Isolated fault domains: a crashing plugin cannot crash Letters, Tables, or Decks.
- Cross-platform portability for WebAssembly binaries.
- Strict security permissions configurable via dconf policies.

### Negative
- Serialization overhead for large document buffers across host/guest boundaries.
- Requirement to maintain Rust plugin host SDKs.
