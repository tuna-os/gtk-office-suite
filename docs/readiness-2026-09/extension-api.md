# GTK Office Suite Extension Architecture & WASM/IPC API

## Strategic Context

As GTK Office Suite (Letters, Tables, Decks) moves toward enterprise deployment and post-v1 ecosystem growth, third-party extensibility becomes a critical priority. Corporate IT deployments and power users require custom document processing, domain-specific export filters, macro automation, and integrations with external services.

To maintain the suite's strict architectural boundary (**No business logic in widget code**) and ensure memory safety, stability, and security, extensions must run inside an isolated runtime rather than inside the main GTK application process.

## Architectural Requirements

### 1. Runtime Isolation & Security Sandbox
- **WASI / WebAssembly Core**: Extensions execute within a WebAssembly sandbox (e.g., `wasmtime` or `wasmer` host runtime in `suite-common-core`).
- **Capability-Based Permissions**: Extensions declare explicit permissions in an `extension.toml` manifest:
  - `read_document_content`: Grants read access to document AST/buffers.
  - `modify_document_content`: Grants write/edit transformation access.
  - `network_access`: Specific origin/domain whitelist for external APIs.
  - `filesystem_access`: Bounded path access (sandboxed directory only).

### 2. Event-Driven IPC Protocol
- Host-guest communication occurs over a binary IPC protocol (Cap'n Proto / bincode over stdio or shared memory buffers).
- **Core Event Channels**:
  - `document:opened` / `document:saved` / `document:closed`
  - `selection:changed`
  - `action:invoked` (Command Palette or UI Menu trigger)

### 3. UI Extension Points
Extensions cannot directly instantiate or manipulate GTK4 widgets. UI integration is constrained to declarative declarations host-rendered by `suite-common`:
- **Command Palette Actions**: Custom commands registered with keybindings.
- **Sidebar Panels**: Declarative UI layout definitions rendered natively by `AdwBin` / `GtkBox` host wrappers.
- **Status Bar Indicators**: Read-only text / icon indicators in application status bars.

### 4. Application Integration Map

| Application | Core Crate Integration Point | Host Extension Hooks |
|---|---|---|
| **Letters** | `letters-core` Document Buffer | Paragraph transformations, word count statistics, custom exports |
| **Tables** | `tables-core` Workbook Controller | Custom formula functions, data linting, table formatting rules |
| **Decks** | `decks-core` Presentation Model | Slide layout templates, shape batching, custom animation generators |

## Roadmap Timeline

- **Phase 1 (Q4 2026)**: Specification, manifest schema validation, and host WASI runtime prototype in `suite-common-core`.
- **Phase 2 (Q1 2027)**: Statically typed SDK bindings for Rust, AssemblyScript, and C.
- **Phase 3 (Q2 2027)**: Declarative UI sidebar panel host rendering and enterprise distribution policy integration via GSettings/dconf.
