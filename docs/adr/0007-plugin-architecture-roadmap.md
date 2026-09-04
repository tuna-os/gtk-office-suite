# Strategic Plan: WASM/IPC Plugin Extension Architecture & Ecosystem Marketplace

> **Status**: Draft / Strategic Proposal  
> **Horizon**: Mid-term / Long-term (Q4 2026 – Q1 2027)  
> **Target Crate**: `suite-common-core` (Host API & Sandbox), `suite-common` (UI / Management)

---

## 1. Executive Summary

As the GTK office suite (Letters, Tables, Decks) matures, enabling third-party extensions and power-user automation becomes essential for enterprise adoption and ecosystem growth. Rather than exposing native Rust or C shared library APIs—which present security, stability, and memory-safety risks—we propose standardizing a **WebAssembly (WASM) and IPC-based plugin architecture**.

This architecture guarantees:
- **Sandbox Security**: Third-party code runs in a isolated Wasmtime execution sandbox with explicit capability granting (WASI).
- **Cross-Language Support**: Plugins can be written in Rust, TypeScript/AssemblyScript, Python (via Component Model), or C/C++.
- **Stability**: Crashing or unresponsive plugins will never abort or freeze the host GTK4 UI process.
- **IPC Isolation**: Out-of-process plugins (e.g. D-Bus services or external RPC workers) interface cleanly over lightweight IPC channels.

---

## 2. Core Architecture & Component Layout

```
                               ┌──────────────────────────────────────────┐
                               │       GTK4 / Libadwaita Host App         │
                               │        (Letters / Tables / Decks)        │
                               └────────────────────┬─────────────────────┘
                                                    │ Host IPC / Dispatcher
                               ┌────────────────────┴─────────────────────┐
                               │      suite-common-core::plugins          │
                               │      (Plugin Host Runtime & Registry)    │
                               └──────────┬───────────────────────┬───────┘
                                          │                       │
                       ┌──────────────────┴────────┐     ┌────────┴───────────────────┐
                       │   WASM Plugin Sandbox     │     │  Out-of-Process IPC Host   │
                       │   (Wasmtime + WASI)       │     │  (D-Bus / Unix Socket RPC) │
                       └───────────────────────────┘     └────────────────────────────┘
```

### 2.1 Crate Responsibilities

- **`suite-common-core::plugins`**: GTK-free pure Rust plugin runtime engine. Manages manifest parsing, WASM host bindings, function export invocation, and permission checks.
- **`suite-common::plugin_dialogs`**: GTK4/Libadwaita user interfaces for plugin installation, enablement toggles, permission approval, and marketplace browsing.

---

## 3. Extension Surface & Capability Scopes

Plugins must request capabilities in their `plugin.json` manifest. User approval is requested upon installation.

| Extension Type | Target App | Target Capability Scope | Host API Interface |
|---|---|---|---|
| **Custom Formula Functions** | Tables | `calc.formula` | High-performance scalar/array calculation functions evaluated in WASM. |
| **Document Transformers** | Letters / Decks | `document.transform` | Process AST fragments (e.g., Markdown, CommonMark, or ODF elements) for auto-formatting. |
| **External Reference Managers** | Letters | `reference.bibliography` | Integration with reference systems (Zotero, Mendeley, BibTeX) for dynamic citations. |
| **Export Converters** | All Apps | `export.converter` | Custom file format serializers/deserializers without touching GTK rendering logic. |
| **AI / NLP Assist Plugins** | Letters / Decks | `ai.assist` | Privacy-preserving text synthesis, grammar check, or summarization hooks. |

---

## 4. WASM Host API & Manifest Specification

### 4.1 Manifest Format (`plugin.json`)

```json
{
  "id": "org.tunaos.plugin.zotero-citations",
  "name": "Zotero Citation Manager",
  "version": "1.0.0",
  "description": "Insert and format academic citations directly from Zotero library",
  "author": "TunaOS Ecosystem",
  "entry_wasm": "plugin.wasm",
  "permissions": [
    "network:outbound",
    "document:read-write"
  ],
  "host_api_version": "1"
}
```

### 4.2 Security & Permission Model

1. **Deny-by-Default Capabilities**: Network access, file system reading, and clipboard access are forbidden unless explicitly granted by the user via Flatpak portal-style prompts.
2. **Deterministic Time & Memory Budgeting**: WASM instances are limited in execution time per function call (e.g. 500ms timeout for cell formula evaluation) and memory usage (e.g. max 64MB per plugin instance).

---

## 5. Phased Implementation Roadmap

### Phase 1: Core Host Runtime (`Q4 2026`)
- Implement `suite-common-core::plugins` host runtime powered by `wasmtime` / `wit-bindgen`.
- Define initial WIT (WASM Interface Type) contracts for document transformers and custom formula functions.
- Add unit test suite for plugin loading, execution sandboxing, and timeout enforcement.

### Phase 2: Host App Integration (`Q1 2027`)
- Wire plugin hook execution into `Tables` cell calculation loop and `Letters` export pipeline.
- Implement plugin settings UI in `suite-common` using Libadwaita preferences windows.

### Phase 3: Ecosystem Marketplace (`Q2 2027`)
- Publish plugin authoring documentation and template repositories for Rust and AssemblyScript.
- Define Flathub-compatible extension bundle specs (`.flatpakref` / `.tar.gz` signed plugin archives).

---

## 6. Proving & Verification Strategy

- **Unit Testing**: Unit tests in `suite-common-core` verifying WASM sandbox trap recovery, memory limits, and invalid manifest rejection.
- **Integration Testing**: Automated integration tests in `tables-core` ensuring WASM-defined OpenFormula functions execute deterministically.
- **Security Audit**: Fuzzing WASM module deserialization and input boundary parsing.

---
*Maintained by the strategist agent (tuna-os hive).*
