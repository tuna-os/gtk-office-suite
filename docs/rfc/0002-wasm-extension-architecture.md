# RFC 0002: WebAssembly (WASM) & Asynchronous IPC Extension Architecture

- **Status**: Draft
- **Date**: 2026-09-18
- **Authors**: GTK Office Suite Contributors
- **Tracking Issue**: #579

---

## 1. Abstract

This RFC proposes a unified Extension API and sandboxed WebAssembly (WASM) plugin runtime architecture for **Letters**, **Tables**, and **Decks**. It defines the host-guest ABI, event loop model, capability isolation, and asynchronous IPC transport enabling third-party extensions (custom formula functions, document filters, UI panels, automated workflows) to run safely without endangering host GTK application stability.

---

## 2. Motivation & Constraints

### 2.1 Strategic Rationale
As `gtk-office-suite` matures beyond v1.0, third-party extensibility becomes a key adoption enabler for enterprise deployments and specialized document workflows. Currently, extending any application requires editing core Rust source code and recompiling the application binary.

### 2.2 Hard Constraints
1. **Host Stability**: An unhandled panic or infinite loop in a plugin must never crash or block the GTK main loop or corrupt host document memory.
2. **Security & Sandboxing**: Plugins must operate with zero default host filesystem or network permissions. Resource access (files, network, IPC) requires explicit user approval via XDG Desktop Portals.
3. **Multi-Language Guest Support**: The WASM ABI must support compiled guests written in Rust, C/C++, Go, or TypeScript/AssemblyScript.
4. **GTK-Free Core**: The plugin host runtime logic must live in `suite-common-core` (or a dedicated `suite-plugin-core` crate), remaining GTK-free and fully unit-testable.

---

## 3. Architecture Specification

### 3.1 Host-Guest Architecture Overview

```
 ┌────────────────────────────────────────────────────────┐
 │                 GTK Office Suite Application           │
 │  (Letters / Tables / Decks - window.rs & GTK widgets)  │
 └──────────────────────────┬─────────────────────────────┘
                            │ Async Channels (mpsc)
 ┌──────────────────────────▼─────────────────────────────┐
 │                suite-plugin-core Host                  │
 │  (WASM engine: wasmtime / wasmer sandboxed runtime)    │
 └──────────────────────────┬─────────────────────────────┘
                            │ C-ABI / WIT Contract
 ┌──────────────────────────▼─────────────────────────────┐
 │                    WASM Guest Plugin                   │
 │  (Rust / C / TS compiled to wasm32-unknown-unknown)    │
 └────────────────────────────────────────────────────────┘
```

### 3.2 Component Model & WIT Interface Definition

The plugin ABI is defined using Wasmtime Component Model interface definitions (`wit`):

```wit
package suite:plugin@0.1.0;

world document-plugin {
    import host-log: func(level: string, message: string);
    import host-read-document: func() -> result<string, string>;
    import host-write-document: func(content: string) -> result<_, string>;

    export init: func() -> result<_, string>;
    export process-event: func(event-type: string, payload: string) -> result<string, string>;
}
```

### 3.3 Asynchronous Execution & Memory Safety

- **Isolation**: Each loaded plugin executes inside an isolated WASM instance with strict memory boundaries (e.g., maximum 64MB heap limit).
- **Timeouts & Interrupts**: The host runtime configures Wasmtime epoch interruption. Any WASM invocation exceeding a 200ms compute ceiling triggers an automatic guest interruption and error reporting.
- **IPC Event Queue**: Host GTK UI signals dispatch non-blocking events across `tokio::sync::mpsc` channels to a worker pool, ensuring the main looper thread never waits synchronously on WASM code.

---

## 4. Implementation Phasing

| Phase | Target Horizon | Deliverable | Exit Criteria |
|---|---|---|---|
| **Phase 1** | Q4 2026 | `suite-plugin-core` crate & WIT ABI contract | Unit test suite verifying WASM load, epoch interrupt, and zero-copy JSON payload exchange |
| **Phase 2** | Q1 2027 | Host engine integration in Letters, Tables, Decks | Formula extensions in Tables & document transform filters in Letters |
| **Phase 3** | Q2 2027 | Flatpak permission integration & plugin manager UI | Portal-driven sandbox granting & extension catalog settings panel |

---

## 5. Alternatives Considered

1. **Native Dynamic Libraries (`.so` / `dlopen`)**:
   - *Rejected*: Insecure; a null pointer dereference or memory corruption in a C plugin directly crashes the main GTK application process.
2. **D-Bus IPC Services**:
   - *Rejected*: High IPC serialization overhead for large document buffers and extra deployment complexity outside Flatpak.

---

## 6. References

- [ROADMAP.md](../../ROADMAP.md)
- [docs/readiness-2026-09/README.md](../readiness-2026-09/README.md)
- [docs/ARCHITECTURE.md](../ARCHITECTURE.md)
