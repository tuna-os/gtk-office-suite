# ADR-0007: Extension API & WASM/IPC Plugin Architecture

Date: 2026-09-12 · Status: proposed

## Context

As `gtk-office-suite` matures towards daily-driver readiness (tracked in `docs/readiness-2026-09/README.md`), community members and enterprise deployments require extensible capabilities. These include custom document import/export converters, specialized spreadsheet formula functions in Tables, automated batch macro execution, and bespoke document formatting tools.

Currently, all feature logic must be directly compiled into the core Rust binaries or integrated via custom sidecars. Without a standardized, safe plugin extension model, adding custom features increases binary bloat, threatens memory and thread safety, and violates the architecture rule separating business logic from GTK UI widgets.

## Decision

1. **WASM/IPC Sandboxed Plugin Model**:
   - **WASM Runtime**: Compute-only plugins (custom converters, formula extensions, text transformations) run inside a lightweight, sandboxed WebAssembly execution environment (e.g. `wasmtime`).
   - **IPC Host Boundary**: UI-heavy or external tool integrations execute via isolated out-of-process IPC channels over UNIX domain sockets using structured JSON-RPC / Protocol Buffers payload contracts.

2. **Strict GTK-Free API Boundaries**:
   - Plugins interact exclusively with `suite-common-core` data models and event interfaces.
   - Plugins have **zero direct access** to GTK4/libadwaita widgets or UI thread looper primitives.
   - Host applications expose asynchronous message-passing interfaces for document operations and UI menu/palette registration.

3. **Capability-Based Security Scoping**:
   - Plugins must declare explicit manifest permissions (e.g., `filesystem:read-only`, `network:disabled`, `document:write`).
   - Plugin capabilities are enforced by the WASM host environment and IPC channel policies.

## Consequences

- **Safety & Stability**: Malfunctioning or malicious third-party plugins cannot crash the main GTK UI thread or corrupt core document state.
- **Ecosystem Growth**: Community developers can author plugins in Rust, C/C++, Go, Python, or TypeScript targeting WASM without needing to compile against GTK4 or `gtk-office-suite` internal Rust crates.
- **Maintenance**: Core API host bindings in `suite-common-core` must be versioned and backwards-compatible with ratcheted contract tests.
