# RFC-0002: WebAssembly plugin runtime and IPC extension architecture

Date: 2026-09-12 · Status: **draft, not accepted**

## Summary

This RFC proposes a WebAssembly (WASM) host runtime and isolated IPC sidecar architecture for GTK Office Suite (Letters, Tables, and Decks). It defines how third-party extensions—such as custom formula functions in Tables, document formatters and spell/grammar linters in Letters, and export rendering filters in Decks—can run safely within memory sandbox boundaries without compromising GTK GUI stability or violating Flatpak isolation guarantees.

## Constraints

1. **GTK-Free Host Interface**: The core plugin sandbox and host interface must reside in `suite-common-core`. Plugin invocation logic must compile and run in headless Rust unit test suites without requiring X11, Wayland, or GTK libraries (ADR-0001).
2. **Flatpak Sandbox Safety**: Direct loading of dynamic native shared objects (`.so` files) into the main process space is prohibited. Plugins must run either inside a WASM sandbox (`wasmi`/`wasmtime`) or as isolated out-of-process IPC sidecars over Unix domain sockets.
3. **Deterministic State & Data Copies**: Memory passed to WASM plugins must be scoped to value types or byte slices. Core document buffers must never expose raw internal pointers to guest plugin code.
4. **Zero Impact on Daily-Driver Editing**: Plugin loading and execution must be strictly opt-in and non-blocking. Faulty or crashing plugin code must be caught by host trap handlers without terminating the parent GTK process.

## Extension API Categories

### 1. Tables (`tables-core`): Custom OpenFormula Functions
- **Capability**: Register custom spreadsheet functions (e.g., domain-specific financial or scientific routines).
- **Interface**: Pure input-array to scalar/array return values passed as serialized JSON or WIT (Wasm Interface Type) records.
- **Security**: Strict execution timeouts (default 100ms) and stack memory ceilings (default 16MB).

### 2. Letters (`letters-core`): Document Transformers & Linters
- **Capability**: Real-time syntax checkers, terminology validators, and custom document format converters.
- **Interface**: Reads AST nodes or plain text buffers; returns diagnostics (line/col ranges + messages) or transformed text chunks.

### 3. Decks (`decks-core`): Canvas Assets & Export Filters
- **Capability**: Custom vector asset generators and export rendering pipelines.
- **Interface**: Generates SVG or bitmap rendering buffers for insertion into slide layouts.

## Phased Implementation Roadmap

### Phase 1: Core Sandboxed Host Engine (`suite-common-core`)
- Integrate a lightweight WASM interpreter (`wasmi`) into `suite-common-core`.
- Implement host function bindings for basic string and numeric parameter passing.
- Add unit tests verifying trap recovery when WASM guest code panics or out-of-bounds accesses memory.

### Phase 2: Host Extension Registries
- Wire extension registries into `tables-core` (formula evaluator) and `letters-core` (document verification pipeline).
- Support manifest parsing (`plugin.json` / `plugin.toml`) defining permissions, function names, and author metadata.

### Phase 3: Flatpak & UI Integration (`suite-common`)
- Define Flatpak extension point structure (`org.tuna_os.GtkOfficeSuite.Plugin/*`).
- Build plugin management UI dialog in `suite-common` for discovering, enabling, and managing installed extensions.

## Verification & Proving Strategy

- **Core Unit Tests**: 100% headless coverage in `suite-common-core` verifying plugin instantiation, execution timeout traps, and error reporting.
- **GUI Journeys**: AT-SPI integration tests in `tests/gui` verifying enabling a plugin in settings and observing non-blocking UI behavior.
